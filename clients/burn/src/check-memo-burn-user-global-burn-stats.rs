use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::read_keypair_file,
    signer::Signer,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use chrono::{DateTime, Utc};
use memo_token_client::{get_rpc_url, get_program_id};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-BURN USER GLOBAL BURN STATISTICS CHECKER ===");
    println!("Checking user's global burn statistics and account information...");
    println!();

    // Connect to network
    let rpc_url = get_rpc_url();
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load wallet
    let user = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    println!("🔍 Connecting to: {}", get_rpc_url());
    println!("👤 User: {}", user.pubkey());
    println!();

    // Program ID
    let memo_burn_program_id = get_program_id("memo_burn").expect("Failed to get memo_burn program ID");

    println!("📋 Memo-burn program: {}", memo_burn_program_id);
    println!();

    // Derive user global burn statistics PDA
    let (user_global_burn_stats_pda, bump) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", user.pubkey().as_ref()],
        &memo_burn_program_id,
    );

    println!("=== USER GLOBAL BURN STATISTICS INFORMATION ===");
    println!("📊 User Global Burn Stats PDA: {}", user_global_burn_stats_pda);
    println!("🎯 PDA bump: {}", bump);
    println!();

    // Check if account exists and get data
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(account) => {
            println!("✅ User global burn statistics account found");
            println!("   Owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            println!("   Rent exempt: {:.6} SOL", account.lamports as f64 / 1_000_000_000.0);
            println!();

            // Verify account is owned by memo-burn program
            if account.owner != memo_burn_program_id {
                println!("❌ Error: Account not owned by memo-burn program!");
                println!("   Expected: {}", memo_burn_program_id);
                println!("   Actual: {}", account.owner);
                return Ok(());
            }

            // Parse account data
            if account.data.len() >= 65 { // 8 (discriminator) + 32 (user) + 8 (total_burned) + 8 (burn_count) + 8 (last_burn_time) + 1 (bump)
                parse_and_display_burn_stats(&account.data, &user.pubkey())?;
            } else {
                println!("❌ Invalid account data size: {} bytes", account.data.len());
                println!("   Expected at least 65 bytes for UserGlobalBurnStats");
            }
        },
        Err(e) => {
            println!("❌ User global burn statistics account not found: {}", e);
            println!();
            println!("💡 The account has not been initialized yet.");
            println!("   Run the following command to initialize it:");
            println!("   cargo run --bin init-user-global-burn-stats");
            println!();
            println!("🔥 IMPORTANT: UserGlobalBurnStats is now REQUIRED for all burn operations!");
            println!("   You must initialize this account before performing any burns.");
        }
    }

    Ok(())
}

fn parse_and_display_burn_stats(data: &[u8], expected_user: &Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== BURN STATISTICS DATA ===");
    
    // Skip discriminator (first 8 bytes)
    let data = &data[8..];
    
    // Parse user pubkey (32 bytes)
    let user_bytes = &data[0..32];
    let user_pubkey = Pubkey::new_from_array(user_bytes.try_into()?);
    
    // Parse total_burned (8 bytes)
    let total_burned_bytes = &data[32..40];
    let total_burned = u64::from_le_bytes(total_burned_bytes.try_into()?);
    
    // Parse burn_count (8 bytes)
    let burn_count_bytes = &data[40..48];
    let burn_count = u64::from_le_bytes(burn_count_bytes.try_into()?);
    
    // Parse last_burn_time (8 bytes)
    let last_burn_time_bytes = &data[48..56];
    let last_burn_time = i64::from_le_bytes(last_burn_time_bytes.try_into()?);
    
    // Parse bump (1 byte)
    let bump = data[56];
    
    println!("👤 User: {}", user_pubkey);
    
    // Verify user matches expected
    if user_pubkey != *expected_user {
        println!("⚠️  Warning: Account user doesn't match wallet user!");
        println!("   Account user: {}", user_pubkey);
        println!("   Wallet user:  {}", expected_user);
    } else {
        println!("   ✅ User verification passed");
    }
    
    println!();
    println!("🔥 BURN STATISTICS:");
    println!("   Total burned: {} units ({} tokens)", total_burned, total_burned / 1_000_000);
    println!("   Burn count: {} transactions", burn_count);
    
    if burn_count > 0 {
        let avg_burn = total_burned / burn_count;
        println!("   Average burn: {} units ({} tokens) per transaction", avg_burn, avg_burn / 1_000_000);
    }
    
    println!("   PDA bump: {}", bump);
    
    // Format last burn time
    if last_burn_time > 0 {
        match DateTime::from_timestamp(last_burn_time, 0) {
            Some(datetime) => {
                let utc_time: DateTime<Utc> = datetime.into();
                println!("   Last burn: {} UTC", utc_time.format("%Y-%m-%d %H:%M:%S"));
                
                // Calculate time since last burn
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(utc_time);
                
                if duration.num_days() > 0 {
                    println!("   Time since: {} days ago", duration.num_days());
                } else if duration.num_hours() > 0 {
                    println!("   Time since: {} hours ago", duration.num_hours());
                } else if duration.num_minutes() > 0 {
                    println!("   Time since: {} minutes ago", duration.num_minutes());
                } else {
                    println!("   Time since: {} seconds ago", duration.num_seconds());
                }
            },
            None => {
                println!("   Last burn: {} (invalid timestamp)", last_burn_time);
            }
        }
    } else {
        println!("   Last burn: Never");
    }
    
    println!();
    
    // Display burn activity level
    display_burn_activity_analysis(total_burned, burn_count);
    
    Ok(())
}

fn display_burn_activity_analysis(total_burned: u64, burn_count: u64) {
    println!("=== BURN ACTIVITY ANALYSIS ===");
    
    let total_tokens = total_burned / 1_000_000;
    
    if burn_count == 0 {
        println!("📊 Activity Level: No burns yet");
        println!("💡 This account is initialized but hasn't been used for burning tokens.");
    } else if burn_count < 5 {
        println!("📊 Activity Level: New burner (< 5 transactions)");
    } else if burn_count < 20 {
        println!("📊 Activity Level: Regular burner (5-19 transactions)");
    } else if burn_count < 100 {
        println!("📊 Activity Level: Active burner (20-99 transactions)");
    } else {
        println!("📊 Activity Level: Power burner (100+ transactions)");
    }
    
    if total_tokens == 0 {
        println!("🔥 Burn Volume: No tokens burned");
    } else if total_tokens < 1000 {
        println!("🔥 Burn Volume: Small scale (< 1K tokens)");
    } else if total_tokens < 10000 {
        println!("🔥 Burn Volume: Medium scale (1K-10K tokens)");
    } else if total_tokens < 100000 {
        println!("🔥 Burn Volume: Large scale (10K-100K tokens)");
    } else {
        println!("🔥 Burn Volume: Massive scale (100K+ tokens)");
    }
    
    println!();
    println!("✅ Account is ready for burn operations!");
    println!("💡 All memo-burn operations will automatically update these statistics.");
}