// clients/src/test-batch-mint.rs
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer, Keypair},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use sha2::{Sha256, Digest};
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse number of mints (default: 10)
    let mint_count = if args.len() > 1 {
        args[1].parse().unwrap_or(10)
    } else {
        10
    };
    
    // Parse compute units (default: 200_000)
    let compute_units = if args.len() > 2 {
        args[2].parse().unwrap_or(200_000)
    } else {
        200_000
    };

    // display input information
    println!("Batch mint configuration:");
    println!("  Number of mints: {}", mint_count);
    println!("  Compute units:   {}", compute_units);
    println!();

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and token address
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
        .expect("Invalid mint address");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),  // Use token-2022 program ID
    );
    
    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    // Check if user profile exists
    let user_profile_exists = match client.get_account(&user_profile_pda) {
        Ok(_) => {
            println!("User profile found at: {}", user_profile_pda);
            println!("Mint statistics will be tracked in your profile");
            true
        },
        Err(_) => {
            println!("No user profile found. Mints will succeed but won't track statistics.");
            println!("To create a profile, use 'cargo run --bin init-user-profile <username> [profile_image_url]'");
            false
        }
    };

    // Calculate Anchor instruction sighash for process_transfer once
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let sighash_result = hasher.finalize()[..8].to_vec();

    // Start batch minting
    println!("\nStarting batch mint test with {} mints", mint_count);
    println!("Compute units per transaction: {}", compute_units);
    println!("----------------------------------------\n");

    let mut successful_mints = 0;
    let mut failed_mints = 0;
    let delay = Duration::from_secs(1); // 1 second delay between transactions

    for i in 1..=mint_count {
        println!("Processing mint #{}/{}...", i, mint_count);
        
        // Generate a unique message for each mint to track it
        let message = format!("Batch mint #{} of {}", i, mint_count);
        
        // Use a deterministic signature for testing
        let signature = format!("BatchMintSig{}", i);
        
        // Build JSON memo
        let memo_json = serde_json::json!({
            "signature": signature,
            "message": message
        });
        
        // Convert to string with compact formatting
        let memo_text = serde_json::to_string(&memo_json)
            .expect("Failed to serialize JSON");

        // Ensure memo length is at least 69 bytes
        let memo_text = ensure_min_length(memo_text, 69);
        
        // Create process_transfer instruction
        let instruction_data = sighash_result.clone();

        // Create compute budget instruction
        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
        
        // Create memo instruction
        let memo_ix = spl_memo::build_memo(
            memo_text.as_bytes(),
            &[&payer.pubkey()],
        );
        
        // Create mint instruction - include user profile account if it exists
        let mut accounts = vec![
            AccountMeta::new(payer.pubkey(), true),         // user
            AccountMeta::new(mint, false),                  // mint
            AccountMeta::new(mint_authority_pda, false),    // mint_authority (PDA)
            AccountMeta::new(token_account, false),         // token_account
            AccountMeta::new_readonly(token_2022_id(), false), // token_program (use token-2022)
            AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        ];
        
        // Add user profile account if it exists
        if user_profile_exists {
            accounts.push(AccountMeta::new(user_profile_pda, false)); // user_profile
        }
        
        let mint_ix = Instruction::new_with_bytes(
            program_id,
            &instruction_data,
            accounts,
        );

        // Get latest blockhash
        let recent_blockhash = client
            .get_latest_blockhash()
            .expect("Failed to get recent blockhash");

        // Create transaction
        let transaction = Transaction::new_signed_with_payer(
            &[compute_budget_ix, memo_ix, mint_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        // Send and confirm transaction
        match client.send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            solana_client::rpc_config::RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: None,
                encoding: None,
                max_retries: Some(3),
                min_context_slot: None,
            },
        ) {
            Ok(sig) => {
                successful_mints += 1;
                println!("Mint #{} successful: {}", i, sig);
                
                // Check current balance periodically
                if i % 10 == 0 || i == mint_count {
                    if let Ok(balance) = client.get_token_account_balance(&token_account) {
                        println!("Current token balance: {} tokens", balance.ui_amount.unwrap());
                    }
                }
            }
            Err(err) => {
                failed_mints += 1;
                println!("Mint #{} failed: {}", i, err);
                
                // Check the error type
                if err.to_string().contains("AccountNotEnoughKeys") {
                    println!("Error: Not enough account keys. Make sure to create a user profile or update the script.");
                    println!("To create a profile, use 'cargo run --bin init-user-profile <username> [profile_image_url]'");
                }
            }
        }

        // Small delay between transactions to avoid rate limiting
        if i < mint_count {
            sleep(delay);
        }
    }

    // Print summary
    println!("\n----------------------------------------");
    println!("Batch Mint Test Summary:");
    println!("Total mints attempted: {}", mint_count);
    println!("Successful mints: {}", successful_mints);
    println!("Failed mints: {}", failed_mints);
    println!("----------------------------------------");

    // Check final token balance
    if let Ok(balance) = client.get_token_account_balance(&token_account) {
        println!("Final token balance: {} tokens", balance.ui_amount.unwrap());
    }
    
    // Check user profile if it exists
    if user_profile_exists {
        println!("\nYour mint statistics have been updated in your user profile.");
        println!("To view your profile stats, run: cargo run --bin check-user-profile");
    }

    Ok(())
}

// Function to ensure minimum memo length while preserving JSON format
fn ensure_min_length(text: String, min_length: usize) -> String {
    if text.as_bytes().len() >= min_length {
        return text;
    }
    
    // Parse existing JSON
    let mut json: serde_json::Value = serde_json::from_str(&text)
        .expect("Failed to parse JSON");
    
    // Get existing message
    let message = json["message"].as_str().unwrap_or("");
    
    // Calculate padding length needed
    let current_length = text.as_bytes().len();
    let padding_needed = min_length - current_length;
    
    // Create padding with periods
    let padding = ".".repeat(padding_needed);
    
    // Update message field with padding
    let new_message = format!("{}{}", message, padding);
    json["message"] = serde_json::Value::String(new_message);
    
    // Convert back to string with compact formatting
    let result = serde_json::to_string(&json)
        .expect("Failed to serialize JSON");
    
    result
}