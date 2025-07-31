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
    system_program,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use serde_json;
use sha2::{Sha256, Digest};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 {
        println!("Usage: cargo run --bin test-memo-chat-create-group -- <burn_amount> <test_type> [memo_length]");
        println!("Parameters:");
        println!("  burn_amount   - Number of tokens to burn for group creation (decimal=6)");
        println!("  test_type     - Type of memo test to perform");
        println!("  memo_length   - Custom memo length (only for custom-length test)");
        println!();
        println!("Note: group_id is automatically assigned by the contract (0, 1, 2, ...)");
        println!();
        println!("Test types:");
        println!("  valid-memo    - Valid memo (between 69-800 bytes) - should succeed");
        println!("  memo-69       - Memo exactly 69 bytes - should succeed");
        println!("  memo-800      - Memo exactly 800 bytes - should succeed");
        println!("  no-memo       - No memo instruction - should fail");
        println!("  short-memo    - Memo less than 69 bytes - should fail");
        println!("  long-memo     - Memo more than 800 bytes - should fail");
        println!("  amount-mismatch - Wrong amount in memo - should fail");
        println!("  custom-length - Custom memo length (requires memo_length parameter)");
        println!();
        println!("Examples:");
        println!("  cargo run --bin test-memo-chat-create-group -- 5 valid-memo");
        println!("  cargo run --bin test-memo-chat-create-group -- 10 memo-69");
        println!("  cargo run --bin test-memo-chat-create-group -- 1 amount-mismatch");
        return Ok(());
    }

    // Parse parameters
    let burn_amount_tokens = args[1].parse::<u64>().unwrap_or_else(|_| {
        eprintln!("Error: Invalid burn amount '{}'", args[1]);
        std::process::exit(1);
    });
    let burn_amount = burn_amount_tokens * 1_000_000; // Convert to units (decimal=6)
    let test_type = &args[2];

    // Parse custom memo length (for custom-length test)
    let custom_memo_length = if test_type == "custom-length" {
        if args.len() < 4 {
            println!("ERROR: custom-length test requires memo_length parameter");
            println!("Usage: cargo run --bin test-memo-chat-create-group -- <burn_amount> custom-length <memo_length>");
            return Ok(());
        }
        Some(args[3].parse::<usize>().unwrap_or_else(|_| {
            eprintln!("Error: Invalid memo length '{}'", args[3]);
            std::process::exit(1);
        }))
    } else {
        None
    };

    println!("=== MEMO-CHAT CREATE GROUP TEST ===");
    println!("Burn amount: {} tokens ({} units, decimal=6)", burn_amount_tokens, burn_amount);
    println!("Test type: {}", test_type);
    if let Some(length) = custom_memo_length {
        println!("Custom memo length: {} bytes", length);
    }
    println!();

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program addresses
    let memo_chat_program_id = Pubkey::from_str("54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF")
        .expect("Invalid memo-chat program ID");
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid memo-burn program ID");
    let mint = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");

    // Calculate global counter PDA
    let (global_counter_pda, _bump) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_chat_program_id,
    );

    // Get current group count to determine next group_id
    let next_group_id = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            // Parse the account data to get total_groups
            if account.data.len() >= 16 { // 8 bytes discriminator + 8 bytes u64
                let total_groups_bytes = &account.data[8..16];
                u64::from_le_bytes(total_groups_bytes.try_into().unwrap())
            } else {
                println!("⚠️  Global counter account exists but has invalid data length");
                0
            }
        },
        Err(_) => {
            println!("ℹ️  Global counter not found - this will be group 0 (or counter needs initialization)");
            0
        }
    };

    // Calculate chat group PDA using the next group_id
    let (chat_group_pda, _bump) = Pubkey::find_program_address(
        &[b"chat_group", &next_group_id.to_le_bytes()],
        &memo_chat_program_id,
    );

    // Get user's token account
    let creator_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    println!("Program addresses:");
    println!("  Memo-chat program: {}", memo_chat_program_id);
    println!("  Memo-burn program: {}", memo_burn_program_id);
    println!("  Mint: {}", mint);
    println!("  Global counter PDA: {}", global_counter_pda);
    println!("  Next group ID: {}", next_group_id);
    println!("  Chat group PDA: {}", chat_group_pda);
    println!("  Creator token account: {}", creator_token_account);
    println!();

    // Check if group already exists (should not happen with auto-increment)
    match client.get_account(&chat_group_pda) {
        Ok(_) => {
            println!("❌ ERROR: Chat group {} already exists!", next_group_id);
            println!("   This suggests there's an issue with the group counter or PDA derivation");
            return Ok(());
        },
        Err(_) => {
            println!("✅ Group ID {} is available", next_group_id);
        }
    }

    // Check token balance
    match client.get_token_account_balance(&creator_token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!("Current token balance: {} tokens", current_balance);
            
            if current_balance < burn_amount_tokens as f64 {
                println!("❌ ERROR: Insufficient token balance!");
                println!("   Required: {} tokens", burn_amount_tokens);
                println!("   Available: {} tokens", current_balance);
                return Ok(());
            }
        },
        Err(err) => {
            println!("❌ Error checking token balance: {}", err);
            return Ok(());
        }
    }

    // Generate memo based on test type and get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let memo_result = generate_memo_for_test(test_type, next_group_id, burn_amount, custom_memo_length);
    
    match memo_result {
        Ok(memo_text) => {
            println!("Generated memo:");
            println!("  Length: {} bytes", memo_text.as_bytes().len());
            
            // Show memo content appropriately
            if memo_text.len() > 200 {
                println!("  Content (first 100 chars): {}...", &memo_text[..100]);
                println!("  Content (last 100 chars): ...{}", &memo_text[memo_text.len()-100..]);
            } else {
                println!("  Content: {}", memo_text);
            }
            println!();

            // Create memo instruction
            let memo_ix = spl_memo::build_memo(
                memo_text.as_bytes(),
                &[&payer.pubkey()],
            );

            // Create create_chat_group instruction
            let create_group_ix = create_chat_group_instruction(
                &memo_chat_program_id,
                &payer.pubkey(),
                &global_counter_pda,
                &chat_group_pda,
                &mint,
                &creator_token_account,
                &memo_burn_program_id,
                next_group_id, // Pass the expected group_id
                burn_amount,
            );

            // Simulate transaction to get optimal CU limit
            let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(600_000);
            let sim_transaction = Transaction::new_signed_with_payer(
                &[dummy_compute_budget_ix, memo_ix.clone(), create_group_ix.clone()],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            println!("Simulating transaction to calculate optimal compute units...");
            let optimal_cu = match client.simulate_transaction_with_config(
                &sim_transaction,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: false,
                    commitment: Some(CommitmentConfig::confirmed()),
                    encoding: None,
                    accounts: None,
                    min_context_slot: None,
                    inner_instructions: false,
                },
            ) {
                Ok(result) => {
                    if let Some(err) = result.value.err {
                        println!("Simulation shows expected error: {:?}", err);
                        let default_cu = 500_000u32;
                        println!("Using default compute units: {}", default_cu);
                        default_cu
                    } else if let Some(units_consumed) = result.value.units_consumed {
                        let optimal_cu = ((units_consumed as f64) * 1.2) as u32; // 20% margin for group creation
                        println!("Simulation consumed {} CUs, setting limit to {} CUs (+20% margin)", 
                            units_consumed, optimal_cu);
                        optimal_cu
                    } else {
                        let default_cu = 500_000u32;
                        println!("Simulation successful but no CU data, using default: {}", default_cu);
                        default_cu
                    }
                },
                Err(err) => {
                    println!("Simulation failed: {}, using default CU", err);
                    500_000u32
                }
            };

            // Create transaction with optimal compute budget
            let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
            let transaction = Transaction::new_signed_with_payer(
                &[compute_budget_ix, memo_ix, create_group_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            send_and_check_transaction(&client, transaction, test_type, &chat_group_pda, next_group_id, burn_amount_tokens, memo_text.as_bytes().len());
        },
        Err(_) => {
            // For no-memo test case
            println!("Testing without memo instruction");
            println!();

            // Create create_chat_group instruction without memo
            let create_group_ix = create_chat_group_instruction(
                &memo_chat_program_id,
                &payer.pubkey(),
                &global_counter_pda,
                &chat_group_pda,
                &mint,
                &creator_token_account,
                &memo_burn_program_id,
                next_group_id, // Pass the next_group_id
                burn_amount,
            );

            // Simulate and send transaction without memo
            let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(600_000);
            let sim_transaction = Transaction::new_signed_with_payer(
                &[dummy_compute_budget_ix, create_group_ix.clone()],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            println!("Simulating transaction to calculate optimal compute units...");
            let optimal_cu = match client.simulate_transaction_with_config(
                &sim_transaction,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: false,
                    commitment: Some(CommitmentConfig::confirmed()),
                    encoding: None,
                    accounts: None,
                    min_context_slot: None,
                    inner_instructions: false,
                },
            ) {
                Ok(result) => {
                    if let Some(err) = result.value.err {
                        println!("Simulation shows expected error: {:?}", err);
                        500_000u32
                    } else if let Some(units_consumed) = result.value.units_consumed {
                        ((units_consumed as f64) * 1.2) as u32
                    } else {
                        500_000u32
                    }
                },
                Err(_) => 500_000u32
            };

            let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
            let transaction = Transaction::new_signed_with_payer(
                &[compute_budget_ix, create_group_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            send_and_check_transaction(&client, transaction, test_type, &chat_group_pda, next_group_id, burn_amount_tokens, 0);
        }
    }

    Ok(())
}

fn generate_memo_for_test(
    test_type: &str, 
    group_id: u64, 
    burn_amount: u64, 
    custom_length: Option<usize>
) -> Result<String, String> {
    match test_type {
        "valid-memo" => {
            let memo_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "Test Chat Group",
                "description": "A test group for memo-chat contract testing with comprehensive fields",
                "tags": ["test", "crypto", "chat"],
                "min_memo_interval": 60,
                "operation": "create_group",
                "timestamp": chrono::Utc::now().timestamp()
            });
            Ok(serde_json::to_string(&memo_json).unwrap())
        },
        "memo-69" => {
            // Create a memo that's exactly 69 bytes with all required fields
            let mut memo_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "",
                "description": "",
                "tags": [],
                "min_memo_interval": 60
            });
            
            // Calculate current length and adjust name to reach exactly 69 bytes
            let mut current_str = serde_json::to_string(&memo_json).unwrap();
            let target_length = 69;
            
            if current_str.len() < target_length {
                let needed_chars = target_length - current_str.len() + 2; // +2 for quotes
                let padding = "x".repeat(needed_chars);
                memo_json["name"] = serde_json::Value::String(padding);
            } else if current_str.len() > target_length {
                // If it's already too long, make minimal memo
                memo_json = serde_json::json!({
                    "amount": burn_amount,
                    "group_id": group_id,
                    "name": "T",
                    "description": "",
                    "tags": []
                });
            }
            
            let result = serde_json::to_string(&memo_json).unwrap();
            println!("Generated {}-byte memo (target: 69)", result.as_bytes().len());
            Ok(result)
        },
        "memo-800" => {
            // Create a memo that's exactly 800 bytes with all required fields
            let base_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "Test Group for 800 Byte Memo",
                "description": "",
                "tags": ["test", "max-length"],
                "min_memo_interval": 60
            });
            let base_str = serde_json::to_string(&base_json).unwrap();
            let target_length = 800;
            
            if base_str.len() < target_length {
                let needed_chars = target_length - base_str.len() + 2; // +2 for quotes around description
                let padding = "x".repeat(needed_chars);
                
                let memo_json = serde_json::json!({
                    "amount": burn_amount,
                    "group_id": group_id,
                    "name": "Test Group for 800 Byte Memo",
                    "description": padding,
                    "tags": ["test", "max-length"],
                    "min_memo_interval": 60
                });
                let result = serde_json::to_string(&memo_json).unwrap();
                println!("Generated {}-byte memo (target: 800)", result.as_bytes().len());
                Ok(result)
            } else {
                println!("Base memo already {} bytes, using as-is", base_str.len());
                Ok(base_str)
            }
        },
        "short-memo" => {
            // Create a memo shorter than 69 bytes (should fail) - missing some required fields
            let memo_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "T"
            });
            Ok(serde_json::to_string(&memo_json).unwrap())
        },
        "long-memo" => {
            // Create a memo longer than 800 bytes (should fail)
            let long_description = "x".repeat(850);
            let memo_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "Test Group with Very Long Description",
                "description": long_description,
                "tags": ["test", "long-description", "should-fail", "exceeds-limit"],
                "min_memo_interval": 60
            });
            Ok(serde_json::to_string(&memo_json).unwrap())
        },
        "amount-mismatch" => {
            // Create memo with wrong amount (should fail)
            let wrong_amount = burn_amount + 1_000_000; // Add 1 token
            let memo_json = serde_json::json!({
                "amount": wrong_amount,
                "group_id": group_id,
                "name": "Amount Mismatch Test Group",
                "description": "Testing amount validation - this memo has wrong amount",
                "tags": ["test", "amount-mismatch"],
                "min_memo_interval": 60,
                "operation": "create_group"
            });
            Ok(serde_json::to_string(&memo_json).unwrap())
        },
        "custom-length" => {
            // Create a memo with custom specified length
            let target_length = custom_length.unwrap_or(100);
            
            let base_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "Custom Length Test",
                "description": "",
                "tags": ["custom"],
                "min_memo_interval": 60,
                "operation": "create_group"
            });
            let base_str = serde_json::to_string(&base_json).unwrap();
            
            if target_length <= base_str.len() {
                // If target is smaller, create minimal memo with required fields
                let memo_json = serde_json::json!({
                    "amount": burn_amount,
                    "group_id": group_id,
                    "name": "x".repeat(std::cmp::max(1, target_length.saturating_sub(60))),
                    "description": "",
                    "tags": []
                });
                return Ok(serde_json::to_string(&memo_json).unwrap());
            }
            
            // Calculate padding needed for description field
            let needed_chars = target_length - base_str.len() + 2; // +2 for quotes
            let padding = "x".repeat(needed_chars);
            
            let memo_json = serde_json::json!({
                "amount": burn_amount,
                "group_id": group_id,
                "name": "Custom Length Test",
                "description": padding,
                "tags": ["custom"],
                "min_memo_interval": 60,
                "operation": "create_group"
            });
            
            let result = serde_json::to_string(&memo_json).unwrap();
            println!("Attempted to create {}-byte memo, actual length: {} bytes", 
                target_length, result.as_bytes().len());
            Ok(result)
        },
        "no-memo" => {
            // Return error to indicate no memo should be included
            Err("no-memo".to_string())
        },
        _ => {
            println!("Unknown test type: {}", test_type);
            std::process::exit(1);
        }
    }
}

fn create_chat_group_instruction(
    program_id: &Pubkey,
    creator: &Pubkey,
    global_counter: &Pubkey,
    chat_group: &Pubkey,
    mint: &Pubkey,
    creator_token_account: &Pubkey,
    memo_burn_program: &Pubkey,
    expected_group_id: u64,
    burn_amount: u64,
) -> Instruction {
    // Calculate Anchor instruction sighash for "create_chat_group"
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_chat_group");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add expected_group_id (u64)
    instruction_data.extend_from_slice(&expected_group_id.to_le_bytes());
    
    // Add burn_amount (u64)
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*creator, true),                          // creator (signer)
        AccountMeta::new(*global_counter, false),                  // global_counter (PDA)
        AccountMeta::new(*chat_group, false),                      // chat_group (PDA)
        AccountMeta::new(*mint, false),                            // mint
        AccountMeta::new(*creator_token_account, false),           // creator_token_account
        AccountMeta::new_readonly(token_2022_id(), false),         // token_program
        AccountMeta::new_readonly(*memo_burn_program, false),      // memo_burn_program
        AccountMeta::new_readonly(system_program::id(), false),    // system_program
        AccountMeta::new_readonly(
            Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
            false
        ), // instructions sysvar
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn send_and_check_transaction(
    client: &RpcClient,
    transaction: Transaction,
    test_type: &str,
    chat_group_pda: &Pubkey,
    group_id: u64,
    burn_amount_tokens: u64,
    memo_length: usize
) {
    println!("Sending create chat group transaction...");
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 TRANSACTION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            
            // Check if this should have succeeded
            match test_type {
                "valid-memo" | "memo-69" | "memo-800" => {
                    println!("✅ EXPECTED SUCCESS: {} test passed", test_type);
                    println!("Chat group {} created successfully!", group_id);
                    println!("Burned {} tokens for group creation", burn_amount_tokens);
                },
                "custom-length" => {
                    println!("✅ CUSTOM LENGTH SUCCESS: {}-byte memo test passed", memo_length);
                    println!("Chat group {} created successfully!", group_id);
                    
                    if memo_length < 69 {
                        println!("⚠️  Note: Memo < 69 bytes succeeded (unexpected)");
                    } else if memo_length > 800 {
                        println!("⚠️  Note: Memo > 800 bytes succeeded (unexpected)");
                    } else {
                        println!("✅ Memo length within expected range (69-800 bytes)");
                    }
                },
                _ => {
                    println!("❌ UNEXPECTED SUCCESS: {} test should have failed but succeeded", test_type);
                }
            }
            
            // Try to fetch the created group
            match client.get_account(chat_group_pda) {
                Ok(account) => {
                    println!("✅ Chat group account created:");
                    println!("   PDA: {}", chat_group_pda);
                    println!("   Data length: {} bytes", account.data.len());
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created group: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            // Check if this failure was expected
            match test_type {
                "no-memo" | "short-memo" | "long-memo" | "amount-mismatch" => {
                    println!("✅ EXPECTED FAILURE: {} test correctly failed", test_type);
                    print_specific_error_for_test(test_type, &err.to_string());
                },
                "custom-length" => {
                    println!("📊 CUSTOM LENGTH FAILURE: {}-byte memo test failed", memo_length);
                    print_custom_length_analysis(memo_length, &err.to_string());
                },
                _ => {
                    println!("❌ UNEXPECTED FAILURE: {} test should have succeeded", test_type);
                    print_error_guidance(&err.to_string());
                }
            }
        }
    }
}

fn print_custom_length_analysis(memo_length: usize, error_msg: &str) {
    println!("📊 Custom length analysis for {} bytes:", memo_length);
    
    if memo_length < 69 {
        if error_msg.contains("MemoTooShort") {
            println!("✅ Expected: Contract correctly rejects memo < 69 bytes");
        } else {
            println!("⚠️  Unexpected error for short memo: {}", error_msg);
        }
    } else if memo_length > 800 {
        if error_msg.contains("MemoTooLong") {
            println!("✅ Expected: Contract correctly rejects memo > 800 bytes");
        } else {
            println!("⚠️  Unexpected error for long memo: {}", error_msg);
        }
    } else {
        println!("⚠️  Unexpected failure for memo within valid range (69-800): {}", error_msg);
    }
}

fn print_specific_error_for_test(test_type: &str, error_msg: &str) {
    match test_type {
        "no-memo" => {
            if error_msg.contains("MemoRequired") {
                println!("✅ Correct error: Contract properly requires memo instruction");
            } else {
                println!("⚠️  Unexpected error for no-memo test: {}", error_msg);
            }
        },
        "short-memo" => {
            if error_msg.contains("MemoTooShort") {
                println!("✅ Correct error: Contract properly rejects memo < 69 bytes");
            } else {
                println!("⚠️  Unexpected error for short-memo test: {}", error_msg);
            }
        },
        "long-memo" => {
            if error_msg.contains("MemoTooLong") {
                println!("✅ Correct error: Contract properly rejects memo > 800 bytes");
            } else {
                println!("⚠️  Unexpected error for long-memo test: {}", error_msg);
            }
        },
        "amount-mismatch" => {
            if error_msg.contains("AmountMismatch") {
                println!("✅ Correct error: Contract properly validates amount consistency");
            } else {
                println!("⚠️  Unexpected error for amount-mismatch test: {}", error_msg);
            }
        },
        _ => {
            println!("Unexpected test type: {}", test_type);
        }
    }
}

fn print_error_guidance(error_msg: &str) {
    println!("\n=== ERROR ANALYSIS ===");
    
    if error_msg.contains("MemoRequired") {
        println!("💡 Missing Memo: This contract requires a memo instruction.");
    } else if error_msg.contains("MemoTooShort") {
        println!("💡 Memo Too Short: Memo must be at least 69 bytes long.");
    } else if error_msg.contains("MemoTooLong") {
        println!("💡 Memo Too Long: Memo must not exceed 800 bytes.");
    } else if error_msg.contains("AmountMismatch") {
        println!("💡 Amount Mismatch: The amount field in memo doesn't match burn amount.");
    } else if error_msg.contains("InvalidGroupIdFormat") {
        println!("💡 Invalid Group ID Format: Group ID must be a valid u64 number.");
    } else if error_msg.contains("InvalidGroupName") {
        println!("💡 Invalid Group Name: Name must be 1-64 characters.");
    } else if error_msg.contains("BurnAmountTooSmall") {
        println!("💡 Burn Amount Too Small: Must burn at least 1 token (1,000,000 units).");
    } else if error_msg.contains("already in use") {
        println!("💡 Group Already Exists: This group ID is already taken.");
    } else {
        println!("💡 Error: {}", error_msg);
    }
} 