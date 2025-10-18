use solana_client::{
    rpc_client::RpcClient,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;

// Get RPC URL from environment or use default testnet
fn get_rpc_url() -> String {
    std::env::var("X1_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let command = &args[1];
    
    match command.as_str() {
        "own" => check_own_profile(),
        "user" => {
            if args.len() < 3 {
                println!("Usage: check-memo-profile user <pubkey>");
                return Ok(());
            }
            let user_pubkey = Pubkey::from_str(&args[2])?;
            check_user_profile(user_pubkey)
        },
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn check_own_profile() -> Result<(), Box<dyn std::error::Error>> {
    // Constants
    let rpc_url = get_rpc_url();
    let wallet_path = std::env::var("WALLET_PATH").unwrap_or_else(|_| {
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        format!("{}/.config/solana/id.json", home)
    });

    println!("=== CHECK OWN MEMO PROFILE ===");
    println!("RPC URL: {}", rpc_url);
    println!("Wallet: {}", wallet_path);

    // Create RPC client
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load wallet
    let wallet_path_expanded = shellexpand::tilde(&wallet_path).to_string();
    let payer = read_keypair_file(&wallet_path_expanded)
        .map_err(|e| format!("Failed to read keypair from {}: {}", wallet_path_expanded, e))?;

    println!("User: {}", payer.pubkey());

    check_user_profile(payer.pubkey())
}

fn check_user_profile(user_pubkey: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    // Constants
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string());
    
    println!("=== CHECK MEMO PROFILE ===");
    println!("User: {}", user_pubkey);

    // Create RPC client
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Program IDs
    let memo_profile_program_id = Pubkey::from_str("BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US")?;

    // Derive profile PDA
    let (profile_pda, profile_bump) = Pubkey::find_program_address(
        &[b"profile", user_pubkey.as_ref()],
        &memo_profile_program_id,
    );

    println!("Profile PDA: {}", profile_pda);
    println!("Expected Bump: {}", profile_bump);
    println!();

    // Fetch profile account
    match client.get_account(&profile_pda) {
        Ok(account) => {
            println!("✅ Profile Found!");
            println!("Account Details:");
            println!("  Address: {}", profile_pda);
            println!("  Owner: {}", account.owner);
            println!("  Data Length: {} bytes", account.data.len());
            println!("  Lamports: {}", account.lamports);
            println!();

            // Verify account owner
            if account.owner != memo_profile_program_id {
                println!("❌ Error: Account not owned by memo-profile program!");
                println!("   Expected: {}", memo_profile_program_id);
                println!("   Actual: {}", account.owner);
                return Ok(());
            }

            // Try to decode the profile data
            if account.data.len() > 8 {
                // Skip the 8-byte discriminator
                let profile_data = &account.data[8..];
                
                println!("Attempting to parse profile data...");
                println!("Profile data length (without discriminator): {} bytes", profile_data.len());
                
                match parse_profile_data(profile_data) {
                    Ok(profile) => {
                        println!("✅ Profile Data Successfully Parsed:");
                        display_profile_info(&profile, user_pubkey, profile_bump);
                    }
                    Err(e) => {
                        println!("❌ Failed to parse profile data: {}", e);
                        
                        // Show raw data for debugging
                        println!();
                        println!("Raw profile data (hex dump):");
                        for (i, chunk) in profile_data.chunks(32).enumerate() {
                            let hex_chunk = chunk.iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            println!("  {:04x}: {}", i * 32, hex_chunk);
                        }
                    }
                }
            } else {
                println!("❌ Account data too small to contain profile information");
            }
        }
        Err(e) => {
            println!("❌ Profile Not Found");
            println!("Error: {}", e);
            println!();
            println!("This user has not created a profile yet.");
        }
    }

    Ok(())
}

#[derive(Debug)]
struct Profile {
    pub user: Pubkey,
    pub username: String,
    pub image: String,
    pub created_at: i64,
    pub last_updated: i64,
    pub about_me: Option<String>,
    pub bump: u8,
}

fn parse_profile_data(data: &[u8]) -> Result<Profile, Box<dyn std::error::Error>> {
    let mut offset = 0;
    
    // Parse user (32 bytes)
    if data.len() < offset + 32 {
        return Err("Not enough data for user".into());
    }
    let user = Pubkey::new_from_array(
        data[offset..offset + 32].try_into()
            .map_err(|_| "Invalid user data")?
    );
    offset += 32;
    
    // Parse username (4 bytes length + string data)
    if data.len() < offset + 4 {
        return Err("Not enough data for username length".into());
    }
    let username_len = u32::from_le_bytes(
        data[offset..offset + 4].try_into().unwrap()
    ) as usize;
    offset += 4;
    
    if data.len() < offset + username_len {
        return Err(format!("Not enough data for username (need {} bytes)", username_len).into());
    }
    let username = String::from_utf8(data[offset..offset + username_len].to_vec())
        .map_err(|_| "Invalid UTF-8 in username")?;
    offset += username_len;
    
    // Parse image (4 bytes length + string data)
    if data.len() < offset + 4 {
        return Err("Not enough data for image length".into());
    }
    let image_len = u32::from_le_bytes(
        data[offset..offset + 4].try_into().unwrap()
    ) as usize;
    offset += 4;
    
    if data.len() < offset + image_len {
        return Err(format!("Not enough data for image (need {} bytes)", image_len).into());
    }
    let image = String::from_utf8(data[offset..offset + image_len].to_vec())
        .map_err(|_| "Invalid UTF-8 in image")?;
    offset += image_len;
    
    // Parse created_at (8 bytes)
    if data.len() < offset + 8 {
        return Err("Not enough data for created_at".into());
    }
    let created_at = i64::from_le_bytes(
        data[offset..offset + 8].try_into().unwrap()
    );
    offset += 8;
    
    // Parse last_updated (8 bytes)
    if data.len() < offset + 8 {
        return Err("Not enough data for last_updated".into());
    }
    let last_updated = i64::from_le_bytes(
        data[offset..offset + 8].try_into().unwrap()
    );
    offset += 8;
    
    // Parse about_me (1 byte variant + optional string)
    if data.len() < offset + 1 {
        return Err("Not enough data for about_me variant".into());
    }
    let about_me_variant = data[offset];
    offset += 1;
    
    let about_me = if about_me_variant == 1 {
        // Some(String)
        if data.len() < offset + 4 {
            return Err("Not enough data for about_me length".into());
        }
        let about_me_len = u32::from_le_bytes(
            data[offset..offset + 4].try_into().unwrap()
        ) as usize;
        offset += 4;
        
        if data.len() < offset + about_me_len {
            return Err(format!("Not enough data for about_me (need {} bytes)", about_me_len).into());
        }
        let about_me_str = String::from_utf8(data[offset..offset + about_me_len].to_vec())
            .map_err(|_| "Invalid UTF-8 in about_me")?;
        offset += about_me_len;
        Some(about_me_str)
    } else {
        // None
        None
    };
    
    // Parse bump (1 byte)
    if data.len() < offset + 1 {
        return Err("Not enough data for bump".into());
    }
    let bump = data[offset];
    offset += 1;
    
    println!("Debug: Parsed {} bytes out of {} total bytes", offset, data.len());
    
    if offset != data.len() {
        println!("⚠️  Warning: {} bytes remaining after parsing", data.len() - offset);
    }
    
    Ok(Profile {
        user,
        username,
        image,
        created_at,
        last_updated,
        about_me,
        bump,
    })
}

fn display_profile_info(profile: &Profile, expected_user: Pubkey, expected_bump: u8) {
    println!("  User: {}", profile.user);
    println!("  Username: '{}'", profile.username);
    println!("  Image: '{}'", if profile.image.is_empty() { "(empty)" } else { &profile.image });
    
    // Format timestamps
    let created_time = chrono::DateTime::from_timestamp(profile.created_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| format!("Invalid timestamp: {}", profile.created_at));
    
    let updated_time = chrono::DateTime::from_timestamp(profile.last_updated, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| format!("Invalid timestamp: {}", profile.last_updated));
    
    println!("  Created At: {} ({})", profile.created_at, created_time);
    println!("  Last Updated: {} ({})", profile.last_updated, updated_time);
    
    match &profile.about_me {
        Some(about) => println!("  About Me: '{}'", about),
        None => println!("  About Me: (not set)"),
    }
    
    println!("  PDA Bump: {}", profile.bump);
    
    // Verify the user matches
    if profile.user == expected_user {
        println!("✅ Profile user verification passed");
    } else {
        println!("❌ Profile user mismatch! Expected: {}, Found: {}", 
                 expected_user, profile.user);
    }
    
    // Verify the bump
    if profile.bump == expected_bump {
        println!("✅ PDA bump verification passed");
    } else {
        println!("❌ PDA bump mismatch! Expected: {}, Found: {}", 
                 expected_bump, profile.bump);
    }
}

fn print_usage() {
    println!("Usage: check-memo-profile <command>");
    println!();
    println!("Commands:");
    println!("  own                    - Check your own profile");
    println!("  user <pubkey>          - Check a specific user's profile");
    println!();
    println!("Environment Variables:");
    println!("  RPC_URL      - Solana RPC endpoint (default: testnet)");
    println!("  WALLET_PATH  - Path to wallet keypair file (for 'own' command)");
    println!();
    println!("Examples:");
    println!("  check-memo-profile own");
    println!("  check-memo-profile user 11111111111111111111111111111111");
}
