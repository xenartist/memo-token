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
    
    let test_scenario = if args.len() > 1 {
        args[1].as_str()
    } else {
        "help"
    };
    
    match test_scenario {
        "no-memo" => test_no_memo(),
        "short-memo" => test_short_memo(),
        "valid-memo" => test_valid_memo(),
        "long-memo" => test_long_memo(),
        "memo-69" => test_memo_exact_69(),
        "memo-769" => test_memo_exact_769(),
        "help" | _ => {
            println!("Usage: {} <test_scenario>", args[0]);
            println!("Test scenarios:");
            println!("  no-memo      - Test mint without memo (should fail)");
            println!("  short-memo   - Test mint with memo < 69 bytes (should fail)");
            println!("  memo-69      - Test mint with memo exactly 69 bytes (should succeed)");
            println!("  valid-memo   - Test mint with memo 69-769 bytes (should succeed)");
            println!("  memo-769     - Test mint with memo exactly 769 bytes (should succeed)");
            println!("  long-memo    - Test mint with memo > 769 bytes (should fail)");
            Ok(())
        }
    }
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
        "test": "boundary-test",
        "length": target_length,
        "data": ""
    });
    
    let base_text = base_json.to_string();
    let base_length = base_text.len();
    
    if base_length >= target_length {
        // If base is already too long, create a simpler JSON
        let simple_json = serde_json::json!({
            "data": "x".repeat(target_length.saturating_sub(20))
        });
        let mut result = simple_json.to_string();
        
        // Fine-tune to exact length
        while result.len() < target_length {
            result.push('x');
        }
        while result.len() > target_length {
            result.pop();
        }
        result
    } else {
        // Add padding to reach exact length
        let padding_needed = target_length - base_length;
        let padding = "x".repeat(padding_needed);
        
        let final_json = serde_json::json!({
            "test": "boundary-test",
            "length": target_length,
            "data": padding
        });
        
        let mut result = final_json.to_string();
        
        // Fine-tune to exact length (account for JSON formatting)
        while result.len() < target_length {
            result.push('x');
        }
        while result.len() > target_length {
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
    let sim_transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Simulate to get compute units
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
                return Err(format!("Simulation failed: {:?}", err).into());
            } else if let Some(units_consumed) = result.value.units_consumed {
                let required_cu = ((units_consumed as f64 * 1.2) as u32).max(5000);
                println!("Simulation consumed {} CUs, requesting {} CUs", units_consumed, required_cu);
                required_cu
            } else {
                10_000
            }
        },
        Err(_) => 10_000,
    };
    
    // Create compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // Create final transaction with compute budget
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