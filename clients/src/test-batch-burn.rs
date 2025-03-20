use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer, Keypair},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use sha2::{Sha256, Digest};
use serde_json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse number of burns (default: 80)
    let burn_count = if args.len() > 1 {
        args[1].parse().unwrap_or(80)
    } else {
        80
    };
    
    // Parse burn amount per transaction (default: 0.01 tokens)
    let burn_amount = if args.len() > 2 {
        args[2].parse::<f64>().unwrap_or(0.01) * 1_000_000_000.0 as f64
    } else {
        0.01 * 1_000_000_000.0 // 0.01 tokens in lamports
    } as u64;

    // Parse compute units (default: 200_000)
    let compute_units = if args.len() > 3 {
        args[3].parse().unwrap_or(200_000)
    } else {
        200_000
    };

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
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")
        .expect("Invalid mint address");

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Calculate PDAs
    let (global_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_burn_index"],
        &program_id,
    );
    
    let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id,
    );

    // Check if shard exists
    match client.get_account(&latest_burn_shard_pda) {
        Ok(_) => {
            println!("Found burn shard");
        },
        Err(_) => {
            println!("Warning: Burn shard does not exist.");
            println!("The transaction may fail. Please initialize the shard first using init-latest-burn-shard.");
            println!("Continue anyway? (y/n)");
            
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                return Ok(());
            }
        }
    }

    // Check token balance
    let balance = client.get_token_account_balance(&token_account)?;
    let token_balance = balance.ui_amount.unwrap();
    let required_tokens = (burn_amount as f64 * burn_count as f64) / 1_000_000_000.0;
    
    println!("Current token balance: {} tokens", token_balance);
    println!("Required tokens for {} burns: {:.6} tokens", burn_count, required_tokens);
    
    if token_balance < required_tokens {
        println!("Warning: Insufficient token balance for all burns.");
        println!("You need at least {:.6} tokens but have {:.6} tokens.", required_tokens, token_balance);
        println!("Continue anyway? (This will burn as many tokens as possible) (y/n)");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    // Calculate Anchor instruction sighash for process_burn once
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn");
    let sighash_result = hasher.finalize()[..8].to_vec();

    // Start batch burning
    println!("\nStarting batch burn test with {} burns of {:.6} tokens each", 
            burn_count, (burn_amount as f64) / 1_000_000_000.0);
    println!("Compute units per transaction: {}", compute_units);
    println!("----------------------------------------\n");

    let mut successful_burns = 0;
    let mut failed_burns = 0;
    let delay = Duration::from_secs(1); // 1 second delay between transactions

    for i in 1..=burn_count {
        println!("Processing burn #{}/{}...", i, burn_count);
        
        // Generate a unique message for each burn to track it
        let message = format!("Batch burn #{} of {}", i, burn_count);
        
        // Use a deterministic signature for testing
        let signature = format!("BatchBurnSig{}", i);
        
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
        
        // Create burn instruction data
        let mut instruction_data = sighash_result.clone();
        instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

        // Create compute budget instruction
        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
        
        // Create memo instruction
        let memo_ix = spl_memo::build_memo(
            memo_text.as_bytes(),
            &[&payer.pubkey()],
        );
        
        // Create burn instruction
        let burn_ix = Instruction::new_with_bytes(
            program_id,
            &instruction_data,
            vec![
                AccountMeta::new(payer.pubkey(), true),         // user
                AccountMeta::new(mint, false),                  // mint
                AccountMeta::new(token_account, false),         // token_account
                AccountMeta::new_readonly(spl_token::id(), false), // token_program
                AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
                AccountMeta::new(latest_burn_shard_pda, false), // latest burn shard
            ],
        );

        // Get latest blockhash
        let recent_blockhash = client
            .get_latest_blockhash()
            .expect("Failed to get recent blockhash");

        // Create transaction
        let transaction = Transaction::new_signed_with_payer(
            &[compute_budget_ix, memo_ix, burn_ix],
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
                successful_burns += 1;
                println!("Burn #{} successful: {}", i, sig);
                
                // Check remaining balance periodically
                if i % 10 == 0 || i == burn_count {
                    if let Ok(balance) = client.get_token_account_balance(&token_account) {
                        println!("Current token balance: {} tokens", balance.ui_amount.unwrap());
                    }
                }
            }
            Err(err) => {
                failed_burns += 1;
                println!("Burn #{} failed: {}", i, err);
                
                // If we're out of tokens, stop
                if err.to_string().contains("insufficient funds") {
                    println!("Insufficient funds to continue. Stopping batch burn.");
                    break;
                }
            }
        }

        // Small delay between transactions to avoid rate limiting
        if i < burn_count {
            sleep(delay);
        }
    }

    // Print summary
    println!("\n----------------------------------------");
    println!("Batch Burn Test Summary:");
    println!("Total burns attempted: {}", burn_count);
    println!("Successful burns: {}", successful_burns);
    println!("Failed burns: {}", failed_burns);
    println!("Tokens burned: {:.6}", (successful_burns as f64 * burn_amount as f64) / 1_000_000_000.0);
    println!("----------------------------------------");

    // Check latest burn shard state
    println!("\nChecking latest burn shard state...");
    match client.get_account(&latest_burn_shard_pda) {
        Ok(account) => {
            println!("Latest burn shard has {} bytes of data", account.data.len());
            println!("To view details, run: cargo run --bin check-latest-burn-shard");
        },
        Err(err) => {
            println!("Failed to get latest burn shard account: {}", err);
        }
    }

    println!("\nTest completed. You should verify:");
    println!("1. Only the most recent 69 records are retained in the shard");
    println!("2. The current_index has wrapped around correctly");
    println!("3. The record_count is updated correctly in the global burn index");

    Ok(())
}

// Keep JSON format and ensure minimum length
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
    
    // Create padding with spaces
    let padding = " ".repeat(padding_needed);
    
    // Update message field with padding
    let new_message = format!("{}{}", message, padding);
    json["message"] = serde_json::Value::String(new_message);
    
    // Convert back to string with compact formatting
    let result = serde_json::to_string(&json)
        .expect("Failed to serialize JSON");
    
    result
}
