use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, program_pack::Pack, pubkey::Pubkey, signature::{read_keypair_file, Keypair, Signer}, system_instruction, transaction::Transaction
};
use spl_token_2022::state::Mint;
use spl_token_2022::instruction as token_instruction;
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

    // Program ID
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap")
        .expect("Invalid program ID");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    println!("Mint authority PDA: {}", mint_authority_pda);

    // Create new mint account
    let mint_keypair = Keypair::new();
    println!("New token mint address: {}", mint_keypair.pubkey());

    // Calculate rent - Token-2022 mint accounts may need more space for extensions
    let mint_len = Mint::LEN;
    let mint_rent = client
        .get_minimum_balance_for_rent_exemption(mint_len)
        .await?;

    // Create mint account - Note we're using the Token-2022 program ID
    let create_mint_account_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_keypair.pubkey(),
        mint_rent,
        mint_len as u64,
        &spl_token_2022::id(), // Use Token-2022 program ID
    );

    // Initialize mint with PDA as mint authority
    let init_mint_ix = token_instruction::initialize_mint(
        &spl_token_2022::id(), // Use Token-2022 program ID
        &mint_keypair.pubkey(),
        &mint_authority_pda,  // Use PDA as mint authority
        None,                 // No freeze authority
        9,                   // 9 decimals
    )?;

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .await?;

    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[create_mint_account_ix, init_mint_ix],
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        recent_blockhash,
    );

    let signature = client
        .send_and_confirm_transaction(&transaction)
        .await?;

    println!("Transaction signature: {}", signature);
    println!("Token-2022 mint created successfully: {}", mint_keypair.pubkey());
    
    // Optional: Add token extensions here if needed
    // For example, you could add metadata, transfer fees, etc.
    
    // Example of how to add metadata extension (commented out)
    // let metadata_ix = spl_token_2022::extension::metadata::instruction::initialize(
    //     &spl_token_2022::id(),
    //     &mint_keypair.pubkey(),
    //     &mint_authority_pda,    
    //     "Memo Token".to_string(),
    //     "MEMO".to_string(),
    //     "https://example.com/logo.png".to_string(),
    //     None, // No additional metadata
    // ).unwrap();
    
    // let metadata_tx = Transaction::new_signed_with_payer(
    //     &[metadata_ix],
    //     Some(&payer.pubkey()),
    //     &[&payer],
    //     client.get_latest_blockhash().await.unwrap(),
    // );
    
    // client.send_and_confirm_transaction(&metadata_tx).await.unwrap();

    Ok(())
} 