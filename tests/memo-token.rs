use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_program;
use anchor_spl::token::{self, TokenAccount};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

#[tokio::test]
async fn test_memo_token() {
    // Initialize program test environment
    let program_id = memo_token::id();
    let mut program_test = ProgramTest::new(
        "memo_token",
        program_id,
        processor!(memo_token::entry),
    );

    // Start the test
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Create test user
    let user = Keypair::new();
    
    // Get mint address (from deploy script output)
    let mint = Pubkey::from_str("your_mint_address_here").unwrap();

    // Create user's token account
    let user_token_account = Keypair::new();
    {
        let rent = banks_client.get_rent().await.unwrap();
        let token_account_rent = rent.minimum_balance(TokenAccount::LEN);

        let mut transaction = Transaction::new_with_payer(
            &[
                system_instruction::create_account(
                    &payer.pubkey(),
                    &user_token_account.pubkey(),
                    token_account_rent,
                    TokenAccount::LEN as u64,
                    &token::ID,
                ),
                token::instruction::initialize_account(
                    &token::ID,
                    &user_token_account.pubkey(),
                    &mint,
                    &user.pubkey(),
                )
                .unwrap(),
            ],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer, &user_token_account], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .expect("Failed to create token account");
    }

    // Test minting
    {
        let mut transaction = Transaction::new_with_payer(
            &[memo_token::instruction::process_transfer(
                program_id,
                mint,
                user_token_account.pubkey(),
                user.pubkey(),
            )],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer, &user], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .expect("Failed to mint token");
    }

    // Verify token balance
    let token_account = banks_client
        .get_account(user_token_account.pubkey())
        .await
        .expect("Failed to get token account")
        .expect("Token account does not exist");

    let token_account = TokenAccount::unpack(&token_account.data)
        .expect("Failed to parse token account");

    assert_eq!(token_account.amount, 1);
}