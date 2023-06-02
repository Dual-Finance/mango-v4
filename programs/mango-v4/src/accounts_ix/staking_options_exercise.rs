/// Exercises a staking option inside of a user's account.
///
/// Takes tokens from the banks for quote and options and calls a CPI into
/// StakingOptions program to get tokens and deposit them in the base bank.
/// Does health check to verify that this exercise is ITM or at least does not
/// drop health past maint threshold.
use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::Token;
use anchor_spl::token::TokenAccount;
use staking_options::program::StakingOptions as StakingOptionsProgram;

#[derive(Accounts)]
pub struct StakingOptionsExercise<'info> {
    #[account(
        constraint = group.load()?.is_ix_enabled(IxGate::StakingOptionsExercise) @ MangoError::IxIsDisabled,
    )]
    pub group: AccountLoader<'info, Group>,

    #[account(
        mut,
        has_one = group,
        constraint = account.load()?.is_operational() @ MangoError::AccountIsFrozen
    )]
    pub account: AccountLoader<'info, MangoAccountFixed>,
    pub owner: Signer<'info>,

    /// Accounts for the CPI into StakingOptions.
    /// CHECK: cpi
    pub so_authority: AccountInfo<'info>,
    #[account(mut)]
    pub staking_options_state: Box<Account<'info, staking_options::State>>,

    #[account(mut)]
    pub option_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    /// CHECK: cpi
    pub option_mint: AccountInfo<'info>,
    #[account(mut)]
    pub quote_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    /// CHECK: cpi
    pub staking_options_project_quote_account: AccountInfo<'info>,
    #[account(mut)]
    /// CHECK: cpi
    pub staking_options_fee_quote_account: AccountInfo<'info>,
    #[account(mut)]
    /// CHECK: cpi
    pub staking_options_base_vault: AccountInfo<'info>,
    #[account(mut)]
    pub base_vault: Account<'info, TokenAccount>,

    #[account(mut, has_one = group)]
    pub base_bank: AccountLoader<'info, Bank>,
    #[account(mut, has_one = group)]
    pub quote_bank: AccountLoader<'info, Bank>,
    #[account(mut, has_one = group)]
    pub option_bank: AccountLoader<'info, Bank>,

    pub token_program: Program<'info, Token>,
    pub staking_options_program: Program<'info, StakingOptionsProgram>,
}
