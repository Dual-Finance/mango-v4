/// Liquidates an ITM staking option in a user's account in the last 1 hour.
///
/// Any liquidator can call this and receive a fee. This protects the system
/// from allowing ITM options to expire unexercised which is a sudden 1->0
/// health drop. To address this, the liquidator gets to pretend that health
/// drop has already happened and liquidate, while the option is still not yet
/// expired and exercisable for value.  This is similar to token_liq_with_token
/// except the circumstances and health thresholds.
use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct StakingOptionsLiq<'info> {
    #[account(
        constraint = group.load()?.is_ix_enabled(IxGate::StakingOptionsLiq) @ MangoError::IxIsDisabled,
    )]
    pub group: AccountLoader<'info, Group>,

    #[account(
        mut,
        has_one = group,
        constraint = liqor.load()?.is_operational() @ MangoError::AccountIsFrozen
    )]
    pub liqor: AccountLoader<'info, MangoAccountFixed>,
    pub liqor_owner: Signer<'info>,

    #[account(
        mut,
        has_one = group,
        constraint = liqee.load()?.is_operational() @ MangoError::AccountIsFrozen
    )]
    pub liqee: AccountLoader<'info, MangoAccountFixed>,
}
