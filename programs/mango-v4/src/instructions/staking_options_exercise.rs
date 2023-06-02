use anchor_lang::prelude::*;

use crate::accounts_ix::*;
use crate::error::*;
use crate::health::*;
use crate::logs::{StakingOptionExerciseLog, TokenBalanceLog};
use crate::state::*;
use fixed::types::I80F48;

// Amount is in native of options. Note that staking options are zero decimals,
// so native is number of tokens.
pub fn staking_options_exercise(
    ctx: Context<StakingOptionsExercise>,
    amount: u64,
    strike: u64,
) -> Result<()> {
    let group_pk = &ctx.accounts.group.key();
    let mut account = ctx.accounts.account.load_full_mut()?;

    let pre_health_opt = if !account.fixed.is_in_health_region() {
        let retriever =
            new_fixed_order_account_retriever(ctx.remaining_accounts, &account.borrow())?;
        let health_cache =
            new_health_cache(&account.borrow(), &retriever).context("pre-exercise init health")?;
        let pre_init_health = account.check_health_pre(&health_cache)?;
        Some((health_cache, pre_init_health))
    } else {
        None
    };

    let mut base_bank = ctx.accounts.base_bank.load_mut()?;
    let mut quote_bank = ctx.accounts.quote_bank.load_mut()?;
    let mut option_bank = ctx.accounts.option_bank.load_mut()?;
    let base_token_index = base_bank.token_index;
    let quote_token_index = quote_bank.token_index;
    let option_token_index = option_bank.token_index;

    // Verify the staking_options_state. CPI will fail if incorrect, so not a
    // security concern, just a sanity check.
    require_keys_neq!(option_bank.staking_options_state, Pubkey::default());
    require_keys_eq!(
        option_bank.staking_options_state,
        ctx.accounts.staking_options_state.key()
    );

    // Verify that the given token accounts for the exercise match what is on the banks.
    require_keys_eq!(ctx.accounts.base_vault.key(), base_bank.vault);
    require_keys_eq!(ctx.accounts.option_vault.key(), option_bank.vault);
    require_keys_eq!(ctx.accounts.quote_vault.key(), quote_bank.vault);

    // Get the amounts from before exercise, this is a safety to verify that the
    // StakingOptions program is properly handling the exercise.
    let bank_base_native_amount_before = ctx.accounts.base_vault.amount;
    let bank_quote_native_amount_before = ctx.accounts.quote_vault.amount;
    let bank_option_native_amount_before = ctx.accounts.option_vault.amount;

    // Do the staking options exercise CPI.
    let so_exercise_accounts = staking_options::cpi::accounts::Exercise {
        authority: ctx.accounts.so_authority.to_account_info(),
        state: ctx.accounts.staking_options_state.to_account_info(),
        user_so_account: ctx.accounts.option_vault.to_account_info(),
        option_mint: ctx.accounts.option_mint.to_account_info(),
        user_quote_account: ctx.accounts.quote_vault.to_account_info(),
        project_quote_account: ctx
            .accounts
            .staking_options_project_quote_account
            .to_account_info(),
        fee_quote_account: ctx
            .accounts
            .staking_options_fee_quote_account
            .to_account_info(),
        base_vault: ctx.accounts.staking_options_base_vault.to_account_info(),
        user_base_account: ctx.accounts.base_vault.to_account_info(),
        token_program: ctx.accounts.token_program.to_account_info(),
    };
    let cpi_program_config = ctx.accounts.staking_options_program.to_account_info();

    let group = ctx.accounts.group.load()?;
    let group_seeds = group_seeds!(group);
    staking_options::cpi::exercise(
        CpiContext::new(cpi_program_config, so_exercise_accounts).with_signer(&[group_seeds]),
        amount,
        strike,
    )?;

    // Verify that the CPI changed token amounts as expected. This protects
    // mango from a malicious change in the staking options program.
    ctx.accounts.base_vault.reload()?;
    ctx.accounts.quote_vault.reload()?;
    ctx.accounts.option_vault.reload()?;
    let bank_base_native_amount_after = ctx.accounts.base_vault.amount;
    let bank_quote_native_amount_after = ctx.accounts.quote_vault.amount;
    let bank_option_native_amount_after = ctx.accounts.option_vault.amount;
    let base_atoms_per_option = ctx.accounts.staking_options_state.lot_size;

    require!(
        bank_base_native_amount_after - bank_base_native_amount_before
            == amount * base_atoms_per_option,
        MangoError::StakingOptionsError
    );
    require!(
        bank_quote_native_amount_before - bank_quote_native_amount_after == amount * strike,
        MangoError::StakingOptionsError
    );
    require!(
        bank_option_native_amount_before - bank_option_native_amount_after == amount,
        MangoError::StakingOptionsError
    );

    // Update the banks and account token positions
    let (base_position, base_raw_index) = account.token_position_mut(base_token_index)?;
    base_bank.deposit(
        base_position,
        I80F48::from(amount * base_atoms_per_option),
        Clock::get()?.unix_timestamp.try_into().unwrap(),
    )?;
    let base_indexed_position = base_position.indexed_position;

    let (quote_position, quote_raw_index) = account.token_position_mut(quote_token_index)?;
    require!(quote_position.is_active(), MangoError::SomeError);
    let (quote_position_is_active, _quote_loan_origination_fee) = {
        quote_bank.withdraw_with_fee(
            quote_position,
            I80F48::from(amount * strike),
            Clock::get()?.unix_timestamp.try_into().unwrap(),
        )?
    };
    let quote_indexed_position = quote_position.indexed_position;
    if !quote_position_is_active {
        account.deactivate_token_position_and_log(quote_raw_index, ctx.accounts.account.key());
    }

    let (option_position, option_raw_index) = account.token_position_mut(option_token_index)?;
    require!(option_position.is_active(), MangoError::SomeError);
    // Use without_fee because no loans can happen for option bank.
    let option_position_is_active = {
        option_bank.withdraw_without_fee_with_dusting(
            option_position,
            I80F48::from(amount),
            Clock::get()?.unix_timestamp.try_into().unwrap(),
        )?
    };
    let option_indexed_position = option_position.indexed_position;
    if !option_position_is_active {
        account.deactivate_token_position_and_log(option_raw_index, ctx.accounts.account.key());
    }

    //
    // Health check after exercise. Note that health may decrease if the user
    // makes a bad exercise, but that is their choice as long as the account
    // stays healthy enough. Use the pre health because an exercise can still
    // have negative but closer to zero health.
    //
    if let Some((mut health_cache, pre_init_health)) = pre_health_opt {
        health_cache
            .adjust_token_balance(&base_bank, I80F48::from(amount * base_atoms_per_option))?;
        health_cache.adjust_token_balance(&quote_bank, -I80F48::from(amount * strike))?;
        health_cache.adjust_token_balance(&option_bank, -I80F48::from(amount))?;
        account.check_health_post(&health_cache, pre_init_health)?;
    }

    // Emit logs
    emit!(TokenBalanceLog {
        mango_group: ctx.accounts.group.key(),
        mango_account: ctx.accounts.account.key(),
        token_index: base_token_index,
        indexed_position: base_indexed_position.to_bits(),
        deposit_index: base_bank.deposit_index.to_bits(),
        borrow_index: base_bank.borrow_index.to_bits(),
    });
    emit!(TokenBalanceLog {
        mango_group: ctx.accounts.group.key(),
        mango_account: ctx.accounts.account.key(),
        token_index: quote_token_index,
        indexed_position: quote_indexed_position.to_bits(),
        deposit_index: quote_bank.deposit_index.to_bits(),
        borrow_index: quote_bank.borrow_index.to_bits(),
    });
    emit!(TokenBalanceLog {
        mango_group: ctx.accounts.group.key(),
        mango_account: ctx.accounts.account.key(),
        token_index: option_token_index,
        indexed_position: option_indexed_position.to_bits(),
        deposit_index: option_bank.deposit_index.to_bits(),
        borrow_index: option_bank.borrow_index.to_bits(),
    });
    emit!(StakingOptionExerciseLog {
        mango_group: ctx.accounts.group.key(),
        mango_account: ctx.accounts.account.key(),
        amount: amount,
        staking_options_state: ctx.accounts.staking_options_state.key(),
    });

    Ok(())
}
