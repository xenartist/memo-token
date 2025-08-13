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
pub struct ChatGroupCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "chat" for memo-chat contract)
    pub category: String,
    
    /// Operation type (must be "create_group" for group creation)
    pub operation: String,
    
    /// Group ID (must match expected_group_id)
    pub group_id: u64,
    
    /// Group name (required, 1-64 characters)
    pub name: String,
    
    /// Group description (optional, max 128 characters)  
    pub description: String,
    
    /// Group image info (optional, max 256 characters)
    pub image: String,
    
    /// Tags (optional, max 4 tags, each max 32 characters)
    pub tags: Vec<String>,
    
    /// Minimum memo interval in seconds (optional, defaults to 60)
    pub min_memo_interval: Option<i64>,
}

impl ChatGroupCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_group_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Validate version
        if self.version != CHAT_GROUP_CREATION_DATA_VERSION {
            println!("Unsupported chat group creation data version: {} (expected: {})", 
                 self.version, CHAT_GROUP_CREATION_DATA_VERSION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported chat group creation data version")));
        }
        
        // Validate category (must be exactly "chat")
        if self.category != EXPECTED_CATEGORY {
            println!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category")));
        }
        
        // Validate category length (must be exactly the expected length)
        if self.category.len() != EXPECTED_CATEGORY.len() {
            println!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category length")));
        }
        
        // Validate operation (must be exactly "create_group")
        if self.operation != EXPECTED_OPERATION {
            println!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation")));
        }
        
        // Validate operation length (must be exactly the expected length)
        if self.operation.len() != EXPECTED_OPERATION.len() {
            println!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_OPERATION.len(), EXPECTED_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation length")));
        }
        
        // Validate group_id
        if self.group_id != expected_group_id {
            println!("Group ID mismatch: data contains {}, expected {}", 
                 self.group_id, expected_group_id);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Group ID mismatch")));
        }
        
        // Validate name (required, 1-64 characters)
        if self.name.is_empty() || self.name.len() > 64 {
            println!("Invalid group name: '{}' (must be 1-64 characters)", self.name);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid group name")));
        }
        
        // Validate description (optional, max 128 characters)
        if self.description.len() > 128 {
            println!("Invalid group description: {} characters (max: 128)", self.description.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid group description")));
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > 256 {
            println!("Invalid group image: {} characters (max: 256)", self.image.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid group image")));
        }
        
        // Validate tags (optional, max 4 tags, each max 32 characters)
        if self.tags.len() > 4 {
            println!("Too many tags: {} (max: 4)", self.tags.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Too many tags")));
        }
        
        for (i, tag) in self.tags.iter().enumerate() {
            if tag.is_empty() || tag.len() > 32 {
                println!("Invalid tag {}: '{}' (must be 1-32 characters)", i, tag);
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid tag")));
            }
        }
        
        // Validate min_memo_interval (optional, should be reasonable if provided)
        if let Some(interval) = self.min_memo_interval {
            if interval < 0 || interval > 86400 {  // Max 24 hours
                println!("Invalid min_memo_interval: {} (must be 0-86400 seconds)", interval);
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid min_memo_interval")));
            }
        }
        
        println!("Chat group creation data validation passed: category={}, operation={}, group_id={}, name={}, tags_count={}", 
             self.category, self.operation, self.group_id, self.name, self.tags.len());
        
        Ok(())
    }
}

// Constants matching the contract
const BURN_MEMO_VERSION: u8 = 1;
const CHAT_GROUP_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "chat";
const EXPECTED_OPERATION: &str = "create_group";

#[derive(Debug, Clone)]
struct TestParams {
    pub burn_amount: u64,           // Burn amount in tokens (not units)
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
    
    // Define test cases (removed category field since it's no longer needed)
    let test_params = match test_case.as_str() {
        "valid-basic" => TestParams {
            burn_amount: 50000, // Increased to meet minimum requirement
            name: "Basic Test Group".to_string(),
            description: "A basic test group".to_string(),
            image: "avatar_001.png".to_string(),
            tags: vec!["test".to_string(), "basic".to_string()],
            min_memo_interval: Some(60),
            should_succeed: true,
            test_description: "Valid group creation with all required fields".to_string(),
        },
        "empty-name" => TestParams {
            burn_amount: 50000,
            name: "".to_string(),  // Empty name
            description: "Testing empty name".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Empty group name (should fail)".to_string(),
        },
        "long-name" => TestParams {
            burn_amount: 50000,
            name: "x".repeat(65),  // Name too long (>64 chars)
            description: "Testing long name".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group name too long (>64 characters)".to_string(),
        },
        "long-description" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "x".repeat(129),  // Description too long (>128 chars)
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group description too long (>128 characters)".to_string(),
        },
        "long-image" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "Testing long image".to_string(),
            image: "x".repeat(257),  // Image too long (>256 chars)
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Group image info too long (>256 characters)".to_string(),
        },
        "too-many-tags" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "Testing too many tags".to_string(),
            image: "test.png".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string(), "tag4".to_string(), "tag5".to_string()], // 5 tags (>4)
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Too many tags (>4 tags)".to_string(),
        },
        "long-tag" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "Testing long tag".to_string(),
            image: "test.png".to_string(),
            tags: vec!["x".repeat(33)], // Tag too long (>32 chars)
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Tag too long (>32 characters)".to_string(),
        },
        "small-burn-amount" => TestParams {
            burn_amount: 10000,  // Less than required 42069 tokens
            name: "Test Group".to_string(),
            description: "Testing small burn amount".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Burn amount too small (<42069 tokens)".to_string(),
        },
        "minimal-valid" => TestParams {
            burn_amount: 42069,  // Minimum required amount
            name: "T".to_string(),  // Minimal name
            description: "".to_string(),  // Empty description (allowed)
            image: "".to_string(),  // Empty image (allowed)
            tags: vec![],  // No tags (allowed)
            min_memo_interval: None,  // No interval specified
            should_succeed: true,
            test_description: "Minimal valid group creation".to_string(),
        },
        "max-valid" => TestParams {
            burn_amount: 100000,
            name: "x".repeat(64),  // Max name length
            description: "x".repeat(128),  // Max description length
            image: "x".repeat(256),  // Max image length
            tags: vec!["x".repeat(32), "y".repeat(32), "z".repeat(32), "w".repeat(32)], // Max tags
            min_memo_interval: Some(3600),
            should_succeed: true,
            test_description: "Maximum valid field lengths".to_string(),
        },
        "custom" => {
            if args.len() < 8 {
                println!("Custom test requires additional parameters:");
                println!("Usage: cargo run --bin test-memo-chat-create-group -- custom <burn_amount> <name> <description> <image> <tags_csv> <min_interval>");
                println!("Example: cargo run --bin test-memo-chat-create-group -- custom 50000 \"My Group\" \"Description\" \"image.png\" \"tag1,tag2\" 60");
                return Ok(());
            }
            
            let burn_amount = args[2].parse::<u64>().unwrap_or(42069);
            let name = args[3].clone();
            let description = args[4].clone();
            let image = args[5].clone();
            let tags: Vec<String> = if args[6].is_empty() {
                vec![]
            } else {
                args[6].split(',').map(|s| s.trim().to_string()).collect()
            };
            let min_memo_interval = if args.len() > 7 && !args[7].is_empty() {
                Some(args[7].parse::<i64>().unwrap_or(60))
            } else {
                None
            };
            
            TestParams {
                burn_amount,
                name,
                description,
                image,
                tags,
                min_memo_interval,
                should_succeed: true, // Assume custom tests should succeed unless proven otherwise
                test_description: "Custom test case".to_string(),
            }
        },
        "invalid-category" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "Testing invalid category".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Invalid category (should fail)".to_string(),
        },
        "invalid-operation" => TestParams {
            burn_amount: 50000,
            name: "Test Group".to_string(),
            description: "Testing invalid operation".to_string(),
            image: "test.png".to_string(),
            tags: vec!["test".to_string()],
            min_memo_interval: Some(60),
            should_succeed: false,
            test_description: "Invalid operation (should fail)".to_string(),
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO-CHAT CREATE GROUP TEST (BORSH+BASE64 FORMAT) ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();
    println!("Test parameters:");
    println!("  Burn amount: {} tokens", test_params.burn_amount);
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
    let rpc_url = "https://rpc.testnet.x1.xyz";
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

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, _) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
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
    println!("  Burn leaderboard PDA: {}", burn_leaderboard_pda);
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

    // Generate Borsh+Base64 memo
    let memo_bytes = generate_borsh_memo_from_params(&params, next_group_id)?;
    
    println!("Generated Borsh+Base64 memo:");
    println!("  Base64 length: {} bytes", memo_bytes.len());
    
    // Show the underlying structure by decoding
    if let Ok(base64_str) = std::str::from_utf8(&memo_bytes) {
        if let Ok(decoded_data) = general_purpose::STANDARD.decode(base64_str) {
            println!("  Decoded Borsh length: {} bytes", decoded_data.len());
            
            if let Ok(burn_memo) = BurnMemo::try_from_slice(&decoded_data) {
                println!("  BurnMemo structure:");
                println!("    version: {}", burn_memo.version);
                println!("    burn_amount: {} units", burn_memo.burn_amount);
                println!("    payload: {} bytes", burn_memo.payload.len());
                
                if let Ok(group_data) = ChatGroupCreationData::try_from_slice(&burn_memo.payload) {
                    println!("  ChatGroupCreationData structure:");
                    println!("    version: {}", group_data.version);
                    println!("    category: {}", group_data.category);
                    println!("    operation: {}", group_data.operation);
                    println!("    group_id: {}", group_data.group_id);
                    println!("    name: {}", group_data.name);
                }
            }
        }
    }
    
    if memo_bytes.len() <= 100 {
        println!("  Base64 content: {}", String::from_utf8_lossy(&memo_bytes));
    } else {
        println!("  Base64 preview: {}...", String::from_utf8_lossy(&memo_bytes[..50]));
    }
    println!();

    // Get latest blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create instructions
    let memo_ix = spl_memo::build_memo(
        &memo_bytes,
        &[&payer.pubkey()],
    );

    let create_group_ix = create_chat_group_instruction(
        &memo_chat_program_id,
        &payer.pubkey(),
        &global_counter_pda,
        &chat_group_pda,
        &burn_leaderboard_pda,
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
                // Add 20% margin as requested
                let optimal_cu = ((units_consumed as f64) * 1.2) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+20% margin)", 
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

fn generate_borsh_memo_from_params(params: &TestParams, group_id: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Determine category based on test case
    let category = if params.test_description.contains("invalid category") {
        "wrong_category".to_string()  // intentionally use wrong category
    } else {
        EXPECTED_CATEGORY.to_string()  // use "chat" in normal case
    };
    
    // Determine operation based on test case
    let operation = if params.test_description.contains("invalid operation") {
        "wrong_operation".to_string()  // intentionally use wrong operation
    } else {
        EXPECTED_OPERATION.to_string()  // use "create_group" in normal case
    };
    
    // Create ChatGroupCreationData
    let group_creation_data = ChatGroupCreationData {
        version: CHAT_GROUP_CREATION_DATA_VERSION,
        category,
        operation,
        group_id,
        name: params.name.clone(),
        description: params.description.clone(),
        image: params.image.clone(),
        tags: params.tags.clone(),
        min_memo_interval: params.min_memo_interval,
    };
    
    // Serialize ChatGroupCreationData to bytes (this becomes the payload)
    let payload = group_creation_data.try_to_vec()?;
    
    // Create BurnMemo with the payload
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: params.burn_amount * 1_000_000, // Convert to units
        payload,
    };
    
    // Serialize the entire BurnMemo to bytes
    let borsh_data = burn_memo.try_to_vec()?;
    
    // Encode with Base64
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    let memo_bytes = base64_encoded.into_bytes();
    
    println!("Borsh+Base64 structure sizes:");
    println!("  ChatGroupCreationData payload: {} bytes", burn_memo.payload.len());
    println!("  Complete BurnMemo (Borsh): {} bytes", borsh_data.len());
    println!("  Base64 encoded memo: {} bytes", memo_bytes.len());
    
    Ok(memo_bytes)
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("InvalidGroupName") && (params.name.is_empty() || params.name.len() > 64) {
        println!("✅ Correct: Invalid group name detected");
    } else if error_msg.contains("InvalidGroupDescription") && params.description.len() > 128 {
        println!("✅ Correct: Invalid group description detected");
    } else if error_msg.contains("InvalidGroupImage") && params.image.len() > 256 {
        println!("✅ Correct: Invalid group image detected");
    } else if error_msg.contains("TooManyTags") && params.tags.len() > 4 {
        println!("✅ Correct: Too many tags detected");
    } else if error_msg.contains("InvalidTag") && params.tags.iter().any(|tag| tag.is_empty() || tag.len() > 32) {
        println!("✅ Correct: Invalid tag detected");
    } else if error_msg.contains("BurnAmountTooSmall") && params.burn_amount < 42069 {
        println!("✅ Correct: Burn amount too small detected");
    } else if error_msg.contains("InvalidCategory") && params.test_description.contains("invalid category") {
        println!("✅ Correct: Invalid category detected");
    } else if error_msg.contains("InvalidOperation") && params.test_description.contains("invalid operation") {
        println!("✅ Correct: Invalid operation detected");
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
    } else if error_msg.contains("BurnAmountMismatch") {
        println!("   Burn amount in memo doesn't match burn amount");
    } else if error_msg.contains("GroupIdMismatch") {
        println!("   Group ID in memo doesn't match expected ID");
    } else if error_msg.contains("InvalidOperationLength") {
        println!("   Invalid operation length detected");
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
    burn_leaderboard: &Pubkey,
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
        AccountMeta::new(*burn_leaderboard, false),
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
    println!("  empty-name        - Test empty group name");
    println!("  long-name         - Test group name too long (>64 chars)");
    println!("  long-description  - Test description too long (>128 chars)");
    println!("  long-image        - Test image info too long (>256 chars)");
    println!("  too-many-tags     - Test too many tags (>4 tags)");
    println!("  long-tag          - Test tag too long (>32 chars)");
    println!("  small-burn-amount - Test burn amount too small (<42069 tokens)");
    println!("  minimal-valid     - Test minimal valid parameters");
    println!("  max-valid         - Test maximum valid field lengths");
    println!("  custom            - Custom test with specified parameters");
    println!("  invalid-category  - Test invalid category (should fail)");
    println!("  invalid-operation - Test invalid operation (should fail)");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-chat-create-group -- valid-basic");
    println!("  cargo run --bin test-memo-chat-create-group -- empty-name");
    println!("  cargo run --bin test-memo-chat-create-group -- custom 50000 \"My Group\" \"Description\" \"image.png\" \"tag1,tag2\" 60");
    println!("  cargo run --bin test-memo-chat-create-group -- invalid-category");
} 