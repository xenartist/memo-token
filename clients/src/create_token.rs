use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Keypair, Signer},
    pubkey::Pubkey,
    system_instruction,
    transaction::Transaction,
    program_pack::Pack,
};
use spl_token::instruction as token_instruction;
use std::str::FromStr;

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

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

    // Calculate rent
    let mint_len = spl_token::state::Mint::LEN;
    let mint_rent = client
        .get_minimum_balance_for_rent_exemption(mint_len)
        .expect("Failed to get rent exemption");

    // Create mint account
    let create_mint_account_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_keypair.pubkey(),
        mint_rent,
        mint_len as u64,
        &spl_token::id(),
    );

    // Initialize mint with PDA as mint authority
    let init_mint_ix = token_instruction::initialize_mint(
        &spl_token::id(),
        &mint_keypair.pubkey(),
        &mint_authority_pda,  // Use PDA as mint authority
        None,                 // No freeze authority
        9,                   // 9 decimals
    ).unwrap();

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[create_mint_account_ix, init_mint_ix],
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("\nToken created successfully!");
            println!("Transaction signature: {}", signature);
            println!("\nToken Info:");
            println!("Program ID: {}", program_id);
            println!("Mint address: {}", mint_keypair.pubkey());
            println!("Mint authority (PDA): {}", mint_authority_pda);
            println!("\nSave these addresses for future use!");
        }
        Err(e) => {
            println!("Error creating token: {}", e);
        }
    }
} 