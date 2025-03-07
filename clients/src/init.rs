use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use spl_token_2022::id;

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Fixed addresses
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("H5UtbVueFsiLk5pg9cD8jp8p4TanBX9dr83Q3SLEKRNw")  // Get from create_token output
        .expect("Invalid mint address");

    // Calculate PDA (for information only)
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Create token account if it doesn't exist
    if client.get_account(&token_account).is_err() {
        println!("Creating token account...");
        
        let create_token_account_ix = 
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint,
                &id(),
            );

        let recent_blockhash = client
            .get_latest_blockhash()
            .expect("Failed to get recent blockhash");

        let transaction = Transaction::new_signed_with_payer(
            &[create_token_account_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        client
            .send_and_confirm_transaction(&transaction)
            .expect("Failed to create token account");

        println!("Token account created successfully");
    } else {
        println!("Token account already exists");
    }

    // Print account info
    println!("\nAccount Info:");
    println!("Program ID: {}", program_id);
    println!("Mint: {}", mint);
    println!("Mint Authority (PDA): {}", mint_authority_pda);
    println!("Your wallet: {}", payer.pubkey());
    println!("Your token account: {}", token_account);
} 