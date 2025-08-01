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
use solana_system_interface::program as system_program;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use serde_json;
use sha2::{Sha256, Digest};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

#[derive(Debug, Clone)]
struct TestParams {
    pub burn_amount: u64,           // Burn amount in tokens (not units)
    pub category: String,           // Category field
    pub name: String,               // Group name
    pub description: String,        // Group description
    pub image: String,              // Group image
    pub tags: Vec<String>,          // Group tags
    pub min_memo_interval: Option<i64>, // Min memo interval
    pub should_succeed: bool,       // Whether the test should succeed
    pub test_description: String,   // Description of what this test validates
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
        "valid-basic" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "Basic Test Group".to_string(),
            description: "A basic test group".to_string(),
            image: "avatar_001.png".to_string(),
            tags: vec!["test".to_string(), "basic".to_string()],
            min_memo_interval: Some(60),
            should_succeed: true,
            test_description: "Valid group creation with all required fields".to_string(),
        },
        "invalid-category" => TestParams {
            burn_amount: 5,
            category: "invalid".to_string(),  // Wrong category
            name: "Test Group".to_string(),
            description: "Testing invalid category".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Invalid category field (should be 'chat')".to_string(),
        },
        "empty-name" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "".to_string(),  // Empty name
            description: "Testing empty name".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Empty group name (should fail)".to_string(),
        },
        "long-name" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "x".repeat(65),  // Name too long (>64 chars)
            description: "Testing long name".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group name too long (>64 characters)".to_string(),
        },
        "long-description" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "Test Group".to_string(),
            description: "x".repeat(129),  // Description too long (>128 chars)
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group description too long (>128 characters)".to_string(),
        },
        "long-image" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "Test Group".to_string(),
            description: "Testing long image".to_string(),
            image: "x".repeat(257),  // Image too long (>256 chars)
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group image info too long (>256 characters)".to_string(),
        },
        "too-many-tags" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "Test Group".to_string(),
            description: "Testing too many tags".to_string(),
            image: "test.png".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string(), "tag4".to_string(), "tag5".to_string()], // 5 tags (>4)
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Too many tags (>4 tags)".to_string(),
        },
        "long-tag" => TestParams {
            burn_amount: 5,
            category: "chat".to_string(),
            name: "Test Group".to_string(),
            description: "Testing long tag".to_string(),
            image: "test.png".to_string(),
            tags: vec!["x".repeat(33)], // Tag too long (>32 chars)
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Tag too long (>32 characters)".to_string(),
        },
        "small-burn-amount" => TestParams {
            burn_amount: 0,  // Less than 1 token
            category: "chat".to_string(),
            name: "Test Group".to_string(),
            description: "Testing small burn amount".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Burn amount too small (<1 token)".to_string(),
        },
        "minimal-valid" => TestParams {
            burn_amount: 1,
            category: "chat".to_string(),
            name: "T".to_string(),  // Minimal name
            description: "".to_string(),  // Empty description (allowed)
            image: "".to_string(),  // Empty image (allowed)
            tags: vec![],  // No tags (allowed)
            min_memo_interval: None,  // No interval specified
            should_succeed: true,
            test_description: "Minimal valid group creation".to_string(),
        },
        "max-valid" => TestParams {
            burn_amount: 100,
            category: "chat".to_string(),
            name: "x".repeat(64),  // Max name length
            description: "x".repeat(128),  // Max description length
            image: "x".repeat(256),  // Max image length
            tags: vec!["x".repeat(32), "y".repeat(32), "z".repeat(32), "w".repeat(32)], // Max tags
            min_memo_interval: Some(3600),
            should_succeed: true,
            test_description: "Maximum valid field lengths".to_string(),
        },
        "custom" => {
            if args.len() < 9 {
                println!("Custom test requires additional parameters:");
                println!("Usage: cargo run --bin test-memo-chat-create-group -- custom <burn_amount> <category> <name> <description> <image> <tags_csv> <min_interval>");
                println!("Example: cargo run --bin test-memo-chat-create-group -- custom 5 chat \"My Group\" \"Description\" \"image.png\" \"tag1,tag2\" 60");
                return Ok(());
            }
            
            let burn_amount = args[2].parse::<u64>().unwrap_or(1);
            let category = args[3].clone();
            let name = args[4].clone();
            let description = args[5].clone();
            let image = args[6].clone();
            let tags: Vec<String> = if args[7].is_empty() {
                vec![]
            } else {
                args[7].split(',').map(|s| s.trim().to_string()).collect()
            };
            let min_memo_interval = if args.len() > 8 && !args[8].is_empty() {
                Some(args[8].parse::<i64>().unwrap_or(60))
            } else {
                None
            };
            
            TestParams {
                burn_amount,
                category,
                name,
                description,
                image,
                tags,
                min_memo_interval,
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

    println!("=== MEMO-CHAT CREATE GROUP TEST ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();
    println!("Test parameters:");
    println!("  Burn amount: {} tokens", test_params.burn_amount);
    println!("  Category: {}", test_params.category);
    println!("  Name: {} (length: {})", test_params.name, test_params.name.len());
    println!("  Description: {} (length: {})", 
        if test_params.description.len() > 50 { 
            format!("{}...", &test_params.description[..50]) 
        } else { 
            test_params.description.clone() 
        }, 
        test_params.description.len()
    );
    println!("  Image: {} (length: {})", 
        if test_params.image.len() > 50 { 
            format!("{}...", &test_params.image[..50]) 
        } else { 
            test_params.image.clone() 
        }, 
        test_params.image.len()
    );
    println!("  Tags: {:?} (count: {})", test_params.tags, test_params.tags.len());
    println!("  Min memo interval: {:?}", test_params.min_memo_interval);
    println!();

    run_test(test_params)?;
    Ok(())
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
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid memo-burn program ID");
    let mint = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");

    // Calculate global counter PDA and get next group_id
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_chat_program_id,
    );

    let next_group_id = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            if account.data.len() >= 16 {
                let total_groups_bytes = &account.data[8..16];
                u64::from_le_bytes(total_groups_bytes.try_into().unwrap())
            } else {
                0
            }
        },
        Err(_) => {
            println!("⚠️  Global counter not found. Please run admin-init-global-group-counter first.");
            return Ok(());
        }
    };

    // Calculate chat group PDA
    let (chat_group_pda, _) = Pubkey::find_program_address(
        &[b"chat_group", &next_group_id.to_le_bytes()],
        &memo_chat_program_id,
    );

    // Get user's token account
    let creator_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    println!("Runtime info:");
    println!("  Next group ID: {}", next_group_id);
    println!("  Chat group PDA: {}", chat_group_pda);
    println!("  Creator: {}", payer.pubkey());
    println!();

    // Check token balance if burn amount > 0
    if params.burn_amount > 0 {
        match client.get_token_account_balance(&creator_token_account) {
            Ok(balance) => {
                let current_balance = balance.ui_amount.unwrap_or(0.0);
                println!("Current token balance: {} tokens", current_balance);
                
                if current_balance < params.burn_amount as f64 {
                    println!("❌ ERROR: Insufficient token balance!");
                    println!("   Required: {} tokens", params.burn_amount);
                    println!("   Available: {} tokens", current_balance);
                    return Ok(());
                }
            },
            Err(err) => {
                println!("❌ Error checking token balance: {}", err);
                return Ok(());
            }
        }
    }

    // Generate memo
    let memo_text = generate_memo_from_params(&params, next_group_id);
    
    println!("Generated memo:");
    println!("  Length: {} bytes", memo_text.as_bytes().len());
    if memo_text.len() > 200 {
        println!("  Content (first 100 chars): {}...", &memo_text[..100]);
        println!("  Content (last 100 chars): ...{}", &memo_text[memo_text.len()-100..]);
    } else {
        println!("  Content: {}", memo_text);
    }
    println!();

    // Get latest blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create instructions
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );

    let create_group_ix = create_chat_group_instruction(
        &memo_chat_program_id,
        &payer.pubkey(),
        &global_counter_pda,
        &chat_group_pda,
        &mint,
        &creator_token_account,
        &memo_burn_program_id,
        next_group_id,
        params.burn_amount * 1_000_000, // Convert to units
    );

    // First, simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, memo_ix.clone(), create_group_ix.clone()],
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
                let default_cu = 600_000u32;
                println!("Using default compute units for error case: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin as requested
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 600_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            600_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, create_group_ix],
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
            
            // Verify group creation
            match client.get_account(&chat_group_pda) {
                Ok(account) => {
                    println!("✅ Chat group {} created successfully!", next_group_id);
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

fn generate_memo_from_params(params: &TestParams, group_id: u64) -> String {
    let memo_json = serde_json::json!({
        "amount": params.burn_amount * 1_000_000, // Convert to units
        "category": params.category,
        "group_id": group_id,
        "name": params.name,
        "description": params.description,
        "image": params.image,
        "tags": params.tags,
        "min_memo_interval": params.min_memo_interval,
        "operation": "create_group",
        "timestamp": chrono::Utc::now().timestamp()
    });
    
    serde_json::to_string(&memo_json).unwrap()
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("InvalidCategory") && params.category != "chat" {
        println!("✅ Correct: Invalid category detected");
    } else if error_msg.contains("InvalidGroupName") && (params.name.is_empty() || params.name.len() > 64) {
        println!("✅ Correct: Invalid group name detected");
    } else if error_msg.contains("InvalidGroupDescription") && params.description.len() > 128 {
        println!("✅ Correct: Invalid group description detected");
    } else if error_msg.contains("InvalidGroupImage") && params.image.len() > 256 {
        println!("✅ Correct: Invalid group image detected");
    } else if error_msg.contains("TooManyTags") && params.tags.len() > 4 {
        println!("✅ Correct: Too many tags detected");
    } else if error_msg.contains("InvalidTag") && params.tags.iter().any(|tag| tag.is_empty() || tag.len() > 32) {
        println!("✅ Correct: Invalid tag detected");
    } else if error_msg.contains("BurnAmountTooSmall") && params.burn_amount < 1 {
        println!("✅ Correct: Burn amount too small detected");
    } else {
        println!("⚠️  Unexpected error type: {}", error_msg);
    }
}

fn analyze_unexpected_error(error_msg: &str) {
    println!("💡 Error analysis:");
    if error_msg.contains("MemoRequired") {
        println!("   Missing memo instruction");
    } else if error_msg.contains("InvalidMemoFormat") {
        println!("   Invalid memo format or JSON parsing failed");
    } else if error_msg.contains("AmountMismatch") {
        println!("   Amount in memo doesn't match burn amount");
    } else if error_msg.contains("GroupIdMismatch") {
        println!("   Group ID in memo doesn't match expected ID");
    } else if error_msg.contains("insufficient funds") {
        println!("   Insufficient SOL or token balance");
    } else {
        println!("   {}", error_msg);
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
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_chat_group");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    instruction_data.extend_from_slice(&expected_group_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*creator, true),
        AccountMeta::new(*global_counter, false),
        AccountMeta::new(*chat_group, false),
        AccountMeta::new(*mint, false),
        AccountMeta::new(*creator_token_account, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(*memo_burn_program, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(
            Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
            false
        ),
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn print_usage() {
    println!("Usage: cargo run --bin test-memo-chat-create-group -- <test_case>");
    println!();
    println!("Available test cases:");
    println!("  valid-basic       - Valid group creation with all fields");
    println!("  invalid-category  - Test invalid category field");
    println!("  empty-name        - Test empty group name");
    println!("  long-name         - Test group name too long (>64 chars)");
    println!("  long-description  - Test description too long (>128 chars)");
    println!("  long-image        - Test image info too long (>256 chars)");
    println!("  too-many-tags     - Test too many tags (>4 tags)");
    println!("  long-tag          - Test tag too long (>32 chars)");
    println!("  small-burn-amount - Test burn amount too small (<1 token)");
    println!("  minimal-valid     - Test minimal valid parameters");
    println!("  max-valid         - Test maximum valid field lengths");
    println!("  custom            - Custom test with specified parameters");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-chat-create-group -- valid-basic");
    println!("  cargo run --bin test-memo-chat-create-group -- invalid-category");
    println!("  cargo run --bin test-memo-chat-create-group -- custom 5 chat \"My Group\" \"Description\" \"image.png\" \"tag1,tag2\" 60");
} 