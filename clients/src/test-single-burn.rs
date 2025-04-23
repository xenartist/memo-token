use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use serde_json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // parse compute units (default: 440_000)
    let compute_units = if args.len() > 1 {
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

    // parse burn_history setting (optional parameter: Y/N/NONE)
    // Y - add burn_history field and set it to "Y"
    // N - add burn_history field and set it to "N"
    // NONE or other values - not add burn_history field to memo
    let burn_history_setting = if args.len() > 4 {
        args[4].to_uppercase()
    } else {
        "Y".to_string() // default to add burn_history field and set it to "Y"
    };

    // determine how to build memo based on setting
    let should_include_burn_history_field = burn_history_setting == "Y" || burn_history_setting == "N";
    let record_to_burn_history = burn_history_setting == "Y";

    // build JSON format memo
    let mut memo_json = serde_json::json!({
        "signature": signature,
        "message": message
    });
    
    // only add burn_history field when needed
    if should_include_burn_history_field {
        memo_json["burn_history"] = serde_json::Value::String(
            if record_to_burn_history { "Y".to_string() } else { "N".to_string() }
        );
    }
    
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
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")
        .expect("Invalid mint address");

    // get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // calculate PDAs
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
                
                // skip username
                let username_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                data = &data[4 + username_len..];
                
                // skip stats
                data = &data[32..]; // skip total_minted, total_burned, mint_count, burn_count
                
                // skip profile_image
                let profile_image_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                data = &data[4 + profile_image_len..];
                
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
                                
                                if record_to_burn_history {
                                    println!("Since burn_history is set to 'Y', this burn may fail unless you create a new burn history.");
                                    println!("Continue anyway? (y/n)");
                                    
                                    let mut input = String::new();
                                    std::io::stdin().read_line(&mut input)?;
                                    if !input.trim().eq_ignore_ascii_case("y") {
                                        return Ok(());
                                    }
                                }
                            } else {
                                burn_history_exists = true;
                                println!("Found valid burn history account.");
                                
                                if record_to_burn_history {
                                    println!("Burn will be recorded in burn history (index: {}).", current_index);
                                } else {
                                    println!("But burn_history is not set to 'Y', so burn will NOT be recorded.");
                                }
                            }
                        },
                        Err(_) => {
                            println!("Warning: Burn history index exists in profile, but the burn history account doesn't exist.");
                            println!("Run init-user-profile-burn-history to create the burn history account.");
                            
                            if record_to_burn_history {
                                println!("Since burn_history is set to 'Y', this burn may fail unless you create a burn history account.");
                                println!("Continue anyway? (y/n)");
                                
                                let mut input = String::new();
                                std::io::stdin().read_line(&mut input)?;
                                if !input.trim().eq_ignore_ascii_case("y") {
                                    return Ok(());
                                }
                            }
                        }
                    }
                } else {
                    println!("No burn history index found in user profile.");
                    println!("Run init-user-profile-burn-history to create a burn history account.");
                    
                    if record_to_burn_history {
                        println!("Since burn_history is set to 'Y', this burn may fail unless you create a burn history account.");
                        println!("Continue anyway? (y/n)");
                        
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if !input.trim().eq_ignore_ascii_case("y") {
                            return Ok(());
                        }
                    }
                }
            },
            Err(_) => {
                println!("Failed to read user profile account.");
            }
        }
    }
    
    // print burn_history field status
    if should_include_burn_history_field {
        println!("burn_history field: {}", if record_to_burn_history { "Y" } else { "N" });
    } else {
        println!("burn_history field: Not included in memo");
    }
    
    // special handling of burn_history related prompts
    if record_to_burn_history && !burn_history_exists {
        println!("\nWARNING: burn_history is set to 'Y' but no valid burn history account found.");
        println!("Based on your testing results, this burn operation will likely fail.");
        println!("Please run 'cargo run --bin init-user-profile-burn-history' first.");
        println!("Continue anyway? (y/n)");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }
    
    if !record_to_burn_history || !should_include_burn_history_field {
        println!("\nWARNING: Based on your testing results, burns with burn_history='N' or no burn_history field");
        println!("return a custom program error. Make sure your contract or client configuration is correct.");
    }

    // Check if shards exist
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

    // calculate Anchor instruction sighash for process_burn
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // add burn amount parameter
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // create compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // Convert JSON to bytes directly without any extra escaping
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    // print information
    let memo_length = memo_text.as_bytes().len();
    println!("Memo length: {} bytes", memo_length);
    println!("Raw memo content: {}", memo_text);
    println!("Setting compute budget: {} CUs", compute_units);
    println!("Burning {} tokens", burn_amount / 1_000_000_000);
    
    // Create burn instruction - include user profile account if it exists
    let mut accounts = vec![
        AccountMeta::new(payer.pubkey(), true),         // user
        AccountMeta::new(mint, false),                  // mint
        AccountMeta::new(token_account, false),         // token_account
        AccountMeta::new_readonly(spl_token::id(), false), // token_program
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        AccountMeta::new(latest_burn_shard_pda, false), // latest burn shard
        AccountMeta::new(top_burn_shard_pda, false),    // top burn shard
    ];
    
    // Add user profile PDA to account list if it exists
    if user_profile_exists {
        accounts.push(AccountMeta::new(user_profile_pda, false)); // user_profile
    }
    
    // no matter what burn_history field is, if there is a valid burn_history account, add it
    if burn_history_exists && burn_history_pda.is_some() {
        accounts.push(AccountMeta::new(burn_history_pda.unwrap(), false)); // burn_history
        println!("Added burn_history account to transaction");
    } else if record_to_burn_history {
        println!("WARNING: burn_history set to 'Y' but no valid burn history account to add");
    }
    
    // print account information for debugging
    println!("\nTransaction will include these accounts:");
    for (i, account) in accounts.iter().enumerate() {
        println!("  Account {}: {} (is_signer: {}, is_writable: {})",
               i, account.pubkey, account.is_signer, account.is_writable);
    }
    
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

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
            println!("Burn successful! Signature: {}", signature);
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
            
            // check if record was added to top burn shard
            match client.get_account(&top_burn_shard_pda) {
                Ok(_) => {
                    // check if burn is added to top burn shard (depends on amount)
                    println!("Top burn shard updated. Use check-top-burn-shard to see if your burn qualified for the leaderboard.");
                    println!("Note: Burn is only added to top burn shard if amount is high enough to qualify.");
                },
                Err(err) => {
                    println!("Warning: Could not verify top burn shard update: {}", err);
                }
            }
            
            // If user profile exists, show info about stats being updated
            if user_profile_exists {
                println!("\nYour burn statistics have been updated in your user profile.");
                println!("To view your profile stats, run: cargo run --bin check-user-profile");
            }
            
            // If burn history is enabled and exists
            if burn_history_exists && burn_history_pda.is_some() && record_to_burn_history {
                println!("This burn has been recorded in your burn history.");
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
            
            // burn history related error
            if err.to_string().contains("0xbbd") || err.to_string().contains("BurnHistoryRequired") {
                println!("\nError: Burn history is required but not provided or not found.");
                println!("The contract seems to require a burn history account when burn_history=\"Y\".");
                println!("Run 'cargo run --bin init-user-profile-burn-history' to create a burn history account.");
                println!("\nAlso, based on your observations, the contract seems to always require");
                println!("burn_history=\"Y\" in the memo. Other values or missing the field causes errors.");
            }
            
            if err.to_string().contains("BurnHistoryFull") {
                println!("\nError: Current burn history is full (100 signatures).");
                println!("Run 'cargo run --bin init-user-profile-burn-history' to create a new burn history account.");
            }
    
            // overflow error
            if err.to_string().contains("would overflow") {
                println!("\nWARNING: The burn operation would cause a counter overflow.");
                println!("The system has protection against this, but it indicates you've reached");
                println!("a maximum limit for burning tokens. Your statistics will be capped at the maximum value.");
            }
            
            // Provide more specific advice based on error
            if err.to_string().contains("AccountNotEnoughKeys") {
                println!("\nThe contract is expecting more accounts than provided.");
                println!("To fix this, either create a user profile or update this script to include all required accounts.");
            }
            
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