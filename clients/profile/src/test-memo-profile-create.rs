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
pub struct ProfileCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "profile" for memo-profile contract)
    pub category: String,
    
    /// Operation type (must be "create_profile" for profile creation)
    pub operation: String,
    
    /// User pubkey as string (must match the transaction signer)
    pub user_pubkey: String,
    
    /// Username (required, 1-32 characters)
    pub username: String,
    
    /// Profile image info (optional, max 256 characters)
    pub image: String,
    
    /// About me description (optional, max 128 characters)
    pub about_me: Option<String>,
    
}

impl ProfileCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_user: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
        // Validate version
        if self.version != PROFILE_CREATION_DATA_VERSION {
            println!("Unsupported profile creation data version: {} (expected: {})", 
                 self.version, PROFILE_CREATION_DATA_VERSION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported profile creation data version")));
        }
        
        // Validate category (must be exactly "profile")
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
        
        // Validate operation (must be exactly "create_profile")
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
        
        // Validate username (required, 1-32 characters)
        if self.username.is_empty() || self.username.len() > 32 {
            println!("Invalid username: '{}' (must be 1-32 characters)", self.username);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid username")));
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > 256 {
            println!("Invalid profile image: {} characters (max: 256)", self.image.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid profile image")));
        }
        
        // Validate about_me (optional, max 128 characters)
        if let Some(ref about_me) = self.about_me {
            if about_me.len() > 128 {
                println!("Invalid about_me: {} characters (max: 128)", about_me.len());
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid about_me")));
            }
        }
        
        println!("Profile creation data validation passed: category={}, operation={}, user={}, username={}", 
             self.category, self.operation, self.user_pubkey, self.username);
        
        Ok(())
    }
}

// Constants matching the contract
const BURN_MEMO_VERSION: u8 = 1;
const PROFILE_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "profile";
const EXPECTED_OPERATION: &str = "create_profile";

#[derive(Debug, Clone)]
struct TestParams {
    pub burn_amount: u64,           // Burn amount in tokens (not units)
    pub username: String,           // Username
    pub image: String,              // Profile image
    pub about_me: Option<String>,   // About me text
    pub should_succeed: bool,       // Whether the test should succeed
    pub test_description: String,   // Description of what this test validates
}

// Get RPC URL from environment or use default testnet
fn get_rpc_url() -> String {
    std::env::var("X1_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string())
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
            burn_amount: 420, // Minimum required amount
            username: "alice".to_string(),
            image: "c:32x32:JY3LCsIwEEU/5rRTTYRgCq4Eq2bnskEECTRk/v8bnNi7OIuZ+8BPc2Iiw+g+A7zWdebCs9nxxhV0U1KLWvJStnc4P6gHcTkODEVykhDuyezWQvOAkwrCiS7PrtKhlYWoqjhDwlD4qjaOaBMrGNGetbERsXfunvbPZH4=".to_string(),
            about_me: Some("Hello, I'm Alice!".to_string()),
            should_succeed: true,
            test_description: "Valid profile creation with all fields".to_string(),
        },
        "valid-minimal" => TestParams {
            burn_amount: 420,
            username: "bob".to_string(),
            image: "".to_string(),
            about_me: None,
            should_succeed: true,
            test_description: "Valid profile creation with minimal fields".to_string(),
        },
        "empty-username" => TestParams {
            burn_amount: 420,
            username: "".to_string(),  // Empty username
            image: "c:32x32:JY3LCsIwEEU/5rRTTYRgCq4Eq2bnskEECTRk/v8bnNi7OIuZ+8BPc2Iiw+g+A7zWdebCs9nxxhV0U1KLWvJStnc4P6gHcTkODEVykhDuyezWQvOAkwrCiS7PrtKhlYWoqjhDwlD4qjaOaBMrGNGetbERsXfunvbPZH4=".to_string(),
            about_me: Some("Test".to_string()),
            should_succeed: false,
            test_description: "Invalid profile creation with empty username".to_string(),
        },
        "long-username" => TestParams {
            burn_amount: 420,
            username: "a".repeat(33),  // Too long username
            image: "c:32x32:JY3LCsIwEEU/5rRTTYRgCq4Eq2bnskEECTRk/v8bnNi7OIuZ+8BPc2Iiw+g+A7zWdebCs9nxxhV0U1KLWvJStnc4P6gHcTkODEVykhDuyezWQvOAkwrCiS7PrtKhlYWoqjhDwlD4qjaOaBMrGNGetbERsXfunvbPZH4=".to_string(),
            about_me: Some("Test".to_string()),
            should_succeed: false,
            test_description: "Invalid profile creation with long username".to_string(),
        },
        "long-image" => TestParams {
            burn_amount: 420,
            username: "alice".to_string(),
            image: "a".repeat(257),  // Too long image
            about_me: Some("Test".to_string()),
            should_succeed: false,
            test_description: "Invalid profile creation with long image".to_string(),
        },
        "long-about-me" => TestParams {
            burn_amount: 420,
            username: "alice".to_string(),
            image: "c:32x32:JY3LCsIwEEU/5rRTTYRgCq4Eq2bnskEECTRk/v8bnNi7OIuZ+8BPc2Iiw+g+A7zWdebCs9nxxhV0U1KLWvJStnc4P6gHcTkODEVykhDuyezWQvOAkwrCiS7PrtKhlYWoqjhDwlD4qjaOaBMrGNGetbERsXfunvbPZH4=".to_string(),
            about_me: Some("a".repeat(129)),  // Too long about_me
            should_succeed: false,
            test_description: "Invalid profile creation with long about_me".to_string(),
        },
        "low-burn" => TestParams {
            burn_amount: 419,  // Below minimum
            username: "alice".to_string(),
            image: "c:32x32:JY3LCsIwEEU/5rRTTYRgCq4Eq2bnskEECTRk/v8bnNi7OIuZ+8BPc2Iiw+g+A7zWdebCs9nxxhV0U1KLWvJStnc4P6gHcTkODEVykhDuyezWQvOAkwrCiS7PrtKhlYWoqjhDwlD4qjaOaBMrGNGetbERsXfunvbPZH4=".to_string(),
            about_me: Some("Test".to_string()),
            should_succeed: false,
            test_description: "Invalid profile creation with insufficient burn amount".to_string(),
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO PROFILE CREATE TEST ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!();

    // Constants
    let rpc_url = get_rpc_url();
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

    println!("Payer: {}", payer.pubkey());

    // Program IDs
    let memo_profile_program_id = Pubkey::from_str("BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US")?;
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")?;
    let mint_pubkey = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")?;

    println!("Memo Profile Program: {}", memo_profile_program_id);
    println!("Memo Burn Program: {}", memo_burn_program_id);
    println!("Mint: {}", mint_pubkey);

    // Derive profile PDA
    let (profile_pda, profile_bump) = Pubkey::find_program_address(
        &[b"profile", payer.pubkey().as_ref()],
        &memo_profile_program_id,
    );

    println!("Profile PDA: {}", profile_pda);
    println!("Profile Bump: {}", profile_bump);

    // Check if profile already exists
    match client.get_account(&profile_pda) {
        Ok(_) => {
            println!("❌ Profile already exists for this user. Cannot create again.");
            return Ok(());
        }
        Err(_) => {
            println!("✅ No existing profile found. Proceeding with creation.");
        }
    }

    // Get user's token account
    let user_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_pubkey,
        &token_2022_id(),
    );

    println!("User Token Account: {}", user_token_account);

    // Check token balance
    match client.get_token_account_balance(&user_token_account) {
        Ok(balance) => {
            let balance_tokens = balance.ui_amount.unwrap_or(0.0);
            println!("Token Balance: {} tokens", balance_tokens);
            
            if balance_tokens < test_params.burn_amount as f64 {
                println!("❌ Insufficient token balance. Need {} tokens, have {}", 
                         test_params.burn_amount, balance_tokens);
                return Ok(());
            }
        }
        Err(e) => {
            println!("❌ Failed to get token balance: {}", e);
            return Ok(());
        }
    }

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

    // Generate Borsh memo
    let memo_data = generate_borsh_memo_from_params(&test_params, payer.pubkey())?;

    // Create memo instruction
    let memo_instruction = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: memo_data,
    };

    // Create the profile creation instruction using proper Anchor discriminator
    let burn_amount_units = test_params.burn_amount * 1_000_000; // Convert to units

    let profile_instruction = create_profile_instruction(
        &memo_profile_program_id,
        &payer.pubkey(),
        &profile_pda,
        &mint_pubkey,
        &user_token_account,
        &memo_burn_program_id,
        &user_global_burn_stats_pda,
        burn_amount_units,
    );

    // Prepare instructions for simulation (without compute budget)
    let sim_instructions = vec![
        memo_instruction.clone(),
        profile_instruction.clone(),
    ];

    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;

    // Create simulation transaction with high CU limit
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
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
                let default_cu = 400_000u32;
                println!("Using default compute units: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% safety margin to actual consumption
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

    println!();
    println!("=== TRANSACTION EXECUTION ===");
    println!("Burn Amount: {} tokens ({} units)", test_params.burn_amount, burn_amount_units);
    println!("Username: {}", test_params.username);
    println!("Image: {}", if test_params.image.is_empty() { "(empty)" } else { &test_params.image });
    println!("About Me: {:?}", test_params.about_me);
    println!("Compute Units: {}", optimal_cu);

    // Create final transaction with optimized CU
    let optimized_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let final_transaction = Transaction::new_signed_with_payer(
        &[
            // Index 0: Compute budget instruction (required for CU optimization)
            optimized_compute_budget_ix,
            // Index 1: SPL Memo instruction (REQUIRED at this position)
            memo_instruction,
            // Index 2: Profile creation instruction
            profile_instruction,
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send transaction
    println!();
    println!("Sending optimized transaction...");
    
    match client.send_and_confirm_transaction(&final_transaction) {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESS!");
            println!("Signature: {}", signature);

            // Fetch the created profile
            println!();
            println!("=== VERIFICATION ===");
            match client.get_account(&profile_pda) {
                Ok(account) => {
                    println!("✅ Profile account created successfully");
                    println!("Profile Account:");
                    println!("  Address: {}", profile_pda);
                    println!("  Owner: {}", account.owner);
                    println!("  Data Length: {} bytes", account.data.len());
                    println!("  Lamports: {}", account.lamports);
                    
                    // Try to decode profile data
                    if account.data.len() > 8 {
                        println!("  Profile data created (account discriminator present)");
                    }
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created profile: {}", e);
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

fn generate_borsh_memo_from_params(params: &TestParams, user_pubkey: Pubkey) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Create ProfileCreationData
    let profile_creation_data = ProfileCreationData {
        version: PROFILE_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        user_pubkey: user_pubkey.to_string(),
        username: params.username.clone(),
        image: params.image.clone(),
        about_me: params.about_me.clone(),
    };
    
    // Validate profile creation data
    profile_creation_data.validate(user_pubkey)?;
    
    // Serialize ProfileCreationData to bytes (this becomes the payload)
    let payload = profile_creation_data.try_to_vec()?;
    
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
    println!("  ProfileCreationData payload: {} bytes", burn_memo.payload.len());
    println!("  Complete BurnMemo (Borsh): {} bytes", borsh_data.len());
    println!("  Base64 encoded memo: {} bytes", memo_bytes.len());
    
    Ok(memo_bytes)
}

fn create_profile_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    profile: &Pubkey,
    mint: &Pubkey,
    user_token_account: &Pubkey,
    memo_burn_program: &Pubkey,
    user_global_burn_stats: &Pubkey,
    burn_amount: u64,
) -> Instruction {
    // Calculate Anchor instruction sighash for "create_profile"
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_profile");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add burn_amount parameter (8 bytes for u64)
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let accounts = vec![
        AccountMeta::new(*user, true),                                           // user
        AccountMeta::new(*profile, false),                                       // profile
        AccountMeta::new(*mint, false),                                          // mint
        AccountMeta::new(*user_token_account, false),                            // user_token_account
        AccountMeta::new(*user_global_burn_stats, false),                        // user_global_burn_stats
        AccountMeta::new_readonly(token_2022_id(), false),                       // token_program
        AccountMeta::new_readonly(*memo_burn_program, false),                    // memo_burn_program
        AccountMeta::new_readonly(system_program::id(), false),                  // system_program
        AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false), // instructions
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn analyze_expected_error(error_msg: &str, params: &TestParams) {
    if error_msg.contains("EmptyUsername") && params.username.is_empty() {
        println!("✅ Correct: Empty username detected");
    } else if error_msg.contains("UsernameTooLong") && params.username.len() > 32 {
        println!("✅ Correct: Username too long detected");
    } else if error_msg.contains("ProfileImageTooLong") && params.image.len() > 256 {
        println!("✅ Correct: Profile image too long detected");
    } else if error_msg.contains("AboutMeTooLong") && params.about_me.as_ref().map_or(false, |s| s.len() > 128) {
        println!("✅ Correct: About me too long detected");
    } else if error_msg.contains("BurnAmountTooSmall") && params.burn_amount < 420 {
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
        println!("   Invalid memo format or encoding");
    } else if error_msg.contains("InvalidTokenAccount") {
        println!("   Token account validation failed");
    } else if error_msg.contains("UnauthorizedMint") {
        println!("   Wrong mint address");
    } else if error_msg.contains("UnauthorizedTokenAccount") {
        println!("   Token account ownership issue");
    } else if error_msg.contains("InvalidBurnAmount") {
        println!("   Burn amount validation failed");
    } else if error_msg.contains("already in use") {
        println!("   Profile already exists for this user");
    } else {
        println!("   Unexpected error type");
    }
}

fn print_usage() {
    println!("Usage: test-memo-profile-create <test_case>");
    println!();
    println!("Test Cases:");
    println!("  valid-basic     - Valid profile creation with all fields");
    println!("  valid-minimal   - Valid profile creation with minimal fields");
    println!("  empty-username  - Invalid: Empty username");
    println!("  long-username   - Invalid: Username too long (>32 chars)");
    println!("  long-image      - Invalid: Image too long (>256 chars)");
    println!("  long-about-me   - Invalid: About me too long (>128 chars)");
    println!("  low-burn        - Invalid: Burn amount too low (<420 tokens)");
    println!();
    println!("Environment Variables:");
    println!("  RPC_URL      - Solana RPC endpoint (default: testnet)");
    println!("  WALLET_PATH  - Path to wallet keypair file");
}
