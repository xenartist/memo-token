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
pub struct BlogBurnData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub burner: String,
    pub message: String,
}

// Constants
const BLOG_BURN_DATA_VERSION: u8 = 1;
const BURN_MEMO_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "blog";
const EXPECTED_OPERATION: &str = "burn_for_blog";
const DECIMAL_FACTOR: u64 = 1_000_000;
const MIN_BLOG_BURN_TOKENS: u64 = 1;
const MIN_BLOG_BURN_AMOUNT: u64 = MIN_BLOG_BURN_TOKENS * DECIMAL_FACTOR;

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-BLOG BURN FOR BLOG TEST ===");
    println!("This program burns tokens for a blog (only creator can burn).");
    println!();

    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <burn_amount_tokens> [message]", args[0]);
        println!();
        println!("Examples:");
        println!("  {} 1", args[0]);
        println!("  {} 100 \"Supporting this blog!\"", args[0]);
        return Ok(());
    }

    let burn_amount_tokens = args[1].parse::<u64>()
        .map_err(|_| format!("Invalid burn_amount: {}", args[1]))?;
    
    let message = if args.len() > 2 {
        args[2].clone()
    } else {
        String::new()
    };

    if burn_amount_tokens < MIN_BLOG_BURN_TOKENS {
        return Err(format!("Burn amount too small. Minimum: {} token(s)", MIN_BLOG_BURN_TOKENS).into());
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
    let memo_blog_program_id = get_program_id("memo_blog").expect("Failed to get memo_blog program ID");
    let memo_burn_program_id = get_program_id("memo_burn").expect("Failed to get memo_burn program ID");
    let mint_address = get_token_mint("memo_token").expect("Failed to get memo_token mint address");

    println!("Network: {}", get_rpc_url());
    println!("User: {}", user.pubkey());
    println!("Burn amount: {} token(s)", burn_amount_tokens);
    println!("Message: {}", if message.is_empty() { "(none)" } else { &message });
    println!();

    // Calculate blog PDA (using user's pubkey as seed)
    let (blog_pda, _) = Pubkey::find_program_address(
        &[b"blog", user.pubkey().as_ref()],
        &memo_blog_program_id,
    );

    // Verify blog exists and user is the creator
    match client.get_account(&blog_pda) {
        Ok(account) => {
            if account.data.len() < 40 {
                return Err("Blog account data too short".into());
            }
            // Extract creator pubkey (offset: 8 discriminator, then 32 bytes for Pubkey)
            let creator_bytes = &account.data[8..40];
            let mut creator_array = [0u8; 32];
            creator_array.copy_from_slice(creator_bytes);
            let creator = Pubkey::new_from_array(creator_array);
            
            if creator != user.pubkey() {
                return Err(format!("You are not the creator of this blog. Creator: {}", creator).into());
            }
            println!("✅ Blog exists and you are the creator");
        },
        Err(_) => {
            return Err("Blog not found for this user. Please create a blog first.".into());
        }
    }

    // Create burn data
    let burn_data = BlogBurnData {
        version: BLOG_BURN_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        burner: user.pubkey().to_string(),
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
    println!("  Blog: {}", blog_pda);
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

    // Create burn_for_blog instruction
    let burn_for_blog_ix = create_burn_for_blog_instruction(
        &memo_blog_program_id,
        &memo_burn_program_id,
        &user.pubkey(),
        &blog_pda,
        &mint_address,
        &user_token_account,
        &user_global_burn_stats_pda,
        burn_amount,
    );

    // Simulate transaction
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[memo_ix.clone(), burn_for_blog_ix.clone(), dummy_compute_budget_ix],
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
        &[memo_ix, burn_for_blog_ix, compute_budget_ix],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    println!("Sending burn for blog transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 BURN FOR BLOG SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            println!("✅ Successfully burned {} tokens for your blog!", burn_amount_tokens);
            println!("   Burner: {}", user.pubkey());
        },
        Err(err) => {
            println!("❌ BURN FOR BLOG FAILED!");
            println!("Error: {}", err);
        }
    }

    Ok(())
}

fn create_burn_for_blog_instruction(
    program_id: &Pubkey,
    memo_burn_program_id: &Pubkey,
    burner: &Pubkey,
    blog: &Pubkey,
    mint: &Pubkey,
    burner_token_account: &Pubkey,
    user_global_burn_stats: &Pubkey,
    amount: u64,
) -> Instruction {
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_for_blog");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    instruction_data.extend_from_slice(&amount.to_le_bytes());

    let accounts = vec![
        AccountMeta::new(*burner, true),
        AccountMeta::new(*blog, false),
        AccountMeta::new(*mint, false),
        AccountMeta::new(*burner_token_account, false),
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
