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
use std::str::FromStr;
use std::path::PathBuf;
use sha2::{Sha256, Digest};
use solana_system_interface::program as system_program;
use memo_token_client::{get_rpc_url, get_program_id};

// Get admin authority keypair path (unified for all environments)
fn get_admin_authority_keypair_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    PathBuf::from(home)
        .join(".config/solana/memo-token/authority/deploy_admin-keypair.json")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-CHAT INITIALIZE BURN LEADERBOARD (ADMIN ONLY) ===");
    println!("This is a one-time setup operation to initialize the burn leaderboard.");
    println!("Only the authorized admin can perform this operation.");
    println!();

    // Connect to network
    let rpc_url = get_rpc_url();
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new(rpc_url);

    // Load admin wallet from unified authority keypair location
    let admin_keypair_path = get_admin_authority_keypair_path();
    println!("Loading admin keypair from: {}", admin_keypair_path.display());
    
    let admin = read_keypair_file(&admin_keypair_path)
        .expect(&format!("Failed to read admin keypair file from {:?}. Run setup-keypairs.sh first.", admin_keypair_path));

    println!("✅ Admin keypair loaded successfully!");
    println!("   Admin address: {}", admin.pubkey());
    println!();

    // Program address
    let memo_chat_program_id = get_program_id("memo_chat").expect("Failed to get memo_chat program ID");

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, bump) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &memo_chat_program_id,
    );

    println!("Program addresses:");
    println!("  Memo-chat program: {}", memo_chat_program_id);
    println!("  Admin: {}", admin.pubkey());
    println!("  Burn leaderboard PDA: {}", burn_leaderboard_pda);
    println!("  PDA bump: {}", bump);
    println!();

    // Check if burn leaderboard already exists
    match client.get_account(&burn_leaderboard_pda) {
        Ok(account) => {
            println!("✅ Burn leaderboard already exists!");
            println!("   Account owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            
            // Verify it's owned by the correct program
            if account.owner == memo_chat_program_id {
                println!("   ✅ Owned by memo-chat program");
            } else {
                println!("   ⚠️  Owned by different program: {}", account.owner);
            }
            
            // Try to read leaderboard data - Updated for Vec<LeaderboardEntry> format
            if account.data.len() >= 13 { // 8 bytes discriminator + 1 byte current_size + 4 bytes Vec length
                let current_size = account.data[8];
                
                // Read Vec length (4 bytes after current_size)
                let vec_length_bytes = &account.data[9..13];
                let vec_length = u32::from_le_bytes(vec_length_bytes.try_into().unwrap());
                
                println!("   Current leaderboard size: {}/100", current_size);
                println!("   Vec entries count: {}", vec_length);
                
                // Verify data consistency
                if current_size as u32 != vec_length {
                    println!("   ⚠️  Warning: current_size ({}) != vec_length ({})", current_size, vec_length);
                }
                
                // If there are entries, show some
                if vec_length > 0 && account.data.len() >= 13 + (vec_length as usize * 16) {
                    println!("   📊 Current top entries:");
                    for i in 0..std::cmp::min(vec_length as usize, 5) {
                        let entry_start = 13 + (i * 16); // Start after discriminator(8) + current_size(1) + vec_length(4)
                        
                        if entry_start + 16 <= account.data.len() {
                            let group_id_bytes = &account.data[entry_start..entry_start + 8];
                            let burned_amount_bytes = &account.data[entry_start + 8..entry_start + 16];
                            
                            let group_id = u64::from_le_bytes(group_id_bytes.try_into().unwrap());
                            let burned_amount = u64::from_le_bytes(burned_amount_bytes.try_into().unwrap());
                            
                            println!("     Rank {}: Group {} - {} tokens", 
                                    i + 1, group_id, burned_amount / 1_000_000);
                        }
                    }
                    
                    if vec_length > 5 {
                        println!("     ... and {} more entries", vec_length - 5);
                    }
                } else if vec_length > 0 {
                    println!("   ⚠️  Expected {} entries but account data is too short", vec_length);
                    println!("   Expected: {} bytes, Actual: {} bytes", 
                            13 + (vec_length as usize * 16), account.data.len());
                } else {
                    println!("   📊 Leaderboard is empty (no entries yet)");
                }
            } else {
                println!("   ⚠️  Account data too short to parse leaderboard structure");
                println!("   Expected at least 13 bytes, got {} bytes", account.data.len());
            }
            
            println!();
            println!("No action needed. The burn leaderboard is already initialized.");
            return Ok(());
        },
        Err(_) => {
            println!("ℹ️  Burn leaderboard not found. Proceeding with initialization...");
        }
    }

    // Check admin SOL balance
    let admin_balance = client.get_balance(&admin.pubkey())?;
    let admin_sol = admin_balance as f64 / 1_000_000_000.0; // Convert lamports to SOL
    println!("Admin SOL balance: {:.4} SOL", admin_sol);
    
    if admin_sol < 0.01 {
        println!("⚠️  Warning: Low SOL balance. You may need more SOL to pay for transaction fees.");
        if admin_sol < 0.001 {
            println!("❌ ERROR: Insufficient SOL balance for transaction fees.");
            return Ok(());
        }
    }
    println!();

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create initialize_burn_leaderboard instruction
    let init_leaderboard_ix = create_initialize_burn_leaderboard_instruction(
        &memo_chat_program_id,
        &admin.pubkey(),
        &burn_leaderboard_pda,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(500_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, init_leaderboard_ix.clone()],
        Some(&admin.pubkey()),
        &[&admin],
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
                200_000u32
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 200_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            200_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, init_leaderboard_ix],
        Some(&admin.pubkey()),
        &[&admin],
        recent_blockhash,
    );

    println!("Sending initialize burn leaderboard transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 BURN LEADERBOARD INITIALIZATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            
            // Verify the account was created correctly
            match client.get_account(&burn_leaderboard_pda) {
                Ok(account) => {
                    println!("✅ Burn leaderboard account created:");
                    println!("   PDA: {}", burn_leaderboard_pda);
                    println!("   Owner: {}", account.owner);
                    println!("   Data length: {} bytes", account.data.len());
                    
                    if account.data.len() >= 9 {
                        let current_size_byte = &account.data[8..9];
                        let current_size = current_size_byte[0];
                        println!("   Initial leaderboard size: {}/100", current_size);
                    }
                    
                    println!();
                    println!("✅ The burn leaderboard is now ready!");
                    println!("   📊 Will track top 100 groups by total burned tokens");
                    println!("   🚀 Groups will automatically enter leaderboard when creating groups or burning tokens");
                    println!("   🏆 Leaderboard will be updated in real-time during group creation and token burns");
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created account: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ BURN LEADERBOARD INITIALIZATION FAILED!");
            println!("Error: {}", err);
            
            // Provide helpful error analysis
            let error_msg = err.to_string();
            if error_msg.contains("UnauthorizedAdmin") {
                println!("💡 Authorization Error: Only the authorized admin can initialize the burn leaderboard.");
                println!("   Current wallet: {}", admin.pubkey());
                println!("   Make sure this matches the AUTHORIZED_ADMIN_PUBKEY in the contract code.");
            } else if error_msg.contains("already in use") {
                println!("💡 The burn leaderboard account already exists. This is normal if initialization was run before.");
            } else if error_msg.contains("insufficient funds") {
                println!("💡 Insufficient SOL balance. Please add more SOL to the admin wallet.");
            } else if error_msg.contains("Invalid program id") {
                println!("💡 Check that the memo-chat program is deployed and the program ID is correct.");
            } else {
                println!("💡 Unexpected error. Please check the program deployment and network connection.");
            }
        }
    }

    Ok(())
}

fn create_initialize_burn_leaderboard_instruction(
    program_id: &Pubkey,
    admin: &Pubkey,
    burn_leaderboard: &Pubkey,
) -> Instruction {
    // Calculate Anchor instruction sighash for "initialize_burn_leaderboard"
    let mut hasher = Sha256::new();
    hasher.update(b"global:initialize_burn_leaderboard");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // Only the sighash, no additional parameters

    let accounts = vec![
        AccountMeta::new(*admin, true),                            // admin (signer, must be authorized)
        AccountMeta::new(*burn_leaderboard, false),                // burn_leaderboard (PDA to be created)
        AccountMeta::new_readonly(system_program::id(), false),    // system_program
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}