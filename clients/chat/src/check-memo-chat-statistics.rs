use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use chrono::{DateTime, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-CHAT STATISTICS CHECKER ===");
    println!("Checking global statistics and all group information...");
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

    // 1. Check global counter statistics
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_chat_program_id,
    );

    println!("=== GLOBAL STATISTICS ===");
    println!("🌐 Global counter PDA: {}", global_counter_pda);

    let total_groups = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            println!("✅ Global counter found");
            println!("   Owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            println!("   Rent exempt: {} SOL", account.lamports as f64 / 1_000_000_000.0);

            if account.owner != memo_chat_program_id {
                println!("⚠️  Warning: Account not owned by memo-chat program!");
                return Ok(());
            }

            if account.data.len() >= 16 {
                let total_groups_bytes = &account.data[8..16];
                let total_groups = u64::from_le_bytes(total_groups_bytes.try_into().unwrap());
                println!("   Total groups created: {}", total_groups);
                total_groups
            } else {
                println!("❌ Invalid account data size");
                return Ok(());
            }
        },
        Err(e) => {
            println!("❌ Global counter not found: {}", e);
            println!("💡 Please run admin-init-global-group-counter first to initialize the system.");
            return Ok(());
        }
    };

    println!();

    if total_groups == 0 {
        println!("📭 No groups have been created yet.");
        println!("💡 Users can create groups by calling the create_chat_group instruction.");
        return Ok(());
    }

    // 2. Iterate through all groups and display information
    println!("=== GROUP LISTING ({} groups) ===", total_groups);
    println!();

    let mut valid_groups = 0;
    let mut total_memos = 0;
    let mut total_burned_tokens = 0;

    for group_id in 0..total_groups {
        println!("📁 Group ID: {}", group_id);

        // Calculate group PDA
        let (chat_group_pda, _) = Pubkey::find_program_address(
            &[b"chat_group", &group_id.to_le_bytes()],
            &memo_chat_program_id,
        );

        println!("   PDA: {}", chat_group_pda);

        match client.get_account(&chat_group_pda) {
            Ok(account) => {
                if account.owner != memo_chat_program_id {
                    println!("   ❌ Invalid owner: {}", account.owner);
                    continue;
                }

                println!("   ✅ Account found ({} bytes)", account.data.len());

                // Parse group data
                if let Ok(group_info) = parse_chat_group_data(&account.data) {
                    valid_groups += 1;
                    total_memos += group_info.memo_count;
                    total_burned_tokens += group_info.burned_amount;

                    println!("   📝 Name: \"{}\"", group_info.name);
                    println!("   👤 Creator: {}", group_info.creator);
                    
                    // Convert timestamp to human readable
                    if group_info.created_at > 0 {
                        let dt = DateTime::<Utc>::from_timestamp(group_info.created_at, 0);
                        if let Some(datetime) = dt {
                            println!("   🕐 Created: {} UTC", datetime.format("%Y-%m-%d %H:%M:%S"));
                        } else {
                            println!("   🕐 Created: {} (timestamp)", group_info.created_at);
                        }
                    }

                    if !group_info.description.is_empty() {
                        println!("   📄 Description: \"{}\"", group_info.description);
                    }

                    if !group_info.image.is_empty() {
                        println!("   🖼️  Image: \"{}\"", group_info.image);
                    }

                    if !group_info.tags.is_empty() {
                        println!("   🏷️  Tags: {}", group_info.tags.join(", "));
                    }

                    println!("   💬 Memo count: {}", group_info.memo_count);
                    println!("   🔥 Burned tokens: {} MEMO", group_info.burned_amount / 1_000_000);
                    println!("   ⏱️  Min memo interval: {} seconds", group_info.min_memo_interval);

                    if group_info.last_memo_time > 0 {
                        let dt = DateTime::<Utc>::from_timestamp(group_info.last_memo_time, 0);
                        if let Some(datetime) = dt {
                            println!("   💌 Last memo: {} UTC", datetime.format("%Y-%m-%d %H:%M:%S"));
                        } else {
                            println!("   💌 Last memo: {} (timestamp)", group_info.last_memo_time);
                        }
                    } else {
                        println!("   💌 Last memo: Never");
                    }

                    println!("   🎯 PDA bump: {}", group_info.bump);
                } else {
                    println!("   ❌ Failed to parse group data");
                }
            },
            Err(e) => {
                println!("   ❌ Group not found: {}", e);
            }
        }

        println!();
    }

    // 3. Display summary statistics
    println!("=== SUMMARY STATISTICS ===");
    println!("📊 Total groups in counter: {}", total_groups);
    println!("✅ Valid groups found: {}", valid_groups);
    println!("💬 Total memos across all groups: {}", total_memos);
    println!("🔥 Total tokens burned: {} MEMO", total_burned_tokens / 1_000_000);

    if valid_groups > 0 {
        println!("📈 Average memos per group: {:.2}", total_memos as f64 / valid_groups as f64);
        println!("🔥 Average tokens burned per group: {:.2} MEMO", 
                 (total_burned_tokens / 1_000_000) as f64 / valid_groups as f64);
    }

    if valid_groups < total_groups {
        println!();
        println!("⚠️  Warning: {} groups are missing or invalid", total_groups - valid_groups);
        println!("💡 This might indicate partially failed group creations.");
    }

    Ok(())
}

// Struct to hold parsed group data
#[derive(Debug)]
struct ChatGroupInfo {
    pub group_id: u64,
    pub creator: Pubkey,
    pub created_at: i64,
    pub name: String,
    pub description: String,
    pub image: String,
    pub tags: Vec<String>,
    pub memo_count: u64,
    pub burned_amount: u64,
    pub min_memo_interval: i64,
    pub last_memo_time: i64,
    pub bump: u8,
}

// Parse ChatGroup account data
fn parse_chat_group_data(data: &[u8]) -> Result<ChatGroupInfo, Box<dyn std::error::Error>> {
    if data.len() < 8 {
        return Err("Data too short for discriminator".into());
    }

    let mut offset = 8; // Skip discriminator

    // Read group_id (u64)
    if data.len() < offset + 8 {
        return Err("Data too short for group_id".into());
    }
    let group_id = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read creator (Pubkey = 32 bytes) - Fixed type annotation
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

    // Read image (String)
    let (image, new_offset) = read_string(data, offset)?;
    offset = new_offset;

    // Read tags (Vec<String>)
    let (tags, new_offset) = read_string_vec(data, offset)?;
    offset = new_offset;

    // Read memo_count (u64)
    if data.len() < offset + 8 {
        return Err("Data too short for memo_count".into());
    }
    let memo_count = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read burned_amount (u64)
    if data.len() < offset + 8 {
        return Err("Data too short for burned_amount".into());
    }
    let burned_amount = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read min_memo_interval (i64)
    if data.len() < offset + 8 {
        return Err("Data too short for min_memo_interval".into());
    }
    let min_memo_interval = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read last_memo_time (i64)
    if data.len() < offset + 8 {
        return Err("Data too short for last_memo_time".into());
    }
    let last_memo_time = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
    offset += 8;

    // Read bump (u8)
    if data.len() < offset + 1 {
        return Err("Data too short for bump".into());
    }
    let bump = data[offset];

    Ok(ChatGroupInfo {
        group_id,
        creator,
        created_at,
        name,
        description,
        image,
        tags,
        memo_count,
        burned_amount,
        min_memo_interval,
        last_memo_time,
        bump,
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