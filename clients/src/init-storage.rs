use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
    rent::Rent,
};
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
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate storage PDA
    let (storage_pda, _bump) = Pubkey::find_program_address(
        &[b"test_onchain_storage"],
        &program_id,
    );

    println!("Storage PDA: {}", storage_pda);

    // calculate required space (8 bytes discriminator + 32 bytes pubkey)
    let space = 8 + 32;
    
    // calculate required lamports for rent exemption
    let rent = client.get_minimum_balance_for_rent_exemption(space)
        .expect("Failed to get rent exemption");

    println!("Required lamports for rent exemption: {} SOL", rent as f64 / 1_000_000_000.0);

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),      // payer (writable, signer)
        AccountMeta::new(storage_pda, false),        // storage account (writable, but NOT signer)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // Initialize storage instruction (correct discriminator from IDL)
    let data = vec![190, 129, 110, 149, 188, 153, 142, 131];  // Anchor discriminator for 'initialize_storage'

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    // Create and send transaction
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],  // only payer signature
        recent_blockhash,
    );

    println!("Sending transaction to initialize storage...");

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Storage initialized successfully!");
            println!("Transaction signature: {}", signature);
            
            // Print account info
            println!("\nStorage Account Info:");
            println!("Program ID: {}", program_id);
            println!("Storage PDA: {}", storage_pda);
            println!("Your wallet (payer): {}", payer.pubkey());
        }
        Err(err) => {
            println!("Failed to initialize storage: {}", err);
            if err.to_string().contains("already in use") {
                println!("Note: This error might mean the storage account is already initialized, which is normal if you've run this before.");
            }
        }
    }
}
