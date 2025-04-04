// clients/src/close-user-profile.rs
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    println!("User: {}", payer.pubkey());
    println!("User profile PDA: {}", user_profile_pda);
    
    // Check if user profile exists
    match client.get_account(&user_profile_pda) {
        Ok(account) => {
            println!("Found user profile at: {}", user_profile_pda);
            println!("Account rent-exempt balance: {} lamports", account.lamports);
            println!("Note: This will permanently delete your profile, including your pixel art!");
            println!("Are you sure you want to continue? (y/n)");

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Operation cancelled.");
                return Ok(());
            }
            
            // Create instruction data
            let mut instruction_data = Vec::new();
            
            // Note: You'll need to replace this with the actual discriminator from your compiled program
            // For 'close_user_profile', this will be the first 8 bytes of the SHA256 hash of "global:close_user_profile"
            let data = vec![242,80,248,79,81,251,65,113]; // Example discriminator
            instruction_data.extend_from_slice(&data);
            
            // Create the close instruction
            let accounts = vec![
                AccountMeta::new(payer.pubkey(), true),        // user
                AccountMeta::new(user_profile_pda, false),     // user_profile
                AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
            ];
            
            let close_ix = Instruction::new_with_bytes(
                program_id,
                &instruction_data,
                accounts,
            );
            
            // Get recent blockhash
            let recent_blockhash = client.get_latest_blockhash()?;
            
            // Create and send transaction
            let transaction = Transaction::new_signed_with_payer(
                &[close_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );
            
            println!("Sending transaction to close user profile...");
            
            // Send transaction with spinner
            let signature = client.send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::confirmed(),
                solana_client::rpc_config::RpcSendTransactionConfig {
                    skip_preflight: false,
                    preflight_commitment: None,
                    encoding: None,
                    max_retries: Some(5),
                    min_context_slot: None,
                },
            )?;
            
            println!("User profile closed successfully!");
            println!("Transaction signature: {}", signature);
            println!("The SOL from this account has been returned to your wallet.");
        },
        Err(_) => {
            println!("No user profile found for this wallet.");
            println!("There is nothing to close.");
        }
    }
    
    Ok(())
}