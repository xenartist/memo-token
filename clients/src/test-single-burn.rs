use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
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

    // Calculate GlobalTopBurnIndex PDA
    let (global_top_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_top_burn_index"],
        &program_id,
    );
    
    let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id,
    );
    
    // get the current top_burn_shard
    // first check the GlobalTopBurnIndex account
    let mut current_top_burn_shard_pda = None;

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
                    
                    // calculate the current shard PDA using the current index
                    let (shard_pda, _) = Pubkey::find_program_address(
                        &[b"top_burn_shard", &current_index.to_le_bytes()],
        &program_id,
    );
                    current_top_burn_shard_pda = Some(shard_pda);
                    println!("Current top burn shard PDA: {}", shard_pda);
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

    // check if the burn amount is enough to use top burn shard
    let using_top_burn = burn_amount >= 420 * 1_000_000_000;
    if using_top_burn {
        println!("Burn amount ({} tokens) meets threshold for top burn shard (420+ tokens)", 
                 burn_amount / 1_000_000_000);
        
        if current_top_burn_shard_pda.is_none() {
            println!("Warning: No current top burn shard available.");
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
    if let Some(top_burn_shard_pda) = current_top_burn_shard_pda {
    match client.get_account(&top_burn_shard_pda) {
        Ok(_) => {
                println!("Found current top burn shard");
        },
        Err(_) => {
                println!("Warning: Current top burn shard does not exist.");
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
    }

    // calculate Anchor instruction sighash for process_burn
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // add burn amount parameter
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // Create burn instruction - include user profile account if it exists
    let mut accounts = vec![
        AccountMeta::new(payer.pubkey(), true),         // user
        AccountMeta::new(mint, false),                  // mint
        AccountMeta::new(token_account, false),         // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program (use token-2022)
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        AccountMeta::new(latest_burn_shard_pda, false), // latest burn shard
        AccountMeta::new(global_top_burn_index_pda, false), // global_top_burn_index
    ];
    
    // Add user profile PDA to account list if it exists
    if user_profile_exists {
        accounts.push(AccountMeta::new(user_profile_pda, false)); // user_profile
    }

    // add current top burn shard if available
    if let Some(top_burn_shard_pda) = current_top_burn_shard_pda {
        accounts.push(AccountMeta::new(top_burn_shard_pda, false)); // top_burn_shard
    }
    
    // print account information for debugging
    println!("\nTransaction will include these accounts:");
    for (i, account) in accounts.iter().enumerate() {
        println!("  Account {}: {} (is_signer: {}, is_writable: {})",
               i, account.pubkey, account.is_signer, account.is_writable);
    }

    // Convert JSON to bytes directly without any extra escaping
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts.clone(),  // Clone here to avoid moving ownership
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
                
                // check if it's NoMoreShardsAvailable error
                if err.to_string().contains("Custom(6017)") {
                    println!("ERROR: The current top burn shard is full and there are no more pre-allocated shards available.");
                    println!("Please create a new shard first using: cargo run --bin init-top-burn-shard");
                    println!("Then try your burn operation again.");
                    return Ok(());  // return early, don't attempt to send a doomed transaction
                }
                
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
    println!("Burning {} tokens", burn_amount / 1_000_000_000);

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
            if let Some(top_burn_shard_pda) = current_top_burn_shard_pda {
            match client.get_account(&top_burn_shard_pda) {
                Ok(_) => {
                        if using_top_burn {
                            println!("Top burn shard updated. Use check-top-burn-shard to see your burn on the leaderboard.");
                        } else {
                            println!("Note: Burn amount was below threshold (420 tokens) for top burn shard.");
                        }
                },
                Err(err) => {
                    println!("Warning: Could not verify top burn shard update: {}", err);
                    }
                }
            }
            
            // If user profile exists, show info about stats being updated
            if user_profile_exists {
                println!("\nYour burn statistics have been updated in your user profile.");
                println!("To view your profile stats, run: cargo run --bin check-user-profile");
            }
        },
        Err(err) => {
            let err_string = err.to_string();
            println!("Failed to burn tokens: {}", err_string);
            
            // extract error code - improved extraction method
            let mut error_code = None;
            
            // method 1: find "custom program error: 0x" format
            if let Some(i) = err_string.find("custom program error: 0x") {
                let code_part = &err_string[i + 22..];
                let end = code_part.find(' ').unwrap_or(code_part.len());
                let hex_code = &code_part[..end];
                if let Ok(code) = u64::from_str_radix(hex_code, 16) {
                    error_code = Some(code);
                    println!("Error hex code: 0x{} ({})", hex_code, code);
                }
            }
            
            // method 2: find any 0x-prefixed hex error code
            if error_code.is_none() {
                for word in err_string.split_whitespace() {
                    if word.starts_with("0x") {
                        let hex_code = &word[2..];
                        if let Ok(code) = u64::from_str_radix(hex_code, 16) {
                            error_code = Some(code);
                            println!("Error hex code: {} ({})", word, code);
                            break;
                        }
                    }
                }
            }
            
            // try to extract signature from error - improved extraction logic
            let mut signature_str = None;
            
            // method 1: find line containing "signature"
            for line in err_string.lines() {
                if line.contains("signature") {
                    for word in line.split_whitespace() {
                        if word.len() >= 86 && word.len() <= 88 {
                            signature_str = Some(word.to_string());
                            break;
                        }
                    }
                }
                if signature_str.is_some() {
                    break;
                }
            }
            
            // method 2: find any long string that looks like a signature (base58 encoded, 87-88 characters)
            if signature_str.is_none() {
                for word in err_string.split_whitespace() {
                    if word.len() >= 86 && word.len() <= 88 && !word.contains(":") {
                        // simple check if it might be base58 encoded
                        if word.chars().all(|c| c.is_ascii_alphanumeric()) {
                            signature_str = Some(word.to_string());
                            break;
                        }
                    }
                }
            }
            
            // handle extracted signature
            if let Some(sig_str) = signature_str {
                println!("\nDetected transaction signature: {}", sig_str);
                
                println!("Fetching detailed logs for the failed transaction...");
                
                match sig_str.parse::<solana_sdk::signature::Signature>() {
                    Ok(sig) => {
                        // use RPC to get more detailed transaction info, including all logs
                        let tx_result = client.get_transaction_with_config(
                            &sig,
                            solana_client::rpc_config::RpcTransactionConfig {
                                encoding: None,
                                commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
                                max_supported_transaction_version: None,
                            },
                        );
                        
                        match tx_result {
                            Ok(tx_data) => {
                                println!("\n=== TRANSACTION DETAILS ===");
                                
                                // extract transaction metadata
                                if let Some(meta) = &tx_data.transaction.meta {
                                    // print error info
                                    if let Some(err) = &meta.err {
                                        println!("\nTransaction Error: {:?}", err);
                                    }
                                    
                                    // print all logs
                                    println!("\n=== FULL TRANSACTION LOGS ===");
                                    match &meta.log_messages {
                                        solana_transaction_status::option_serializer::OptionSerializer::Some(logs) => {
                                            for (i, log) in logs.iter().enumerate() {
                                                println!("{:3}: {}", i, log);
                                            }
                                        },
                                        solana_transaction_status::option_serializer::OptionSerializer::None => {
                                            println!("No logs available");
                                        },
                                        solana_transaction_status::option_serializer::OptionSerializer::Skip => {
                                            println!("Transaction logs skipped");
                                        }
                                    }
                                    
                                    // try to parse custom program error code
                                    let error_code = err_string.find("custom program error: 0x").map(|i| {
                                        let code_part = &err_string[i + 22..];
                                        let end = code_part.find(' ').unwrap_or(code_part.len());
                                        let hex_code = &code_part[..end];
                                        u64::from_str_radix(hex_code, 16).ok()
                                    }).flatten();
                                    
                                    if let Some(code) = error_code {
                                        println!("\n=== PROGRAM ERROR INTERPRETATION ===");
                                        match code {
                                            6001 => println!("Error: MemoTooShort - Memo is too short. Must be at least 69 bytes."),
                                            6002 => println!("Error: MemoTooLong - Memo is too long. Must be at most 700 bytes."),
                                            6003 => println!("Error: MemoRequired - Transaction must include a memo."),
                                            6004 => println!("Error: InvalidMemoFormat - Invalid memo format. Expected JSON format."),
                                            6005 => println!("Error: MissingSignature - Missing signature field in memo JSON."),
                                            6006 => println!("Error: UnauthorizedAuthority - Unauthorized: Only the authority can perform this action"),
                                            6007 => println!("Error: UnauthorizedAdmin - Unauthorized: Only the admin can perform this action"),
                                            6008 => println!("Error: BurnAmountTooSmall - Burn amount too small. Must burn at least 1 token."),
                                            6009 => println!("Error: UnauthorizedUser - Unauthorized: Only the user can update their own profile"),
                                            6010 => println!("Error: InvalidBurnAmount - Invalid burn amount. Must be an integer multiple of 1 token."),
                                            6011 => println!("Error: InvalidBurnHistoryIndex - Invalid burn history index"),
                                            6012 => println!("Error: BurnHistoryFull - Burn history account is full"),
                                            6013 => println!("Error: InvalidSignatureLength - Invalid signature length"),
                                            6014 => println!("Error: BurnHistoryRequired - Burn history account is required for recording burn history"),
                                            6015 => println!("Error: CounterOverflow - Counter overflow: maximum number of shards reached"),
                                            6016 => println!("Error: TopBurnShardFull - Top burn shard is full. Try using the next available shard."),
                                            6017 => println!("Error: NoMoreShardsAvailable - No more pre-allocated shards available."),
                                            6018 => println!("Error: NeedToUseDifferentShard - Need to use a different shard. The current shard is full."),
                                            _ => println!("Unknown error code: 0x{:x} ({})", code, code),
                                        }
                                    }
                                } else {
                                    println!("No transaction metadata available");
                                }
                            },
                            Err(error) => {
                                println!("Failed to get transaction details: {}", error);
                            }
                        }
                    },
                    Err(_) => {
                        println!("Could not parse transaction signature from error message");
                    }
                }
            } else {
                println!("Could not extract transaction signature from error");
            }
            
            // even if there's no signature, parse and print error code info
            println!("\n=== PROGRAM ERROR INTERPRETATION ===");
            match error_code {
                Some(6001) => println!("Error: MemoTooShort - Memo is too short. Must be at least 69 bytes."),
                Some(6002) => println!("Error: MemoTooLong - Memo is too long. Must be at most 700 bytes."),
                // ... other error codes ...
                Some(6017) => {
                    println!("Error: NoMoreShardsAvailable - No more pre-allocated shards available.");
                    println!("Solution: Create new shards first using 'cargo run --bin init-top-burn-shard'");
                    println!("Then try your burn operation again.");
                },
                Some(6018) => println!("Error: NeedToUseDifferentShard - Need to use a different shard. The current shard is full."),
                Some(101) => println!("Error: InstructionFallbackNotFound (0x65) - Anchor could not match your instruction to any defined in the program."),
                Some(code) => println!("Unknown error code: 0x{:x} ({})", code, code),
                None => println!("Could not extract specific error code from the error message.")
            }
            
            println!("\n=== TROUBLESHOOTING GUIDE ===");
            println!("1. The burn shards don't exist - run initialization scripts first");
            println!("2. Insufficient token balance");
            println!("3. Issues with the memo format - ensure it's valid JSON with required fields");
            println!("4. Burn amount too small (< 1 token) or not an integer multiple of 1 token");
            println!("5. Current top burn shard may be full - create a new shard with init-top-burn-shard");
            println!("6. Global top burn index may need initialization");
            println!("7. Compute units might be insufficient (currently set to: {})", compute_units);
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