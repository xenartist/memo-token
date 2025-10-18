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
use borsh::{BorshDeserialize, BorshSerialize, BorshSchema};
use base64::{Engine as _, engine::general_purpose};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Borsh memo structure (must match the contract)
#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug)]
pub struct BurnMemo {
    /// version of the BurnMemo structure (for future compatibility)
    pub version: u8,
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    /// application payload (variable length, max 787 bytes)
    pub payload: Vec<u8>,
}

// Current version constant
const BURN_MEMO_VERSION: u8 = 1;

// Get RPC URL from environment or use default testnet
fn get_rpc_url() -> String {
    std::env::var("X1_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 {
        println!("Usage: cargo run -- <burn_amount> <test_type> [memo_length]");
        println!("Parameters:");
        println!("  burn_amount   - Number of tokens to burn (decimal=6)");
        println!("  test_type     - Type of memo test to perform");
        println!("  memo_length   - Custom memo length (only for custom-length test)");
        println!();
        println!("Test types:");
        println!("  valid-memo    - Valid memo (between 69-800 bytes) - should succeed");
        println!("  no-memo       - No memo instruction - should fail");
        println!("  short-memo    - Memo less than 69 bytes - should fail");
        println!("  long-memo     - Memo more than 800 bytes - should fail");
        println!("  custom-length - Custom memo length (requires memo_length parameter)");
        println!();
        println!("Examples:");
        println!("  cargo run -- 1 valid-memo           # Burn 1 token with valid memo");
        println!("  cargo run -- 2 custom-length 666    # Burn 2 tokens with 666-byte memo");
        println!("  cargo run -- 10 long-memo           # Burn 10 tokens with long memo (should fail)");
        return Ok(());
    }

    // Parse burn amount (first parameter)
    let burn_amount_tokens = args[1].parse::<u64>().unwrap_or_else(|_| {
        eprintln!("Error: Invalid burn amount '{}'", args[1]);
        std::process::exit(1);
    });
    // For decimal=6, multiply by 1,000,000 to get units
    let burn_amount = burn_amount_tokens * 1_000_000;

    // Parse test type (second parameter)
    let test_type = &args[2];

    // Parse custom memo length (third parameter, only for custom-length test)
    let custom_memo_length = if test_type == "custom-length" {
        if args.len() < 4 {
            println!("ERROR: custom-length test requires memo_length parameter");
            println!("Usage: cargo run -- <burn_amount> custom-length <memo_length>");
            println!("Example: cargo run -- 1 custom-length 800");
            return Ok(());
        }
        Some(args[3].parse::<usize>().unwrap_or_else(|_| {
            eprintln!("Error: Invalid memo length '{}'", args[3]);
            std::process::exit(1);
        }))
    } else {
        None
    };

    println!("=== MEMO-BURN CONTRACT TEST (BORSH+BASE64 FORMAT) ===");
    println!("Burn amount: {} tokens ({} units, decimal=6)", burn_amount_tokens, burn_amount);
    println!("Test type: {}", test_type);
    if let Some(length) = custom_memo_length {
        println!("Custom memo length: {} bytes", length);
    }
    println!();

    // Connect to network
    let rpc_url = get_rpc_url();
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and token addresses
    let program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &program_id,
    );

    // Check if user global burn statistics account exists
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("✅ User global burn statistics account found: {}", user_global_burn_stats_pda);
        },
        Err(_) => {
            println!("❌ User global burn statistics account not found: {}", user_global_burn_stats_pda);
            println!("💡 Please run init-user-global-burn-stats first:");
            println!("   cargo run --bin init-user-global-burn-stats");
            return Ok(());
        }
    }

    // Check token balance
    match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!("Current token balance: {} tokens", current_balance);
            
            if current_balance < burn_amount_tokens as f64 {
                println!("ERROR: Insufficient token balance!");
                println!("Requested burn: {} tokens", burn_amount_tokens);
                println!("Available balance: {} tokens", current_balance);
                return Ok(());
            }
        },
        Err(err) => {
            println!("Error checking token balance: {}", err);
            return Ok(());
        }
    }

    // Create instruction data for process_burn
    let discriminator = [220, 214, 24, 210, 116, 16, 167, 18];
    let mut instruction_data = discriminator.to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // Build accounts list
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),        // user (signer)
        AccountMeta::new(mint, false),                 // mint
        AccountMeta::new(token_account, false),        // token_account
        AccountMeta::new(user_global_burn_stats_pda, false), // user_global_burn_stats
        AccountMeta::new_readonly(token_2022_id(), false), // token_program
        AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(),false), // instructions sysvar
    ];

    // Create burn instruction
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Generate memo based on test type and simulate to get CU requirements
    let memo_result = generate_memo_for_test(test_type, burn_amount, custom_memo_length);
    
    match memo_result {
        Ok(memo_bytes) => {
            println!("Base64-encoded memo length: {} bytes", memo_bytes.len());
            
            // Show memo structure info by decoding the Base64 first
            if let Ok(base64_str) = std::str::from_utf8(&memo_bytes) {
                if let Ok(decoded_data) = general_purpose::STANDARD.decode(base64_str) {
                    println!("Decoded Borsh data length: {} bytes", decoded_data.len());
                    
                    // Try to deserialize to show structure
                    if decoded_data.len() >= 13 {
                        if let Ok(borsh_memo) = BurnMemo::try_from_slice(&decoded_data) {
                            println!("Borsh memo structure:");
                            println!("  version: {}", borsh_memo.version);
                            println!("  burn_amount: {} units ({} tokens)", borsh_memo.burn_amount, borsh_memo.burn_amount / 1_000_000);
                            println!("  payload: {} bytes", borsh_memo.payload.len());
                            
                            // Show payload preview
                            if !borsh_memo.payload.is_empty() {
                                if let Ok(preview) = std::str::from_utf8(&borsh_memo.payload[..borsh_memo.payload.len().min(50)]) {
                                    println!("  payload preview (text): {}...", preview);
                                } else {
                                    println!("  payload preview (hex): {}...", hex::encode(&borsh_memo.payload[..borsh_memo.payload.len().min(32)]));
                                }
                            }
                        } else {
                            println!("Decoded data (hex, first 50): {:?}...", &decoded_data[..decoded_data.len().min(50)]);
                        }
                    } else {
                        println!("Decoded data too short: {:?}", decoded_data);
                    }
                } else {
                    println!("Failed to decode Base64, raw memo bytes (first 50): {:?}...", &memo_bytes[..memo_bytes.len().min(50)]);
                }
            } else {
                println!("Invalid UTF-8 in memo bytes");
            }
            println!();

            // Create memo instruction
            let memo_ix = spl_memo::build_memo(
                &memo_bytes,
                &[&payer.pubkey()],
            );

            // Simulate transaction to get optimal CU limit
            let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
            let sim_transaction = Transaction::new_signed_with_payer(
                &[dummy_compute_budget_ix, memo_ix.clone(), burn_ix.clone()],
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
                        // For expected failures in simulation, use default CU
                        println!("Simulation shows expected error: {:?}", err);
                        let default_cu = 300_000u32;
                        println!("Using default compute units: {}", default_cu);
                        default_cu
                    } else if let Some(units_consumed) = result.value.units_consumed {
                        // Add 10% safety margin to actual consumption
                        let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                        println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                            units_consumed, optimal_cu);
                        optimal_cu
                    } else {
                        let default_cu = 300_000u32;
                        println!("Simulation successful but no CU data, using default: {}", default_cu);
                        default_cu
                    }
                },
                Err(err) => {
                    println!("Simulation failed: {}, using default CU", err);
                    300_000u32
                }
            };

            // Create transaction with optimal compute budget
            let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
            let transaction = Transaction::new_signed_with_payer(
                &[compute_budget_ix, memo_ix, burn_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            send_and_check_transaction(&client, transaction, test_type, &token_account, burn_amount_tokens, memo_bytes.len());
        },
        Err(_) => {
            // For no-memo test case
            println!("Testing without memo instruction");
            println!();

            // Simulate transaction without memo to get CU requirements
            let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
            let sim_transaction = Transaction::new_signed_with_payer(
                &[dummy_compute_budget_ix, burn_ix.clone()],
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
                        let default_cu = 300_000u32;
                        println!("Using default compute units: {}", default_cu);
                        default_cu
                    } else if let Some(units_consumed) = result.value.units_consumed {
                        let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                        println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                            units_consumed, optimal_cu);
                        optimal_cu
                    } else {
                        let default_cu = 300_000u32;
                        println!("Simulation successful but no CU data, using default: {}", default_cu);
                        default_cu
                    }
                },
                Err(err) => {
                    println!("Simulation failed: {}, using default CU", err);
                    300_000u32
                }
            };

            // Create transaction without memo with optimal compute budget
            let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
            let transaction = Transaction::new_signed_with_payer(
                &[compute_budget_ix, burn_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            send_and_check_transaction(&client, transaction, test_type, &token_account, burn_amount_tokens, 0);
        }
    }

    Ok(())
}

fn generate_memo_for_test(test_type: &str, burn_amount: u64, custom_length: Option<usize>) -> Result<Vec<u8>, String> {
    match test_type {
        "valid-memo" => {
            // Create valid Borsh memo with reasonable user data
            let user_data = b"Testing memo-burn contract with Borsh+Base64 format. This is valid user data for burn operation.".to_vec();
            let memo = BurnMemo {
                version: BURN_MEMO_VERSION,
                burn_amount,
                payload: user_data,
            };
            let borsh_data = borsh::to_vec(&memo).unwrap();
            let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
            Ok(base64_encoded.into_bytes())
        },
        "short-memo" => {
            // Create memo less than 69 bytes (should fail)
            Ok(b"short".to_vec()) // 5 bytes, definitely too short
        },
        "long-memo" => {
            // Create memo more than 800 bytes (should fail)
            let large_data = vec![b'x'; 1000]; // Much larger than needed
            let memo = BurnMemo {
                version: BURN_MEMO_VERSION,
                burn_amount,
                payload: large_data,
            };
            let borsh_data = borsh::to_vec(&memo).unwrap();
            let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
            Ok(base64_encoded.into_bytes())
        },
        "custom-length" => {
            let target_length = custom_length.unwrap_or(100);
            
            if target_length < 13 {
                // Too small for valid Base64 of Borsh structure
                Ok(vec![b'x'; target_length])
            } else {
                // Calculate approximate raw data size: target_length / 1.33 (Base64 overhead)
                let estimated_raw_size = (target_length as f64 / 1.33) as usize;
                
                if estimated_raw_size < 13 {
                    // Too small for valid Borsh structure
                    Ok(vec![b'x'; target_length])
                } else {
                    // Calculate user data size: estimated_raw_size - 13 (Borsh overhead)
                    let user_data_size = estimated_raw_size.saturating_sub(13);
                    let user_data = if user_data_size > 0 {
                        let pattern = b"TestData123";
                        let mut data = Vec::with_capacity(user_data_size);
                        for i in 0..user_data_size {
                            data.push(pattern[i % pattern.len()]);
                        }
                        data
                    } else {
                        vec![]
                    };
                    
                    let memo = BurnMemo {
                        version: BURN_MEMO_VERSION,
                        burn_amount,
                        payload: user_data,
                    };
                    let borsh_data = borsh::to_vec(&memo).unwrap();
                    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
                    
                    println!("Generated Base64 memo: {} bytes (target: {}, from {} bytes Borsh)", 
                        base64_encoded.len(), target_length, borsh_data.len());
                    Ok(base64_encoded.into_bytes())
                }
            }
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

fn send_and_check_transaction(
    client: &RpcClient,
    transaction: Transaction,
    test_type: &str,
    token_account: &Pubkey,
    burn_amount_tokens: u64,
    memo_length: usize
) {
    println!("Sending burn transaction...");
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🔥 TRANSACTION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            
            // Check if this should have succeeded
            match test_type {
                "valid-memo" => {
                    println!("✅ EXPECTED SUCCESS: {} test passed", test_type);
                    println!("Burned {} tokens successfully", burn_amount_tokens);
                },
                "custom-length" => {
                    println!("✅ CUSTOM LENGTH SUCCESS: {}-byte memo test passed", memo_length);
                    println!("Burned {} tokens successfully", burn_amount_tokens);
                    
                    // Analysis of custom length result
                    if memo_length < 69 {
                        println!("⚠️  Note: Memo < 69 bytes succeeded (unexpected if contract enforces minimum)");
                    } else if memo_length > 800 {
                        println!("⚠️  Note: Memo > 800 bytes succeeded (unexpected if contract enforces maximum)");
                    } else {
                        println!("✅ Memo length within expected range (69-800 bytes)");
                    }
                },
                _ => {
                    println!("✅ SUCCESS: {} test passed", test_type);
                    println!("Burned {} tokens successfully", burn_amount_tokens);
                }
            }
            
            // Check new balance
            if let Ok(balance) = client.get_token_account_balance(token_account) {
                println!("New token balance: {} tokens", balance.ui_amount.unwrap_or(0.0));
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            // Check if this failure was expected
            match test_type {
                "no-memo" | "short-memo" | "long-memo" => {
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
        if error_msg.contains("Custom(6010)") || error_msg.contains("MemoTooShort") {
            println!("✅ Expected: Contract correctly rejects memo < 69 bytes");
        } else {
            println!("⚠️  Unexpected error for short memo: {}", error_msg);
        }
    } else if memo_length > 800 {
        if error_msg.contains("Custom(6011)") || error_msg.contains("MemoTooLong") {
            println!("✅ Expected: Contract correctly rejects memo > 800 bytes");
        } else if error_msg.contains("UserDataTooLong") {
            println!("✅ Expected: Contract correctly rejects user data > 788 bytes");
        } else if error_msg.contains("Program failed to complete") {
            println!("⚠️  System limit: Memo might exceed system-level limits");
            println!("   This could be a Solana transaction size limit (~1232 bytes total)");
        } else {
            println!("⚠️  Unexpected error for long memo: {}", error_msg);
        }
    } else {
        println!("⚠️  Unexpected failure for memo within valid range (69-800): {}", error_msg);
    }
    
    // General system limit analysis
    if memo_length > 1000 {
        println!("💡 Note: Very large memos may hit Solana transaction size limits");
        println!("   Maximum transaction size is ~1232 bytes including all instructions");
    }
}

fn print_specific_error_for_test(test_type: &str, error_msg: &str) {
    match test_type {
        "no-memo" => {
            if error_msg.contains("Custom(6000)") || error_msg.contains("MemoRequired") {
                println!("✅ Correct error: Contract properly requires memo instruction");
            } else {
                println!("⚠️  Unexpected error for no-memo test: {}", error_msg);
            }
        },
        "short-memo" => {
            if error_msg.contains("Custom(6010)") || error_msg.contains("MemoTooShort") {
                println!("✅ Correct error: Contract properly rejects memo < 69 bytes");
            } else {
                println!("⚠️  Unexpected error for short-memo test: {}", error_msg);
            }
        },
        "long-memo" => {
            if error_msg.contains("Custom(6011)") || error_msg.contains("MemoTooLong") {
                println!("✅ Correct error: Contract properly rejects memo > 800 bytes");
            } else if error_msg.contains("UserDataTooLong") {
                println!("✅ Correct error: Contract properly rejects user data > 788 bytes");
            } else {
                println!("⚠️  Unexpected error for long-memo test: {}", error_msg);
            }
        },
        _ => {
            println!("Unexpected test type: {}", test_type);
        }
    }
}

fn print_error_guidance(error_msg: &str) {
    println!("\n=== ERROR ANALYSIS (BORSH+BASE64 FORMAT) ===");
    
    if error_msg.contains("Custom(6000)") || error_msg.contains("MemoRequired") {
        println!("💡 Missing Memo: This contract requires a memo instruction.");
    } else if error_msg.contains("Custom(6010)") || error_msg.contains("MemoTooShort") {
        println!("💡 Memo Too Short: Memo must be at least 69 bytes long.");
    } else if error_msg.contains("Custom(6011)") || error_msg.contains("MemoTooLong") {
        println!("💡 Memo Too Long: Memo must not exceed 800 bytes.");
    } else if error_msg.contains("Custom(6001)") || error_msg.contains("InvalidMemoFormat") {
        println!("💡 Invalid Memo Format: Expected Base64-encoded Borsh-serialized BurnMemo structure");
        println!("   Structure: {{ version: u8, burn_amount: u64, payload: Vec<u8> }}");
        println!("   Process: Borsh serialize -> Base64 encode -> UTF-8 memo");
    } else if error_msg.contains("Custom(6002)") || error_msg.contains("UnsupportedMemoVersion") {
        println!("💡 Unsupported Version: Memo version mismatch (expected version 1)");
        println!("   Make sure to use the current BurnMemo structure version");
    } else if error_msg.contains("Custom(6012)") || error_msg.contains("UserDataTooLong") {
        println!("💡 User Data Too Long: User data must not exceed 787 bytes");
        println!("   Total memo size = 1 (version) + 8 (burn_amount) + 4 (vec length) + user_data.len()");
    } else if error_msg.contains("Custom(6005)") || error_msg.contains("BurnAmountMismatch") {
        println!("💡 Burn Amount Mismatch: burn_amount in memo must match actual burn amount.");
    } else if error_msg.contains("Custom(6003)") || error_msg.contains("UnauthorizedMint") {
        println!("💡 Wrong Mint: Only authorized mint can be used.");
        println!("   Expected: HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");
    } else {
        println!("💡 Error: {}", error_msg);
    }
} 
