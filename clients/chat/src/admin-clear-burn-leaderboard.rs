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
use memo_token_client::get_rpc_url;

// Get admin authority keypair path (unified for all environments)
fn get_admin_authority_keypair_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    PathBuf::from(home)
        .join(".config/solana/memo-token/authority/deploy_admin-keypair.json")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-CHAT CLEAR BURN LEADERBOARD (ADMIN ONLY) ===");
    println!("This operation will clear all entries from the burn leaderboard.");
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
    let memo_chat_program_id = Pubkey::from_str("54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF")
        .expect("Invalid memo-chat program ID");

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

    // Check if burn leaderboard exists and show current state
    match client.get_account(&burn_leaderboard_pda) {
        Ok(account) => {
            println!("✅ Burn leaderboard found!");
            println!("   Account owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            
            // Verify it's owned by the correct program
            if account.owner == memo_chat_program_id {
                println!("   ✅ Owned by memo-chat program");
            } else {
                println!("   ⚠️  Owned by different program: {}", account.owner);
                println!("❌ ERROR: Cannot clear leaderboard owned by another program");
                return Ok(());
            }
            
            // Try to read leaderboard data - For Vec<LeaderboardEntry> format
            if account.data.len() >= 13 { // 8 bytes discriminator + 1 byte current_size + 4 bytes Vec length
                let current_size = account.data[8];
                
                // Read Vec length (4 bytes after current_size)
                let vec_length_bytes = &account.data[9..13];
                let vec_length = u32::from_le_bytes(vec_length_bytes.try_into().unwrap());
                
                println!("   📊 Current leaderboard state:");
                println!("      Current size: {}/100", current_size);
                println!("      Vec entries count: {}", vec_length);
                
                // Verify data consistency
                if current_size as u32 != vec_length {
                    println!("   ⚠️  Warning: current_size ({}) != vec_length ({})", current_size, vec_length);
                    println!("   This indicates potential data corruption that clearing will fix.");
                }
                
                // If there are entries, show some
                if vec_length > 0 && account.data.len() >= 13 + (vec_length as usize * 16) {
                    println!("   🏆 Current leaderboard state (will be cleared):");
                    let mut group_ids_seen = std::collections::HashSet::new();
                    let mut duplicate_count = 0;
                    
                    // collect all entries
                    let mut entries = Vec::new();
                    for i in 0..vec_length as usize {
                        let entry_start = 13 + (i * 16);
                        
                        if entry_start + 16 <= account.data.len() {
                            let group_id_bytes = &account.data[entry_start..entry_start + 8];
                            let burned_amount_bytes = &account.data[entry_start + 8..entry_start + 16];
                            
                            let group_id = u64::from_le_bytes(group_id_bytes.try_into().unwrap());
                            let burned_amount = u64::from_le_bytes(burned_amount_bytes.try_into().unwrap());
                            
                            entries.push((group_id, burned_amount));
                        }
                    }
                    
                    // sort by burned_amount in descending order to show real rankings
                    entries.sort_by(|a, b| b.1.cmp(&a.1));
                    
                    // show statistics
                    let total_burned: u64 = entries.iter().map(|(_, amount)| amount).sum();
                    let total_tokens = total_burned / 1_000_000;
                    println!("      📊 Total entries: {}", entries.len());
                    println!("      🔥 Total burned: {} MEMO tokens", format_number(total_tokens));
                    
                    if let Some((_, highest)) = entries.first() {
                        println!("      👑 Highest: {} MEMO", format_number(highest / 1_000_000));
                    }
                    if let Some((_, lowest)) = entries.last() {
                        println!("      🎯 Lowest: {} MEMO", format_number(lowest / 1_000_000));
                    }
                    println!();
                    
                    // show top 10 (by actual rankings)
                    println!("      Top 10 rankings:");
                    for (rank, (group_id, burned_amount)) in entries.iter().take(10).enumerate() {
                        let status = if group_ids_seen.contains(group_id) {
                            duplicate_count += 1;
                            "🔄 DUPLICATE"
                        } else {
                            group_ids_seen.insert(*group_id);
                            ""
                        };
                        
                        let medal = match rank + 1 {
                            1 => "🥇",
                            2 => "🥈",
                            3 => "🥉",
                            _ => "🔥",
                        };
                        
                        println!("        {} Rank {:2}: Group {:5} - {:>8} MEMO {}", 
                                medal, rank + 1, group_id, format_number(burned_amount / 1_000_000), status);
                    }
                    
                    if entries.len() > 10 {
                        println!("        ... and {} more entries", entries.len() - 10);
                    }
                    
                    if duplicate_count > 0 {
                        println!("   🚨 Found {} duplicate entries - clearing will fix this!", duplicate_count);
                    }
                } else if vec_length > 0 {
                    println!("   ⚠️  Expected {} entries but account data is too short", vec_length);
                    println!("   Expected: {} bytes, Actual: {} bytes", 
                            13 + (vec_length as usize * 16), account.data.len());
                } else {
                    println!("   📊 Leaderboard is empty (no entries to clear)");
                    println!();
                    println!("No action needed. The burn leaderboard is already empty.");
                    return Ok(());
                }
            } else {
                println!("   ⚠️  Account data too short to parse leaderboard structure");
                println!("   Expected at least 13 bytes, got {} bytes", account.data.len());
            }
            
            println!();
        },
        Err(_) => {
            println!("❌ Burn leaderboard not found!");
            println!("   The leaderboard PDA does not exist. Nothing to clear.");
            println!("   Initialize the leaderboard first using admin-init-burn-leaderboard.");
            return Ok(());
        }
    }

    // Confirm before proceeding
    println!("⚠️  WARNING: This operation will permanently clear ALL leaderboard entries!");
    println!("   All ranking data will be lost and cannot be recovered.");
    println!("   Groups can rebuild their ranking by burning more tokens after clearing.");
    println!();
    print!("Are you sure you want to proceed? Type 'YES' to confirm: ");
    use std::io::{self, Write};
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read input");
    let input = input.trim();
    
    if input != "YES" {
        println!("Operation cancelled by user.");
        return Ok(());
    }
    
    println!("✅ Confirmation received. Proceeding with leaderboard clearing...");
    println!();

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

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create clear_burn_leaderboard instruction
    let clear_leaderboard_ix = create_clear_burn_leaderboard_instruction(
        &memo_chat_program_id,
        &admin.pubkey(),
        &burn_leaderboard_pda,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, clear_leaderboard_ix.clone()],
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
                100_000u32
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 100_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            100_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, clear_leaderboard_ix],
        Some(&admin.pubkey()),
        &[&admin],
        recent_blockhash,
    );

    println!("Sending clear burn leaderboard transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 BURN LEADERBOARD CLEARING SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            
            // Verify the leaderboard was cleared correctly
            match client.get_account(&burn_leaderboard_pda) {
                Ok(account) => {
                    println!("✅ Burn leaderboard state after clearing:");
                    println!("   PDA: {}", burn_leaderboard_pda);
                    println!("   Owner: {}", account.owner);
                    println!("   Data length: {} bytes", account.data.len());
                    
                    if account.data.len() >= 13 {
                        let current_size = account.data[8];
                        let vec_length_bytes = &account.data[9..13];
                        let vec_length = u32::from_le_bytes(vec_length_bytes.try_into().unwrap());
                        
                        println!("   Current size: {}/100", current_size);
                        println!("   Vec entries count: {}", vec_length);
                        
                        if current_size == 0 && vec_length == 0 {
                            println!("   ✅ Leaderboard successfully cleared!");
                        } else {
                            println!("   ⚠️  Unexpected state after clearing:");
                            println!("      current_size: {} (expected: 0)", current_size);
                            println!("      vec_length: {} (expected: 0)", vec_length);
                        }
                    }
                    
                    println!();
                    println!("🚀 The burn leaderboard is now empty and ready for new entries!");
                    println!("   Groups will enter the leaderboard when they create groups or burn tokens");
                    println!("   Rankings will be calculated correctly without duplicate entries");
                },
                Err(e) => {
                    println!("⚠️  Could not fetch leaderboard account after clearing: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ BURN LEADERBOARD CLEARING FAILED!");
            println!("Error: {}", err);
            
            // Provide helpful error analysis
            let error_msg = err.to_string();
            if error_msg.contains("UnauthorizedAdmin") {
                println!("💡 Authorization Error: Only the authorized admin can clear the burn leaderboard.");
                println!("   Current wallet: {}", admin.pubkey());
                println!("   Make sure this matches the AUTHORIZED_ADMIN_PUBKEY in the contract code.");
            } else if error_msg.contains("insufficient funds") {
                println!("💡 Insufficient SOL balance. Please add more SOL to the admin wallet.");
            } else if error_msg.contains("Invalid program id") {
                println!("💡 Check that the memo-chat program is deployed and the program ID is correct.");
            } else if error_msg.contains("AccountNotFound") {
                println!("💡 The burn leaderboard account was not found. Initialize it first with admin-init-burn-leaderboard.");
            } else {
                println!("💡 Unexpected error. Please check the program deployment and network connection.");
            }
        }
    }

    Ok(())
}

fn create_clear_burn_leaderboard_instruction(
    program_id: &Pubkey,
    admin: &Pubkey,
    burn_leaderboard: &Pubkey,
) -> Instruction {
    // Calculate Anchor instruction sighash for "clear_burn_leaderboard"
    let mut hasher = Sha256::new();
    hasher.update(b"global:clear_burn_leaderboard");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // Only the sighash, no additional parameters

    let accounts = vec![
        AccountMeta::new(*admin, true),                            // admin (signer, must be authorized)
        AccountMeta::new(*burn_leaderboard, false),                // burn_leaderboard (PDA to be cleared)
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

// add number formatting helper function
fn format_number(num: u64) -> String {
    let num_str = num.to_string();
    let mut result = String::new();
    
    for (i, ch) in num_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    
    result.chars().rev().collect()
}