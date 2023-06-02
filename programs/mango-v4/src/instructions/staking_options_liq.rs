use anchor_lang::prelude::*;
use fixed::types::I80F48;
use std::cmp::min;

use crate::accounts_ix::*;
use crate::error::*;
use crate::health::*;
use crate::logs::{StakingOptionsLiqLog, TokenBalanceLog};
use crate::state::*;

pub fn staking_options_liq(
    ctx: Context<StakingOptionsLiq>,
    asset_token_index: TokenIndex,
    liab_token_index: TokenIndex,
    max_liab_transfer: I80F48,
) -> Result<()> {
    // Differences with token_liq_with_token:
    // Liquor can only receive staking options from the last hour of expiration.
    // Do not set is_liquidating.
    // Liqee does not need to have a negative native position.

    let group_pk = &ctx.accounts.group.key();

    require!(asset_token_index != liab_token_index, MangoError::SomeError);
    let mut account_retriever = ScanningAccountRetriever::new(ctx.remaining_accounts, group_pk)
        .context("create account retriever")?;

    require_keys_neq!(ctx.accounts.liqor.key(), ctx.accounts.liqee.key());
    let mut liqor = ctx.accounts.liqor.load_full_mut()?;
    require!(
        liqor
            .fixed
            .is_owner_or_delegate(ctx.accounts.liqor_owner.key()),
        MangoError::SomeError
    );

    let mut liqee = ctx.accounts.liqee.load_full_mut()?;

    // Initial liqee health check. Uses init health. Because the health
    // contribution drops to zero, this could result in just one liquidation,
    // rather than a continuous stream of them all the way to Init threshold.
    let mut liqee_health_cache = new_health_cache(&liqee.borrow(), &account_retriever)
        .context("create liqee health cache")?;
    let liqee_liq_end_health = liqee_health_cache.health(HealthType::Init);

    //
    // Transfer some liab_token from liqor to liqee and
    // transfer some asset_token from liqee to liqor.
    //
    let now_ts = Clock::get()?.unix_timestamp.try_into().unwrap();
    liquidation_action(
        &mut account_retriever,
        liab_token_index,
        asset_token_index,
        &mut liqor.borrow_mut(),
        ctx.accounts.liqor.key(),
        &mut liqee.borrow_mut(),
        ctx.accounts.liqee.key(),
        &mut liqee_health_cache,
        liqee_liq_end_health,
        now_ts,
        max_liab_transfer,
    )?;

    // Check liqor's health
    if !liqor.fixed.is_in_health_region() {
        let liqor_health = compute_health(&liqor.borrow(), HealthType::Init, &account_retriever)
            .context("compute liqor health")?;
        require!(liqor_health >= 0, MangoError::HealthMustBePositive);
    }

    Ok(())
}

pub(crate) fn liquidation_action(
    account_retriever: &mut ScanningAccountRetriever,
    liab_token_index: TokenIndex,
    asset_token_index: TokenIndex,
    liqor: &mut MangoAccountRefMut,
    liqor_key: Pubkey,
    liqee: &mut MangoAccountRefMut,
    liqee_key: Pubkey,
    liqee_health_cache: &mut HealthCache,
    liqee_liq_end_health: I80F48,
    now_ts: u64,
    max_liab_transfer: I80F48,
) -> Result<()> {
    let (asset_bank, asset_oracle_price, opt_liab_bank_and_price) =
        account_retriever.banks_mut_and_oracles(asset_token_index, liab_token_index)?;
    let (liab_bank, liab_oracle_price) = opt_liab_bank_and_price.unwrap();

    // Verify that the asset bank is for a staking option that expires in
    // the next hour. The liab bank should be USDC, likely caused from a short
    // perp position that is negative but has been collateralized with the long
    // staking option.
    require!(
        asset_bank.staking_options_expiration > 0,
        MangoError::StakingOptionsError
    );
    let time_remaining: u64 = asset_bank.staking_options_expiration - now_ts;
    require!(time_remaining < 60 * 60, MangoError::StakingOptionsError);

    // The main complication here is that we can't keep the liqee_asset_position and liqee_liab_position
    // borrows alive at the same time. Possibly adding get_mut_pair() would be helpful.
    let (liqee_asset_position, liqee_asset_raw_index) =
        liqee.token_position_and_raw_index(asset_token_index)?;
    let liqee_asset_native = liqee_asset_position.native(asset_bank);
    require!(liqee_asset_native.is_positive(), MangoError::SomeError);

    let (liqee_liab_position, liqee_liab_raw_index) =
        liqee.token_position_and_raw_index(liab_token_index)?;
    let liqee_liab_native = liqee_liab_position.native(liab_bank);
    require!(liqee_liab_native.is_negative(), MangoError::SomeError);

    // Liquidation fees work by giving the liqor more assets than the oracle price would
    // indicate. Specifically we choose
    //   assets =
    //     liabs * liab_oracle_price/asset_oracle_price * (1 + liab_liq_fee)
    // Which means that we use a increased liab oracle price for the conversion.
    // For simplicity we write
    //   assets = liabs * liab_oracle_price / asset_oracle_price * fee_factor
    //   assets = liabs * liab_oracle_price_adjusted / asset_oracle_price
    //          = liabs * lopa / aop
    let fee_factor = I80F48::ONE + liab_bank.liquidation_fee;
    let liab_oracle_price_adjusted = liab_oracle_price * fee_factor;

    let init_asset_weight = 0;
    let init_liab_weight = liab_bank.init_liab_weight;

    // The price the Init health computation uses for a liability of one native liab token
    let liab_liq_end_price = liqee_health_cache
        .token_info(liab_token_index)
        .unwrap()
        .prices
        .liab(HealthType::Init);
    // Health price for an asset of one native asset token
    let asset_liq_end_price = liqee_health_cache
        .token_info(asset_token_index)
        .unwrap()
        .prices
        .asset(HealthType::Init);

    // How much asset would need to be exchanged to liab in order to bring health to 0?
    // This is the same as token_liq_with_token except there is no health gain
    // from reducing borrow because they have no health contribution in the last
    // hour.
    //
    // That means: what is x (unit: native liab tokens) such that
    //   init_health
    //     + x * ilw * llep     health gain from reducing liabs
    //     = 0
    // where
    //   ilw = init_liab_weight,
    //   llep = liab_liq_end_price,
    //   lopa = liab_oracle_price_adjusted, (see above)
    //   aop = asset_oracle_price
    //   ff = fee_factor
    // and the asset cost of getting x native units of liab is:
    //   y = x * lopa / aop   (native asset tokens, see above)
    //
    // Result: x = -init_health / (ilw * llep)
    let liab_needed = -liqee_liq_end_health / (liab_liq_end_price * init_liab_weight);

    // How much liab can we get at most for the asset balance?
    let liab_possible = liqee_asset_native * asset_oracle_price / liab_oracle_price_adjusted;

    // The amount of liab native tokens we will transfer
    let liab_transfer = min(
        min(min(liab_needed, -liqee_liab_native), liab_possible),
        max_liab_transfer,
    );

    // The amount of asset native tokens we will give up for them
    let asset_transfer = liab_transfer * liab_oracle_price_adjusted / asset_oracle_price;

    // During liquidation, we mustn't leave small positive balances in the liqee. Those
    // could break bankruptcy-detection. Thus we dust them even if the token position
    // is nominally in-use.

    // Apply the balance changes to the liqor and liqee accounts
    let liqee_liab_position = liqee.token_position_mut_by_raw_index(liqee_liab_raw_index);
    let liqee_liab_active =
        liab_bank.deposit_with_dusting(liqee_liab_position, liab_transfer, now_ts)?;
    let liqee_liab_indexed_position = liqee_liab_position.indexed_position;

    let (liqor_liab_position, liqor_liab_raw_index, _) =
        liqor.ensure_token_position(liab_token_index)?;
    let (liqor_liab_active, loan_origination_fee) =
        liab_bank.withdraw_with_fee(liqor_liab_position, liab_transfer, now_ts)?;
    let liqor_liab_indexed_position = liqor_liab_position.indexed_position;
    let liqee_liab_native_after = liqee_liab_position.native(liab_bank);

    let (liqor_asset_position, liqor_asset_raw_index, _) =
        liqor.ensure_token_position(asset_token_index)?;
    let liqor_asset_active = asset_bank.deposit(liqor_asset_position, asset_transfer, now_ts)?;
    let liqor_asset_indexed_position = liqor_asset_position.indexed_position;

    let liqee_asset_position = liqee.token_position_mut_by_raw_index(liqee_asset_raw_index);
    let liqee_asset_active = asset_bank.withdraw_without_fee_with_dusting(
        liqee_asset_position,
        asset_transfer,
        now_ts,
    )?;
    let liqee_asset_indexed_position = liqee_asset_position.indexed_position;
    let liqee_assets_native_after = liqee_asset_position.native(asset_bank);

    // Update the health cache
    liqee_health_cache
        .adjust_token_balance(liab_bank, liqee_liab_native_after - liqee_liab_native)?;
    liqee_health_cache
        .adjust_token_balance(asset_bank, liqee_assets_native_after - liqee_asset_native)?;

    msg!(
        "liquidated {} liab for {} asset",
        liab_transfer,
        asset_transfer
    );

    // liqee asset
    emit!(TokenBalanceLog {
        mango_group: liqee.fixed.group,
        mango_account: liqee_key,
        token_index: asset_token_index,
        indexed_position: liqee_asset_indexed_position.to_bits(),
        deposit_index: asset_bank.deposit_index.to_bits(),
        borrow_index: asset_bank.borrow_index.to_bits(),
    });
    // liqee liab
    emit!(TokenBalanceLog {
        mango_group: liqee.fixed.group,
        mango_account: liqee_key,
        token_index: liab_token_index,
        indexed_position: liqee_liab_indexed_position.to_bits(),
        deposit_index: liab_bank.deposit_index.to_bits(),
        borrow_index: liab_bank.borrow_index.to_bits(),
    });
    // liqor asset
    emit!(TokenBalanceLog {
        mango_group: liqee.fixed.group,
        mango_account: liqor_key,
        token_index: asset_token_index,
        indexed_position: liqor_asset_indexed_position.to_bits(),
        deposit_index: asset_bank.deposit_index.to_bits(),
        borrow_index: asset_bank.borrow_index.to_bits(),
    });
    // liqor liab
    emit!(TokenBalanceLog {
        mango_group: liqee.fixed.group,
        mango_account: liqor_key,
        token_index: liab_token_index,
        indexed_position: liqor_liab_indexed_position.to_bits(),
        deposit_index: liab_bank.deposit_index.to_bits(),
        borrow_index: liab_bank.borrow_index.to_bits(),
    });

    // Since we use a scanning account retriever, it's safe to deactivate inactive token positions
    if !liqee_asset_active {
        liqee.deactivate_token_position_and_log(liqee_asset_raw_index, liqee_key);
    }
    if !liqee_liab_active {
        liqee.deactivate_token_position_and_log(liqee_liab_raw_index, liqee_key);
    }
    if !liqor_asset_active {
        liqor.deactivate_token_position_and_log(liqor_asset_raw_index, liqor_key);
    }
    if !liqor_liab_active {
        liqor.deactivate_token_position_and_log(liqor_liab_raw_index, liqor_key)
    }

    emit!(StakingOptionsLiqLog {
        mango_group: liqee.fixed.group,
        liqee: liqee_key,
        liqor: liqor_key,
        asset_token_index,
        liab_token_index,
        asset_transfer: asset_transfer.to_bits(),
        liab_transfer: liab_transfer.to_bits(),
        asset_price: asset_oracle_price.to_bits(),
        liab_price: liab_oracle_price.to_bits(),
    });

    Ok(())
}
