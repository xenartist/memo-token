use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSimulateTransactionConfig, RpcSendTransactionConfig},
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // parse compute units (default: 440_000)
    let initial_compute_units = if args.len() > 1 {
        args[1].parse().unwrap_or(440_000)
    } else {
        440_000
    };
    
    // parse burn amount (in actual token units)
    let burn_amount = if args.len() > 2 {
        args[2].parse::<u64>().unwrap_or(1) * 1_000_000_000 // convert to lamports
    } else {
        1_000_000_000 // default burn 1 token
    };

    // default fake signature
    let default_signature = "3GZFMnLbY2kaV1EpS8sa2rXjMGJaGjZ2QtVE5EANSicTqAWrmmqrKcyEg2m44D2Zs1cJ9r226K8F1zuoqYfU7KFr";

    // Parse memo and signature from args
    let (message, signature) = if args.len() > 3 {
        // check if signature separator "|" is included
        if args[3].contains("|") {
            let parts: Vec<&str> = args[3].split("|").collect();
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (args[3].clone(), default_signature.to_string())
        }
    } else {
        (String::from("Default burn message"), default_signature.to_string())
    };

    // build JSON format memo
    let memo_json = serde_json::json!({
        "signature": signature,
        "message": message
    });
    
    // convert to string with compact formatting
    let memo_text = serde_json::to_string(&memo_json)
        .expect("Failed to serialize JSON");

    // ensure memo length is at least 69 bytes
    let memo_text = ensure_min_length(memo_text, 69);

    // print detailed information
    println!("Original JSON structure:");
    println!("{:#?}", memo_json);
    println!("\nFinal memo text (length: {} bytes):", memo_text.as_bytes().len());
    println!("{}", memo_text);
    println!("\nMemo text bytes:");
    for (i, byte) in memo_text.as_bytes().iter().enumerate() {
        print!("{:02x} ", byte);
        if (i + 1) % 16 == 0 {
            println!();
        }
    }
    println!("\n");

    // connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // program and token address
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
        .expect("Invalid mint address");

    // get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),  // use token-2022 program ID
    );

    // calculate GlobalTopBurnIndex PDA
    let (global_top_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_top_burn_index"],
        &program_id,
    );
    
    let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id,
    );
    
    // get the current top_burn_shard and backup shard
    // first check the GlobalTopBurnIndex account
    let mut primary_shard_pda = None;
    let mut backup_shard_pda = None;

    // check if the burn amount is enough to use top burn shard
    let using_top_burn = burn_amount >= 420 * 1_000_000_000;

    if using_top_burn {
        println!("Burn amount ({} tokens) meets threshold for top burn shard (420+ tokens)", 
                burn_amount / 1_000_000_000);
        
        match client.get_account(&global_top_burn_index_pda) {
            Ok(account) => {
                println!("Found global top burn index account");
                // parse the data to find the current index and total count
                if account.data.len() >= 17 { // 8 bytes discriminator + 8 bytes total_count + 1 byte option tag
                    let data = &account.data[8..]; // skip discriminator
                    
                    // parse total_count
                    let total_count = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    println!("Top burn shard total count: {}", total_count);
                    
                    // parse current_index (Option<u64>)
                    let option_tag = data[8];
                    
                    if option_tag == 1 && data.len() >= 17 { // Option::Some
                        let current_index = u64::from_le_bytes(data[9..17].try_into().unwrap());
                        println!("Current top burn shard index: {}", current_index);
                        
                        // calculate the primary shard PDA using the current index
                        let (shard_pda, _) = Pubkey::find_program_address(
                            &[b"top_burn_shard", &current_index.to_le_bytes()],
                            &program_id,
                        );
                        primary_shard_pda = Some(shard_pda);
                        println!("Primary top burn shard PDA: {}", shard_pda);
                        
                        // if there is a next shard available, calculate the backup shard PDA
                        if current_index + 1 < total_count {
                            let next_index = current_index + 1;
                            let (next_shard_pda, _) = Pubkey::find_program_address(
                                &[b"top_burn_shard", &next_index.to_le_bytes()],
                                &program_id,
                            );
                            backup_shard_pda = Some(next_shard_pda);
                            println!("Backup top burn shard PDA: {}", next_shard_pda);
                        } else {
                            println!("No backup shard available. All shards allocated.");
                        }
                    } else if option_tag == 0 { // Option::None
                        println!("No active top burn shard. Need to initialize one first.");
                    }
                }
            },
            Err(_) => {
                println!("Warning: Global top burn index account not found.");
                println!("Top burn tracking will not be available. Initialize it first using init-global-top-burn-index.");
            }
        };
        
        if primary_shard_pda.is_none() {
            println!("Warning: No primary top burn shard available.");
            println!("Burn will succeed but won't be recorded in top burn shards.");
            println!("Initialize top burn shards using init-global-top-burn-index and init-top-burn-shard.");
        }
    } else {
        println!("Burn amount ({} tokens) is below threshold for top burn shard (420+ tokens)", 
                burn_amount / 1_000_000_000);
    }
    
    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );

    // Check if user profile exists
    let user_profile_exists = match client.get_account(&user_profile_pda) {
        Ok(account) => {
            println!("User profile found at: {}", user_profile_pda);
            println!("Burn statistics will be tracked in your profile");
            true
        },
        Err(_) => {
            println!("No user profile found. The burn will succeed but won't track your statistics.");
            println!("To create a profile, use 'cargo run --bin init-user-profile <username> [profile_image_url]'");
            false
        }
    };

    // try to read burn_history_index and find current burn_history account
    let mut burn_history_pda = None;
    let mut burn_history_exists = false;

    if user_profile_exists {
        match client.get_account(&user_profile_pda) {
            Ok(account) => {
                // read burn_history_index
                let mut data = &account.data[8..]; // skip discriminator
                data = &data[32..]; // skip pubkey
                
                // skip total_minted, total_burned, mint_count, burn_count
                data = &data[32..];
                
                // skip timestamps
                data = &data[16..]; // skip created_at and last_updated
                
                // read burn_history_index
                let has_burn_history = data[0] == 1;
                if has_burn_history {
                    let current_index = u64::from_le_bytes([
                        data[1], data[2], data[3], data[4],
                        data[5], data[6], data[7], data[8]
                    ]);
                    
                    // calculate current burn_history PDA
                    let (current_burn_history_pda, _) = Pubkey::find_program_address(
                        &[
                            b"burn_history",
                            payer.pubkey().as_ref(),
                            &current_index.to_le_bytes()
                        ],
                        &program_id,
                    );
                    
                    burn_history_pda = Some(current_burn_history_pda);
                    
                    // check if current burn history exists
                    match client.get_account(&current_burn_history_pda) {
                        Ok(burn_history_account) => {
                            // parse burn history data, check signature count
                            let burn_history_data = &burn_history_account.data[8..]; // skip discriminator
                            
                            // skip owner and index
                            let data = &burn_history_data[40..]; // 32 bytes owner + 8 bytes index
                            
                            // read signature array length
                            let signatures_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                            
                            println!("Current burn history (index {}) has {} signatures.", current_index, signatures_len);
                            
                            // check if signature count reaches maximum
                            if signatures_len >= 100 {
                                println!("Warning: Current burn history is full (100 signatures).");
                                println!("Create a new burn history using init-user-profile-burn-history.");
                                println!("This burn will fail unless you create a new burn history.");
                                println!("Continue anyway? (y/n)");
                                
                                let mut input = String::new();
                                std::io::stdin().read_line(&mut input)?;
                                if !input.trim().eq_ignore_ascii_case("y") {
                                    return Ok(());
                                }
                            } else {
                                burn_history_exists = true;
                                println!("Found valid burn history account.");
                                println!("Burn will be recorded in burn history (index: {}).", current_index);
                            }
                        },
                        Err(_) => {
                            println!("Warning: Burn history index exists in profile, but the burn history account doesn't exist.");
                            println!("Run init-user-profile-burn-history to create the burn history account.");
                            println!("This burn will fail unless you create a burn history account.");
                            println!("Continue anyway? (y/n)");
                            
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                return Ok(());
                            }
                        }
                    }
                } else {
                    println!("No burn history index found in user profile.");
                    println!("Run init-user-profile-burn-history to create a burn history account.");
                    println!("This burn will fail unless you create a burn history account.");
                    println!("Continue anyway? (y/n)");
                    
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        return Ok(());
                    }
                }
            },
            Err(_) => {
                println!("Failed to read user profile account.");
            }
        }
    }
    
    if !burn_history_exists {
        println!("\nWARNING: No valid burn history account found.");
        println!("This burn with history operation will likely fail.");
        println!("Please run 'cargo run --bin init-user-profile-burn-history' first.");
        println!("Continue anyway? (y/n)");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

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
    
    // now create the account list for the burn_with_history instruction
    let mut accounts = vec![
        AccountMeta::new(payer.pubkey(), true),         // user
        AccountMeta::new(mint, false),                  // mint
        AccountMeta::new(token_account, false),         // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        AccountMeta::new(latest_burn_shard_pda, false), // latest burn shard
    ];
    
    // add global index (if available)
    if client.get_account(&global_top_burn_index_pda).is_ok() {
        accounts.push(AccountMeta::new(global_top_burn_index_pda, false)); // global_top_burn_index
    }
    
    // add user profile (if exists)
    if user_profile_exists {
        accounts.push(AccountMeta::new(user_profile_pda, false)); // user_profile
    }
    
    // add primary and backup shard (if available)
    if let Some(primary_pda) = primary_shard_pda {
        accounts.push(AccountMeta::new(primary_pda, false)); // primary_top_burn_shard
    }
    
    if let Some(backup_pda) = backup_shard_pda {
        accounts.push(AccountMeta::new(backup_pda, false)); // backup_top_burn_shard
    }
    
    // add burn_history (required)
    if let Some(history_pda) = burn_history_pda {
        accounts.push(AccountMeta::new(history_pda, false)); // burn_history
    } else {
        println!("Error: Missing burn history account!");
        return Ok(());
    }
    
    // print account information for debugging
    println!("\nTransaction will include these accounts:");
    for (i, account) in accounts.iter().enumerate() {
        println!("  Account {}: {} (is_signer: {}, is_writable: {})",
               i, account.pubkey, account.is_signer, account.is_writable);
    }
    
    // calculate the Anchor instruction sighash (using process_burn_with_history)
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn_with_history");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // add burn amount parameter
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // create burn instruction
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // create memo instruction
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    // get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // create a transaction without compute budget instruction for simulation
    let sim_transaction = Transaction::new_signed_with_payer(
        &[memo_ix.clone(), burn_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // simulate transaction to determine required compute units
    println!("Simulating transaction to determine required compute units...");
    let compute_units = match client.simulate_transaction_with_config(
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
                initial_compute_units
            } else if let Some(units_consumed) = result.value.units_consumed {
                // add 10% safety margin
                let required_cu = (units_consumed as f64 * 1.1) as u32;
                println!("Simulation consumed {} CUs, requesting {} CUs with 10% safety margin", 
                    units_consumed, required_cu);
                required_cu
            } else {
                println!("Simulation didn't return units consumed, using default: {}", initial_compute_units);
                initial_compute_units
            }
        },
        Err(err) => {
            println!("Failed to simulate transaction: {}", err);
            println!("Using default compute units: {}", initial_compute_units);
            initial_compute_units
        }
    };

    // create compute budget instruction with dynamically calculated CU
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    println!("Setting compute budget: {} CUs", compute_units);
    println!("Burning {} tokens with history", burn_amount / 1_000_000_000);

    // create and send transaction with instruction order:
    // 1. compute budget instruction
    // 2. memo instruction
    // 3. burn instruction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, burn_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // send and confirm transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Burn with history successful! Signature: {}", signature);
            println!("Memo: {}", memo_text);

            // print token balance
            if let Ok(balance) = client.get_token_account_balance(&token_account) {
                println!("New token balance: {}", balance.ui_amount.unwrap());
            }
            
            // Check if record was added to latest burn shard
            match client.get_account(&latest_burn_shard_pda) {
                Ok(_) => {
                    println!("Burn record added to latest burn shard. Use check-latest-burn-shard to view records.");
                },
                Err(err) => {
                    println!("Warning: Could not verify latest burn shard update: {}", err);
                }
            }
            
            // Check if record was added to top burn shard
            if using_top_burn {
                if let Some(primary_pda) = primary_shard_pda {
                    match client.get_account(&primary_pda) {
                        Ok(_) => {
                            println!("Top burn shard (primary) may have been updated. Check if your burn qualified.");
                        },
                        Err(_) => {}
                    }
                }
                
                if let Some(backup_pda) = backup_shard_pda {
                    match client.get_account(&backup_pda) {
                        Ok(_) => {
                            println!("Top burn shard (backup) may have been updated. Check if your burn qualified.");
                        },
                        Err(_) => {}
                    }
                }
            }
            
            // If user profile exists, show info about stats being updated
            if user_profile_exists {
                println!("\nYour burn statistics have been updated in your user profile.");
                println!("To view your profile stats, run: cargo run --bin check-user-profile");
            }
            
            // check if burn history was updated
            if burn_history_exists && burn_history_pda.is_some() {
                match client.get_account(&burn_history_pda.unwrap()) {
                    Ok(burn_history_account) => {
                        // parse burn history data, check signature count
                        let burn_history_data = &burn_history_account.data[8..]; // skip discriminator
                        
                        // skip owner and index
                        let data = &burn_history_data[40..]; // 32 bytes owner + 8 bytes index
                        
                        // read signature array length
                        let signatures_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                        
                        println!("Burn history updated - now contains {} signatures.", signatures_len);
                    },
                    Err(err) => {
                        println!("Warning: Could not verify burn history update: {}", err);
                    }
                }
            }
        }
        Err(err) => {
            println!("Failed to burn tokens: {}", err);
            println!("This may happen if:");
            println!("1. The burn shards don't exist - run initialization scripts first");
            println!("2. Insufficient token balance");
            println!("3. Issues with the memo format");
            println!("4. Burn amount is less than the minimum required (1 token)");
            println!("5. Burn amount is not an integer multiple of 1 token");
            println!("6. Compute units might be insufficient: {}", compute_units);
            println!("7. No valid burn history account, or it's full (100 signatures)");
            
            // try to get transaction logs
            if let Some(sig_str) = err.to_string().split("signature ").nth(1) {
                if let Some(signature) = sig_str.split_whitespace().next() {
                    println!("\nAttempting to get logs for failed transaction: {}", signature);
                    if let Ok(sig) = signature.parse::<solana_sdk::signature::Signature>() {
                        if let Ok(tx_data) = client.get_transaction_with_config(
                            &sig,
                            solana_client::rpc_config::RpcTransactionConfig {
                                encoding: None,
                                commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
                                max_supported_transaction_version: None,
                            },
                        ) {
                            if let Some(meta) = tx_data.transaction.meta {
                                println!("\nTransaction logs:");
                                match meta.log_messages {
                                    solana_transaction_status::option_serializer::OptionSerializer::Some(logs) => {
                                        for log in logs {
                                            println!("{}", log);
                                        }
                                    },
                                    solana_transaction_status::option_serializer::OptionSerializer::None => {
                                        println!("No logs available");
                                    },
                                    solana_transaction_status::option_serializer::OptionSerializer::Skip => {
                                        println!("Transaction logs skipped");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// modify ensure_min_length function to keep JSON format
fn ensure_min_length(text: String, min_length: usize) -> String {
    if text.as_bytes().len() >= min_length {
        return text;
    }
    
    // parse existing JSON
    let mut json: serde_json::Value = serde_json::from_str(&text)
        .expect("Failed to parse JSON");
    
    // get existing message
    let message = json["message"].as_str().unwrap_or("");
    
    // calculate padding length needed
    let current_length = text.as_bytes().len();
    let padding_needed = min_length - current_length;
    
    // create padding with spaces
    let padding = " ".repeat(padding_needed);
    
    // update message field with padding
    let new_message = format!("{}{}", message, padding);
    json["message"] = serde_json::Value::String(new_message);
    
    // convert back to string with compact formatting (no extra whitespace)
    let result = serde_json::to_string(&json)
        .expect("Failed to serialize JSON");
    
    println!("Memo was padded to meet minimum length requirement of {} bytes", min_length);
    println!("Final JSON: {}", result);
    
    result
}
