use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Fixed addresses
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("HgJGY6N9R1JcF7VHa6tkc7zQPWCD3ZrhuDeXFwnHnU7Y")  // Get from create_token output
        .expect("Invalid mint address");

    // Calculate PDA (for information only)
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account - Note we're using the Token-2022 program ID
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &spl_token_2022::id(), // Use Token-2022 program ID
    );

    // Create token account if it doesn't exist
    let account_result = client.get_account(&token_account).await;
    if account_result.is_err() {
        println!("Creating Token-2022 associated token account...");
        
        let create_token_account_ix = create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
            &spl_token_2022::id(), // Use Token-2022 program ID
        );

        let recent_blockhash = client
            .get_latest_blockhash()
            .await?;

        let transaction = Transaction::new_signed_with_payer(
            &[create_token_account_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let signature = client
            .send_and_confirm_transaction(&transaction)
            .await?;

        println!("Token-2022 account created successfully: {}", signature);
    } else {
        println!("Token-2022 account already exists");
    }

    // Print account info
    println!("\nAccount Info:");
    println!("Program ID: {}", program_id);
    println!("Mint: {}", mint);
    println!("Mint Authority (PDA): {}", mint_authority_pda);
    println!("Your wallet: {}", payer.pubkey());
    println!("Your Token-2022 account: {}", token_account);
    
    Ok(())
} 