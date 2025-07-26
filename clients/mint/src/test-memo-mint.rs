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
use std::str::FromStr;
use sha2::{Sha256, Digest};
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Mint Test Client ===\n");
    
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
        "memo-769" => test_memo_exact_769(),
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
    println!("  valid-memo      - Test mint with memo 69-769 bytes (should succeed)");
    println!("  memo-769        - Test mint with memo exactly 769 bytes (should succeed)");
    println!("  long-memo       - Test mint with memo > 769 bytes (should fail)");
    println!("  custom-length   - Test mint with custom memo length (requires memo_length parameter)");
    println!("\nExamples:");
    println!("  {} valid-memo", program_name);
    println!("  {} custom-length 800    # Test 800-byte memo", program_name);
    println!("  {} custom-length 50     # Test 50-byte memo", program_name);
    println!("  {} custom-length 1000   # Test 1000-byte memo", program_name);
}

fn test_custom_length(target_length: usize) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with CUSTOM LENGTH memo ({} bytes)...\n", target_length);
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance
    let balance_before = get_token_balance(&client, &token_account);
    
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
    } else if actual_length > 769 {
        "FAIL (> 769 bytes)"
    } else {
        "SUCCESS (69-769 bytes)"
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
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], &format!("Custom Length ({} bytes) Test", actual_length));
    
    match result {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESSFUL!");
            println!("   Signature: {}", signature);
            
            // Check token balance after mint
            let balance_after = get_token_balance(&client, &token_account);
            println!("   Token balance before: {}", balance_before);
            println!("   Token balance after:  {}", balance_after);
            println!("   Tokens minted: {} (expected: 1)", balance_after - balance_before);
            
            // Analyze result
            if actual_length < 69 {
                println!("   ❌ UNEXPECTED SUCCESS: Memo < 69 bytes should have failed");
                println!("   🔍 This suggests the contract may not be enforcing minimum length");
            } else if actual_length > 769 {
                println!("   ❌ UNEXPECTED SUCCESS: Memo > 769 bytes should have failed");
                println!("   🔍 This suggests either:");
                println!("      - Contract is not enforcing maximum length");
                println!("      - System limit is higher than expected");
            } else {
                println!("   ✅ EXPECTED SUCCESS: Memo length within valid range (69-769 bytes)");
            }
            
            if balance_after - balance_before == 1 {
                println!("   ✅ Correct amount minted (1 token with decimal=0)");
            } else {
                println!("   ❌ Unexpected mint amount");
            }
        },
        Err(e) => {
            println!("❌ TRANSACTION FAILED!");
            println!("   Error: {}", e);
            
            // Analyze failure
            if actual_length < 69 {
                if e.to_string().contains("Custom(6004)") || e.to_string().contains("MemoTooShort") {
                    println!("   ✅ EXPECTED FAILURE: Contract correctly rejects memo < 69 bytes");
                } else {
                    println!("   ⚠️  UNEXPECTED ERROR for short memo: {}", e);
                }
            } else if actual_length > 769 {
                if e.to_string().contains("Custom(6008)") || e.to_string().contains("MemoTooLong") {
                    println!("   ✅ EXPECTED FAILURE: Contract correctly rejects memo > 769 bytes");
                } else if e.to_string().contains("Program failed to complete") || e.to_string().contains("Transaction too large") {
                    println!("   ✅ EXPECTED FAILURE: Hit system-level transaction size limit");
                    println!("   🔍 Solana transaction size limit (~1232 bytes) exceeded");
                } else {
                    println!("   ⚠️  UNEXPECTED ERROR for long memo: {}", e);
                }
            } else {
                println!("   ❌ UNEXPECTED FAILURE: Memo within valid range (69-769 bytes) should succeed");
                println!("   🔍 Possible issues:");
                println!("      - Contract bug");
                println!("      - Network/RPC issue");
                println!("      - Other validation failure");
            }
        }
    }
    
    // Summary for custom length test
    println!("\n📊 CUSTOM LENGTH TEST SUMMARY:");
    println!("   Target length: {} bytes", target_length);
    println!("   Actual length: {} bytes", actual_length);
    println!("   Contract valid range: 69-769 bytes");
    println!("   System limit: ~1000+ bytes (varies)");
    
    if actual_length != target_length {
        println!("   ⚠️  Note: Actual length differs from target due to JSON formatting");
    }
    
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
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![mint_ix], "No Memo Test");
    
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
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], "Short Memo Test");
    
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
    
    // Get current token balance
    let balance_before = get_token_balance(&client, &token_account);
    
    // Create memo with exactly 69 bytes
    let memo_text = create_memo_with_exact_length(69);
    
    println!("Memo length: {} bytes (exactly 69 bytes)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], "Exact 69 Bytes Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            
            // Check token balance after mint
            let balance_after = get_token_balance(&client, &token_account);
            println!("   Token balance before: {}", balance_before);
            println!("   Token balance after:  {}", balance_after);
            println!("   Tokens minted: {} (expected: 1)", balance_after - balance_before);
            
            if balance_after - balance_before == 1 {
                println!("   ✅ Correct amount minted (1 token with decimal=0)");
                println!("   ✅ Boundary condition (69 bytes) handled correctly");
            } else {
                println!("   ❌ Unexpected mint amount");
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
            println!("   The contract should accept memos of exactly 69 bytes.");
        }
    }
    
    Ok(())
}

fn test_valid_memo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with VALID memo (69-769 bytes) (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance
    let balance_before = get_token_balance(&client, &token_account);
    
    // Create valid memo (between 69-769 bytes)
    let message = "This is a valid memo test for the memo-mint contract. ".repeat(2);
    let memo_json = serde_json::json!({
        "message": message,
        "test": "valid-memo",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "additional_data": "padding_to_ensure_minimum_length_requirement_is_met"
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (69-769 bytes range)", memo_text.len());
    println!("Memo content: {}", memo_text);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], "Valid Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            
            // Check token balance after mint
            let balance_after = get_token_balance(&client, &token_account);
            println!("   Token balance before: {}", balance_before);
            println!("   Token balance after:  {}", balance_after);
            println!("   Tokens minted: {} (expected: 1)", balance_after - balance_before);
            
            if balance_after - balance_before == 1 {
                println!("   ✅ Correct amount minted (1 token with decimal=0)");
            } else {
                println!("   ❌ Unexpected mint amount");
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
        }
    }
    
    Ok(())
}

fn test_memo_exact_769() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with memo EXACTLY 769 bytes (expected to succeed)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get current token balance
    let balance_before = get_token_balance(&client, &token_account);
    
    // Create memo with exactly 769 bytes
    let memo_text = create_memo_with_exact_length(769);
    
    println!("Memo length: {} bytes (exactly 769 bytes)", memo_text.len());
    println!("Memo content preview: {}...", &memo_text[..100]);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], "Exact 769 Bytes Memo Test");
    
    match result {
        Ok(signature) => {
            println!("✅ SUCCESS: Transaction completed successfully!");
            println!("   Signature: {}", signature);
            
            // Check token balance after mint
            let balance_after = get_token_balance(&client, &token_account);
            println!("   Token balance before: {}", balance_before);
            println!("   Token balance after:  {}", balance_after);
            println!("   Tokens minted: {} (expected: 1)", balance_after - balance_before);
            
            if balance_after - balance_before == 1 {
                println!("   ✅ Correct amount minted (1 token with decimal=0)");
                println!("   ✅ Boundary condition (769 bytes) handled correctly");
            } else {
                println!("   ❌ Unexpected mint amount");
            }
        },
        Err(e) => {
            println!("❌ UNEXPECTED: Transaction failed when it should have succeeded!");
            println!("   Error: {}", e);
            println!("   The contract should accept memos of exactly 769 bytes.");
        }
    }
    
    Ok(())
}

fn test_long_memo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing mint with LONG memo > 769 bytes (expected to fail)...\n");
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Create long memo (more than 769 bytes)
    let long_message = "This is a very long memo test that exceeds the maximum allowed length. ".repeat(15);
    let memo_json = serde_json::json!({
        "message": long_message,
        "test": "long-memo",
        "additional_padding": "x".repeat(100)
    });
    let memo_text = memo_json.to_string();
    
    println!("Memo length: {} bytes (> 769 bytes)", memo_text.len());
    println!("Memo content preview: {}...", &memo_text[..100]);
    
    // Create memo instruction
    let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction
    let result = execute_transaction(&client, &payer, vec![memo_ix, mint_ix], "Long Memo Test");
    
    match result {
        Ok(_) => {
            println!("❌ UNEXPECTED: Transaction succeeded when it should have failed!");
            println!("   The contract should reject memos longer than 769 bytes.");
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
    let rpc_url = "https://rpc-testnet.x1.wiki";
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
    let program_id = Pubkey::from_str("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy")
        .expect("Invalid program ID");
    let mint_address = Pubkey::from_str("memoX1g5dtnxeN6zVdHMYWCCg3Qgre8WGFNs7YF2Mbc")
        .expect("Invalid mint address");
    
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
    // Calculate Anchor instruction sighash for "mint_token"
    let mut hasher = Sha256::new();
    hasher.update(b"global:mint_token");
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

fn execute_transaction(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
    test_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("Executing {}...", test_name);
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction for simulation
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
    let mut sim_instructions = vec![dummy_compute_budget_ix];
    sim_instructions.extend(instructions.clone());
    
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
    
    // Create compute budget instruction with optimal CU
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    
    // Create final transaction with optimal compute budget
    let mut final_instructions = vec![compute_budget_ix];
    final_instructions.extend(instructions);
    
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

fn get_token_balance(client: &RpcClient, token_account: &Pubkey) -> u64 {
    match client.get_token_account_balance(token_account) {
        Ok(balance) => {
            // For decimal=0 tokens, ui_amount should equal the raw amount
            balance.ui_amount.unwrap_or(0.0) as u64
        },
        Err(_) => 0,
    }
} 