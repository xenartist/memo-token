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
use borsh::{BorshSerialize, BorshDeserialize};
use base64::{Engine as _, engine::general_purpose};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Define structures matching the contract
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct BurnMemo {
    /// version of the BurnMemo structure (for future compatibility)
    pub version: u8,
    
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    
    /// application payload (variable length, max 787 bytes)
    pub payload: Vec<u8>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct ChatMessageData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "chat" for memo-chat contract)
    pub category: String,
    
    /// Operation type (must be "send_message" for sending messages)
    pub operation: String,
    
    /// Group ID (must match the target group)
    pub group_id: u64,
    
    /// Sender pubkey as string (must match the transaction signer)
    pub sender: String,
    
    /// Message content (required, 1-512 characters)
    pub message: String,
    
    /// Optional receiver pubkey as string (for direct messages within group)
    pub receiver: Option<String>,
    
    /// Optional reply to signature (for message threading)
    pub reply_to_sig: Option<String>,
}

// Constants matching the contract
const CHAT_GROUP_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "chat";
const EXPECTED_SEND_MESSAGE_OPERATION: &str = "send_message";

#[derive(Debug, Clone)]
struct TestParams {
    pub group_id: u64,                     // Target group ID
    pub message_content: String,           // Message to send
    pub receiver: Option<Pubkey>,          // Optional receiver
    pub reply_to_sig: Option<String>,      // Optional reply signature
    pub should_succeed: bool,              // Whether the test should succeed
    pub test_description: String,          // Description of what this test validates
    pub invalid_category: bool,            // Use invalid category for negative testing
    pub invalid_operation: bool,           // Use invalid operation for negative testing
    pub wrong_group_id: bool,              // Use wrong group ID for negative testing
    pub wrong_sender: bool,                // Use wrong sender for negative testing
}

fn generate_borsh_memo_from_params(params: &TestParams, sender: &Pubkey) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Create ChatMessageData
    let message_data = ChatMessageData {
        version: CHAT_GROUP_CREATION_DATA_VERSION,
        category: if params.invalid_category { "wrong_category".to_string() } else { EXPECTED_CATEGORY.to_string() },
        operation: if params.invalid_operation { "wrong_operation".to_string() } else { EXPECTED_SEND_MESSAGE_OPERATION.to_string() },
        group_id: if params.wrong_group_id { params.group_id + 999 } else { params.group_id },
        sender: if params.wrong_sender { Pubkey::new_unique().to_string() } else { sender.to_string() },
        message: params.message_content.clone(),
        receiver: params.receiver.map(|pk| pk.to_string()),
        reply_to_sig: params.reply_to_sig.clone(),
    };
    
    // Serialize ChatMessageData to Borsh
    let borsh_data = message_data.try_to_vec()?;
    
    // Encode with Base64
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    let memo_bytes = base64_encoded.into_bytes();
    
    println!("Borsh+Base64 structure sizes:");
    println!("  ChatMessageData (Borsh): {} bytes", borsh_data.len());
    println!("  Base64 encoded memo: {} bytes", memo_bytes.len());
    
    Ok(memo_bytes)
}

use memo_token_client::get_rpc_url;

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
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Hello, world! This is a test memo.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Simple text message".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "long-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "a".repeat(512), // Maximum allowed length
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Maximum length message (512 characters)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "too-long-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "a".repeat(513), // Over maximum length
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message too long (>512 characters)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "empty-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "".to_string(), // Empty message
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Empty message (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "with-receiver" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Direct message to specific user.".to_string(),
            receiver: if args.len() > 3 {
                Some(Pubkey::from_str(&args[3]).unwrap_or_else(|_| {
                    eprintln!("Error: Invalid receiver pubkey '{}'", args[3]);
                    std::process::exit(1);
                }))
            } else {
                Some(Pubkey::new_unique()) // Use random pubkey for testing
            },
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Message with specific receiver".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "with-reply" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "This is a reply to previous message.".to_string(),
            receiver: None,
            reply_to_sig: if args.len() > 3 {
                Some(args[3].clone())
            } else {
                Some("5".repeat(88)) // Create a valid looking signature for testing
            },
            should_succeed: true,
            test_description: "Message replying to another message".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "invalid-reply-sig" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid reply signature.".to_string(),
            receiver: None,
            reply_to_sig: Some("invalid_signature".to_string()), // Invalid signature format
            should_succeed: false,
            test_description: "Invalid reply signature format".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "nonexistent-group" => TestParams {
            group_id: 99999, // Non-existent group
            message_content: "Message to non-existent group.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message to non-existent group".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "unicode-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "🚀 Unicode test: 你好世界 こんにちは 🎉 Emoji and multilingual support! 🌟".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Unicode and emoji message".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "invalid-category" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid category.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Invalid category field".to_string(),
            invalid_category: true,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "invalid-operation" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid operation.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Invalid operation field".to_string(),
            invalid_category: false,
            invalid_operation: true,
            wrong_group_id: false,
            wrong_sender: false,
        },
        "wrong-group-id" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with mismatched group ID.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Group ID mismatch".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: true,
            wrong_sender: false,
        },
        "wrong-sender" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with wrong sender.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Sender mismatch".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: true,
        },
        "custom" => {
            if args.len() < 4 {
                println!("Custom test requires additional parameters:");
                println!("Usage: cargo run --bin test-memo-chat-send-memo -- custom <group_id> <message> [receiver] [reply_to_sig]");
                println!("Example: cargo run --bin test-memo-chat-send-memo -- custom 0 \"Hello world!\"");
                return Ok(());
            }
            
            let group_id = args[2].parse::<u64>().unwrap_or_else(|_| {
                eprintln!("Error: Invalid group ID '{}'", args[2]);
                std::process::exit(1);
            });
            let message = args[3].clone();
            let receiver = if args.len() > 4 && !args[4].is_empty() {
                Some(Pubkey::from_str(&args[4]).unwrap_or_else(|_| {
                    eprintln!("Error: Invalid receiver pubkey '{}'", args[4]);
                    std::process::exit(1);
                }))
            } else {
                None
            };
            let reply_to_sig = if args.len() > 5 && !args[5].is_empty() {
                Some(args[5].clone())
            } else {
                None
            };
            
            TestParams {
                group_id,
                message_content: message,
                receiver,
                reply_to_sig,
                should_succeed: true,
                test_description: "Custom test case".to_string(),
                invalid_category: false,
                invalid_operation: false,
                wrong_group_id: false,
                wrong_sender: false,
            }
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO-CHAT SEND MESSAGE TEST (BORSH+BASE64 FORMAT) ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();
    println!("Test parameters:");
    println!("  Group ID: {}", test_params.group_id);
    println!("  Message: {} (length: {})", 
        if test_params.message_content.len() > 100 { 
            format!("{}...", &test_params.message_content[..100]) 
        } else { 
            test_params.message_content.clone() 
        }, 
        test_params.message_content.len()
    );
    println!("  Receiver: {:?}", test_params.receiver);
    println!("  Reply to: {:?}", test_params.reply_to_sig.as_ref().map(|s| &s[..16.min(s.len())]));
    println!();

    run_test(test_params)?;
    Ok(())
}

fn run_test(params: TestParams) -> Result<(), Box<dyn std::error::Error>> {
    // Connect to network
    let rpc_url = get_rpc_url();
    println!("🔍 Connecting to: {}", rpc_url);
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

    // Calculate mint authority PDA
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
    println!("  Group ID: {}", params.group_id);
    println!("  Chat group PDA: {}", chat_group_pda);
    println!("  Sender: {}", payer.pubkey());
    println!("  Sender token account: {}", sender_token_account);
    println!();

    // Check if group exists
    match client.get_account(&chat_group_pda) {
        Ok(_) => {
            println!("✅ Chat group {} exists", params.group_id);
        },
        Err(_) => {
            if params.should_succeed {
                println!("❌ ERROR: Chat group {} does not exist!", params.group_id);
                println!("   Please create the group first using test-memo-chat-create-group");
                return Ok(());
            } else {
                println!("ℹ️  Chat group {} does not exist (expected for this test)", params.group_id);
            }
        }
    }

    // Generate Borsh+Base64 memo
    let memo_bytes = generate_borsh_memo_from_params(&params, &payer.pubkey())?;
    
    println!("Generated Borsh+Base64 memo:");
    println!("  Base64 length: {} bytes", memo_bytes.len());
    
    // Show the underlying structure by decoding
    if let Ok(base64_str) = std::str::from_utf8(&memo_bytes) {
        if let Ok(decoded_data) = general_purpose::STANDARD.decode(base64_str) {
            println!("  Decoded Borsh length: {} bytes", decoded_data.len());
            
            if let Ok(message_data) = ChatMessageData::try_from_slice(&decoded_data) {
                println!("  ChatMessageData structure:");
                println!("    version: {}", message_data.version);
                println!("    category: {}", message_data.category);
                println!("    operation: {}", message_data.operation);
                println!("    group_id: {}", message_data.group_id);
                println!("    sender: {}", message_data.sender);
                println!("    message: {} (len: {})", 
                    if message_data.message.len() > 50 { 
                        format!("{}...", &message_data.message[..50]) 
                    } else { 
                        message_data.message.clone() 
                    }, 
                    message_data.message.len()
                );
                println!("    receiver: {:?}", message_data.receiver);
                println!("    reply_to_sig: {:?}", message_data.reply_to_sig.as_ref().map(|s| &s[..16.min(s.len())]));
            }
        }
    }
    
    if memo_bytes.len() <= 100 {
        println!("  Base64 content: {}", String::from_utf8_lossy(&memo_bytes));
    } else {
        println!("  Base64 preview: {}", String::from_utf8_lossy(&memo_bytes[..50]));
    }
    println!();

    // Get latest blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create instructions
    let memo_ix = spl_memo::build_memo(
        &memo_bytes,
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
                // Add 20% margin
                let optimal_cu = ((units_consumed as f64) * 1.2) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+20% margin)", 
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
            println!("📨 TRANSACTION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            
            if params.should_succeed {
                println!("✅ EXPECTED SUCCESS: Test passed as expected");
                println!("Message sent to group {} successfully!", params.group_id);
            } else {
                println!("❌ UNEXPECTED SUCCESS: Test should have failed but succeeded");
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

fn get_group_id_from_args(args: &[String], index: usize, default: u64) -> u64 {
    if args.len() > index {
        args[index].parse::<u64>().unwrap_or_else(|_| {
            eprintln!("Error: Invalid group ID '{}'", args[index]);
            std::process::exit(1);
        })
    } else {
        default
    }
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("EmptyMessage") && params.message_content.is_empty() {
        println!("✅ Correct: Empty message detected");
    } else if error_msg.contains("MessageTooLong") && params.message_content.len() > 512 {
        println!("✅ Correct: Message too long detected");
    } else if error_msg.contains("InvalidCategory") && params.invalid_category {
        println!("✅ Correct: Invalid category detected");
    } else if error_msg.contains("InvalidOperation") && params.invalid_operation {
        println!("✅ Correct: Invalid operation detected");
    } else if error_msg.contains("GroupIdMismatch") && params.wrong_group_id {
        println!("✅ Correct: Group ID mismatch detected");
    } else if error_msg.contains("SenderMismatch") && params.wrong_sender {
        println!("✅ Correct: Sender mismatch detected");
    } else if error_msg.contains("InvalidReplySignatureFormat") {
        println!("✅ Correct: Invalid reply signature format detected");
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
        println!("   Invalid memo format, Base64 decoding, or Borsh parsing failed");
    } else if error_msg.contains("UnsupportedMemoVersion") {
        println!("   Unsupported memo version");
    } else if error_msg.contains("InvalidCategory") {
        println!("   Category field not 'chat'");
    } else if error_msg.contains("InvalidOperation") {
        println!("   Operation field not 'send_message'");
    } else if error_msg.contains("GroupIdMismatch") {
        println!("   Group ID in memo doesn't match instruction parameter");
    } else if error_msg.contains("SenderMismatch") {
        println!("   Sender in memo doesn't match transaction signer");
    } else if error_msg.contains("EmptyMessage") {
        println!("   Message field in memo is empty");
    } else if error_msg.contains("MessageTooLong") {
        println!("   Message field exceeds maximum length (512 characters)");
    } else if error_msg.contains("InvalidReplySignatureFormat") {
        println!("   Reply signature field has invalid format");
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
    let mut hasher = Sha256::new();
    hasher.update(b"global:send_memo_to_group");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    instruction_data.extend_from_slice(&group_id.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*sender, true),                        // sender (user as signer)
        AccountMeta::new(*chat_group, false),                   // chat_group
        AccountMeta::new(*mint, false),                         // mint
        AccountMeta::new_readonly(*mint_authority, false),      // mint_authority (memo-mint PDA)
        AccountMeta::new(*sender_token_account, false),         // sender_token_account
        AccountMeta::new_readonly(token_2022_id(), false),      // token_program
        AccountMeta::new_readonly(*memo_mint_program, false),   // memo_mint_program
        AccountMeta::new_readonly(
            solana_sdk::sysvar::instructions::id(),
            false
        ), // instructions
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn print_usage() {
    println!("Usage: cargo run --bin test-memo-chat-send-memo -- <test_case> [group_id] [additional_params...]");
    println!();
    println!("Available test cases:");
    println!("  simple-text          - Send simple text message");
    println!("  long-message         - Send maximum length message (512 chars)");
    println!("  too-long-message     - Send message too long (>512 chars) - should fail");
    println!("  empty-message        - Send empty message - should fail");
    println!("  with-receiver        - Send message to specific receiver");
    println!("  with-reply           - Send message as reply to another message");
    println!("  invalid-reply-sig    - Send message with invalid reply signature - should fail");
    println!("  nonexistent-group    - Send message to non-existent group - should fail");
    println!("  unicode-message      - Send message with Unicode and emoji");
    println!("  invalid-category     - Send message with invalid category - should fail");
    println!("  invalid-operation    - Send message with invalid operation - should fail");
    println!("  wrong-group-id       - Send message with mismatched group ID - should fail");
    println!("  wrong-sender         - Send message with wrong sender - should fail");
    println!("  custom               - Custom test with specified parameters");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-chat-send-memo -- simple-text 0");
    println!("  cargo run --bin test-memo-chat-send-memo -- with-receiver 0 <receiver_pubkey>");
    println!("  cargo run --bin test-memo-chat-send-memo -- custom 0 \"Hello world!\"");
    println!("  cargo run --bin test-memo-chat-send-memo -- unicode-message 0");
    println!();
    println!("Note: Make sure the target group exists before sending messages!");
} 
