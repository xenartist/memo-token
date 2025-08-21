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
pub struct ProfileUpdateData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "profile" for memo-profile contract)
    pub category: String,
    
    /// Operation type (must be "update_profile" for profile update)
    pub operation: String,
    
    /// User pubkey as string (must match the transaction signer)
    pub user_pubkey: String,
    
    /// Updated fields (all optional)
    pub username: Option<String>,
    pub image: Option<String>,
    pub about_me: Option<Option<String>>,
    pub url: Option<Option<String>>,
}

impl ProfileUpdateData {
    /// Validate the structure fields
    pub fn validate(&self, expected_user: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
        // Validate version
        if self.version != PROFILE_UPDATE_DATA_VERSION {
            println!("Unsupported profile update data version: {} (expected: {})", 
                 self.version, PROFILE_UPDATE_DATA_VERSION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported profile update data version")));
        }
        
        // Validate category (must be exactly "profile")
        if self.category != EXPECTED_CATEGORY {
            println!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category")));
        }
        
        // Validate operation (must be exactly "update_profile")
        if self.operation != EXPECTED_UPDATE_OPERATION {
            println!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_UPDATE_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation")));
        }
        
        // Validate user_pubkey matches expected user
        let parsed_pubkey = Pubkey::from_str(&self.user_pubkey)
            .map_err(|_| {
                println!("Invalid user_pubkey format: {}", self.user_pubkey);
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid user_pubkey format")
            })?;
        
        if parsed_pubkey != expected_user {
            println!("User pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_user);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "User pubkey mismatch")));
        }
        
        println!("Profile update data validation passed: category={}, operation={}, user={}", 
             self.category, self.operation, self.user_pubkey);
        
        Ok(())
    }
}

// Constants matching the contract
const BURN_MEMO_VERSION: u8 = 1;
const PROFILE_UPDATE_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "profile";
const EXPECTED_UPDATE_OPERATION: &str = "update_profile";
const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)
const MIN_PROFILE_UPDATE_BURN_TOKENS: u64 = 420; // Minimum tokens to burn for profile update
const MIN_PROFILE_UPDATE_BURN_AMOUNT: u64 = MIN_PROFILE_UPDATE_BURN_TOKENS * DECIMAL_FACTOR;

#[derive(Debug, Clone)]
struct UpdateParams {
    pub username: Option<String>,         // New username (None = don't update)
    pub image: Option<String>,            // New image (None = don't update)
    pub about_me: Option<Option<String>>, // None = don't update, Some(None) = clear, Some(Some(text)) = set
    pub url: Option<Option<String>>,      // None = don't update, Some(None) = clear, Some(Some(url)) = set
    pub should_succeed: bool,             // Whether the test should succeed
    pub test_description: String,         // Description of what this test validates
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
        "update-username" => UpdateParams {
            username: Some("alice_updated".to_string()),
            image: None,
            about_me: None,
            url: None,
            should_succeed: true,
            test_description: "Update only username".to_string(),
        },
        "update-image" => UpdateParams {
            username: None,
            image: Some("c:64x64:NEW_IMAGE_DATA_HERE".to_string()),
            about_me: None,
            url: None,
            should_succeed: true,
            test_description: "Update only image".to_string(),
        },
        "update-about-me" => UpdateParams {
            username: None,
            image: None,
            about_me: Some(Some("Updated about me text!".to_string())),
            url: None,
            should_succeed: true,
            test_description: "Update only about me".to_string(),
        },
        "clear-about-me" => UpdateParams {
            username: None,
            image: None,
            about_me: Some(None), // Clear the about_me field
            url: None,
            should_succeed: true,
            test_description: "Clear about me field".to_string(),
        },
        "update-url" => UpdateParams {
            username: None,
            image: None,
            about_me: None,
            url: Some(Some("https://updated.example.com".to_string())),
            should_succeed: true,
            test_description: "Update only URL".to_string(),
        },
        "clear-url" => UpdateParams {
            username: None,
            image: None,
            about_me: None,
            url: Some(None), // Clear the URL field
            should_succeed: true,
            test_description: "Clear URL field".to_string(),
        },
        "update-all" => UpdateParams {
            username: Some("alice_complete".to_string()),
            image: Some("c:128x128:UPDATED_COMPLETE_IMAGE".to_string()),
            about_me: Some(Some("Completely updated profile!".to_string())),
            url: Some(Some("https://complete.example.com".to_string())),
            should_succeed: true,
            test_description: "Update all fields".to_string(),
        },
        "empty-username" => UpdateParams {
            username: Some("".to_string()), // Empty username should fail
            image: None,
            about_me: None,
            url: None,
            should_succeed: false,
            test_description: "Invalid update with empty username".to_string(),
        },
        "long-username" => UpdateParams {
            username: Some("a".repeat(33)), // Too long username should fail
            image: None,
            about_me: None,
            url: None,
            should_succeed: false,
            test_description: "Invalid update with long username".to_string(),
        },
        "long-image" => UpdateParams {
            username: None,
            image: Some("a".repeat(257)), // Too long image should fail
            about_me: None,
            url: None,
            should_succeed: false,
            test_description: "Invalid update with long image".to_string(),
        },
        "long-about-me" => UpdateParams {
            username: None,
            image: None,
            about_me: Some(Some("a".repeat(129))), // Too long about_me should fail
            url: None,
            should_succeed: false,
            test_description: "Invalid update with long about me".to_string(),
        },
        "long-url" => UpdateParams {
            username: None,
            image: None,
            about_me: None,
            url: Some(Some("a".repeat(129))), // Too long URL should fail
            should_succeed: false,
            test_description: "Invalid update with long URL".to_string(),
        },
        "no-changes" => UpdateParams {
            username: None,
            image: None,
            about_me: None,
            url: None,
            should_succeed: true,
            test_description: "Update with no changes (should succeed)".to_string(),
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO PROFILE UPDATE TEST ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!("Burn amount: {} tokens", MIN_PROFILE_UPDATE_BURN_TOKENS);
    println!();

    // Constants
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string());
    let wallet_path = std::env::var("WALLET_PATH").unwrap_or_else(|_| {
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        format!("{}/.config/solana/id.json", home)
    });

    println!("RPC URL: {}", rpc_url);
    println!("Wallet: {}", wallet_path);

    // Create RPC client
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load wallet
    let wallet_path_expanded = shellexpand::tilde(&wallet_path).to_string();
    let payer = read_keypair_file(&wallet_path_expanded)
        .map_err(|e| format!("Failed to read keypair from {}: {}", wallet_path_expanded, e))?;

    println!("User: {}", payer.pubkey());

    // Program IDs
    let memo_profile_program_id = Pubkey::from_str("BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US")?;
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")?;
    let mint_pubkey = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")?;

    println!("Memo Profile Program: {}", memo_profile_program_id);
    println!("Memo Burn Program: {}", memo_burn_program_id);
    println!("Token Mint: {}", mint_pubkey);

    // Derive profile PDA
    let (profile_pda, profile_bump) = Pubkey::find_program_address(
        &[b"profile", payer.pubkey().as_ref()],
        &memo_profile_program_id,
    );

    println!("Profile PDA: {}", profile_pda);
    println!("Profile Bump: {}", profile_bump);

    // Get user's token account
    let user_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_pubkey,
        &token_2022_id(),
    );

    println!("User Token Account: {}", user_token_account);

    // Check if profile exists
    match client.get_account(&profile_pda) {
        Ok(account) => {
            println!("✅ Profile exists. Current data length: {} bytes", account.data.len());
        }
        Err(_) => {
            println!("❌ Profile does not exist for this user. Create a profile first using test-memo-profile-create.");
            return Ok(());
        }
    }

    // Check user's token balance
    match client.get_token_account_balance(&user_token_account) {
        Ok(balance) => {
            let balance_tokens = balance.ui_amount.unwrap_or(0.0);
            println!("Token Balance: {} MEMO", balance_tokens);
            
            if balance_tokens < MIN_PROFILE_UPDATE_BURN_TOKENS as f64 {
                println!("❌ Insufficient token balance. Need at least {} MEMO tokens.", MIN_PROFILE_UPDATE_BURN_TOKENS);
                return Ok(());
            }
        }
        Err(e) => {
            println!("❌ Failed to get token balance: {}", e);
            return Ok(());
        }
    }

    // Generate memo content
    let memo_content = generate_profile_update_memo(&payer.pubkey(), &test_params)?;
    println!("Generated memo ({} bytes)", memo_content.len());

    // Create the profile update instruction
    let memo_instruction = create_memo_instruction(&memo_content)?;
    let update_instruction = create_update_profile_instruction(
        &memo_profile_program_id,
        &memo_burn_program_id,
        &payer.pubkey(),
        &profile_pda,
        &mint_pubkey,
        &user_token_account,
        &test_params,
    )?;

    // Prepare instructions for simulation
    let sim_instructions = vec![memo_instruction.clone(), update_instruction.clone()];

    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;

    // Create simulation transaction with high CU limit
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
    let mut sim_transaction_instructions = vec![compute_budget_ix];
    sim_transaction_instructions.extend(sim_instructions.clone());

    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_transaction_instructions,
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!();
    println!("=== COMPUTE UNIT OPTIMIZATION ===");
    println!("Simulating transaction to calculate optimal compute units...");

    // Simulate to get optimal CU
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
                if !test_params.should_succeed {
                    println!("✅ EXPECTED FAILURE: Simulation failed as expected");
                    return Ok(());
                }
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

    println!();
    println!("=== TRANSACTION EXECUTION ===");
    print_update_summary(&test_params);
    println!("Compute Units: {}", optimal_cu);

    // Create final transaction with optimized CU
    let optimized_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let final_transaction = Transaction::new_signed_with_payer(
        &[
            optimized_compute_budget_ix,
            memo_instruction,
            update_instruction,
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send transaction
    println!();
    println!("Sending update transaction...");
    
    match client.send_and_confirm_transaction(&final_transaction) {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESS!");
            println!("Signature: {}", signature);

            // Verify the update
            println!();
            println!("=== VERIFICATION ===");
            match client.get_account(&profile_pda) {
                Ok(account) => {
                    println!("✅ Profile account updated successfully");
                    println!("Updated Profile Account:");
                    println!("  Address: {}", profile_pda);
                    println!("  Data Length: {} bytes", account.data.len());
                    println!("  Lamports: {}", account.lamports);
                    
                    // Try to decode and show updated profile data
                    if account.data.len() > 8 {
                        println!("  Profile data updated (checking with check-memo-profile for details)");
                    }
                },
                Err(e) => {
                    println!("⚠️  Could not fetch updated profile: {}", e);
                }
            }

            if !test_params.should_succeed {
                println!("❌ UNEXPECTED SUCCESS: Test should have failed");
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            if !test_params.should_succeed {
                println!("✅ EXPECTED FAILURE: Test failed as expected");
                analyze_expected_error(&err.to_string(), &test_params);
            } else {
                println!("❌ UNEXPECTED FAILURE: Test should have succeeded");
                analyze_unexpected_error(&err.to_string());
            }
        }
    }

    Ok(())
}

fn generate_profile_update_memo(user: &Pubkey, params: &UpdateParams) -> Result<String, Box<dyn std::error::Error>> {
    println!("=== MEMO GENERATION ===");
    
    // Create ProfileUpdateData structure
    let profile_data = ProfileUpdateData {
        version: PROFILE_UPDATE_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_UPDATE_OPERATION.to_string(),
        user_pubkey: user.to_string(),
        username: params.username.clone(),
        image: params.image.clone(),
        about_me: params.about_me.clone(),
        url: params.url.clone(),
    };
    
    // Validate the profile data
    profile_data.validate(*user)?;
    
    // Serialize ProfileUpdateData to bytes
    let profile_data_bytes = profile_data.try_to_vec()?;
    println!("ProfileUpdateData serialized: {} bytes", profile_data_bytes.len());
    
    // Create BurnMemo structure
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: MIN_PROFILE_UPDATE_BURN_AMOUNT,
        payload: profile_data_bytes,
    };
    
    // Serialize BurnMemo to bytes
    let burn_memo_bytes = burn_memo.try_to_vec()?;
    println!("BurnMemo serialized: {} bytes", burn_memo_bytes.len());
    
    // Encode to Base64
    let base64_memo = general_purpose::STANDARD.encode(&burn_memo_bytes);
    println!("Base64 encoded: {} bytes -> {} characters", burn_memo_bytes.len(), base64_memo.len());
    
    // Validate memo length
    if base64_memo.len() < 69 {
        return Err(format!("Memo too short: {} bytes (minimum: 69)", base64_memo.len()).into());
    }
    if base64_memo.len() > 800 {
        return Err(format!("Memo too long: {} bytes (maximum: 800)", base64_memo.len()).into());
    }
    
    println!("✅ Memo validation passed: {} characters (range: 69-800)", base64_memo.len());
    println!("Memo preview: {}...", &base64_memo[..base64_memo.len().min(50)]);
    
    Ok(base64_memo)
}

fn create_memo_instruction(memo_content: &str) -> Result<Instruction, Box<dyn std::error::Error>> {
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")?;
    
    Ok(Instruction::new_with_bytes(
        memo_program_id,
        memo_content.as_bytes(),
        vec![],
    ))
}

fn create_update_profile_instruction(
    program_id: &Pubkey,
    memo_burn_program_id: &Pubkey,
    user: &Pubkey,
    profile: &Pubkey,
    mint: &Pubkey,
    user_token_account: &Pubkey,
    params: &UpdateParams,
) -> Result<Instruction, Box<dyn std::error::Error>> {
    // Calculate Anchor instruction sighash for "update_profile"
    let mut hasher = Sha256::new();
    hasher.update(b"global:update_profile");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Serialize parameters in order: burn_amount, username, image, about_me, url
    let burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
    let username = &params.username;
    let image = &params.image;
    let about_me = &params.about_me;
    let url = &params.url;
    
    // Serialize each parameter using Borsh
    instruction_data.extend(burn_amount.try_to_vec()?);
    instruction_data.extend(username.try_to_vec()?);
    instruction_data.extend(image.try_to_vec()?);
    instruction_data.extend(about_me.try_to_vec()?);
    instruction_data.extend(url.try_to_vec()?);
    
    let accounts = vec![
        AccountMeta::new(*user, true),                      // user (signer)
        AccountMeta::new(*mint, false),                     // mint
        AccountMeta::new(*user_token_account, false),       // user_token_account
        AccountMeta::new(*profile, false),                  // profile (PDA)
        AccountMeta::new_readonly(token_2022_id(), false),  // token_program
        AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false), // instructions
        AccountMeta::new_readonly(*memo_burn_program_id, false), // memo_burn_program
    ];
    
    Ok(Instruction::new_with_bytes(*program_id, &instruction_data, accounts))
}

fn print_update_summary(params: &UpdateParams) {
    println!("Update Summary:");
    match &params.username {
        Some(username) => println!("  Username: -> '{}'", username),
        None => println!("  Username: (no change)"),
    }
    
    match &params.image {
        Some(image) => {
            let preview = if image.len() > 50 { format!("{}...", &image[..50]) } else { image.clone() };
            println!("  Image: -> '{}'", preview);
        },
        None => println!("  Image: (no change)"),
    }
    
    match &params.about_me {
        Some(Some(text)) => println!("  About Me: -> '{}'", text),
        Some(None) => println!("  About Me: -> (cleared)"),
        None => println!("  About Me: (no change)"),
    }
    
    match &params.url {
        Some(Some(url)) => println!("  URL: -> '{}'", url),
        Some(None) => println!("  URL: -> (cleared)"),
        None => println!("  URL: (no change)"),
    }
}

fn analyze_expected_error(error_msg: &str, params: &UpdateParams) {
    if error_msg.contains("EmptyUsername") && params.username.as_ref().map_or(false, |s| s.is_empty()) {
        println!("✅ Correct: Empty username detected");
    } else if error_msg.contains("UsernameTooLong") && params.username.as_ref().map_or(false, |s| s.len() > 32) {
        println!("✅ Correct: Username too long detected");
    } else if error_msg.contains("ProfileImageTooLong") && params.image.as_ref().map_or(false, |s| s.len() > 256) {
        println!("✅ Correct: Profile image too long detected");
    } else if error_msg.contains("AboutMeTooLong") && params.about_me.as_ref().and_then(|opt| opt.as_ref()).map_or(false, |s| s.len() > 128) {
        println!("✅ Correct: About me too long detected");
    } else if error_msg.contains("UrlTooLong") && params.url.as_ref().and_then(|opt| opt.as_ref()).map_or(false, |s| s.len() > 128) {
        println!("✅ Correct: URL too long detected");
    } else if error_msg.contains("BurnAmountTooSmall") {
        println!("✅ Correct: Insufficient burn amount detected");
    } else {
        println!("⚠️  Unexpected error type: {}", error_msg);
    }
}

fn analyze_unexpected_error(error_msg: &str) {
    println!("💡 Error analysis:");
    if error_msg.contains("UnauthorizedProfileAccess") {
        println!("   Profile access authorization failed");
    } else if error_msg.contains("Account does not exist") {
        println!("   Profile does not exist - create it first");
    } else if error_msg.contains("insufficient funds") {
        println!("   Insufficient token balance for burning");
    } else if error_msg.contains("MemoRequired") {
        println!("   Memo instruction missing or invalid");
    } else {
        println!("   Unexpected error type");
    }
}

fn print_usage() {
    println!("Usage: test-memo-profile-update <test_case>");
    println!();
    println!("Test Cases:");
    println!("  update-username     - Update only username");
    println!("  update-image        - Update only image");
    println!("  update-about-me     - Update only about me");
    println!("  clear-about-me      - Clear about me field");
    println!("  update-url          - Update only URL");
    println!("  clear-url           - Clear URL field");
    println!("  update-all          - Update all fields");
    println!("  no-changes          - Update with no changes");
    println!("  empty-username      - Invalid: Empty username");
    println!("  long-username       - Invalid: Username too long (>32 chars)");
    println!("  long-image          - Invalid: Image too long (>256 chars)");
    println!("  long-about-me       - Invalid: About me too long (>128 chars)");
    println!("  long-url            - Invalid: URL too long (>128 chars)");
    println!();
    println!("Environment Variables:");
    println!("  RPC_URL      - Solana RPC endpoint (default: testnet)");
    println!("  WALLET_PATH  - Path to wallet keypair file");
    println!();
    println!("Prerequisites:");
    println!("  A profile must already exist for the user");
    println!("  User must have at least {} MEMO tokens", MIN_PROFILE_UPDATE_BURN_TOKENS);
    println!("  Use test-memo-profile-create to create a profile first");
}
