use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use chrono::{DateTime, Utc};

use memo_token_client::{get_rpc_url, get_program_id};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-PROJECT BURN LEADERBOARD CHECKER ===");
    println!("Checking burn leaderboard rankings and statistics...");
    println!();

    // Connect to network
    let rpc_url = get_rpc_url();
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Program address
    let memo_project_program_id = get_program_id("memo_project").expect("Failed to get memo_project program ID");

    println!("🔍 Connecting to: {}", get_rpc_url());
    println!("📋 Memo-project program: {}", memo_project_program_id);
    println!();

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, bump) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &memo_project_program_id,
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

            if account.owner != memo_project_program_id {
                println!("❌ Error: Account not owned by memo-project program!");
                println!("   Expected: {}", memo_project_program_id);
                println!("   Actual: {}", account.owner);
                return Ok(());
            }

            account.data
        },
        Err(e) => {
            println!("❌ Burn leaderboard not found: {}", e);
            println!();
            println!("💡 The burn leaderboard may not be initialized yet.");
            println!("   Run 'admin-memo-project-init-burn-leaderboard' to initialize it first.");
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
                
                // Optionally display project details
                if !leaderboard.entries.is_empty() {
                    println!();
                    display_top_projects_details(&client, &memo_project_program_id, &leaderboard)?;
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
    pub project_id: u64,
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
        
        let project_id = u64::from_le_bytes(
            data[entry_offset..entry_offset + 8].try_into().unwrap()
        );
        let burned_amount = u64::from_le_bytes(
            data[entry_offset + 8..entry_offset + 16].try_into().unwrap()
        );

        entries.push(LeaderboardEntry {
            project_id,
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
    println!("📭 No projects have entered the burn leaderboard yet.");
    println!();
    println!("💡 How to enter the leaderboard:");
    println!("   1. Create a project (burns minimum MEMO tokens)");
    println!("   2. Update existing projects to burn additional tokens");
    println!("   3. Only the top 100 projects by total burn amount are ranked");
}

fn display_leaderboard_rankings(leaderboard: &BurnLeaderboard) {
    println!("=== BURN LEADERBOARD RANKINGS ===");
    println!("🏆 Total entries: {}", leaderboard.entries.len());
    println!();

    // Show raw data (contract storage order)
    println!("📋 RAW DATA (Contract Storage Order):");
    println!("   ⚠️  Note: This shows the order as stored in contract (unsorted)");
    for (index, entry) in leaderboard.entries.iter().enumerate() {
        let tokens = entry.burned_amount / 1_000_000;
        println!("   Storage[{}]: Project {:5} - {:>10} MEMO ({:>15} units)", 
                index, entry.project_id, 
                format_number(tokens), format_number(entry.burned_amount));
    }
    
    println!();
    println!("{}", "=".repeat(60));
    println!();

    // Show sorted rankings
    println!("🏆 SORTED RANKINGS (Client-Side Sorted):");
    println!("   ✅ This shows the correct rankings by burned_amount (descending)");
    
    let mut sorted_entries = leaderboard.entries.clone();
    sorted_entries.sort_by(|a, b| b.burned_amount.cmp(&a.burned_amount));

    for (rank, entry) in sorted_entries.iter().enumerate() {
        let rank_display = rank + 1;
        let tokens = entry.burned_amount / 1_000_000;
        let medal = match rank_display {
            1 => "🥇",
            2 => "🥈", 
            3 => "🥉",
            _ => "🔥",
        };

        // Find the position of this entry in the original array
        let original_index = leaderboard.entries.iter()
            .position(|e| e.project_id == entry.project_id && e.burned_amount == entry.burned_amount)
            .unwrap_or(999);

        println!("   {} Rank {:3}: Project {:5} - {:>10} MEMO ({:>15} units) [was Storage[{}]]", 
                medal, rank_display, entry.project_id, 
                format_number(tokens), format_number(entry.burned_amount), original_index);
    }
    
    // Show position change summary
    println!();
    println!("🔄 POSITION CHANGES SUMMARY:");
    
    let mut position_changes = Vec::new();
    for (new_rank, entry) in sorted_entries.iter().enumerate() {
        let original_index = leaderboard.entries.iter()
            .position(|e| e.project_id == entry.project_id && e.burned_amount == entry.burned_amount)
            .unwrap_or(999);
        
        let change = (original_index as i32) - (new_rank as i32);
        position_changes.push((entry.project_id, original_index, new_rank + 1, change));
    }
    
    // Sort by change magnitude (largest changes first)
    position_changes.sort_by(|a, b| b.3.abs().cmp(&a.3.abs()));
    
    for (project_id, original_pos, new_rank, change) in position_changes.iter().take(5) {
        let change_text = if *change > 0 {
            format!("↑ moved up {} positions", change)
        } else if *change < 0 {
            format!("↓ moved down {} positions", change.abs())
        } else {
            "→ no change".to_string()
        };
        
        println!("   Project {:5}: Storage[{}] → Rank {} ({})", 
                project_id, original_pos, new_rank, change_text);
    }
    
    if position_changes.len() > 5 {
        println!("   ... and {} more projects with position changes", position_changes.len() - 5);
    }
    
    // Sorting verification
    println!();
    println!("📊 SORTING VERIFICATION:");
    let is_correctly_sorted = sorted_entries.windows(2)
        .all(|window| window[0].burned_amount >= window[1].burned_amount);
    
    if is_correctly_sorted {
        println!("   ✅ Sorted data is correctly ordered (descending by burned_amount)");
    } else {
        println!("   ❌ Sorted data has ordering issues!");
    }
    
    // Check if original data is already sorted
    let was_already_sorted = leaderboard.entries.windows(2)
        .all(|window| window[0].burned_amount >= window[1].burned_amount);
    
    if was_already_sorted {
        println!("   ℹ️  Original contract data was already sorted");
    } else {
        println!("   ℹ️  Original contract data was unsorted (as expected)");
    }
}

fn display_leaderboard_summary(leaderboard: &BurnLeaderboard) {
    if leaderboard.entries.is_empty() {
        return;
    }

    println!("=== SUMMARY STATISTICS ===");
    
    // Sort by burned_amount in descending order
    let mut sorted_entries = leaderboard.entries.clone();
    sorted_entries.sort_by(|a, b| b.burned_amount.cmp(&a.burned_amount));
    
    let total_burned: u64 = leaderboard.entries.iter().map(|e| e.burned_amount).sum();
    let total_tokens = total_burned / 1_000_000;
    let avg_burned = total_burned / leaderboard.entries.len() as u64;
    let avg_tokens = avg_burned / 1_000_000;

    println!("🔥 Total burned across all ranked projects: {} MEMO", format_number(total_tokens));
    println!("📊 Average burned per ranked project: {} MEMO", format_number(avg_tokens));
    
    // Get highest and lowest using sorted data
    if let Some(top_entry) = sorted_entries.first() {
        println!("👑 Highest burn: Project {} with {} MEMO", 
                top_entry.project_id, format_number(top_entry.burned_amount / 1_000_000));
    }
    
    if let Some(last_entry) = sorted_entries.last() {
        let rank = sorted_entries.len();
        println!("🎯 Rank {} threshold: {} MEMO", 
                rank, format_number(last_entry.burned_amount / 1_000_000));
    }

    // Show distribution - using sorted data
    if sorted_entries.len() >= 10 {
        println!();
        println!("📈 Distribution breakdown:");
        let top_10_total: u64 = sorted_entries.iter().take(10).map(|e| e.burned_amount).sum();
        let top_10_percentage = (top_10_total as f64 / total_burned as f64) * 100.0;
        println!("   Top 10 projects: {:.1}% of total burn", top_10_percentage);
        
        if sorted_entries.len() >= 50 {
            let top_50_total: u64 = sorted_entries.iter().take(50).map(|e| e.burned_amount).sum();
            let top_50_percentage = (top_50_total as f64 / total_burned as f64) * 100.0;
            println!("   Top 50 projects: {:.1}% of total burn", top_50_percentage);
        }
    }
}

fn display_top_projects_details(
    client: &RpcClient, 
    program_id: &Pubkey, 
    leaderboard: &BurnLeaderboard
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TOP PROJECTS DETAILS ===");
    println!("📋 Fetching details for top 5 projects...");
    println!();

    // Sort by burned_amount in descending order and get top 5
    let mut sorted_entries = leaderboard.entries.clone();
    sorted_entries.sort_by(|a, b| b.burned_amount.cmp(&a.burned_amount));
    
    let top_projects = sorted_entries.iter().take(5);
    
    for (rank, entry) in top_projects.enumerate() {
        let rank_display = rank + 1;
        let medal = match rank_display {
            1 => "🥇",
            2 => "🥈",
            3 => "🥉", 
            _ => "🏆",
        };

        println!("{} Rank {}: Project {}", medal, rank_display, entry.project_id);

        // Calculate project PDA
        let (project_pda, _) = Pubkey::find_program_address(
            &[b"project", &entry.project_id.to_le_bytes()],
            program_id,
        );

        match client.get_account(&project_pda) {
            Ok(account) => {
                if let Ok(project_info) = parse_project_basic(&account.data) {
                    println!("   📝 Name: \"{}\"", project_info.name);
                    println!("   👤 Creator: {}", project_info.creator);
                    println!("   💬 Total memos: {}", project_info.memo_count);
                    println!("   🔥 Burned: {} MEMO", format_number(entry.burned_amount / 1_000_000));
                    
                    if project_info.created_at > 0 {
                        if let Some(datetime) = DateTime::<Utc>::from_timestamp(project_info.created_at, 0) {
                            println!("   🕐 Created: {}", datetime.format("%Y-%m-%d %H:%M:%S UTC"));
                        }
                    }
                    
                    if project_info.last_updated > 0 && project_info.last_updated != project_info.created_at {
                        if let Some(datetime) = DateTime::<Utc>::from_timestamp(project_info.last_updated, 0) {
                            println!("   🕐 Updated: {}", datetime.format("%Y-%m-%d %H:%M:%S UTC"));
                        }
                    }
                    
                    if !project_info.description.is_empty() {
                        let desc = if project_info.description.len() > 50 {
                            format!("{}...", &project_info.description[..47])
                        } else {
                            project_info.description.clone()
                        };
                        println!("   📄 Description: \"{}\"", desc);
                    }

                    if !project_info.website.is_empty() {
                        println!("   🌐 Website: {}", project_info.website);
                    }

                    if !project_info.tag.is_empty() {
                        println!("   🏷️ Tag: {}", project_info.tag);
                    }
                } else {
                    println!("   ❌ Failed to parse project data");
                }
            },
            Err(_) => {
                println!("   ❌ Project account not found");
            }
        }
        
        println!();
    }

    Ok(())
}

// Basic project info struct for details display
#[derive(Debug)]
struct ProjectBasic {
    pub name: String,
    pub creator: Pubkey,
    pub created_at: i64,
    pub last_updated: i64,
    pub description: String,
    pub website: String,
    pub tag: String,
    pub memo_count: u64,
}

// Parse basic Project data (only what we need for display)
fn parse_project_basic(data: &[u8]) -> Result<ProjectBasic, Box<dyn std::error::Error>> {
    if data.len() < 8 {
        return Err("Data too short".into());
    }

    let mut offset = 8; // Skip discriminator

    // Skip project_id (u64)
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

    // Read last_updated (i64)
    if data.len() < offset + 8 {
        return Err("Data too short for last_updated".into());
    }
    let last_updated = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
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

    // Read website (String)
    let (website, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    let (tags, new_offset) = read_string_vec(data, offset)?;
    offset = new_offset;
    
    // The first tag is used for display, if there is no tag, it is an empty string
    let tag = tags.first().cloned().unwrap_or_default();

    // Read memo_count (u64) - The offset is now correct
    if data.len() < offset + 8 {
        return Err("Data too short for memo_count".into());
    }
    let memo_count = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Skip burned_amount (u64)
    offset += 8;

    // Skip last_memo_time (i64)
    offset += 8;

    Ok(ProjectBasic {
        name,
        creator,
        created_at,
        last_updated,
        description,
        website,
        tag,
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
