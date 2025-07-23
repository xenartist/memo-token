// clients/src/test-batch-mint.rs
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSimulateTransactionConfig, RpcSendTransactionConfig},
};
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
use rand::Rng;

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
    
    // Parse initial compute units (default: 200_000) - used as fallback
    let initial_compute_units = if args.len() > 2 {
        args[2].parse().unwrap_or(200_000)
    } else {
        200_000
    };
    
    // Parse initial balance check delay in seconds (default: 30 seconds)
    let initial_balance_check_delay_sec = if args.len() > 3 {
        args[3].parse().unwrap_or(30)
    } else {
        30
    };
    
    // Parse max retry count for balance checks (default: 5)
    let max_balance_check_retries = if args.len() > 4 {
        args[4].parse().unwrap_or(5)
    } else {
        5
    };

    // display input information
    println!("Batch mint configuration:");
    println!("  Number of mints: {}", mint_count);
    println!("  Initial compute units: {}", initial_compute_units);
    println!("  Initial balance check delay: {} seconds", initial_balance_check_delay_sec);
    println!("  Max balance check retries: {}", max_balance_check_retries);
    println!();

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
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

    // Get initial token balance
    let initial_balance = match client.get_token_account_balance(&token_account) {
        Ok(balance) => balance.ui_amount.unwrap_or(0.0),
        Err(_) => {
            println!("Warning: Could not get initial token balance. Creating token account...");
            // 如果需要创建token账户，这里可以添加创建逻辑
            0.0
        }
    };
    
    println!("Initial token balance: {} tokens", initial_balance);

    // Start batch minting
    println!("\nStarting batch mint test with {} mints", mint_count);
    println!("----------------------------------------\n");

    let mut successful_mints = 0;
    let mut failed_mints = 0;
    let mut total_tokens_minted = 0.0;
    let mut total_compute_units_simulated = 0;
    let mut compute_units_per_mint = Vec::new();
    let mut tokens_per_mint = Vec::new();
    let mut current_balance = initial_balance;
    let tx_delay = Duration::from_secs(1); // 1 second delay between transactions
    let mut rng = rand::thread_rng();

    for i in 1..=mint_count {
        println!("Processing mint #{}/{}...", i, mint_count);
        
        // Use a deterministic signature for testing
        let signature = format!("BatchMintSig{}", i);
        
        // Generate a random length between 26 and 659 for the message
        let message_length = rng.gen_range(26..=659);
        
        // Generate a unique message for each mint with random padding to achieve target length
        let base_message = format!("Batch mint #{} of {}", i, mint_count);
        let padding_length = message_length - base_message.len();
        let padding = if padding_length > 0 {
            " ".repeat(padding_length)
        } else {
            "".to_string()
        };
        let message = format!("{}{}", base_message, padding);
        
        // Build JSON memo
        let memo_json = serde_json::json!({
            "signature": signature,
            "message": message
        });
        
        // Convert to string with compact formatting
        let memo_text = serde_json::to_string(&memo_json)
            .expect("Failed to serialize JSON");

        // Print memo text length
        let memo_length = memo_text.as_bytes().len();
        println!("Memo text length: {} bytes", memo_length);
        if memo_length < 69 || memo_length > 700 {
            println!("Warning: Memo length {} is outside target range 69-700 bytes", memo_length);
        }
        
        // Create process_transfer instruction
        let instruction_data = sighash_result.clone();
        
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
            accounts.clone(), // Clone to keep ownership
        );

        // Get latest blockhash
        let recent_blockhash = client
            .get_latest_blockhash()
            .expect("Failed to get recent blockhash");

        // Create simulation transaction without compute budget instruction
        let sim_transaction = Transaction::new_signed_with_payer(
            &[memo_ix.clone(), mint_ix.clone()],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        // Simulate transaction to determine required compute units
        println!("Simulating transaction to determine required compute units...");
        let (compute_units, sim_units_consumed) = match client.simulate_transaction_with_config(
            &sim_transaction,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: false,
                commitment: Some(CommitmentConfig::confirmed()),
                encoding: None,
                accounts: None,
                min_context_slot: None,
                inner_instructions: true,
            },
        ) {
            Ok(result) => {
                if let Some(err) = result.value.err {
                    println!("Warning: Transaction simulation failed: {:?}", err);
                    println!("Using default compute units: {}", initial_compute_units);
                    (initial_compute_units, None)
                } else if let Some(units_consumed) = result.value.units_consumed {
                    // Add 10% safety margin
                    let required_cu = (units_consumed as f64 * 1.1) as u32;
                    println!("Simulation consumed {} CUs, requesting {} CUs with 10% safety margin", 
                        units_consumed, required_cu);
                    (required_cu, Some(units_consumed))
                } else {
                    println!("Simulation didn't return units consumed, using default: {}", initial_compute_units);
                    (initial_compute_units, None)
                }
            },
            Err(err) => {
                println!("Failed to simulate transaction: {}", err);
                println!("Using default compute units: {}", initial_compute_units);
                (initial_compute_units, None)
            }
        };

        // Update total compute units if simulation was successful
        if let Some(units) = sim_units_consumed {
            total_compute_units_simulated += units;
            compute_units_per_mint.push((i, units));
        }

        // Create compute budget instruction with dynamic compute units
        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
        println!("Setting compute budget: {} CUs", compute_units);
        
        // Create transaction with appropriate instructions
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
            RpcSendTransactionConfig {
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
                
                // Use advanced balance checking with exponential backoff
                let new_balance = wait_for_token_balance_change(
                    &client, 
                    &token_account,
                    current_balance,
                    initial_balance_check_delay_sec,
                    max_balance_check_retries
                );
                
                let tokens_minted = new_balance - current_balance;
                println!("Tokens minted in this transaction: {} tokens", tokens_minted);
                
                // If tokens_minted is still 0, warn the user
                if tokens_minted <= 0.0 {
                    println!("WARNING: No tokens appear to have been minted despite waiting and retrying.");
                    println!("This could be due to RPC node delays or issues with the contract.");
                }
                
                // Update totals and tracking
                total_tokens_minted += tokens_minted;
                tokens_per_mint.push((i, tokens_minted));
                
                // Update current balance for next iteration
                current_balance = new_balance;
                
                // If we have simulation data for this mint, update the compute units per token ratio
                if let Some(units) = sim_units_consumed {
                    if tokens_minted > 0.0 {
                        println!("Compute units per token for this mint: {:.2} CUs/token", 
                               units as f64 / tokens_minted);
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
            sleep(tx_delay);
        }
    }

    // Calculate SOL costs
    // 1 CU = 0.0000001 SOL
    const SOL_PER_COMPUTE_UNIT: f64 = 0.0000001;
    let total_sol_cost = total_compute_units_simulated as f64 * SOL_PER_COMPUTE_UNIT;
    
    let avg_cu_per_mint = if successful_mints > 0 {
        total_compute_units_simulated as f64 / successful_mints as f64
    } else {
        0.0
    };
    
    let avg_sol_per_mint = avg_cu_per_mint * SOL_PER_COMPUTE_UNIT;
    
    let avg_cu_per_token = if total_tokens_minted > 0.0 {
        total_compute_units_simulated as f64 / total_tokens_minted
    } else {
        0.0
    };
    
    let avg_sol_per_token = avg_cu_per_token * SOL_PER_COMPUTE_UNIT;

    // Print summary
    println!("\n----------------------------------------");
    println!("Batch Mint Test Summary:");
    println!("----------------------------------------");
    println!("1. Total mints attempted: {}", mint_count);
    println!("   - Successful mints: {}", successful_mints);
    println!("   - Failed mints: {}", failed_mints);
    println!("2. Total tokens minted: {:.2} tokens", total_tokens_minted);
    println!("3. Total simulated compute units: {} CUs", total_compute_units_simulated);
    println!("   - Equivalent cost in SOL: {:.8} SOL", total_sol_cost);
    println!("4. Average compute units per mint: {:.2} CUs", avg_cu_per_mint);
    println!("   - Equivalent cost in SOL: {:.8} SOL", avg_sol_per_mint);
    println!("5. Average compute units per token: {:.2} CUs", avg_cu_per_token);
    println!("   - Equivalent cost in SOL: {:.8} SOL", avg_sol_per_token);
    println!("----------------------------------------");

    // Print detailed token minting results
    println!("\nDetailed Token Minting Results:");
    println!("----------------------------------------");
    for (mint_num, tokens) in &tokens_per_mint {
        println!("Mint #{}: {:.2} tokens", mint_num, tokens);
    }
    println!("----------------------------------------");

    // Print detailed compute unit usage
    println!("\nDetailed Compute Unit Usage:");
    println!("----------------------------------------");
    for (mint_num, units) in &compute_units_per_mint {
        println!("Mint #{}: {} CUs", mint_num, units);
    }
    println!("----------------------------------------");

    // Check final token balance
    if let Ok(balance) = client.get_token_account_balance(&token_account) {
        let final_balance = balance.ui_amount.unwrap_or(0.0);
        println!("Final token balance: {} tokens", final_balance);
        println!("Net change from initial balance: +{:.2} tokens", final_balance - initial_balance);
    }
    
    // Check user profile if it exists
    if user_profile_exists {
        println!("\nYour mint statistics have been updated in your user profile.");
        println!("To view your profile stats, run: cargo run --bin check-user-profile");
    }

    Ok(())
}

// Advanced function to wait for token balance changes with exponential backoff
fn wait_for_token_balance_change(
    client: &RpcClient, 
    token_account: &Pubkey, 
    current_balance: f64,
    initial_delay_seconds: u64,
    max_retries: u64
) -> f64 {
    let mut delay_seconds = initial_delay_seconds;
    let mut total_wait_time = 0;
    
    // Initial wait after transaction confirmation
    println!("Waiting {} seconds for initial balance check...", delay_seconds);
    sleep(Duration::from_secs(delay_seconds));
    total_wait_time += delay_seconds;
    
    // Check balance
    match client.get_token_account_balance(token_account) {
        Ok(balance) => {
            let new_balance = balance.ui_amount.unwrap_or(current_balance);
            
            // If balance has changed, we're done
            if new_balance > current_balance {
                println!("Balance updated after {} seconds", total_wait_time);
                return new_balance;
            }
            
            // Otherwise, start retrying with exponential backoff
            println!("No balance change detected, starting retry sequence...");
            
            for retry in 1..=max_retries {
                // Double the delay for each retry (exponential backoff)
                delay_seconds = std::cmp::min(delay_seconds * 2, 60); // Cap at 60 seconds
                println!("Retry {}/{}: Waiting {} seconds...", retry, max_retries, delay_seconds);
                sleep(Duration::from_secs(delay_seconds));
                total_wait_time += delay_seconds;
                
                match client.get_token_account_balance(token_account) {
                    Ok(balance) => {
                        let new_balance = balance.ui_amount.unwrap_or(current_balance);
                        if new_balance > current_balance {
                            println!("Balance updated after {} seconds and {} retries", total_wait_time, retry);
                            return new_balance;
                        }
                    },
                    Err(err) => {
                        println!("Error checking balance on retry {}: {}", retry, err);
                    }
                }
            }
            
            println!("Maximum retries ({}) reached. No balance change detected after {} seconds.", 
                    max_retries, total_wait_time);
            return current_balance; // Return original balance if no change detected
        },
        Err(err) => {
            println!("Error during initial balance check: {}", err);
            println!("Returning original balance");
            return current_balance;
        }
    }
}