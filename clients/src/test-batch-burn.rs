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
use rand::Rng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse number of burns (default: 80)
    let burn_count = if args.len() > 1 {
        args[1].parse().unwrap_or(80)
    } else {
        80
    };
    
    // Parse burn amount per transaction (default: random between 1.001 and 1.999 tokens)
    let use_random_amount = if args.len() > 2 {
        args[2].to_lowercase() == "random"
    } else {
        true
    };
    
    let base_burn_amount = if !use_random_amount && args.len() > 2 {
        let amount = args[2].parse::<f64>().unwrap_or(1.0);
        if amount < 1.0 {
            println!("Warning: Burn amount must be at least 1 token. Setting to 1 token.");
            1.0 * 1_000_000_000.0
        } else {
            amount * 1_000_000_000.0
        }
    } else {
        1.0 * 1_000_000_000.0
    } as u64;

    // Parse compute units (default: 200_000)
    let compute_units = if args.len() > 3 {
        args[3].parse().unwrap_or(200_000)
    } else {
        200_000
    };

    // display input information, emphasizing the number of tokens burned per transaction
    println!("Burn configuration:");
    println!("  Number of burns: {}", burn_count);
    if use_random_amount {
        println!("  Tokens per burn: Random between 1.001 and 1.999 tokens (for testing top burn sorting)");
    } else {
        println!("  Tokens per burn: {} (fixed amount)", (base_burn_amount as f64) / 1_000_000_000.0);
    }
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
    
    // calculate top_burn_shard_pda
    let (top_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"top_burn_shard"],
        &program_id,
    );

    // Check if latest burn shard exists
    match client.get_account(&latest_burn_shard_pda) {
        Ok(_) => {
            println!("Found latest burn shard");
        },
        Err(_) => {
            println!("Warning: Latest burn shard does not exist.");
            println!("The transaction may fail. Please initialize the shard first using init-latest-burn-shard.");
            println!("Continue anyway? (y/n)");
            
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                return Ok(());
            }
        }
    }
    
    // check if top burn shard exists
    match client.get_account(&top_burn_shard_pda) {
        Ok(_) => {
            println!("Found top burn shard");
        },
        Err(_) => {
            println!("Warning: Top burn shard does not exist.");
            println!("Burns will be recorded in latest burn shard, but not in top burn shard.");
            println!("To enable top burn tracking, initialize the shard using init-top-burn-shard.");
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
    let required_tokens = (base_burn_amount as f64 * burn_count as f64) / 1_000_000_000.0;
    
    println!("Current token balance: {} tokens", token_balance);
    println!("Required tokens for {} burns: {:.6} tokens", burn_count, required_tokens);
    
    if token_balance < required_tokens {
        println!("Warning: Insufficient token balance for all burns.");
        println!("You need at least {:.6} tokens but have {:.6} tokens.", required_tokens, token_balance);
        
        // calculate the maximum number of burns that can be performed
        let max_possible_burns = (token_balance / ((base_burn_amount as f64) / 1_000_000_000.0)) as usize;
        println!("With your current balance, you can perform at most {} burns.", max_possible_burns);
        println!("Continue with {} burns instead? (y/n)", max_possible_burns);
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            // adjust the burn count
            if max_possible_burns > 0 {
                println!("Adjusted burn count to {} burns", max_possible_burns);
                // note: here we reassign burn_count
                let old_burn_count = burn_count;
                // cannot directly modify burn_count, as it is passed from the command line, so use a new variable
                let adjusted_burn_count = max_possible_burns;
                println!("Reducing burn count from {} to {}", old_burn_count, adjusted_burn_count);
                // use this new variable instead of burn_count
                return run_burns(&client, &payer, program_id, mint, token_account, 
                                latest_burn_shard_pda, top_burn_shard_pda, adjusted_burn_count, 
                                base_burn_amount, use_random_amount, compute_units);
            } else {
                println!("Not enough tokens for even a single burn. Operation cancelled.");
                return Ok(());
            }
        } else {
            println!("Operation cancelled");
            return Ok(());
        }
    }

    // perform the burn operation
    run_burns(&client, &payer, program_id, mint, token_account, 
             latest_burn_shard_pda, top_burn_shard_pda, burn_count, 
             base_burn_amount, use_random_amount, compute_units)
}

// extract the burn logic into a separate function, for easy adjustment of the burn count
fn run_burns(
    client: &RpcClient,
    payer: &Keypair,
    program_id: Pubkey,
    mint: Pubkey,
    token_account: Pubkey,
    latest_burn_shard_pda: Pubkey,
    top_burn_shard_pda: Pubkey,
    burn_count: usize,
    base_burn_amount: u64,
    use_random_amount: bool,
    compute_units: u32
) -> Result<(), Box<dyn std::error::Error>> {
    // Create random number generator
    let mut rng = rand::thread_rng();
    
    // Calculate Anchor instruction sighash for process_burn once
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn");
    let sighash_result = hasher.finalize()[..8].to_vec();

    // Create a vector to store actual burn amounts for final statistics
    let mut actual_burn_amounts: Vec<f64> = Vec::with_capacity(burn_count);

    // Start batch burning
    println!("\nStarting batch burn test with {} burns", burn_count);
    if use_random_amount {
        println!("Using random amounts between 1.001 and 1.999 tokens per burn");
    } else {
        println!("Using fixed amount of {:.6} tokens per burn", 
                (base_burn_amount as f64) / 1_000_000_000.0);
    }
    println!("Compute units per transaction: {}", compute_units);
    println!("----------------------------------------\n");

    let mut successful_burns = 0;
    let mut failed_burns = 0;
    let delay = Duration::from_secs(1); // 1 second delay between transactions

    for i in 1..=burn_count {
        // Generate a random burn amount if using random amounts
        let burn_amount = if use_random_amount {
            // Generate a random number between 1.001 and 1.999
            let random_float = 1.0 + rng.gen_range(0.001..0.999);
            // Convert to lamports (multiply by 10^9)
            (random_float * 1_000_000_000.0) as u64
        } else {
            base_burn_amount
        };
        
        // Save actual burn amount in tokens
        let burn_amount_in_tokens = (burn_amount as f64) / 1_000_000_000.0;
        actual_burn_amounts.push(burn_amount_in_tokens);
        
        println!("Processing burn #{}/{} - Amount: {:.6} tokens", i, burn_count, burn_amount_in_tokens);
        
        // Generate a unique message for each burn to track it
        let message = format!("Batch burn #{} of {} - Amount: {:.6}", i, burn_count, burn_amount_in_tokens);
        
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
                AccountMeta::new(top_burn_shard_pda, false),    // top burn shard
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
            &[payer],
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
                
                // check the error type, if it is because the burn amount is too small, give a more specific prompt
                if err.to_string().contains("BurnAmountTooSmall") {
                    println!("Error: Burn amount too small. Contract requires at least 1 token per burn.");
                    println!("Please restart with a higher burn amount. Stopping batch burn.");
                    break;
                }
                // If we're out of tokens, stop
                else if err.to_string().contains("insufficient funds") {
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

    // Calculate actual total tokens burned
    let total_tokens_burned: f64 = actual_burn_amounts.iter().sum();
    
    // Find maximum and minimum burn amounts
    let max_amount = actual_burn_amounts.iter().cloned().fold(0.0, f64::max);
    let min_amount = if actual_burn_amounts.is_empty() { 0.0 } else {
        actual_burn_amounts.iter().cloned().fold(f64::INFINITY, f64::min)
    };

    // Print summary
    println!("\n----------------------------------------");
    println!("Batch Burn Test Summary:");
    println!("Total burns attempted: {}", burn_count);
    println!("Successful burns: {}", successful_burns);
    println!("Failed burns: {}", failed_burns);
    println!("Tokens burned: {:.6}", total_tokens_burned);
    println!("Highest burn amount: {:.6}", max_amount);
    println!("Lowest burn amount: {:.6}", min_amount);
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
    
    // check if top burn shard exists
    println!("\nChecking top burn shard state...");
    match client.get_account(&top_burn_shard_pda) {
        Ok(account) => {
            println!("Top burn shard has {} bytes of data", account.data.len());
            println!("To view the leaderboard, run: cargo run --bin check-top-burn-shard");
            println!("The top burn shard should contain the highest amount burns in descending order");
        },
        Err(err) => {
            println!("Failed to get top burn shard account: {}", err);
        }
    }

    // If random amounts, display recommendations
    if use_random_amount {
        println!("\nSince you used random burn amounts, we recommend:");
        println!("1. Run 'cargo run --bin check-top-burn-shard' to verify the sorting");
        println!("2. The burn amounts should be in descending order in the top burn shard");
        println!("3. Only the top {} burns should be recorded if more than {} burns were performed", 
                 69, 69);
    }

    println!("\nTest completed. You should verify:");
    println!("1. Only the most recent 69 records are retained in the latest burn shard");
    println!("2. The top burn shard contains only the highest amount burns (sorted in descending order)");
    println!("3. Each burn record includes the correct amount value");

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
