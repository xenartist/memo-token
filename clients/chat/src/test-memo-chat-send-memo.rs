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
const BURN_MEMO_VERSION: u8 = 1;
const CHAT_GROUP_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "chat";
const EXPECTED_SEND_MESSAGE_OPERATION: &str = "send_message";

#[derive(Debug, Clone)]
struct TestParams {
    pub group_id: u64,             // Target group ID
    pub message_content: String,   // Message content to send
    pub receiver: Option<Pubkey>,  // Optional receiver
    pub reply_to_sig: Option<String>, // Optional reply signature
    pub should_succeed: bool,      // Whether the test should succeed
    pub test_description: String,  // Description of what this test validates
    pub invalid_category: bool,    // Use invalid category
    pub invalid_operation: bool,   // Use invalid operation
    pub wrong_group_id: bool,      // Use wrong group ID
    pub wrong_sender: bool,        // Use wrong sender
    pub missing_fields: Vec<String>, // Fields to omit
}

/// Generate Borsh-formatted memo for sending messages
fn generate_borsh_memo_from_params(params: &TestParams, sender: &Pubkey) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    //  Create ChatMessageData
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
    
    //  Serialize ChatMessageData
    let memo_bytes = message_data.try_to_vec()?;
    
    println!("Borsh structure sizes:");
    println!("  ChatMessageData: {} bytes", memo_bytes.len());
    
    Ok(memo_bytes)
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
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Hello, world! This is a test memo.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Simple text message to existing group".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "long-text" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "This is a much longer message content that tests the message length limits. ".repeat(5),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Longer text message to test message length handling".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "emoji-text" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Hello! 😀 This message contains emojis 🚀✨ and unicode text! 中文测试".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Message with emojis and unicode characters".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "with-receiver" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Hello @user! This message mentions someone.".to_string(),
            receiver: Some(Pubkey::from_str("11111111111111111111111111111112").unwrap()),
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Message with receiver field (@ mention)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "with-reply" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "This is a reply to a previous message.".to_string(),
            receiver: None,
            reply_to_sig: Some("5VfQvdK2VkgFUBX8bEcAQbVJkchLUdfq4Rn9RDUNwHAK1sF8NNYK2nGChtdkRxLLfq4wnJ2W4FfGwM8EjwzQJsm".to_string()),
            should_succeed: true,
            test_description: "Message with reply_to_sig field (reply to message)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "max-length" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "X".repeat(512),
            receiver: None,
            reply_to_sig: None,
            should_succeed: true,
            test_description: "Maximum length message (512 chars)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "too-long-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "X".repeat(513),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message exceeding maximum length (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "empty-message" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Empty message (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "invalid-category" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid category".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message with invalid category field (should fail)".to_string(),
            invalid_category: true,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "invalid-operation" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid operation".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message with invalid operation field (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: true,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "wrong-group-id" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with wrong group ID".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message with wrong group_id field (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: true,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "wrong-sender" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with wrong sender".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message with wrong sender field (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: true,
            missing_fields: vec![],
        },
        "invalid-reply-sig" => TestParams {
            group_id: get_group_id_from_args(&args, 2, 0),
            message_content: "Message with invalid reply signature".to_string(),
            receiver: None,
            reply_to_sig: Some("invalid_signature_format".to_string()),
            should_succeed: false,
            test_description: "Message with invalid reply_to_sig format (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "nonexistent-group" => TestParams {
            group_id: 99999,
            message_content: "This message is for a non-existent group.".to_string(),
            receiver: None,
            reply_to_sig: None,
            should_succeed: false,
            test_description: "Message to non-existent group (should fail)".to_string(),
            invalid_category: false,
            invalid_operation: false,
            wrong_group_id: false,
            wrong_sender: false,
            missing_fields: vec![],
        },
        "custom" => {
            if args.len() < 4 {
                println!("Custom test requires additional parameters:");
                println!("Usage: cargo run --bin test-memo-chat-send-memo -- custom <group_id> <message_content>");
                println!("Example: cargo run --bin test-memo-chat-send-memo -- custom 0 \"Hello, world!\"");
                return Ok(());
            }
            
            let group_id = args[2].parse::<u64>().unwrap_or(0);
            let message_content = args[3].clone();
            
            TestParams {
                group_id,
                message_content,
                receiver: None,
                reply_to_sig: None,
                should_succeed: true,
                test_description: "Custom test case".to_string(),
                invalid_category: false,
                invalid_operation: false,
                wrong_group_id: false,
                wrong_sender: false,
                missing_fields: vec![],
            }
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO-CHAT SEND MEMO TEST (BORSH FORMAT) ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();
    println!("Test parameters:");
    println!("  Target group ID: {}", test_params.group_id);
    println!("  Message length: {} chars", test_params.message_content.len());
    if test_params.message_content.len() > 100 {
        println!("  Message content (first 50 chars): {}...", &test_params.message_content[..50]);
        println!("  Message content (last 50 chars): ...{}", &test_params.message_content[test_params.message_content.len()-50..]);
    } else {
        println!("  Message content: {}", test_params.message_content);
    }
    println!("  Receiver: {:?}", test_params.receiver);
    println!("  Reply to sig: {:?}", test_params.reply_to_sig.as_ref().map(|s| &s[..16.min(s.len())]));
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
    let rpc_url = "https://rpc.testnet.x1.xyz";
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

    // Generate Borsh memo
    let memo_bytes = generate_borsh_memo_from_params(&params, &payer.pubkey())?;
    
    println!("Generated Borsh memo:");
    println!("  Length: {} bytes", memo_bytes.len());
    if memo_bytes.len() <= 100 {
        println!("  Hex: {}", hex::encode(&memo_bytes));
    } else {
        println!("  Hex (first 50 bytes): {}", hex::encode(&memo_bytes[..50]));
        println!("  Hex (last 50 bytes): {}", hex::encode(&memo_bytes[memo_bytes.len()-50..]));
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
    if error_msg.contains("MemoTooShort") {
        println!("✅ Correct: Memo too short detected");
    } else if error_msg.contains("MemoTooLong") {
        println!("✅ Correct: Memo too long detected");
    } else if error_msg.contains("InvalidCategory") && params.invalid_category {
        println!("✅ Correct: Invalid category detected");
    } else if error_msg.contains("InvalidOperation") && params.invalid_operation {
        println!("✅ Correct: Invalid operation detected");
    } else if error_msg.contains("MessageTooLong") && params.message_content.len() > 512 {
        println!("✅ Correct: Message too long detected");
    } else if error_msg.contains("EmptyMessage") && params.message_content.is_empty() {
        println!("✅ Correct: Empty message detected");
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
        println!("   Invalid memo format or Borsh parsing failed");
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
    println!("  simple-text         - Simple text message to existing group");
    println!("  long-text           - Longer text message");
    println!("  emoji-text          - Message with emojis and unicode characters");
    println!("  with-receiver       - Message with receiver field (@ mention)");
    println!("  with-reply          - Message with reply_to_sig field (reply to message)");
    println!("  max-length          - Maximum length message (512 chars)");
    println!("  too-long-message    - Message exceeding maximum length (should fail)");
    println!("  empty-message       - Empty message (should fail)");
    println!("  invalid-category    - Message with invalid category field (should fail)");
    println!("  invalid-operation   - Message with invalid operation field (should fail)");
    println!("  wrong-group-id      - Message with wrong group_id field (should fail)");
    println!("  wrong-sender        - Message with wrong sender field (should fail)");
    println!("  invalid-reply-sig   - Message with invalid reply signature format (should fail)");
    println!("  nonexistent-group   - Message to non-existent group (should fail)");
    println!("  custom              - Custom test with specified parameters");
    println!();
    println!("Optional parameters:");
    println!("  [group_id]          - Target group ID (default: 0)");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-chat-send-memo -- simple-text 0");
    println!("  cargo run --bin test-memo-chat-send-memo -- with-receiver 1");
    println!("  cargo run --bin test-memo-chat-send-memo -- invalid-category 0");
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