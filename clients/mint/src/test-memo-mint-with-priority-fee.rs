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
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};
use sha2::{Sha256, Digest};
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint, validate_mint_amount};

// Default priority fee in micro-lamports per CU
const DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS: u64 = 100_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Mint Test Client (WITH Priority Fee) ===\n");
    println!("💰 This client sets BOTH compute unit limits AND priority fees");
    println!("   Testing if mint operations work with priority fee settings\n");
    
    // Get command line arguments for test scenario
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_help(&args[0]);
        return Ok(());
    }
    
    let test_scenario = &args[1];
    
    // Parse priority fee from command line (optional)
    let priority_fee = if args.len() > 2 && test_scenario != "custom-length" {
        match args[2].parse::<u64>() {
            Ok(fee) => {
                println!("Using custom priority fee: {} micro-lamports per CU\n", fee);
                fee
            },
            Err(_) => {
                println!("Invalid priority fee value, using default: {} micro-lamports per CU\n", DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS);
                DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS
            }
        }
    } else {
        println!("Using default priority fee: {} micro-lamports per CU\n", DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS);
        DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS
    };
    
    // Parse custom memo length for custom-length test
    let custom_memo_length = if args.len() > 2 && test_scenario == "custom-length" {
        Some(args[2].parse::<usize>().unwrap_or(100))
    } else if test_scenario == "custom-length" {
        println!("ERROR: custom-length test requires memo_length parameter");
        println!("Usage: cargo run -- custom-length <memo_length> [priority_fee]");
        println!("Example: cargo run -- custom-length 800 150000");
        return Ok(());
    } else {
        None
    };
    
    // Parse priority fee for custom-length test (third argument)
    let priority_fee_for_custom = if args.len() > 3 && test_scenario == "custom-length" {
        args[3].parse::<u64>().unwrap_or(priority_fee)
    } else {
        priority_fee
    };
    
    match test_scenario.as_str() {
        "no-memo" => test_no_memo(priority_fee),
        "short-memo" => test_short_memo(priority_fee),
        "valid-memo" => test_valid_memo(priority_fee),
        "long-memo" => test_long_memo(priority_fee),
        "memo-69" => test_memo_exact_69(priority_fee),
        "memo-800" => test_memo_exact_800(priority_fee),
        "custom-length" => test_custom_length(custom_memo_length.unwrap(), priority_fee_for_custom),
        "help" | _ => {
            print_help(&args[0]);
            Ok(())
        }
    }
}

fn print_help(program_name: &str) {
    println!("Usage: {} <test_scenario> [priority_fee_micro_lamports]", program_name);
    println!("Test scenarios:");
    println!("  no-memo         - Test mint without memo (should fail)");
    println!("  short-memo      - Test mint with memo < 69 bytes (should fail)");
    println!("  memo-69         - Test mint with memo exactly 69 bytes (should succeed)");
    println!("  valid-memo      - Test mint with memo of at least 69 bytes (should succeed)");
    println!("  memo-800        - Test mint with memo exactly 800 bytes (should succeed)");
    println!("  long-memo       - Test mint with memo too large for the transaction (should fail at the 1232-byte transaction size limit)");
    println!("  custom-length   - Test mint with custom memo length (requires memo_length parameter)");
    println!("\nPriority Fee (optional):");
    println!("  Specify priority fee in micro-lamports per CU");
    println!("  Default: {} micro-lamports", DEFAULT_PRIORITY_FEE_MICRO_LAMPORTS);
    println!("  Common values:");
    println!("    1,000 - 10,000   = Low priority");
    println!("    10,000 - 100,000 = Medium priority (default: 100,000)");
    println!("    100,000+         = High priority");
    println!("\nExamples:");
    println!("  {} valid-memo                    # Use default priority fee", program_name);
    println!("  {} valid-memo 200000             # High priority (200k micro-lamports)", program_name);
    println!("  {} custom-length 800 150000      # 800-byte memo with 150k priority fee", program_name);
    println!("  {} memo-69 50000                 # Low priority (50k micro-lamports)", program_name);
    println!("\n💰 Note: This client ALWAYS sets priority fees!");
    println!("   Total fee = CU_limit × priority_fee_per_CU / 1,000,000 lamports");
}

fn test_custom_length(target_length: usize, priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with CUSTOM LENGTH memo ({} bytes)...\n", target_length);
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance (raw lamports)
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    // Create memo with custom length
    let memo_text = create_memo_with_exact_length(target_length);
    let actual_length = memo_text.as_bytes().len();
    
    println!("Target memo length: {} bytes", target_length);
    println!("Actual memo length: {} bytes", actual_length);
    
    // Show memo content appropriately
    if actual_length > 200 {
        println!("Memo content (first 100 chars): {}...", &memo_text[..100]);
        println!("Memo content (last 100 chars): ...{}", &memo_text[memo_text.len()-100..]);
    } else {
        println!("Memo content: {}", memo_text);
    }
    println!();
    
    // Analyze expected result
    let expected_result = if actual_length < 69 {
        "FAIL (< 69 bytes)"
    } else if actual_length > 800 {
        "SUCCESS if the transaction fits in 1232 bytes (no contract maximum)"
    } else {
        "SUCCESS (>= 69 bytes)"
    };
    println!("Expected result: {}", expected_result);
    
    // Additional system limit warnings
    if actual_length > 1000 {
        println!("⚠️  Warning: Very large memo may hit Solana transaction size limits");
        println!("   Maximum transaction size is ~1232 bytes including all instructions");
    }
    println!();
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], &format!("Custom Length ({} bytes) Test", actual_length), priority_fee);
    
    match result {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESSFUL!");
            println!("   Signature: {}", signature);
            
            // Check token balance after mint
            let balance_after = get_token_balance_raw(&client, &token_account);
            let raw_minted = balance_after - balance_before;
            
            println!("   Token balance before: {} lamports ({})", balance_before, format_token_amount(balance_before));
            println!("   Token balance after:  {} lamports ({})", balance_after, format_token_amount(balance_after));
            println!("   Tokens minted: {} lamports ({})", raw_minted, format_token_amount(raw_minted));
            
            // Analyze result
            if actual_length < 69 {
                println!("   ❌ UNEXPECTED SUCCESS: Memo < 69 bytes should have failed");
                println!("   🔍 This suggests the contract may not be enforcing minimum length");
            } else {
                println!("   ✅ EXPECTED SUCCESS: Memo is at least 69 bytes (no contract maximum)");
                println!("   ✅ Transaction succeeded WITH priority fee!");
            }
            
            // Validate mint amount
            let (is_valid, description) = validate_mint_amount(raw_minted);
            if is_valid {
                println!("   ✅ Valid mint amount: {}", description);
            } else {
                println!("   ❌ {}", description);
            }
        },
        Err(e) => {
            println!("❌ TRANSACTION FAILED!");
            println!("   Error: {}", e);
            
            // Analyze failure
            if actual_length < 69 {
                if e.to_string().contains("Custom(6001)") || e.to_string().contains("MemoTooShort") {
                    println!("   ✅ EXPECTED FAILURE: Contract correctly rejects memo < 69 bytes");
                } else {
                    println!("   ⚠️  UNEXPECTED ERROR for short memo: {}", e);
                }
            } else if e.to_string().to_lowercase().contains("too large") {
                println!("   ✅ EXPECTED FAILURE: Transaction size limit (1232 bytes) exceeded; the contract has no maximum memo length");
            } else {
                if e.to_string().contains("SupplyLimitReached") {
                    println!("   ✅ EXPECTED FAILURE: Supply limit reached (10 trillion tokens)");
                } else {
                    println!("   ❌ UNEXPECTED FAILURE: Memo of at least 69 bytes should succeed");
                    println!("   🔍 Possible issues:");
                    println!("      - Contract bug");
                    println!("      - Network/RPC issue");
                    println!("      - Priority fee too low (unlikely)");
                    println!("      - Other validation failure");
                }
            }
        }
    }
    
    // Summary for custom length test
    println!("\n📊 CUSTOM LENGTH TEST SUMMARY:");
    println!("   Target length: {} bytes", target_length);
    println!("   Actual length: {} bytes", actual_length);
    println!("   Contract minimum: 69 bytes (no maximum; the transaction size limit applies)");
    println!("   System limit: ~1000+ bytes (varies)");
    println!("   Possible mint amounts: 1, 0.1, 0.01, 0.001, 0.0001, 0.00001, 0.000001 tokens");
    println!("   💰 Priority fee used: {} micro-lamports per CU", priority_fee);
    
    if actual_length != target_length {
        println!("   ⚠️  Note: Actual length differs from target due to JSON formatting");
    }
    
    Ok(())
}

fn test_no_memo(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint WITHOUT memo (expected to fail)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create mint instruction without memo
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![mint_ix], "No Memo Test", priority_fee);
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   The contract should require a memo instruction.");
        },
        Err(e) => {
            println!("✅ EXPECTED: Transaction failed as expected");
            println!("   Error: {}", e);
            println!("   This confirms the contract properly requires memo instructions.");
        }
    }
    
    Ok(())
}

fn test_short_memo(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with SHORT memo < 69 bytes (expected to fail)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create short memo (less than 69 bytes)
    let short_message = "Short memo test";
    let memo_json = serde_json::json!({
        "message": short_message,
        "test": "short-memo"
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (< 69 bytes)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], "Short Memo Test", priority_fee);
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   The contract should reject memos shorter than 69 bytes.");
        },
        Err(e) => {
            println!("✅ EXPECTED: Transaction failed as expected");
            println!("   Error: {}", e);
            println!("   This confirms the contract properly validates minimum memo length.");
        }
    }
    
    Ok(())
}

fn test_memo_exact_69(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with memo EXACTLY 69 bytes (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance (raw lamports)
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    // Create memo with exactly 69 bytes
    let memo_text = create_memo_with_exact_length(69);
    
    println!("Memo length: {} bytes (exactly 69 bytes)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], "Exact 69 Bytes Memo Test", priority_fee);
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITH priority fee!");
            
            // Check token balance after mint
            let balance_after = get_token_balance_raw(&client, &token_account);
            let raw_minted = balance_after - balance_before;
            
            println!("   Token balance before: {} lamports ({})", balance_before, format_token_amount(balance_before));
            println!("   Token balance after:  {} lamports ({})", balance_after, format_token_amount(balance_after));
            println!("   Tokens minted: {} lamports ({})", raw_minted, format_token_amount(raw_minted));
            
            // Validate mint amount
            let (is_valid, description) = validate_mint_amount(raw_minted);
            if is_valid {
                println!("   ✅ Valid mint amount: {}", description);
                println!("   ✅ Boundary condition (69 bytes) handled correctly");
            } else {
                println!("   ❌ {}", description);
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
            if e.to_string().contains("SupplyLimitReached") {
                println!("   ℹ️  Supply limit reached (10 trillion tokens)");
            } else {
                println!("   The contract should accept memos of exactly 69 bytes.");
            }
        }
    }
    
    Ok(())
}

fn test_valid_memo(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with VALID memo (at least 69 bytes) (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance (raw lamports)
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    // Create valid memo (at least 69 bytes)
    let message = "This is a valid memo test for the memo-mint contract. ".repeat(2);
    let memo_json = serde_json::json!({
        "message": message,
        "test": "valid-memo",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "additional_data": "padding_to_ensure_minimum_length_requirement_is_met"
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (at least 69 bytes)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], "Valid Memo Test", priority_fee);
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITH priority fee!");
            
            // Check token balance after mint
            let balance_after = get_token_balance_raw(&client, &token_account);
            let raw_minted = balance_after - balance_before;
            
            println!("   Token balance before: {} lamports ({})", balance_before, format_token_amount(balance_before));
            println!("   Token balance after:  {} lamports ({})", balance_after, format_token_amount(balance_after));
            println!("   Tokens minted: {} lamports ({})", raw_minted, format_token_amount(raw_minted));
            
            // Validate mint amount
            let (is_valid, description) = validate_mint_amount(raw_minted);
            if is_valid {
                println!("   ✅ Valid mint amount: {}", description);
            } else {
                println!("   ❌ {}", description);
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
            if e.to_string().contains("SupplyLimitReached") {
                println!("   ℹ️  Supply limit reached (10 trillion tokens)");
            }
        }
    }
    
    Ok(())
}

fn test_memo_exact_800(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with memo EXACTLY 800 bytes (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance (raw lamports)
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    // Create memo with exactly 800 bytes
    let memo_text = create_memo_with_exact_length(800);
    
    println!("Memo length: {} bytes (exactly 800 bytes)", memo_text.len());
    println!("Memo content preview: {}...", &memo_text[..100]);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], "Exact 800 Bytes Memo Test", priority_fee);
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITH priority fee!");
            
            // Check token balance after mint
            let balance_after = get_token_balance_raw(&client, &token_account);
            let raw_minted = balance_after - balance_before;
            
            println!("   Token balance before: {} lamports ({})", balance_before, format_token_amount(balance_before));
            println!("   Token balance after:  {} lamports ({})", balance_after, format_token_amount(balance_after));
            println!("   Tokens minted: {} lamports ({})", raw_minted, format_token_amount(raw_minted));
            
            // Validate mint amount
            let (is_valid, description) = validate_mint_amount(raw_minted);
            if is_valid {
                println!("   ✅ Valid mint amount: {}", description);
                println!("   ✅ Boundary condition (800 bytes) handled correctly");
            } else {
                println!("   ❌ {}", description);
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
            if e.to_string().contains("SupplyLimitReached") {
                println!("   ℹ️  Supply limit reached (10 trillion tokens)");
            } else {
                println!("   The contract should accept memos of exactly 800 bytes.");
            }
        }
    }
    
    Ok(())
}

fn test_long_memo(priority_fee: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with memo too large for the transaction (expected to fail at the 1232-byte size limit)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create a memo larger than a transaction can hold
    let long_message = "This is a very long memo test that exceeds the maximum allowed length. ".repeat(15);
    let memo_json = serde_json::json!({
        "message": long_message,
        "test": "long-memo",
        "additional_padding": "x".repeat(100)
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (more than fits in a 1232-byte transaction)", memo_text.len());
    println!("Memo content preview: {}...", &memo_text[..100]);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITH priority fee
    let result = execute_transaction_with_priority_fee(&client, &payer, vec![memo_ix, mint_ix], "Long Memo Test", priority_fee);
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   A memo this large should not fit in a 1232-byte transaction.");
        },
        Err(e) => {
            println!("✅ EXPECTED: Transaction failed as expected");
            println!("   Error: {}", e);
            println!("   The transaction size limit bounds the memo length (the contract has no maximum).");
        }
    }
    
    Ok(())
}

// Helper function to create memo with exact length
fn create_memo_with_exact_length(target_length: usize) -> String {
    let base_json = serde_json::json!({
        "test": "length-test",
        "target": target_length,
        "data": ""
    });
    
    let base_text = base_json.to_string();
    let base_length = base_text.len();
    
    if base_length >= target_length {
        // If base is already too long, create a simpler JSON
        let padding_size = target_length.saturating_sub(15); // Account for {"data":"..."}
        let simple_json = serde_json::json!({
            "data": "x".repeat(padding_size)
        });
        let mut result = simple_json.to_string();
        
        // Fine-tune to exact length
        while result.as_bytes().len() < target_length {
            result.push('x');
        }
        while result.as_bytes().len() > target_length {
            result.pop();
        }
        result
    } else {
        // Add padding to reach exact length
        let padding_needed = target_length - base_length + 2; // +2 for quotes around data
        let padding = "x".repeat(padding_needed);
        
        let final_json = serde_json::json!({
            "test": "length-test",
            "target": target_length,
            "data": padding
        });
        
        let mut result = final_json.to_string();
        
        // Fine-tune to exact length (account for JSON formatting differences)
        while result.as_bytes().len() < target_length {
            result.push('x');
        }
        while result.as_bytes().len() > target_length {
            result.pop();
        }
        
        result
    }
}

fn create_rpc_client() -> RpcClient {
    let rpc_url = get_rpc_url();
    println!("Connecting to: {}", rpc_url);
    RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed())
}

fn load_payer_keypair() -> solana_sdk::signature::Keypair {
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read payer keypair file");
    println!("Using payer: {}", payer.pubkey());
    payer
}

fn get_program_addresses() -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    // Program addresses
    let program_id = get_program_id("memo_mint").expect("Failed to get memo_mint program ID");
    let mint_address = get_token_mint("memo_token").expect("Failed to get memo_token mint address");
    
    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    // Calculate associated token account using Token-2022
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_address,
        &token_2022_id(),
    );
    
    println!("Program ID: {}", program_id);
    println!("Mint address: {}", mint_address);
    println!("Mint authority PDA: {}", mint_authority_pda);
    println!("Token account: {}", token_account);
    println!();
    
    (program_id, mint_address, mint_authority_pda, token_account)
}

fn ensure_token_account_exists(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    mint_address: &Pubkey,
    token_account: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if token account exists
    match client.get_account(token_account) {
        Ok(_) => {
            println!("✅ Token account already exists: {}", token_account);
        },
        Err(_) => {
            println!("⚠️  Token account not found, creating...");
            
            // Create associated token account instruction
            let create_ata_ix = create_associated_token_account(
                &payer.pubkey(),    // payer
                &payer.pubkey(),    // wallet (owner)
                mint_address,       // mint
                &token_2022_id(),   // token program (Token-2022)
            );
            
            // Get recent blockhash
            let recent_blockhash = client.get_latest_blockhash()?;
            
            // Create and send transaction
            let transaction = Transaction::new_signed_with_payer(
                &[create_ata_ix],
                Some(&payer.pubkey()),
                &[payer],
                recent_blockhash,
            );
            
            match client.send_and_confirm_transaction(&transaction) {
                Ok(signature) => {
                    println!("✅ Token account created successfully!");
                    println!("   Signature: {}", signature);
                    println!("   Account: {}", token_account);
                },
                Err(e) => {
                    return Err(format!("Failed to create token account: {}", e).into());
                }
            }
        }
    }
    
    Ok(())
}

fn create_mint_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
) -> Instruction {
    // Calculate Anchor instruction sighash for "process_mint"
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_mint");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let accounts = vec![
        AccountMeta::new(*user, true),                    // user (signer)
        AccountMeta::new(*mint, false),                   // mint
        AccountMeta::new_readonly(*mint_authority, false), // mint_authority (PDA)
        AccountMeta::new(*token_account, false),          // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program (Token-2022)
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
    ];
    
    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

/// Execute transaction WITH priority fee setting
/// This sets BOTH compute unit limit (via simulation) AND priority fee
fn execute_transaction_with_priority_fee(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
    test_name: &str,
    priority_fee_micro_lamports: u64,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("Executing {}...", test_name);
    println!("💰 Setting priority fee: {} micro-lamports per CU", priority_fee_micro_lamports);
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction for simulation with correct instruction order
    // IMPORTANT: Memo must be at index 0 for contract validation
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
    let mut sim_instructions = instructions.clone();
    sim_instructions.push(dummy_compute_budget_ix);
    
    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Simulate to get compute units
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
                // For expected failures, still need to send with reasonable CU
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
    
    // Calculate estimated priority fee cost
    let estimated_priority_fee = (optimal_cu as u64 * priority_fee_micro_lamports) / 1_000_000;
    println!("💰 Estimated total priority fee: {} lamports ({:.9} SOL)", 
        estimated_priority_fee, 
        estimated_priority_fee as f64 / 1_000_000_000.0);
    
    // Create compute budget instruction with optimal CU
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    
    // Create priority fee instruction
    let priority_fee_ix = ComputeBudgetInstruction::set_compute_unit_price(priority_fee_micro_lamports);
    
    // Create final transaction with BOTH compute budget AND priority fee
    // IMPORTANT: Instruction ordering to satisfy contract requirements:
    // - Index 0: memo instruction (contract REQUIRES memo at index 0)
    // - Index 1: mint instruction
    // - Index 2+: compute budget instructions (processed by Solana runtime before execution)
    let mut final_instructions = instructions;
    final_instructions.push(compute_budget_ix);
    final_instructions.push(priority_fee_ix);
    
    let transaction = Transaction::new_signed_with_payer(
        &final_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Send transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => Ok(signature.to_string()),
        Err(e) => Err(e.into()),
    }
}

fn get_token_balance_raw(client: &RpcClient, token_account: &Pubkey) -> u64 {
    match client.get_account(token_account) {
        Ok(account) => {
            // Parse the token account data to get the raw amount (in lamports)
            if account.data.len() >= 72 { // SPL Token account is 165 bytes, amount is at offset 64-72
                let amount_bytes = &account.data[64..72];
                u64::from_le_bytes(amount_bytes.try_into().unwrap_or([0; 8]))
            } else {
                0
            }
        },
        Err(_) => 0,
    }
}

fn format_token_amount(raw_amount: u64) -> String {
    // Convert raw lamports to tokens with 6 decimal places
    let tokens = raw_amount as f64 / 1_000_000.0;
    
    // Format to avoid floating point precision issues
    match raw_amount {
        1_000_000 => "1.0".to_string(),
        100_000 => "0.1".to_string(),
        10_000 => "0.01".to_string(),
        1_000 => "0.001".to_string(),
        100 => "0.0001".to_string(),
        10 => "0.00001".to_string(),
        1 => "0.000001".to_string(),
        0 => "0".to_string(),
        _ => format!("{:.6}", tokens), // Fallback for unexpected values
    }
}


