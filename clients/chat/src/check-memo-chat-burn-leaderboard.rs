use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use chrono::{DateTime, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-CHAT BURN LEADERBOARD CHECKER ===");
    println!("Checking burn leaderboard rankings and statistics...");
    println!();

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Program address
    let memo_chat_program_id = Pubkey::from_str("54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF")
        .expect("Invalid memo-chat program ID");

    println!("🔍 Connecting to: {}", rpc_url);
    println!("📋 Memo-chat program: {}", memo_chat_program_id);
    println!();

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, bump) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &memo_chat_program_id,
    );

    println!("=== BURN LEADERBOARD INFORMATION ===");
    println!("🏆 Leaderboard PDA: {}", burn_leaderboard_pda);
    println!("🎯 PDA bump: {}", bump);
    println!();

    // Check if burn leaderboard exists
    let leaderboard_data = match client.get_account(&burn_leaderboard_pda) {
        Ok(account) => {
            println!("✅ Burn leaderboard found");
            println!("   Owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            println!("   Rent exempt: {:.6} SOL", account.lamports as f64 / 1_000_000_000.0);

            if account.owner != memo_chat_program_id {
                println!("❌ Error: Account not owned by memo-chat program!");
                println!("   Expected: {}", memo_chat_program_id);
                println!("   Actual: {}", account.owner);
                return Ok(());
            }

            account.data
        },
        Err(e) => {
            println!("❌ Burn leaderboard not found: {}", e);
            println!();
            println!("💡 The burn leaderboard may not be initialized yet.");
            println!("   Run 'admin-init-burn-leaderboard' to initialize it first.");
            return Ok(());
        }
    };

    println!();

    // Parse leaderboard data
    match parse_burn_leaderboard_data(&leaderboard_data) {
        Ok(leaderboard) => {
            display_leaderboard_statistics(&leaderboard);
            println!();
            
            if leaderboard.entries.is_empty() {
                display_empty_leaderboard();
            } else {
                display_leaderboard_rankings(&leaderboard);
                println!();
                display_leaderboard_summary(&leaderboard);
                
                // Optionally display group details
                if !leaderboard.entries.is_empty() {
                    println!();
                    display_top_groups_details(&client, &memo_chat_program_id, &leaderboard)?;
                }
            }
        },
        Err(e) => {
            println!("❌ Failed to parse leaderboard data: {}", e);
            println!("💡 The leaderboard account may be corrupted or use a different format.");
        }
    }

    Ok(())
}

// Struct to hold parsed leaderboard data
#[derive(Debug)]
struct BurnLeaderboard {
    pub current_size: u8,
    pub entries: Vec<LeaderboardEntry>,
}

#[derive(Debug, Clone)]
struct LeaderboardEntry {
    pub group_id: u64,
    pub burned_amount: u64,
}

// Parse BurnLeaderboard account data
fn parse_burn_leaderboard_data(data: &[u8]) -> Result<BurnLeaderboard, Box<dyn std::error::Error>> {
    if data.len() < 13 { // 8 discriminator + 1 current_size + 4 vec_length
        return Err("Data too short for leaderboard structure".into());
    }

    let mut offset = 8; // Skip discriminator

    // Read current_size (u8)
    let current_size = data[offset];
    offset += 1;

    // Read Vec length (u32)
    let vec_length = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    offset += 4;

    // Verify data consistency
    if current_size as u32 != vec_length {
        return Err(format!("Inconsistent data: current_size ({}) != vec_length ({})", 
                          current_size, vec_length).into());
    }

    // Verify remaining data length
    let expected_data_length = offset + (vec_length as usize * 16);
    if data.len() < expected_data_length {
        return Err(format!("Data too short: expected {} bytes, got {} bytes", 
                          expected_data_length, data.len()).into());
    }

    // Read entries
    let mut entries = Vec::new();
    for i in 0..vec_length {
        let entry_offset = offset + (i as usize * 16);
        
        let group_id = u64::from_le_bytes(
            data[entry_offset..entry_offset + 8].try_into().unwrap()
        );
        let burned_amount = u64::from_le_bytes(
            data[entry_offset + 8..entry_offset + 16].try_into().unwrap()
        );

        entries.push(LeaderboardEntry {
            group_id,
            burned_amount,
        });
    }

    Ok(BurnLeaderboard {
        current_size,
        entries,
    })
}

fn display_leaderboard_statistics(leaderboard: &BurnLeaderboard) {
    println!("=== LEADERBOARD STATISTICS ===");
    println!("📊 Total entries: {}/100", leaderboard.current_size);
    println!("🔢 Vec length: {}", leaderboard.entries.len());
    
    if leaderboard.current_size as usize != leaderboard.entries.len() {
        println!("⚠️  Warning: Size mismatch detected!");
    }
}

fn display_empty_leaderboard() {
    println!("=== EMPTY LEADERBOARD ===");
    println!("📭 No groups have entered the burn leaderboard yet.");
    println!();
    println!("💡 How to enter the leaderboard:");
    println!("   1. Create a chat group (burns minimum 42,069 MEMO tokens)");
    println!("   2. Burn additional tokens for existing groups");
    println!("   3. Only the top 100 groups by total burn amount are ranked");
}

fn display_leaderboard_rankings(leaderboard: &BurnLeaderboard) {
    println!("=== BURN LEADERBOARD RANKINGS ===");
    println!("🏆 Top {} groups by total burned tokens:", leaderboard.entries.len());
    println!();

    for (rank, entry) in leaderboard.entries.iter().enumerate() {
        let rank_display = rank + 1;
        let tokens = entry.burned_amount / 1_000_000;
        let medal = match rank_display {
            1 => "🥇",
            2 => "🥈", 
            3 => "🥉",
            _ => "🔥",
        };

        println!("   {} Rank {:3}: Group {:5} - {:>10} MEMO ({:>15} units)", 
                medal, rank_display, entry.group_id, 
                format_number(tokens), format_number(entry.burned_amount));
    }
}

fn display_leaderboard_summary(leaderboard: &BurnLeaderboard) {
    if leaderboard.entries.is_empty() {
        return;
    }

    println!("=== SUMMARY STATISTICS ===");
    
    let total_burned: u64 = leaderboard.entries.iter().map(|e| e.burned_amount).sum();
    let total_tokens = total_burned / 1_000_000;
    let avg_burned = total_burned / leaderboard.entries.len() as u64;
    let avg_tokens = avg_burned / 1_000_000;

    println!("🔥 Total burned across all ranked groups: {} MEMO", format_number(total_tokens));
    println!("📊 Average burned per ranked group: {} MEMO", format_number(avg_tokens));
    
    if let Some(top_entry) = leaderboard.entries.first() {
        println!("👑 Highest burn: Group {} with {} MEMO", 
                top_entry.group_id, format_number(top_entry.burned_amount / 1_000_000));
    }
    
    if let Some(last_entry) = leaderboard.entries.last() {
        let rank = leaderboard.entries.len();
        println!("🎯 Rank {} threshold: {} MEMO", 
                rank, format_number(last_entry.burned_amount / 1_000_000));
    }

    // Show distribution
    if leaderboard.entries.len() >= 10 {
        println!();
        println!("📈 Distribution breakdown:");
        let top_10_total: u64 = leaderboard.entries.iter().take(10).map(|e| e.burned_amount).sum();
        let top_10_percentage = (top_10_total as f64 / total_burned as f64) * 100.0;
        println!("   Top 10 groups: {:.1}% of total burn", top_10_percentage);
        
        if leaderboard.entries.len() >= 50 {
            let top_50_total: u64 = leaderboard.entries.iter().take(50).map(|e| e.burned_amount).sum();
            let top_50_percentage = (top_50_total as f64 / total_burned as f64) * 100.0;
            println!("   Top 50 groups: {:.1}% of total burn", top_50_percentage);
        }
    }
}

fn display_top_groups_details(
    client: &RpcClient, 
    program_id: &Pubkey, 
    leaderboard: &BurnLeaderboard
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TOP GROUPS DETAILS ===");
    println!("📋 Fetching details for top 5 groups...");
    println!();

    let top_groups = leaderboard.entries.iter().take(5);
    
    for (rank, entry) in top_groups.enumerate() {
        let rank_display = rank + 1;
        let medal = match rank_display {
            1 => "🥇",
            2 => "🥈",
            3 => "🥉", 
            _ => "🏆",
        };

        println!("{} Rank {}: Group {}", medal, rank_display, entry.group_id);

        // Calculate group PDA
        let (chat_group_pda, _) = Pubkey::find_program_address(
            &[b"chat_group", &entry.group_id.to_le_bytes()],
            program_id,
        );

        match client.get_account(&chat_group_pda) {
            Ok(account) => {
                if let Ok(group_info) = parse_chat_group_basic(&account.data) {
                    println!("   📝 Name: \"{}\"", group_info.name);
                    println!("   👤 Creator: {}", group_info.creator);
                    println!("   💬 Total memos: {}", group_info.memo_count);
                    println!("   🔥 Burned: {} MEMO", format_number(entry.burned_amount / 1_000_000));
                    
                    if group_info.created_at > 0 {
                        if let Some(datetime) = DateTime::<Utc>::from_timestamp(group_info.created_at, 0) {
                            println!("   🕐 Created: {}", datetime.format("%Y-%m-%d %H:%M:%S UTC"));
                        }
                    }
                    
                    if !group_info.description.is_empty() {
                        let desc = if group_info.description.len() > 50 {
                            format!("{}...", &group_info.description[..47])
                        } else {
                            group_info.description.clone()
                        };
                        println!("   📄 Description: \"{}\"", desc);
                    }
                } else {
                    println!("   ❌ Failed to parse group data");
                }
            },
            Err(_) => {
                println!("   ❌ Group account not found");
            }
        }
        
        println!();
    }

    Ok(())
}

// Basic group info struct for details display
#[derive(Debug)]
struct ChatGroupBasic {
    pub name: String,
    pub creator: Pubkey,
    pub created_at: i64,
    pub description: String,
    pub memo_count: u64,
}

// Parse basic ChatGroup data (only what we need for display)
fn parse_chat_group_basic(data: &[u8]) -> Result<ChatGroupBasic, Box<dyn std::error::Error>> {
    if data.len() < 8 {
        return Err("Data too short".into());
    }

    let mut offset = 8; // Skip discriminator

    // Skip group_id (u64)
    offset += 8;

    // Read creator (32 bytes)
    if data.len() < offset + 32 {
        return Err("Data too short for creator".into());
    }
    let creator_bytes: [u8; 32] = data[offset..offset + 32].try_into().unwrap();
    let creator = Pubkey::from(creator_bytes);
    offset += 32;

    // Read created_at (i64)
    if data.len() < offset + 8 {
        return Err("Data too short for created_at".into());
    }
    let created_at = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read name (String)
    let (name, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Read description (String)  
    let (description, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Skip image (String)
    let (_, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Skip tags (Vec<String>)
    let (_, new_offset) = read_string_vec(data, offset)?;
    offset = new_offset;

    // Read memo_count (u64)
    if data.len() < offset + 8 {
        return Err("Data too short for memo_count".into());
    }
    let memo_count = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());

    Ok(ChatGroupBasic {
        name,
        creator,
        created_at,
        description,
        memo_count,
    })
}

// Helper function to read a String from account data
fn read_string(data: &[u8], offset: usize) -> Result<(String, usize), Box<dyn std::error::Error>> {
    if data.len() < offset + 4 {
        return Err("Data too short for string length".into());
    }

    let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    let new_offset = offset + 4;

    if data.len() < new_offset + len {
        return Err("Data too short for string content".into());
    }

    let string_data = &data[new_offset..new_offset + len];
    let string = String::from_utf8(string_data.to_vec())?;

    Ok((string, new_offset + len))
}

// Helper function to read a Vec<String> from account data
fn read_string_vec(data: &[u8], offset: usize) -> Result<(Vec<String>, usize), Box<dyn std::error::Error>> {
    if data.len() < offset + 4 {
        return Err("Data too short for vec length".into());
    }

    let vec_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    let mut new_offset = offset + 4;
    let mut strings = Vec::new();

    for _ in 0..vec_len {
        let (string, next_offset) = read_string(data, new_offset)?;
        strings.push(string);
        new_offset = next_offset;
    }

    Ok((strings, new_offset))
}

// Helper function to format large numbers with commas
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
