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
use serde_json;
use sha2::{Sha256, Digest};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

#[derive(Debug, Clone)]
struct TestParams {
    pub group_id: u64,             // Target group ID
    pub memo_content: String,      // Memo content to send
    pub should_succeed: bool,      // Whether the test should succeed
    pub test_description: String,  // Description of what this test validates
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let test_case = &args[1];
    
    // Define test cases
    let test_params = match test_case.as_str() {
        "simple-text" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0), // Default to group 0
            memo_content: "Hello, world! This is a test memo.".to_string(),
            should_succeed: true,
            test_description: "Simple text memo to existing group".to_string(),
        },
        "long-text" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: "This is a much longer memo content that tests the memo length limits. ".repeat(5),
            should_succeed: true,
            test_description: "Longer text memo to test memo length handling".to_string(),
        },
        "json-memo" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: serde_json::json!({
                "type": "chat_message",
                "content": "Hello from JSON memo!",
                "timestamp": chrono::Utc::now().timestamp(),
                "sender_info": {
                    "nickname": "TestUser",
                    "avatar": "default.png"
                }
            }).to_string(),
            should_succeed: true,
            test_description: "JSON formatted memo content".to_string(),
        },
        "emoji-text" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: "Hello! 😀 This memo contains emojis 🚀✨ and unicode text! 中文测试".to_string(),
            should_succeed: true,
            test_description: "Memo with emojis and unicode characters".to_string(),
        },
        "max-length" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: "X".repeat(800), // Maximum memo length
            should_succeed: true,
            test_description: "Maximum length memo (800 bytes)".to_string(),
        },
        "too-long" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: "X".repeat(801), // Exceeds maximum memo length
            should_succeed: false,
            test_description: "Memo exceeding maximum length (should fail)".to_string(),
        },
        "too-short" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            memo_content: "Short".to_string(), // Less than 69 bytes
            should_succeed: false,
            test_description: "Memo shorter than minimum length (should fail)".to_string(),
        },
        "nonexistent-group" => TestParams {
            group_id: 99999, // Non-existent group ID
            memo_content: "This memo is for a non-existent group.".to_string(),
            should_succeed: false,
            test_description: "Memo to non-existent group (should fail)".to_string(),
        },
        "custom" => {
            if args.len() < 4 {
                println!("Custom test requires additional parameters:");
                println!("Usage: cargo run --bin test-memo-chat-send-memo -- custom <group_id> <memo_content>");
                println!("Example: cargo run --bin test-memo-chat-send-memo -- custom 0 \"Hello, world!\"");
                return Ok(());
            }
            
            let group_id = args[2].parse::<u64>().unwrap_or(0);
            let memo_content = args[3].clone();
            
            TestParams {
                group_id,
                memo_content,
                should_succeed: true, // Assume custom tests should succeed unless proven otherwise
                test_description: "Custom test case".to_string(),
            }
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO-CHAT SEND MEMO TEST ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();
    println!("Test parameters:");
    println!("  Target group ID: {}", test_params.group_id);
    println!("  Memo length: {} bytes", test_params.memo_content.as_bytes().len());
    if test_params.memo_content.len() > 100 {
        println!("  Memo content (first 50 chars): {}...", &test_params.memo_content[..50]);
        println!("  Memo content (last 50 chars): ...{}", &test_params.memo_content[test_params.memo_content.len()-50..]);
    } else {
        println!("  Memo content: {}", test_params.memo_content);
    }
    println!();

    run_test(test_params)?;
    Ok(())
}

fn get_group_id_from_args(args: &[String], index: usize, default: u64) -> u64 {
    if args.len() > index {
        args[index].parse::<u64>().unwrap_or(default)
    } else {
        default
    }
}

fn run_test(params: TestParams) -> Result<(), Box<dyn std::error::Error>> {
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
    let memo_mint_program_id = Pubkey::from_str("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy")
        .expect("Invalid memo-mint program ID");
    let mint = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");

    // Calculate chat group PDA
    let (chat_group_pda, _) = Pubkey::find_program_address(
        &[b"chat_group", &params.group_id.to_le_bytes()],
        &memo_chat_program_id,
    );

    // Calculate mint authority PDA (from memo-mint program)
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &memo_mint_program_id,
    );

    // Get user's token account
    let sender_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    println!("Runtime info:");
    println!("  Target group ID: {}", params.group_id);
    println!("  Chat group PDA: {}", chat_group_pda);
    println!("  Mint authority PDA: {}", mint_authority_pda);
    println!("  Sender: {}", payer.pubkey());
    println!("  Sender token account: {}", sender_token_account);
    println!();

    // Check if group exists
    match client.get_account(&chat_group_pda) {
        Ok(account) => {
            println!("✅ Chat group {} exists (data length: {} bytes)", params.group_id, account.data.len());
            
            // Try to parse some basic group info
            if account.data.len() >= 16 {
                // Parse group_id from account data (first u64 after discriminator)
                let stored_group_id = u64::from_le_bytes(
                    account.data[8..16].try_into().unwrap()
                );
                println!("   Stored group ID: {}", stored_group_id);
                
                if stored_group_id != params.group_id {
                    println!("⚠️  Warning: Stored group ID doesn't match expected ID!");
                }
            }
        },
        Err(_) => {
            if params.should_succeed {
                println!("❌ ERROR: Chat group {} does not exist!", params.group_id);
                println!("   Please create a group first using test-memo-chat-create-group");
                return Ok(());
            } else {
                println!("✅ Chat group {} does not exist (expected for this test)", params.group_id);
            }
        }
    }

    // Check token account exists
    match client.get_account(&sender_token_account) {
        Ok(_) => {
            println!("✅ Sender token account exists");
        },
        Err(_) => {
            println!("❌ ERROR: Sender token account does not exist!");
            println!("   Please create the token account first");
            return Ok(());
        }
    }

    // Get latest blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create instructions
    let memo_ix = spl_memo::build_memo(
        params.memo_content.as_bytes(),
        &[&payer.pubkey()],
    );

    let send_memo_ix = send_memo_to_group_instruction(
        &memo_chat_program_id,
        &payer.pubkey(),
        &chat_group_pda,
        &mint,
        &mint_authority_pda,
        &sender_token_account,
        &memo_mint_program_id,
        params.group_id,
    );

    // First, simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, memo_ix.clone(), send_memo_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

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
                
                // For expected errors, use a reasonable default
                let default_cu = 400_000u32;
                println!("Using default compute units for error case: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin as per memory [[memory:4904355]]
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 400_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            400_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, send_memo_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Sending transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 TRANSACTION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            
            if params.should_succeed {
                println!("✅ EXPECTED SUCCESS: Test passed as expected");
            } else {
                println!("❌ UNEXPECTED SUCCESS: Test should have failed but succeeded");
            }
            
            // Check group statistics after sending memo
            match client.get_account(&chat_group_pda) {
                Ok(account) => {
                    if let Ok(memo_count) = parse_memo_count_from_group_data(&account.data) {
                        println!("✅ Memo sent successfully! Group memo count: {}", memo_count);
                    } else {
                        println!("✅ Memo sent successfully! (Could not parse memo count)");
                    }
                },
                Err(e) => {
                    println!("⚠️  Could not fetch updated group data: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            if !params.should_succeed {
                println!("✅ EXPECTED FAILURE: Test failed as expected");
                analyze_expected_error(&err.to_string(), &params);
            } else {
                println!("❌ UNEXPECTED FAILURE: Test should have succeeded");
                analyze_unexpected_error(&err.to_string());
            }
        }
    }

    Ok(())
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("MemoTooShort") && params.memo_content.as_bytes().len() < 69 {
        println!("✅ Correct: Memo too short detected");
    } else if error_msg.contains("MemoTooLong") && params.memo_content.as_bytes().len() > 800 {
        println!("✅ Correct: Memo too long detected");
    } else if error_msg.contains("GroupNotFound") && params.group_id == 99999 {
        println!("✅ Correct: Non-existent group detected");
    } else if error_msg.contains("MemoTooFrequent") {
        println!("✅ Correct: Memo sent too frequently detected");
    } else {
        println!("⚠️  Unexpected error type: {}", error_msg);
    }
}

fn analyze_unexpected_error(error_msg: &str) {
    println!("💡 Error analysis:");
    if error_msg.contains("MemoRequired") {
        println!("   Missing memo instruction");
    } else if error_msg.contains("InvalidMemoFormat") {
        println!("   Invalid memo format or UTF-8 encoding issue");
    } else if error_msg.contains("GroupNotFound") {
        println!("   Chat group does not exist - create it first");
    } else if error_msg.contains("MemoTooFrequent") {
        println!("   Memo sent too frequently - wait before sending another");
    } else if error_msg.contains("insufficient funds") {
        println!("   Insufficient SOL balance for transaction fees");
    } else if error_msg.contains("InvalidTokenAccount") {
        println!("   Token account issue - check if account exists and belongs to correct mint");
    } else {
        println!("   {}", error_msg);
    }
}

fn send_memo_to_group_instruction(
    program_id: &Pubkey,
    sender: &Pubkey,
    chat_group: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    sender_token_account: &Pubkey,
    memo_mint_program: &Pubkey,
    group_id: u64,
) -> Instruction {
    // Generate instruction discriminator for send_memo_to_group
    let mut hasher = Sha256::new();
    hasher.update(b"global:send_memo_to_group");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add group_id parameter
    instruction_data.extend_from_slice(&group_id.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*sender, true),                    // sender
        AccountMeta::new(*chat_group, false),               // chat_group
        AccountMeta::new(*mint, false),                     // mint
        AccountMeta::new(*mint_authority, false),           // mint_authority
        AccountMeta::new(*sender_token_account, false),     // sender_token_account
        AccountMeta::new_readonly(token_2022_id(), false),  // token_program
        AccountMeta::new_readonly(*memo_mint_program, false), // memo_mint_program
        AccountMeta::new_readonly(
            Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
            false
        ), // instructions sysvar
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn print_usage() {
    println!("Usage: cargo run --bin test-memo-chat-send-memo -- <test_case> [group_id]");
    println!();
    println!("Available test cases:");
    println!("  simple-text       - Simple text memo to existing group");
    println!("  long-text         - Longer text memo");
    println!("  json-memo         - JSON formatted memo content");
    println!("  emoji-text        - Memo with emojis and unicode characters");
    println!("  max-length        - Maximum length memo (800 bytes)");
    println!("  too-long          - Memo exceeding maximum length (should fail)");
    println!("  too-short         - Memo shorter than minimum length (should fail)");
    println!("  nonexistent-group - Memo to non-existent group (should fail)");
    println!("  custom            - Custom test with specified parameters");
    println!();
    println!("Optional parameters:");
    println!("  [group_id]        - Target group ID (default: 0)");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-chat-send-memo -- simple-text 0");
    println!("  cargo run --bin test-memo-chat-send-memo -- json-memo 1");
    println!("  cargo run --bin test-memo-chat-send-memo -- custom 0 \"Hello, world!\"");
    println!();
    println!("Note: Make sure the target group exists before sending memos!");
    println!("Use test-memo-chat-create-group to create groups first.");
} 

// Helper function to parse memo count from ChatGroup account data
fn parse_memo_count_from_group_data(data: &[u8]) -> Result<u64, Box<dyn std::error::Error>> {
    if data.len() < 8 {
        return Err("Data too short for discriminator".into());
    }

    let mut offset = 8; // Skip discriminator

    // Skip group_id (u64)
    offset += 8;
    
    // Skip creator (32 bytes)  
    offset += 32;
    
    // Skip created_at (i64)
    offset += 8;

    // Skip name (String)
    let (_, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Skip description (String)
    let (_, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Skip image (String)
    let (_, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Skip tags (Vec<String>)
    let (_, new_offset) = read_string_vec(data, offset)?;
    offset = new_offset;

    // Read memo_count (u64)
    if data.len() < offset + 8 {
        return Err("Data too short for memo_count".into());
    }
    let memo_count = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    
    Ok(memo_count)
}

// Helper function to read a String from account data
fn read_string(data: &[u8], offset: usize) -> Result<(String, usize), Box<dyn std::error::Error>> {
    if data.len() < offset + 4 {
        return Err("Data too short for string length".into());
    }

    let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    let new_offset = offset + 4;

    if data.len() < new_offset + len {
        return Err("Data too short for string content".into());
    }

    let string_data = &data[new_offset..new_offset + len];
    let string = String::from_utf8(string_data.to_vec())?;

    Ok((string, new_offset + len))
}

// Helper function to read a Vec<String> from account data
fn read_string_vec(data: &[u8], offset: usize) -> Result<(Vec<String>, usize), Box<dyn std::error::Error>> {
    if data.len() < offset + 4 {
        return Err("Data too short for vec length".into());
    }

    let vec_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    let mut new_offset = offset + 4;
    let mut strings = Vec::new();

    for _ in 0..vec_len {
        let (string, next_offset) = read_string(data, new_offset)?;
        strings.push(string);
        new_offset = next_offset;
    }

    Ok((strings, new_offset))
} 