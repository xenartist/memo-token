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
pub struct ProjectCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "project" for memo-project contract)
    pub category: String,
    
    /// Operation type (must be "create_project" for project creation)
    pub operation: String,
    
    /// Project ID (must match expected_project_id)
    pub project_id: u64,
    
    /// Project name (required, 1-64 characters)
    pub name: String,
    
    /// Project description (optional, max 256 characters)
    pub description: String,
    
    /// Project image info (optional, max 256 characters)
    pub image: String,
    
    /// Project website URL (optional, max 128 characters)
    pub website: String,
    
    /// Tags (optional, max 4 tags, each max 32 characters)
    pub tags: Vec<String>,
}

// Constants matching the contract
const PROJECT_CREATION_DATA_VERSION: u8 = 1;
const BURN_MEMO_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "project";
const EXPECTED_OPERATION: &str = "create_project";
const DECIMAL_FACTOR: u64 = 1_000_000;
const MIN_PROJECT_CREATION_BURN_TOKENS: u64 = 42_069;
const MIN_PROJECT_CREATION_BURN_AMOUNT: u64 = MIN_PROJECT_CREATION_BURN_TOKENS * DECIMAL_FACTOR;

impl ProjectCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_project_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Validate version
        if self.version != PROJECT_CREATION_DATA_VERSION {
            println!("Unsupported project creation data version: {} (expected: {})", 
                 self.version, PROJECT_CREATION_DATA_VERSION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported project creation data version")));
        }
        
        // Validate category (must be exactly "project")
        if self.category != EXPECTED_CATEGORY {
            println!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid category")));
        }
        
        // Validate operation (must be exactly "create_project")
        if self.operation != EXPECTED_OPERATION {
            println!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_OPERATION);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid operation")));
        }
        
        // Validate project_id
        if self.project_id != expected_project_id {
            println!("Project ID mismatch: data contains {}, expected {}", 
                 self.project_id, expected_project_id);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Project ID mismatch")));
        }
        
        // Validate name (required, 1-64 characters)
        if self.name.is_empty() || self.name.len() > 64 {
            println!("Invalid project name: '{}' (must be 1-64 characters)", self.name);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid project name")));
        }
        
        // Validate description (optional, max 256 characters)
        if self.description.len() > 256 {
            println!("Invalid project description: {} characters (max: 256)", self.description.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid project description")));
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > 256 {
            println!("Invalid project image: {} characters (max: 256)", self.image.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid project image")));
        }
        
        // Validate website (optional, max 128 characters)
        if self.website.len() > 128 {
            println!("Invalid project website: {} characters (max: 128)", self.website.len());
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid project website")));
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
        
        println!("Project creation data validation passed: category={}, operation={}, project_id={}, name={}, tags_count={}", 
             self.category, self.operation, self.project_id, self.name, self.tags.len());
        
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-PROJECT CREATE PROJECT TEST ===");
    println!("This program creates a new project by burning tokens.");
    println!();

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load user wallet
    let user = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program addresses
    let memo_project_program_id = Pubkey::from_str("ENVapgjzzMjbRhLJ279yNsSgaQtDYYVgWq98j54yYnyx")
        .expect("Invalid memo-project program ID");
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid memo-burn program ID");
    let mint_address = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");

    println!("Network: {}", rpc_url);
    println!("User: {}", user.pubkey());
    println!("Memo-project program: {}", memo_project_program_id);
    println!("Memo-burn program: {}", memo_burn_program_id);
    println!("Token mint: {}", mint_address);
    println!();

    // Get the next project ID from global counter
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_project_program_id,
    );

    let next_project_id = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            if account.data.len() >= 16 {
                let total_projects_bytes = &account.data[8..16];
                u64::from_le_bytes(total_projects_bytes.try_into().unwrap())
            } else {
                return Err("Global counter account data too short".into());
            }
        },
        Err(_) => {
            return Err("Global counter not found. Please run admin-memo-project-init-global-project-counter first.".into());
        }
    };

    println!("Next project ID will be: {}", next_project_id);

    // Project details
    let burn_amount_tokens = MIN_PROJECT_CREATION_BURN_TOKENS; // 42,069 tokens
    let burn_amount = burn_amount_tokens * DECIMAL_FACTOR;

    println!("Creating project with {} tokens burn (minimum required)", burn_amount_tokens);

    // Create project data
    let project_data = ProjectCreationData {
        version: PROJECT_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        project_id: next_project_id,
        name: "My Test Project".to_string(),
        description: "This is a test project created via memo-project contract".to_string(),
        image: "https://example.com/project-image.png".to_string(),
        website: "https://example.com".to_string(),
        tags: vec!["DeFi".to_string(), "Test".to_string()],
    };

    // Validate project data
    project_data.validate(next_project_id)?;

    // Serialize project data
    let project_payload = project_data.try_to_vec()?;
    println!("Project payload size: {} bytes", project_payload.len());

    // Create BurnMemo structure
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload: project_payload,
    };

    // Serialize and encode BurnMemo
    let burn_memo_data = burn_memo.try_to_vec()?;
    let base64_memo = general_purpose::STANDARD.encode(&burn_memo_data);
    
    println!("Burn memo data size: {} bytes", burn_memo_data.len());
    println!("Base64 memo size: {} bytes", base64_memo.len());

    if base64_memo.len() > 800 {
        return Err(format!("Memo too long: {} bytes (max: 800)", base64_memo.len()).into());
    }

    // Get user's token account
    let user_token_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &mint_address,
        &token_2022_id(),
    );

    // Check token balance
    match client.get_token_account(&user_token_account) {
        Ok(Some(token_account)) => {
            let balance = token_account.token_amount.ui_amount.unwrap_or(0.0);
            println!("User token balance: {} tokens", balance);
            
            if balance < burn_amount_tokens as f64 {
                return Err(format!("Insufficient token balance. Need {} tokens, have {}", 
                                 burn_amount_tokens, balance).into());
            }
        },
        Ok(None) => {
            return Err("Token account not found or has no balance data".into());
        },
        Err(e) => {
            return Err(format!("Failed to get token account: {}", e).into());
        }
    }

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", user.pubkey().as_ref()],
        &memo_burn_program_id,
    );

    // Calculate PDAs
    let (project_pda, _) = Pubkey::find_program_address(
        &[b"project", next_project_id.to_le_bytes().as_ref()],
        &memo_project_program_id,
    );

    let (burn_leaderboard_pda, _) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &memo_project_program_id,
    );

    println!("PDAs:");
    println!("  Global counter: {}", global_counter_pda);
    println!("  Project: {}", project_pda);
    println!("  Burn leaderboard: {}", burn_leaderboard_pda);
    println!("  User global burn stats: {}", user_global_burn_stats_pda);
    println!();

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create memo instruction
    let memo_ix = Instruction::new_with_bytes(
        spl_memo::id(),
        base64_memo.as_bytes(),
        vec![],
    );

    // Create create_project instruction
    let create_project_ix = create_create_project_instruction(
        &memo_project_program_id,
        &memo_burn_program_id,
        &user.pubkey(),
        &global_counter_pda,
        &project_pda,
        &burn_leaderboard_pda,
        &mint_address,
        &user_token_account,
        &user_global_burn_stats_pda,
        next_project_id,
        burn_amount,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, memo_ix.clone(), create_project_ix.clone()],
        Some(&user.pubkey()),
        &[&user],
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
                println!("Simulation error: {:?}", err);
                1_400_000u32
            } else if let Some(units_consumed) = result.value.units_consumed {
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 1_400_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            1_400_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, create_project_ix],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    println!("Sending create project transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 PROJECT CREATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            
            // Verify the project was created correctly
            match client.get_account(&project_pda) {
                Ok(account) => {
                    println!("✅ Project account created:");
                    println!("   PDA: {}", project_pda);
                    println!("   Owner: {}", account.owner);
                    println!("   Data length: {} bytes", account.data.len());
                    
                    println!();
                    println!("✅ Project {} created successfully!", next_project_id);
                    println!("   Creator: {}", user.pubkey());
                    println!("   Tokens burned: {} tokens", burn_amount_tokens);
                    println!("   Project name: {}", project_data.name);
                    println!("   Tags: {:?}", project_data.tags);
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created project account: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ PROJECT CREATION FAILED!");
            println!("Error: {}", err);
            
            // Provide helpful error analysis
            let error_msg = err.to_string();
            if error_msg.contains("BurnAmountTooSmall") {
                println!("💡 The burn amount is too small. Minimum required: {} tokens", MIN_PROJECT_CREATION_BURN_TOKENS);
            } else if error_msg.contains("insufficient funds") || error_msg.contains("0x1") {
                println!("💡 Insufficient token balance or SOL balance for transaction fees.");
            } else if error_msg.contains("InvalidProjectDataFormat") {
                println!("💡 Invalid project data format in memo. Check the project data structure.");
            } else if error_msg.contains("ProjectIdMismatch") {
                println!("💡 Project ID mismatch. Expected: {}", next_project_id);
            } else if error_msg.contains("MemoRequired") {
                println!("💡 SPL Memo instruction is required but not found.");
            } else {
                println!("💡 Unexpected error. Please check the program deployment and network connection.");
            }
        }
    }

    Ok(())
}

fn create_create_project_instruction(
    program_id: &Pubkey,
    memo_burn_program_id: &Pubkey,
    creator: &Pubkey,
    global_counter: &Pubkey,
    project: &Pubkey,
    burn_leaderboard: &Pubkey,
    mint: &Pubkey,
    creator_token_account: &Pubkey,
    user_global_burn_stats: &Pubkey,
    expected_project_id: u64,
    burn_amount: u64,
) -> Instruction {
    // Calculate Anchor instruction sighash for "create_project"
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_project");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add parameters: expected_project_id (u64) + burn_amount (u64)
    instruction_data.extend_from_slice(&expected_project_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*creator, true),
        AccountMeta::new(*global_counter, false),
        AccountMeta::new(*project, false),
        AccountMeta::new(*burn_leaderboard, false),
        AccountMeta::new(*mint, false),
        AccountMeta::new(*creator_token_account, false),
        AccountMeta::new(*user_global_burn_stats, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(*memo_burn_program_id, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(
            solana_sdk::sysvar::instructions::id(),
            false
        ),
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}
