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
pub struct ProjectBurnData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "project" for memo-project contract)
    pub category: String,
    
    /// Operation type (must be "burn_for_project" for burning tokens)
    pub operation: String,
    
    /// Project ID (must match the target project)
    pub project_id: u64,
    
    /// Burner pubkey as string (must match the transaction signer)
    pub burner: String,
    
    /// Burn message (optional, max 696 characters)
    pub message: String,
}

impl ProjectBurnData {
    /// Validate the structure fields
    pub fn validate(&self, expected_project_id: u64, expected_burner: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
        // Validate version
        if self.version != PROJECT_CREATION_DATA_VERSION {
            println!("Unsupported project burn data version: {} (expected: {})", 
                 self.version, PROJECT_CREATION_DATA_VERSION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported project burn data version")));
        }
        
        // Validate category (must be exactly "project")
        if self.category != EXPECTED_CATEGORY {
            println!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category")));
        }
        
        // Validate category length
        if self.category.len() != EXPECTED_CATEGORY.len() {
            println!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category length")));
        }
        
        // Validate operation (must be exactly "burn_for_project")
        if self.operation != EXPECTED_BURN_OPERATION {
            println!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_BURN_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation")));
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_BURN_OPERATION.len() {
            println!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_BURN_OPERATION.len(), EXPECTED_BURN_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation length")));
        }
        
        // Validate project_id
        if self.project_id != expected_project_id {
            println!("Project ID mismatch: data contains {}, expected {}", 
                 self.project_id, expected_project_id);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Project ID mismatch")));
        }
        
        // Validate burner (convert string to Pubkey and compare)
        let burner_pubkey = Pubkey::from_str(&self.burner)
            .map_err(|_| {
                println!("Invalid burner format: {}", self.burner);
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid burner format"))
            })?;
            
        if burner_pubkey != expected_burner {
            println!("Burner mismatch: data contains {}, expected {}", 
                 burner_pubkey, expected_burner);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Burner mismatch")));
        }
        
        // Validate message (optional, max 696 characters)
        if self.message.len() > 696 {
            println!("Burn message too long: {} characters (max: 696)", self.message.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Burn message too long")));
        }
        
        println!("Project burn data validation passed: category={}, operation={}, project_id={}, burner={}, message_len={}", 
             self.category, self.operation, self.project_id, self.burner, self.message.len());
        
        Ok(())
    }
}

// Constants matching the contract
const BURN_MEMO_VERSION: u8 = 1;
const PROJECT_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "project";
const EXPECTED_BURN_OPERATION: &str = "burn_for_project";

#[derive(Debug, Clone)]
struct TestParams {
    pub project_id: u64,           // Target project ID
    pub burn_amount: u64,          // Burn amount in tokens (not units)
    pub message: String,           // Burn message (optional, max 696 characters)
    pub should_succeed: bool,      // Whether the test should succeed
    pub test_description: String,  // Description of what this test validates
}

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 {
        print_usage();
        return Ok(());
    }

    let test_case = &args[1];
    let project_id_str = &args[2];
    let project_id = project_id_str.parse::<u64>()
        .map_err(|_| format!("Invalid project_id: {}", project_id_str))?;
    
    // Define test cases
    let test_params = match test_case.as_str() {
        "valid-basic" => TestParams {
            project_id,
            burn_amount: 420, // 420 tokens (minimum)
            message: "Basic burn test for project support".to_string(),
            should_succeed: true,
            test_description: "Valid burn operation with minimum amount and basic message".to_string(),
        },
        "valid-large" => TestParams {
            project_id,
            burn_amount: 10000, // 10,000 tokens
            message: "Large amount burn for project development and growth".to_string(),
            should_succeed: true,
            test_description: "Valid burn operation with large amount and detailed message".to_string(),
        },
        "valid-empty-message" => TestParams {
            project_id,
            burn_amount: 1000, // 1,000 tokens
            message: "".to_string(),
            should_succeed: true,
            test_description: "Valid burn operation with empty message".to_string(),
        },
        "valid-long-message" => TestParams {
            project_id,
            burn_amount: 5000, // 5,000 tokens
            message: "This is a very long burn message to test the maximum message length validation for project burning. With the increased limit of 696 characters, we can include much more detailed information about why this burn operation is being performed, what the expected outcomes are, how this supports the project's goals and development roadmap, and provide comprehensive context for the community to understand the reasoning behind this significant token burn. This extended message length allows for better communication and transparency in project support activities, enabling project creators and supporters to document their contributions and intentions more thoroughly than ever before.".to_string(),
            should_succeed: true,
            test_description: "Valid burn operation with long message (near 696 char limit)".to_string(),
        },
        "small-amount" => TestParams {
            project_id,
            burn_amount: 100, // 100 tokens (below minimum 420)
            message: "Testing small amount".to_string(),
            should_succeed: false,
            test_description: "Test burn amount too small (should fail)".to_string(),
        },
        "invalid-project" => TestParams {
            project_id: 999999, // Non-existent project
            burn_amount: 420,
            message: "Testing invalid project".to_string(),
            should_succeed: false,
            test_description: "Test burn for non-existent project (should fail)".to_string(),
        },
        "too-long-message" => TestParams {
            project_id,
            burn_amount: 420,
            message: "A".repeat(697), // 697 characters (should fail)
            should_succeed: false,
            test_description: "Test message too long (should fail)".to_string(),
        },
        "custom" => {
            if args.len() < 4 {
                println!("❌ Custom test requires burn amount parameter");
                print_usage();
                return Ok(());
            }
            let custom_burn_amount = args[3].parse::<u64>()
                .map_err(|_| format!("Invalid burn amount: {}", args[3]))?;
            
            let custom_message = if args.len() >= 5 {
                args[4].clone()
            } else {
                format!("Custom burn: {} tokens for project {}", custom_burn_amount, project_id)
            };
            
            TestParams {
                project_id,
                burn_amount: custom_burn_amount,
                message: custom_message,
                should_succeed: true,
                test_description: format!("Custom burn test: {} tokens for project {} with message", custom_burn_amount, project_id),
            }
        },
        _ => {
            println!("❌ Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    // Program IDs
    let memo_project_program_id = get_program_id("memo_project")?;
    let memo_burn_program_id = get_program_id("memo_burn")?;
    let mint = get_token_mint("memo_token")?;

    // Setup client and keypair
    let rpc_url = get_rpc_url();
    let client = RpcClient::new(rpc_url.to_string());
    
    let payer_path = std::env::var("SOLANA_KEYPAIR_PATH")
        .unwrap_or_else(|_| format!("{}/.config/solana/id.json", std::env::var("HOME").unwrap()));
    let payer = read_keypair_file(&payer_path)?;

    println!("=== Memo Project Burn For Project Test ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Project ID: {}", test_params.project_id);
    println!("Burn amount: {} tokens", test_params.burn_amount);
    println!("Burn message: \"{}\" ({} chars)", test_params.message, test_params.message.len());
    println!("Expected result: {}", if test_params.should_succeed { "SUCCESS" } else { "FAILURE" });
    println!();

    println!("Configuration:");
    println!("  RPC URL: {}", rpc_url);
    println!("  Payer: {}", payer.pubkey());
    println!("  Memo Project Program: {}", memo_project_program_id);
    println!("  Memo Burn Program: {}", memo_burn_program_id);
    println!("  Mint: {}", mint);
    println!();

    // Calculate project PDA
    let (project_pda, _) = Pubkey::find_program_address(
        &[b"project", &test_params.project_id.to_le_bytes()],
        &memo_project_program_id,
    );

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, _) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &memo_project_program_id,
    );

    // Get user's token account
    let burner_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &memo_burn_program_id,
    );

    // Check if user global burn statistics account exists
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("✅ User global burn statistics account found: {}", user_global_burn_stats_pda);
        },
        Err(_) => {
            println!("❌ User global burn statistics account not found: {}", user_global_burn_stats_pda);
            println!("💡 Please run init-user-global-burn-stats first:");
            println!("   cd clients/burn && cargo run --bin init-user-global-burn-stats");
            return Ok(());
        }
    }

    println!("Runtime info:");
    println!("  Target project ID: {}", test_params.project_id);
    println!("  Project PDA: {}", project_pda);
    println!("  Burn leaderboard PDA: {}", burn_leaderboard_pda);
    println!("  Burner: {}", payer.pubkey());
    println!("  Burner token account: {}", burner_token_account);
    println!();

    // Check if project exists and verify creator permission
    match client.get_account(&project_pda) {
        Ok(account) => {
            println!("✅ Project {} found (account size: {} bytes)", test_params.project_id, account.data.len());
            
            // Parse basic project info to check creator
            if let Ok(project_info) = parse_project_creator(&account.data) {
                println!("   Project creator: {}", project_info);
                
                if project_info != payer.pubkey() {
                    println!("❌ ERROR: Only the project creator can burn tokens for this project!");
                    println!("   Project creator: {}", project_info);
                    println!("   Current signer: {}", payer.pubkey());
                    println!();
                    println!("💡 This is expected behavior - only project creators can burn tokens for their projects.");
                    return Ok(());
                }
                
                println!("✅ Permission verified: You are the project creator");
            }
        },
        Err(_) => {
            println!("❌ Project {} not found! Please create the project first.", test_params.project_id);
            return Ok(());
        }
    }

    // Check token balance
    if test_params.burn_amount > 0 {
        match client.get_token_account_balance(&burner_token_account) {
            Ok(balance) => {
                let current_balance = balance.ui_amount.unwrap_or(0.0);
                println!("Current token balance: {} tokens", current_balance);
                
                if current_balance < test_params.burn_amount as f64 {
                    println!("❌ ERROR: Insufficient token balance!");
                    println!("   Required: {} tokens", test_params.burn_amount);
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
    let memo_bytes = generate_borsh_memo_from_params(&test_params, &payer.pubkey())?;
    
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
                
                if let Ok(burn_data) = ProjectBurnData::try_from_slice(&burn_memo.payload) {
                    println!("  ProjectBurnData structure:");
                    println!("    version: {}", burn_data.version);
                    println!("    category: {}", burn_data.category);
                    println!("    operation: {}", burn_data.operation);
                    println!("    project_id: {}", burn_data.project_id);
                    println!("    burner: {}", burn_data.burner);
                    println!("    message: \"{}\" ({} chars)", burn_data.message, burn_data.message.len());
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

    let burn_ix = burn_for_project_instruction(
        &memo_project_program_id,
        &payer.pubkey(),
        &project_pda,
        &burn_leaderboard_pda,
        &mint,
        &burner_token_account,
        &memo_burn_program_id,
        &user_global_burn_stats_pda,
        test_params.project_id,
        test_params.burn_amount * 1_000_000, // Convert to units
    );

    // First, simulate transaction to get optimal CU limit
    // Instruction order: memo (index 0), burn (index 1), compute budget (index 2)
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[memo_ix.clone(), burn_ix.clone(), dummy_compute_budget_ix],
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
                let default_cu = 300_000u32;
                println!("Using default compute units for error case: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin (remembering memory)
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

    // Create final transaction with optimal compute budget
    // Instruction order: memo (index 0), burn (index 1), compute budget (index 2)
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[memo_ix, burn_ix, compute_budget_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Sending transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 TRANSACTION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            
            if test_params.should_succeed {
                println!("✅ EXPECTED SUCCESS: Test passed as expected");
            } else {
                println!("❌ UNEXPECTED SUCCESS: Test should have failed but succeeded");
            }
            
            // Verify burn result
            match client.get_account(&project_pda) {
                Ok(account) => {
                    println!("✅ Burn operation completed for project {}!", test_params.project_id);
                    println!("   Project account size: {} bytes", account.data.len());
                },
                Err(e) => {
                    println!("⚠️  Could not fetch project after burn: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            if test_params.should_succeed {
                println!("❌ UNEXPECTED FAILURE: Test should have succeeded");
                analyze_unexpected_error(&err.to_string());
            } else {
                println!("✅ EXPECTED FAILURE: Test failed as expected");
                analyze_expected_error(&err.to_string(), &test_params);
            }
        }
    }

    Ok(())
}

fn generate_borsh_memo_from_params(params: &TestParams, burner_pubkey: &Pubkey) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Create ProjectBurnData payload
    let burn_data = ProjectBurnData {
        version: PROJECT_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_BURN_OPERATION.to_string(),
        project_id: params.project_id,
        burner: burner_pubkey.to_string(),
        message: params.message.clone(),
    };
    
    // Validate the burn data
    burn_data.validate(params.project_id, *burner_pubkey)?;
    
    // Serialize ProjectBurnData to bytes
    let payload = burn_data.try_to_vec()?;
    
    // Create BurnMemo structure
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
    println!("  ProjectBurnData payload: {} bytes", burn_memo.payload.len());
    println!("  Complete BurnMemo (Borsh): {} bytes", borsh_data.len());
    println!("  Base64 encoded memo: {} bytes", memo_bytes.len());
    
    Ok(memo_bytes)
}

fn parse_project_creator(data: &[u8]) -> Result<Pubkey, Box<dyn std::error::Error>> {
    if data.len() < 48 { // 8 discriminator + 8 project_id + 32 creator
        return Err("Data too short for project creator".into());
    }

    let creator_offset = 16; // Skip discriminator (8) + project_id (8)
    let creator_bytes: [u8; 32] = data[creator_offset..creator_offset + 32].try_into().unwrap();
    Ok(Pubkey::from(creator_bytes))
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("BurnAmountTooSmall") && params.burn_amount < 420 {
        println!("✅ Correct: Burn amount too small detected (minimum 420 tokens required)");
    } else if error_msg.contains("ProjectNotFound") || error_msg.contains("InvalidProjectPDA") {
        println!("✅ Correct: Invalid project detected");
    } else if error_msg.contains("insufficient funds") {
        println!("✅ Correct: Insufficient token balance detected");
    } else if error_msg.contains("BurnMessageTooLong") && params.message.len() > 696 {
        println!("✅ Correct: Burn message too long detected (maximum 696 characters)");
    } else if error_msg.contains("UnauthorizedProjectAccess") {
        println!("✅ Correct: Only project creator can burn tokens for the project");
    } else {
        println!("⚠️  Unexpected error type: {}", error_msg);
    }
}

fn analyze_unexpected_error(error_msg: &str) {
    println!("💡 Error analysis:");
    if error_msg.contains("MemoRequired") {
        println!("   Missing memo instruction");
    } else if error_msg.contains("InvalidProjectBurnDataFormat") {
        println!("   Invalid memo format, Base64 decoding, or Borsh parsing failed");
    } else if error_msg.contains("UnsupportedMemoVersion") {
        println!("   Unsupported memo version");
    } else if error_msg.contains("BurnAmountMismatch") {
        println!("   Burn amount in memo doesn't match burn amount");
    } else if error_msg.contains("ProjectIdMismatch") {
        println!("   Project ID in memo doesn't match expected ID");
    } else if error_msg.contains("BurnerMismatch") {
        println!("   Burner in memo doesn't match transaction signer");
    } else if error_msg.contains("BurnMessageTooLong") {
        println!("   Burn message exceeds 696 character limit");
    } else if error_msg.contains("UnauthorizedProjectAccess") {
        println!("   Only project creator can burn tokens for the project");
    } else if error_msg.contains("InvalidOperationLength") {
        println!("   Invalid operation length detected");
    } else if error_msg.contains("insufficient funds") {
        println!("   Insufficient SOL or token balance");
    } else {
        println!("   {}", error_msg);
    }
}

fn burn_for_project_instruction(
    program_id: &Pubkey,
    burner: &Pubkey,
    project: &Pubkey,
    burn_leaderboard: &Pubkey,
    mint: &Pubkey,
    burner_token_account: &Pubkey,
    memo_burn_program: &Pubkey,
    user_global_burn_stats: &Pubkey,
    project_id: u64,
    amount: u64,
) -> Instruction {
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_for_project");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    instruction_data.extend_from_slice(&project_id.to_le_bytes());
    instruction_data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*burner, true),
        AccountMeta::new(*project, false),
        AccountMeta::new(*burn_leaderboard, false),
        AccountMeta::new(*mint, false),
        AccountMeta::new(*burner_token_account, false),
        AccountMeta::new(*user_global_burn_stats, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(*memo_burn_program, false),
        AccountMeta::new_readonly(
            solana_sdk::sysvar::instructions::id(),
            false
        ),
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn print_usage() {
    println!("Usage: cargo run --bin test-memo-project-burn-for-project -- <test_case> <project_id> [amount] [message]");
    println!();
    println!("Available test cases:");
    println!("  valid-basic         - Valid burn operation with 420 tokens (minimum) and basic message");
    println!("  valid-large         - Valid burn operation with 10,000 tokens and detailed message");
    println!("  valid-empty-message - Valid burn operation with empty message");
    println!("  valid-long-message  - Valid burn operation with long message (near 696 char limit)");
    println!("  small-amount        - Test burn amount too small (should fail)");
    println!("  invalid-project     - Test burn for non-existent project (should fail)");
    println!("  too-long-message    - Test message too long (should fail)");
    println!("  custom              - Custom test with specified amount and message");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-project-burn-for-project -- valid-basic 0");
    println!("  cargo run --bin test-memo-project-burn-for-project -- valid-large 0");
    println!("  cargo run --bin test-memo-project-burn-for-project -- small-amount 0");
    println!("  cargo run --bin test-memo-project-burn-for-project -- invalid-project 999999");
    println!("  cargo run --bin test-memo-project-burn-for-project -- custom 0 5000 \"Supporting project development\"");
    println!();
    println!("Note: Make sure the specified project_id exists and you are the project creator.");
    println!("      Messages are optional and limited to 696 characters.");
    println!("      Minimum burn amount is 420 tokens.");
}
