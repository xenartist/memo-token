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
    // 初始化程序测试环境
    let program_id = memo_token::id();
    let mut program_test = ProgramTest::new(
        "memo_token",
        program_id,
        processor!(memo_token::entry),
    );

    // 开始测试
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // 创建测试用户
    let user = Keypair::new();
    
    // 计算 PDA
    let (mint_pda, bump) = Pubkey::find_program_address(
        &[b"memo_mint"],
        &program_id,
    );

    // 测试初始化 mint
    {
        let mut transaction = Transaction::new_with_payer(
            &[memo_token::instruction::initialize_mint(
                program_id,
                payer.pubkey(),
                mint_pda,
            )],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .expect("初始化 mint 失败");
    }

    // 创建用户的代币账户
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
                    &mint_pda,
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
            .expect("创建代币账户失败");
    }

    // 测试 mint 代币
    {
        let mut transaction = Transaction::new_with_payer(
            &[memo_token::instruction::mint_token(
                program_id,
                mint_pda,
                user_token_account.pubkey(),
                user.pubkey(),
            )],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer, &user], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .expect("Mint 代币失败");
    }

    // 验证代币余额
    let token_account = banks_client
        .get_account(user_token_account.pubkey())
        .await
        .expect("获取代币账户失败")
        .expect("代币账户不存在");

    let token_account = TokenAccount::unpack(&token_account.data)
        .expect("解析代币账户失败");

    assert_eq!(token_account.amount, 1);
}