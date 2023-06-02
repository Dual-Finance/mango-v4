#![allow(dead_code)]

use std::sync::Arc;

use anchor_lang::prelude::System;
use anchor_lang::Id;
use anchor_spl::token::Token;
use solana_program::sysvar::rent::Rent;
use solana_program::sysvar::SysvarId;
use solana_sdk::instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;

use super::*;

#[derive(Clone, Debug)]
pub struct StakingOptionsStateCookie {
    pub state_address: Pubkey,
    pub base_vault: Pubkey,
    pub project_base_account: Pubkey,
    pub project_quote_account: Pubkey,
    pub base_mint_key: Pubkey,
    pub quote_mint_key: Pubkey,
    pub option_mint_key: Pubkey,
    pub fee_quote_account: Pubkey,
}

pub struct StakingOptionsCookie {
    pub solana: Arc<solana::SolanaCookie>,
    pub program_id: Pubkey,
}

impl StakingOptionsCookie {
    pub async fn create_staking_options(&self) -> StakingOptionsStateCookie {
        // For simplicity, the authority over all the mints and the staking options is the context payer.
        let authority = self.solana.context.borrow().payer.pubkey();
        let base_mint_key = self.solana.create_mint(&authority).await;
        let quote_mint_key = self.solana.create_mint(&authority).await;

        let project_base_account = self
            .solana
            .create_token_account(&authority, base_mint_key)
            .await;
        let project_quote_account = self
            .solana
            .create_token_account(&authority, quote_mint_key)
            .await;

        let dual_dao = Pubkey::from_str("7Z36Efbt7a4nLiV7s5bY7J2e4TJ6V9JEKGccsy2od2bE").unwrap();
        let fee_quote_account = self
            .solana
            .create_token_account(&dual_dao, quote_mint_key)
            .await;

        let instructions = [spl_token::instruction::mint_to(
            &spl_token::id(),
            &base_mint_key,
            &project_base_account,
            &authority,
            &[&authority],
            1_000_000_000_000,
        )
        .unwrap()];

        self.solana
            .process_transaction(&instructions, None)
            .await
            .unwrap();

        let name = "SO_STATE";
        // Initialize staking options state with expiration in the next hour.
        let (state_address, _state_address_bump) = Pubkey::find_program_address(
            &[b"so-config", name.as_bytes(), &base_mint_key.to_bytes()],
            &staking_options::id(),
        );
        let (base_vault, _base_vault_bump) = Pubkey::find_program_address(
            &[b"so-vault", name.as_bytes(), &base_mint_key.to_bytes()],
            &staking_options::id(),
        );

        // 1 hour from now.
        let expiration = self.solana.get_clock().await.unix_timestamp + 60 * 60 * 1;
        let configure_so_data = staking_options::instruction::ConfigV2 {
            option_expiration: expiration as u64,
            subscription_period_end: expiration as u64,
            num_tokens: 1_000_000_000_000,
            lot_size: 1_000_000,
            so_name: name.to_string(),
        };
        let configure_so_accounts = staking_options::accounts::ConfigV2 {
            authority: authority,
            so_authority: authority,
            issue_authority: None,
            state: state_address,
            base_vault: base_vault,
            base_account: project_base_account,
            quote_account: project_quote_account,
            base_mint: base_mint_key,
            quote_mint: quote_mint_key,
            token_program: Token::id(),
            system_program: System::id(),
            rent: Rent::id(),
        };

        let instruction = instruction::Instruction {
            program_id: staking_options::id(),
            accounts: anchor_lang::ToAccountMetas::to_account_metas(&configure_so_accounts, None),
            data: anchor_lang::InstructionData::data(&configure_so_data),
        };

        self.solana
            .process_transaction(&[instruction], None)
            .await
            .unwrap();

        let strike: u64 = 1_000_000;
        let (option_mint_key, _option_mint_key_bump) = Pubkey::find_program_address(
            &[b"so-mint", &state_address.to_bytes(), &strike.to_be_bytes()],
            &staking_options::id(),
        );

        let init_strike_data = staking_options::instruction::InitStrike { strike: strike };
        let init_strike_accounts = staking_options::accounts::InitStrike {
            authority: authority,
            state: state_address,
            option_mint: option_mint_key,
            token_program: Token::id(),
            system_program: System::id(),
            rent: Rent::id(),
        };

        let instruction = instruction::Instruction {
            program_id: staking_options::id(),
            accounts: anchor_lang::ToAccountMetas::to_account_metas(&init_strike_accounts, None),
            data: anchor_lang::InstructionData::data(&init_strike_data),
        };
        self.solana
            .process_transaction(&[instruction], None)
            .await
            .unwrap();

        StakingOptionsStateCookie {
            state_address: state_address,
            base_vault: base_vault,
            project_base_account: project_base_account,
            project_quote_account: project_quote_account,
            base_mint_key: base_mint_key,
            quote_mint_key: quote_mint_key,
            option_mint_key: option_mint_key,
            fee_quote_account: fee_quote_account,
        }
    }
}
