use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
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

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Mint Test Client (WITHOUT CU Setting) ===\n");
    println!("⚠️  This client does NOT set compute unit limits");
    println!("   Using default 400,000 CU limit (verified through testing)");
    println!("   Testing if mint operations work without explicit CU settings\n");
    
    // Get command line arguments for test scenario
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_help(&args[0]);
        return Ok(());
    }
    
    let test_scenario = &args[1];
    
    // Parse custom memo length for custom-length test
    let custom_memo_length = if args.len() > 2 && test_scenario == "custom-length" {
        Some(args[2].parse::<usize>().unwrap_or(100))
    } else if test_scenario == "custom-length" {
        println!("ERROR: custom-length test requires memo_length parameter");
        println!("Usage: cargo run -- custom-length <memo_length>");
        println!("Example: cargo run -- custom-length 800");
        return Ok(());
    } else {
        None
    };
    
    match test_scenario.as_str() {
        "no-memo" => test_no_memo(),
        "short-memo" => test_short_memo(),
        "valid-memo" => test_valid_memo(),
        "long-memo" => test_long_memo(),
        "memo-69" => test_memo_exact_69(),
        "memo-800" => test_memo_exact_800(),
        "custom-length" => test_custom_length(custom_memo_length.unwrap()),
        "help" | _ => {
            print_help(&args[0]);
            Ok(())
        }
    }
}

fn print_help(program_name: &str) {
    println!("Usage: {} <test_scenario> [memo_length]", program_name);
    println!("Test scenarios:");
    println!("  no-memo         - Test mint without memo (should fail)");
    println!("  short-memo      - Test mint with memo < 69 bytes (should fail)");
    println!("  memo-69         - Test mint with memo exactly 69 bytes (should succeed)");
    println!("  valid-memo      - Test mint with memo 69-800 bytes (should succeed)");
    println!("  memo-800        - Test mint with memo exactly 800 bytes (should succeed)");
    println!("  long-memo       - Test mint with memo > 800 bytes (should fail)");
    println!("  custom-length   - Test mint with custom memo length (requires memo_length parameter)");
    println!("\nExamples:");
    println!("  {} valid-memo", program_name);
    println!("  {} custom-length 800    # Test 800-byte memo", program_name);
    println!("  {} custom-length 50     # Test 50-byte memo", program_name);
    println!("  {} custom-length 1000   # Test 1000-byte memo", program_name);
    println!("\n⚠️  Note: This client does NOT set compute unit limits!");
    println!("   Using default 400,000 CU allocation (verified through testing).");
    println!("   This tests whether the contract works without explicit CU optimization.");
}

fn test_custom_length(target_length: usize) -> Result<(), Box<dyn std::error::Error>> {
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
        "FAIL (> 800 bytes)"
    } else {
        "SUCCESS (69-800 bytes)"
    };
    println!("Expected result: {}", expected_result);
    println!("Using default CU: 400,000 (no explicit setting)");
    println!();
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], &format!("Custom Length ({} bytes) Test", actual_length));
    
    match result {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESSFUL!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITHOUT explicit CU setting!");
            
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
            println!("❌ TRANSACTION FAILED!");
            println!("   Error: {}", e);
            
            // Analyze failure - check if it's CU related
            let error_str = e.to_string();
            if error_str.contains("exceeded") || error_str.contains("compute") || error_str.contains("units") {
                println!("   ⚠️  POSSIBLE CU ISSUE: Transaction may have exceeded default 400,000 CU limit");
                println!("   💡 Try using the standard test-memo-mint client with CU optimization");
            } else if actual_length < 69 {
                println!("   ✅ EXPECTED FAILURE: Memo < 69 bytes");
            } else if actual_length > 800 {
                println!("   ✅ EXPECTED FAILURE: Memo > 800 bytes");
            } else if error_str.contains("SupplyLimitReached") {
                println!("   ✅ EXPECTED FAILURE: Supply limit reached");
            } else {
                println!("   ❌ UNEXPECTED FAILURE for memo within valid range");
            }
        }
    }
    
    println!("\n📊 TEST SUMMARY:");
    println!("   Target length: {} bytes", target_length);
    println!("   Actual length: {} bytes", actual_length);
    println!("   CU limit: 400,000 (default, verified through testing)");
    println!("   Contract requirement: Memo at index 0");
    
    Ok(())
}

fn test_no_memo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint WITHOUT memo (expected to fail)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create mint instruction without memo
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![mint_ix], "No Memo Test");
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   The contract should require a memo instruction at index 0.");
        },
        Err(e) => {
            println!("✅ EXPECTED: Transaction failed as expected");
            println!("   Error: {}", e);
            println!("   This confirms the contract properly requires memo at index 0.");
        }
    }
    
    Ok(())
}

fn test_short_memo() -> Result<(), Box<dyn std::error::Error>> {
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
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], "Short Memo Test");
    
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

fn test_memo_exact_69() -> Result<(), Box<dyn std::error::Error>> {
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
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], "Exact 69 Bytes Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITHOUT explicit CU setting!");
            
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
            } else if e.to_string().contains("exceeded") || e.to_string().contains("compute") {
                println!("   ⚠️  Possible CU issue: May need explicit CU setting");
            } else {
                println!("   The contract should accept memos of exactly 69 bytes.");
            }
        }
    }
    
    Ok(())
}

fn test_valid_memo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with VALID memo (69-800 bytes) (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance (raw lamports)
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    // Create valid memo (between 69-800 bytes)
    let message = "This is a valid memo test for the memo-mint contract. ".repeat(2);
    let memo_json = serde_json::json!({
        "message": message,
        "test": "valid-memo",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "additional_data": "padding_to_ensure_minimum_length_requirement_is_met"
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (69-800 bytes range)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], "Valid Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITHOUT explicit CU setting!");
            
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
            } else if e.to_string().contains("exceeded") || e.to_string().contains("compute") {
                println!("   ⚠️  Possible CU issue: May need explicit CU setting");
            }
        }
    }
    
    Ok(())
}

fn test_memo_exact_800() -> Result<(), Box<dyn std::error::Error>> {
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
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], "Exact 800 Bytes Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            println!("   ✅ Transaction succeeded WITHOUT explicit CU setting!");
            
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
            } else if e.to_string().contains("exceeded") || e.to_string().contains("compute") {
                println!("   ⚠️  Possible CU issue: May need explicit CU setting");
            } else {
                println!("   The contract should accept memos of exactly 800 bytes.");
            }
        }
    }
    
    Ok(())
}

fn test_long_memo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with LONG memo > 800 bytes (expected to fail)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create long memo (more than 800 bytes)
    let long_message = "This is a very long memo test that exceeds the maximum allowed length. ".repeat(15);
    let memo_json = serde_json::json!({
        "message": long_message,
        "test": "long-memo",
        "additional_padding": "x".repeat(100)
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (> 800 bytes)", memo_text.len());
    println!("Memo content preview: {}...", &memo_text[..100]);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction WITHOUT CU setting
    let result = execute_transaction_without_cu(&client, &payer, vec![memo_ix, mint_ix], "Long Memo Test");
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   The contract should reject memos longer than 800 bytes.");
        },
        Err(e) => {
            println!("✅ EXPECTED: Transaction failed as expected");
            println!("   Error: {}", e);
            println!("   This confirms the contract properly validates maximum memo length.");
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

/// Execute transaction WITHOUT setting compute unit limits
/// This uses the default 400,000 CU limit (verified through testing)
fn execute_transaction_without_cu(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
    test_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("Executing {}...", test_name);
    println!("⚠️  NOT setting compute unit limits - using default 400,000 CU");
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction directly without compute budget instructions
    // Instruction order: memo (index 0), mint (index 1)
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    println!("Transaction size: {} bytes", bincode::serialize(&transaction)?.len());
    println!("Instruction count: {}", instructions.len());
    
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

fn validate_mint_amount(raw_amount: u64) -> (bool, String) {
    match raw_amount {
        1_000_000 => (true, "1.0 token (stage 1: 0-100M supply)".to_string()),
        100_000 => (true, "0.1 token (stage 2: 100M-1B supply)".to_string()),
        10_000 => (true, "0.01 token (stage 3: 1B-10B supply)".to_string()),
        1_000 => (true, "0.001 token (stage 4: 10B-100B supply)".to_string()),
        100 => (true, "0.0001 token (stage 5: 100B-1T supply)".to_string()),
        1 => (true, "0.000001 token (stage 6: 1T+ supply)".to_string()),
        0 => (false, "No tokens minted - supply limit reached".to_string()),
        _ => (false, format!("Unexpected amount: {} lamports ({:.6} tokens)", raw_amount, raw_amount as f64 / 1_000_000.0)),
    }
}

