use super::*;
use anchor_lang::Id;
use anchor_spl::token::Token;
use solana_sdk::instruction;
use solana_sdk::signature::Signer;

#[tokio::test]
async fn test_staking_options_exercise() -> Result<(), TransportError> {
    let mut test_builder = TestContextBuilder::new();
    test_builder.test().set_compute_max_units(170_000); // StakingOptionsExercise needs 162k
    let context = test_builder.start_default().await;
    let solana = &context.solana.clone();
    let initial_token_deposit = 1_000_000;

    let admin = TestKeypair::new();
    let owner = context.users[0].key;
    let payer = context.users[1].key;

    let mut user = context.users[1].clone();

    //
    // SETUP: Create staking options state and mints associated with it.
    //
    let staking_options_state_cookie = context.staking_options.create_staking_options().await;

    let mint_authority = solana.context.borrow().payer.pubkey();

    let user_quote_account = solana
        .create_token_account(&payer.pubkey(), staking_options_state_cookie.quote_mint_key)
        .await;
    let user_base_account = solana
        .create_token_account(&payer.pubkey(), staking_options_state_cookie.base_mint_key)
        .await;
    let user_option_account = solana
        .create_token_account(
            &payer.pubkey(),
            staking_options_state_cookie.option_mint_key,
        )
        .await;
    user.token_accounts.push(user_quote_account);
    user.token_accounts.push(user_base_account);
    user.token_accounts.push(user_option_account);

    solana
        .mint_to(
            &staking_options_state_cookie.quote_mint_key,
            &user_quote_account,
            initial_token_deposit,
        )
        .await;
    solana
        .mint_to(
            &staking_options_state_cookie.base_mint_key,
            &user_base_account,
            initial_token_deposit,
        )
        .await;

    // Amount is in tokens, but gets converted to lots, so multiply by lot size.
    let issue_so_data = staking_options::instruction::Issue {
        amount: initial_token_deposit * 1_000_000,
        strike: 1_000_000,
    };
    let issue_so_accounts = staking_options::accounts::Issue {
        authority: mint_authority,
        state: staking_options_state_cookie.state_address,
        option_mint: staking_options_state_cookie.option_mint_key,
        user_so_account: user_option_account,
        token_program: Token::id(),
    };

    let issue_so_instruction = instruction::Instruction {
        program_id: staking_options::id(),
        accounts: anchor_lang::ToAccountMetas::to_account_metas(&issue_so_accounts, None),
        data: anchor_lang::InstructionData::data(&issue_so_data),
    };

    solana
        .process_transaction(&[issue_so_instruction], None)
        .await
        .unwrap();

    // Authority isnt used, so fill with a default value.
    let mints = vec![
        MintCookie {
            index: 10,
            decimals: 6,
            unit: 10u64.pow(6) as f64,
            base_lot: 100 as f64,
            quote_lot: 10 as f64,
            pubkey: staking_options_state_cookie.quote_mint_key,
            authority: TestKeypair::default(),
        }, // Quote
        MintCookie {
            index: 11,
            decimals: 6,
            unit: 10u64.pow(6) as f64,
            base_lot: 100 as f64,
            quote_lot: 10 as f64,
            pubkey: staking_options_state_cookie.base_mint_key,
            authority: TestKeypair::new(),
        }, // Base
        MintCookie {
            index: 12,
            decimals: 6,
            unit: 10u64.pow(6) as f64,
            base_lot: 100 as f64,
            quote_lot: 10 as f64,
            pubkey: staking_options_state_cookie.option_mint_key,
            authority: TestKeypair::new(),
        }, // StakingOption
    ];

    //
    // SETUP: Create a group and an account. Also initialize oracles and
    // insurance vault.
    //
    let GroupWithTokens { group, tokens, .. } = GroupWithTokensConfig {
        admin,
        payer,
        mints: (&mints[..]).to_vec(),
        ..GroupWithTokensConfig::default()
    }
    .create(solana)
    .await;

    send_tx(
        solana,
        TokenMakeReduceOnly {
            admin,
            group,
            mint: mints[2].pubkey,
            reduce_only: 2,
            force_close: false,
        },
    )
    .await
    .unwrap();

    let now = solana.get_clock().await.unix_timestamp as u64;
    send_tx(
        solana,
        TokenEditStakingOptions {
            group,
            admin,
            mint: mints[2].pubkey,
            staking_options_state: staking_options_state_cookie.state_address,
            staking_options_expiration: now + 60 * 10,
        },
    )
    .await
    .unwrap();

    //
    // SETUP: Prepare mango accounts
    //
    let account_0 = send_tx(
        &solana,
        AccountCreateInstruction {
            account_num: 0,
            token_count: 16,
            serum3_count: 8,
            perp_count: 8,
            perp_oo_count: 8,
            group,
            owner,
            payer: payer,
        },
    )
    .await
    .unwrap()
    .account;

    for mint in &mints[..] {
        send_tx(
            solana,
            TokenDepositInstruction {
                amount: initial_token_deposit,
                reduce_only: false,
                account: account_0,
                owner,
                token_account: user.token_accounts[mint.index],
                token_authority: user.key,
                bank_index: 0,
            },
        )
        .await
        .unwrap();
    }

    send_tx(
        solana,
        StakingOptionsExerciseInstruction {
            amount: 1,
            strike: 1_000_000,
            group: group,
            account: account_0,
            owner: owner,
            so_authority: mint_authority,
            staking_options_state: staking_options_state_cookie.state_address,
            option_mint: staking_options_state_cookie.option_mint_key,
            quote_mint: staking_options_state_cookie.quote_mint_key,
            staking_options_project_quote_account: staking_options_state_cookie
                .project_quote_account,
            staking_options_fee_quote_account: staking_options_state_cookie.fee_quote_account,
            staking_options_base_vault: staking_options_state_cookie.base_vault,
            base_mint: staking_options_state_cookie.base_mint_key,
            quote_bank: tokens[0].bank,
            base_bank: tokens[1].bank,
            option_bank: tokens[2].bank,
        },
    )
    .await
    .unwrap();

    let mango_account = solana.get_account::<MangoAccount>(account_0).await;
    let bank = solana.get_account::<Bank>(tokens[0].bank).await;
    // All the quote is used.
    assert_eq!(mango_account.tokens[0].native(&bank).to_num::<u64>(), 0);
    // Paid the base tokens from the exercise.
    assert_eq!(
        mango_account.tokens[1].native(&bank).to_num::<u64>(),
        initial_token_deposit * 2
    );
    // Use 1 option.
    assert_eq!(
        mango_account.tokens[2].native(&bank).to_num::<u64>(),
        initial_token_deposit - 1
    );

    Ok(())
}

// Note that because the liquidation does not interact with the staking options
// program, do not actually need to make real staking options, just TokenEdit so
// that the bank believes that it has staking options.
#[tokio::test]
async fn test_staking_options_liq() -> Result<(), TransportError> {
    let mut test_builder = TestContextBuilder::new();
    test_builder.test().set_compute_max_units(85_000); // StakingOptionsLiq needs 79k
    let context = test_builder.start_default().await;
    let solana = &context.solana.clone();

    let admin = TestKeypair::new();
    let owner = context.users[0].key;
    let payer = context.users[1].key;
    let mints = &context.mints[0..2];
    let payer_mint_accounts = &context.users[1].token_accounts[0..2];

    //
    // SETUP: Create a group and an account to fill the vaults
    //

    let mango_setup::GroupWithTokens { group, tokens, .. } = mango_setup::GroupWithTokensConfig {
        admin,
        payer,
        mints: mints.to_vec(),
        ..GroupWithTokensConfig::default()
    }
    .create(solana)
    .await;
    let borrow_token1 = &tokens[0];
    let collateral_token1 = &tokens[1];

    // deposit some funds, to the vaults aren't empty
    let liquor = send_tx(
        solana,
        AccountCreateInstruction {
            account_num: 2,
            token_count: 16,
            serum3_count: 8,
            perp_count: 8,
            perp_oo_count: 8,
            group,
            owner,
            payer,
        },
    )
    .await
    .unwrap()
    .account;
    for &token_account in payer_mint_accounts {
        send_tx(
            solana,
            TokenDepositInstruction {
                amount: 100000,
                reduce_only: false,
                account: liquor,
                owner,
                token_account,
                token_authority: payer.clone(),
                bank_index: 0,
            },
        )
        .await
        .unwrap();
    }

    //
    // SETUP: Make an account with some collateral and some borrows
    //
    let liqee = send_tx(
        solana,
        AccountCreateInstruction {
            account_num: 0,
            token_count: 16,
            serum3_count: 8,
            perp_count: 8,
            perp_oo_count: 8,
            group,
            owner,
            payer,
        },
    )
    .await
    .unwrap()
    .account;

    let deposit1_amount = 1000;
    send_tx(
        solana,
        TokenDepositInstruction {
            amount: deposit1_amount,
            reduce_only: false,
            account: liqee,
            owner,
            token_account: payer_mint_accounts[1],
            token_authority: payer.clone(),
            bank_index: 0,
        },
    )
    .await
    .unwrap();

    let borrow1_amount = 350;
    send_tx(
        solana,
        TokenWithdrawInstruction {
            amount: borrow1_amount,
            allow_borrow: true,
            account: liqee,
            owner,
            token_account: payer_mint_accounts[0],
            bank_index: 0,
        },
    )
    .await
    .unwrap();

    //
    // SETUP: Do not need to change oracle price since health contribution goes
    // to zero. Setup the staking options. It must be first made into reduce only.
    //
    send_tx(
        solana,
        TokenMakeReduceOnly {
            admin,
            group,
            mint: mints[1].pubkey,
            reduce_only: 2,
            force_close: false,
        },
    )
    .await
    .unwrap();

    let now = solana.get_clock().await.unix_timestamp as u64;
    send_tx(
        solana,
        TokenEditStakingOptions {
            group,
            admin,
            mint: mints[1].pubkey,
            staking_options_state: Pubkey::new_unique(),
            staking_options_expiration: now + 60 * 10,
        },
    )
    .await
    .unwrap();

    //
    // TEST: StakingOptionsLiq
    //
    send_tx(
        solana,
        StakingOptionsLiqInstruction {
            liqee: liqee,
            liqor: liquor,
            liqor_owner: owner,
            asset_token_index: collateral_token1.index,
            liab_token_index: borrow_token1.index,
            asset_bank_index: 0,
            liab_bank_index: 0,
            max_liab_transfer: I80F48::from_num(1000.0),
        },
    )
    .await
    .unwrap();

    // No more borrow. That got dusted so only a collateral position remains.
    let liqee_account = get_mango_account(solana, liqee).await;
    assert_eq!(liqee_account.active_token_positions().count(), 1);
    // 1000 - 350 * fee = 643
    assert_eq!(
        account_position(solana, liqee, collateral_token1.bank).await,
        643
    );

    assert_eq!(
        account_position(solana, liquor, borrow_token1.bank).await,
        99650
    );
    assert_eq!(
        account_position(solana, liquor, collateral_token1.bank).await,
        100357
    );

    Ok(())
}
