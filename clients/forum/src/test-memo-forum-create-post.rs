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
use sha2::{Sha256, Digest};
use borsh::{BorshSerialize, BorshDeserialize};
use base64::{Engine as _, engine::general_purpose};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Define structures matching the contract
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct BurnMemo {
    pub version: u8,
    pub burn_amount: u64,
    pub payload: Vec<u8>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct PostCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub creator: String,
    pub post_id: u64,
    pub title: String,
    pub content: String,
    pub image: String,
}

// Constants matching the contract
const POST_CREATION_DATA_VERSION: u8 = 1;
const BURN_MEMO_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "forum";
const EXPECTED_OPERATION: &str = "create_post";
const DECIMAL_FACTOR: u64 = 1_000_000;
const MIN_POST_BURN_TOKENS: u64 = 1;
const MIN_POST_BURN_AMOUNT: u64 = MIN_POST_BURN_TOKENS * DECIMAL_FACTOR;

impl PostCreationData {
    pub fn validate(&self, expected_creator: &Pubkey, expected_post_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        if self.version != POST_CREATION_DATA_VERSION {
            return Err(format!("Unsupported version: {} (expected: {})", self.version, POST_CREATION_DATA_VERSION).into());
        }
        
        if self.category != EXPECTED_CATEGORY {
            return Err(format!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY).into());
        }
        
        if self.operation != EXPECTED_OPERATION {
            return Err(format!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_OPERATION).into());
        }
        
        let creator_pubkey = self.creator.parse::<Pubkey>()
            .map_err(|_| "Invalid creator pubkey format")?;
        if creator_pubkey != *expected_creator {
            return Err(format!("Creator mismatch: {} vs {}", self.creator, expected_creator).into());
        }
        
        if self.post_id != expected_post_id {
            return Err(format!("Post ID mismatch: {} vs {}", self.post_id, expected_post_id).into());
        }
        
        if self.title.is_empty() || self.title.len() > 128 {
            return Err(format!("Invalid title: {} chars (must be 1-128)", self.title.len()).into());
        }
        
        if self.content.is_empty() || self.content.len() > 512 {
            return Err(format!("Invalid content: {} chars (must be 1-512)", self.content.len()).into());
        }
        
        if self.image.len() > 256 {
            return Err(format!("Invalid image: {} chars (max: 256)", self.image.len()).into());
        }
        
        println!("Post creation data validation passed");
        Ok(())
    }
}

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-FORUM CREATE POST TEST ===");
    println!("This program creates a new forum post by burning tokens.");
    println!("Post ID is automatically assigned from the global counter.");
    println!();

    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    let title = if args.len() > 1 {
        args[1].clone()
    } else {
        "Test Forum Post".to_string()
    };
    
    let content = if args.len() > 2 {
        args[2].clone()
    } else {
        "This is a test forum post created via memo-forum contract.".to_string()
    };
    
    let image = if args.len() > 3 {
        args[3].clone()
    } else {
        String::new()
    };

    // Connect to network
    let rpc_url = get_rpc_url();
    let client = RpcClient::new(rpc_url);

    // Load user wallet
    let user = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program addresses
    let memo_forum_program_id = get_program_id("memo_forum").expect("Failed to get memo_forum program ID");
    let memo_burn_program_id = get_program_id("memo_burn").expect("Failed to get memo_burn program ID");
    let mint_address = get_token_mint("memo_token").expect("Failed to get memo_token mint address");

    println!("Network: {}", get_rpc_url());
    println!("User: {}", user.pubkey());
    println!("Memo-forum program: {}", memo_forum_program_id);
    println!("Memo-burn program: {}", memo_burn_program_id);
    println!("Token mint: {}", mint_address);
    println!();

    // Calculate global counter PDA
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_forum_program_id,
    );

    // Get next available post_id from global counter
    let post_id = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            if account.data.len() >= 16 { // 8 bytes discriminator + 8 bytes u64
                let total_posts_bytes = &account.data[8..16];
                let total_posts = u64::from_le_bytes(total_posts_bytes.try_into().unwrap());
                println!("✅ Global counter found: total_posts = {}", total_posts);
                println!("   Next post will have ID: {}", total_posts);
                total_posts // Next post ID is current total_posts (0-indexed)
            } else {
                println!("❌ Invalid global counter data (too short: {} bytes)", account.data.len());
                return Err("Invalid global counter account data".into());
            }
        },
        Err(e) => {
            println!("❌ Global counter not found: {}", e);
            println!("💡 The admin needs to initialize the global counter first.");
            println!("   Run: cargo run --bin admin-init-global-post-counter");
            return Err("Global counter not initialized".into());
        }
    };

    let burn_amount_tokens = MIN_POST_BURN_TOKENS;
    let burn_amount = burn_amount_tokens * DECIMAL_FACTOR;

    println!();
    println!("Creating post with {} token burn (minimum required)", burn_amount_tokens);
    println!("Post ID (auto-assigned): {}", post_id);
    println!("Title: {}", title);
    println!("Content: {}", content);
    println!("Image: {}", if image.is_empty() { "(none)" } else { &image });

    // Create post data
    let post_data = PostCreationData {
        version: POST_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        creator: user.pubkey().to_string(),
        post_id,
        title: title.clone(),
        content: content.clone(),
        image: image.clone(),
    };

    // Validate post data
    post_data.validate(&user.pubkey(), post_id)?;

    // Serialize post data
    let post_payload = post_data.try_to_vec()?;
    println!("Post payload size: {} bytes", post_payload.len());

    // Create BurnMemo structure
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload: post_payload,
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

    // Calculate post PDA (using post_id as seed)
    let (post_pda, _) = Pubkey::find_program_address(
        &[b"post", post_id.to_le_bytes().as_ref()],
        &memo_forum_program_id,
    );

    println!("PDAs:");
    println!("  Global counter: {}", global_counter_pda);
    println!("  Post: {}", post_pda);
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

    // Create create_post instruction
    let create_post_ix = create_create_post_instruction(
        &memo_forum_program_id,
        &memo_burn_program_id,
        &user.pubkey(),
        &global_counter_pda,
        &post_pda,
        &mint_address,
        &user_token_account,
        &user_global_burn_stats_pda,
        post_id, // expected_post_id
        burn_amount,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[memo_ix.clone(), create_post_ix.clone(), dummy_compute_budget_ix],
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
                1_400_000u32
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
        &[memo_ix, create_post_ix, compute_budget_ix],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    println!("Sending create post transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 POST CREATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            
            match client.get_account(&post_pda) {
                Ok(account) => {
                    println!("✅ Post account created:");
                    println!("   PDA: {}", post_pda);
                    println!("   Owner: {}", account.owner);
                    println!("   Data length: {} bytes", account.data.len());
                    println!();
                    println!("✅ Post created successfully!");
                    println!("   Creator: {}", user.pubkey());
                    println!("   Post ID: {}", post_id);
                    println!("   Title: {}", title);
                    println!("   Tokens burned: {} token(s)", burn_amount_tokens);
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created post account: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ POST CREATION FAILED!");
            println!("Error: {}", err);
            
            let error_msg = err.to_string();
            if error_msg.contains("BurnAmountTooSmall") {
                println!("💡 The burn amount is too small. Minimum required: {} token(s)", MIN_POST_BURN_TOKENS);
            } else if error_msg.contains("insufficient funds") || error_msg.contains("0x1") {
                println!("💡 Insufficient token balance or SOL balance for transaction fees.");
            } else if error_msg.contains("already in use") {
                println!("💡 Post with this ID already exists. Use a different post_id.");
            } else {
                println!("💡 Unexpected error. Please check the program deployment and network connection.");
            }
        }
    }

    Ok(())
}

fn create_create_post_instruction(
    program_id: &Pubkey,
    memo_burn_program_id: &Pubkey,
    creator: &Pubkey,
    global_counter: &Pubkey,
    post: &Pubkey,
    mint: &Pubkey,
    creator_token_account: &Pubkey,
    user_global_burn_stats: &Pubkey,
    expected_post_id: u64,
    burn_amount: u64,
) -> Instruction {
    // Calculate Anchor instruction sighash for "create_post"
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_post");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add parameters: expected_post_id (u64), burn_amount (u64)
    instruction_data.extend_from_slice(&expected_post_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*creator, true),
        AccountMeta::new(*global_counter, false),       // global_counter PDA (mutable)
        AccountMeta::new(*post, false),
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
