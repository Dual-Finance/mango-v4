use crate::state::*;
use anchor_lang::prelude::*;
use fixed::types::I80F48;

#[derive(Accounts)]
pub struct PerpEditMarket<'info> {
    #[account(
        has_one = admin,
    )]
    pub group: AccountLoader<'info, Group>,
    pub admin: Signer<'info>,

    #[account(
        mut,
        has_one = group
    )]
    pub perp_market: AccountLoader<'info, PerpMarket>,
}

#[allow(clippy::too_many_arguments)]
pub fn perp_edit_market(
    ctx: Context<PerpEditMarket>,
    oracle_opt: Option<Pubkey>,
    oracle_config_opt: Option<OracleConfig>,
    base_token_index_opt: Option<TokenIndex>,
    base_token_decimals_opt: Option<u8>,
    maint_asset_weight_opt: Option<f32>,
    init_asset_weight_opt: Option<f32>,
    maint_liab_weight_opt: Option<f32>,
    init_liab_weight_opt: Option<f32>,
    liquidation_fee_opt: Option<f32>,
    maker_fee_opt: Option<f32>,
    taker_fee_opt: Option<f32>,
    min_funding_opt: Option<f32>,
    max_funding_opt: Option<f32>,
    impact_quantity_opt: Option<i64>,
) -> Result<()> {
    let mut perp_market = ctx.accounts.perp_market.load_mut()?;

    // note: unchanged fields are inline, and match exact definition in perp_register_market
    // please maintain, and don't remove, makes it easy to reason about which support admin modification

    // unchanged -
    // name
    // group

    if let Some(oracle) = oracle_opt {
        perp_market.oracle = oracle;
    }
    if let Some(oracle_config) = oracle_config_opt {
        perp_market.oracle_config = oracle_config;
    };

    // unchanged -
    // bids
    // asks
    // event_queue
    // quote_lot_size
    // base_lot_size

    if let Some(maint_asset_weight) = maint_asset_weight_opt {
        perp_market.maint_asset_weight = I80F48::from_num(maint_asset_weight);
    }
    if let Some(init_asset_weight) = init_asset_weight_opt {
        perp_market.init_asset_weight = I80F48::from_num(init_asset_weight);
    }
    if let Some(maint_liab_weight) = maint_liab_weight_opt {
        perp_market.maint_liab_weight = I80F48::from_num(maint_liab_weight);
    }
    if let Some(init_liab_weight) = init_liab_weight_opt {
        perp_market.init_liab_weight = I80F48::from_num(init_liab_weight);
    }
    if let Some(liquidation_fee) = liquidation_fee_opt {
        perp_market.liquidation_fee = I80F48::from_num(liquidation_fee);
    }

    if let Some(maker_fee) = maker_fee_opt {
        perp_market.maker_fee = I80F48::from_num(maker_fee);
    }
    if let Some(taker_fee) = taker_fee_opt {
        perp_market.taker_fee = I80F48::from_num(taker_fee);
    }

    if let Some(min_funding) = min_funding_opt {
        perp_market.min_funding = I80F48::from_num(min_funding);
    }
    if let Some(max_funding) = max_funding_opt {
        perp_market.max_funding = I80F48::from_num(max_funding);
    }
    if let Some(impact_quantity) = impact_quantity_opt {
        perp_market.impact_quantity = impact_quantity;
    }

    // unchanged -
    // long_funding
    // short_funding
    // funding_last_updated
    // open_interest
    // seq_num
    // fees_accrued
    // bump

    if let Some(base_token_decimals) = base_token_decimals_opt {
        perp_market.base_token_decimals = base_token_decimals;
    }

    // unchanged -
    // perp_market_index

    if let Some(base_token_index) = base_token_index_opt {
        perp_market.base_token_index = base_token_index;
    }

    // unchanged -
    // quote_token_index

    Ok(())
}