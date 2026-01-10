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
pub struct PostBurnData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub user: String,
    pub post_id: u64,
    pub message: String,
}

// Constants
const POST_BURN_DATA_VERSION: u8 = 1;
const BURN_MEMO_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "forum";
const EXPECTED_OPERATION: &str = "burn_for_post";
const DECIMAL_FACTOR: u64 = 1_000_000;
const MIN_POST_BURN_TOKENS: u64 = 1;
const MIN_POST_BURN_AMOUNT: u64 = MIN_POST_BURN_TOKENS * DECIMAL_FACTOR;

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-FORUM BURN FOR POST TEST ===");
    println!("This program burns tokens to reply to a forum post.");
    println!("Note: ANY user can burn for any post (not just the creator).");
    println!();

    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 {
        println!("Usage: {} <post_id> <burn_amount_tokens> [message]", args[0]);
        println!();
        println!("Examples:");
        println!("  {} 12345 1", args[0]);
        println!("  {} 12345 100 \"Great post! I agree!\"", args[0]);
        return Ok(());
    }

    let post_id = args[1].parse::<u64>()
        .map_err(|_| format!("Invalid post_id: {}", args[1]))?;
    
    let burn_amount_tokens = args[2].parse::<u64>()
        .map_err(|_| format!("Invalid burn_amount: {}", args[2]))?;
    
    let message = if args.len() > 3 {
        args[3].clone()
    } else {
        String::new()
    };

    if burn_amount_tokens < MIN_POST_BURN_TOKENS {
        return Err(format!("Burn amount too small. Minimum: {} token(s)", MIN_POST_BURN_TOKENS).into());
    }

    let burn_amount = burn_amount_tokens * DECIMAL_FACTOR;

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
    println!("Post ID: {}", post_id);
    println!("Burn amount: {} token(s)", burn_amount_tokens);
    println!("Message: {}", if message.is_empty() { "(none)" } else { &message });
    println!();

    // Calculate post PDA
    let (post_pda, _) = Pubkey::find_program_address(
        &[b"post", post_id.to_le_bytes().as_ref()],
        &memo_forum_program_id,
    );

    // Verify post exists
    match client.get_account(&post_pda) {
        Ok(account) => {
            if account.data.len() < 48 {
                return Err("Post account data too short".into());
            }
            // Extract creator pubkey (offset: 8 discriminator + 8 post_id, then 32 bytes for Pubkey)
            let creator_bytes = &account.data[16..48];
            let mut creator_array = [0u8; 32];
            creator_array.copy_from_slice(creator_bytes);
            let creator = Pubkey::new_from_array(creator_array);
            
            println!("✅ Post exists");
            println!("   Post creator: {}", creator);
            println!("   Note: Anyone can burn for this post");
        },
        Err(_) => {
            return Err(format!("Post with ID {} not found. Please create the post first.", post_id).into());
        }
    }

    // Create burn data
    let burn_data = PostBurnData {
        version: POST_BURN_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        user: user.pubkey().to_string(),
        post_id,
        message,
    };

    // Serialize burn data
    let burn_payload = burn_data.try_to_vec()?;
    println!("Burn payload size: {} bytes", burn_payload.len());

    // Create BurnMemo structure
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload: burn_payload,
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

    println!("PDAs:");
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

    // Create burn_for_post instruction
    let burn_for_post_ix = create_burn_for_post_instruction(
        &memo_forum_program_id,
        &memo_burn_program_id,
        &user.pubkey(),
        &post_pda,
        &mint_address,
        &user_token_account,
        &user_global_burn_stats_pda,
        post_id,
        burn_amount,
    );

    // Simulate transaction
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[memo_ix.clone(), burn_for_post_ix.clone(), dummy_compute_budget_ix],
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
        Err(_) => 1_400_000u32
    };

    // Create final transaction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[memo_ix, burn_for_post_ix, compute_budget_ix],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    println!("Sending burn for post transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 BURN FOR POST SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            println!("✅ Successfully burned {} tokens for post {}!", burn_amount_tokens, post_id);
            println!("   User: {}", user.pubkey());
        },
        Err(err) => {
            println!("❌ BURN FOR POST FAILED!");
            println!("Error: {}", err);
        }
    }

    Ok(())
}

fn create_burn_for_post_instruction(
    program_id: &Pubkey,
    memo_burn_program_id: &Pubkey,
    user: &Pubkey,
    post: &Pubkey,
    mint: &Pubkey,
    user_token_account: &Pubkey,
    user_global_burn_stats: &Pubkey,
    post_id: u64,
    amount: u64,
) -> Instruction {
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_for_post");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // Add parameters: post_id (u64), amount (u64)
    instruction_data.extend_from_slice(&post_id.to_le_bytes());
    instruction_data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new(*post, false),
        AccountMeta::new(*mint, false),
        AccountMeta::new(*user_token_account, false),
        AccountMeta::new(*user_global_burn_stats, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(*memo_burn_program_id, false),
        AccountMeta::new_readonly(
            solana_sdk::sysvar::instructions::id(),
            false
        ),
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}
