// clients/src/check-user-profile.rs
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
};
use std::str::FromStr;
use std::io::{Cursor, Read};
use std::time::{SystemTime, UNIX_EPOCH};

fn display_pixel_art(hex_string: &str) {
    if hex_string.is_empty() {
        return;
    }

    println!("\nPixel Art Representation:");
    
    // Convert hex to binary
    let mut binary = String::new();
    for c in hex_string.chars() {
        let value = c.to_digit(16).unwrap();
        binary.push_str(&format!("{:04b}", value));
    }
    
    // Calculate grid size (try to make it square)
    let size = (binary.len() as f64).sqrt() as usize;
    
    // Display the grid
    let mut i = 0;
    for _ in 0..size {
        for _ in 0..size {
            if i < binary.len() {
                print!("{}", if &binary[i..i+1] == "1" { "⬛" } else { "⬜" });
                i += 1;
            }
        }
        println!();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse target pubkey (optional)
    let target_pubkey = if args.len() > 1 {
        Pubkey::from_str(&args[1])?
    } else {
        // If not provided, use the local wallet
        let wallet = read_keypair_file(
            shellexpand::tilde("~/.config/solana/id.json").to_string()
        ).expect("Failed to read keypair file");
        wallet.pubkey()
    };
    
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);
    
    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    
    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", target_pubkey.as_ref()],
        &program_id,
    );
    
    println!("Checking user profile for: {}", target_pubkey);
    println!("User profile PDA: {}", user_profile_pda);
    
    // Retrieve user profile account
    match client.get_account(&user_profile_pda) {
        Ok(account) => {
            println!("User profile found!");
            
            // Skip account discriminator (first 8 bytes)
            let mut data = &account.data[8..];
            
            // Read pubkey (32 bytes)
            let pubkey = Pubkey::new(&data[..32]);
            data = &data[32..];
            
            // Read username
            let username_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            data = &data[4..];
            let username = String::from_utf8(data[..username_len].to_vec())?;
            data = &data[username_len..];
            
            // Read stats
            let total_minted = u64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            data = &data[8..];
            
            let total_burned = u64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            data = &data[8..];
            
            let mint_count = u64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            data = &data[8..];
            
            let burn_count = u64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            data = &data[8..];
            
            // Read profile image
            let profile_image_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            data = &data[4..];
            let profile_image = String::from_utf8(data[..profile_image_len].to_vec())?;
            data = &data[profile_image_len..];
            
            // Read timestamps
            let created_at = i64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            data = &data[8..];
            
            let last_updated = i64::from_le_bytes([
                data[0], data[1], data[2], data[3], 
                data[4], data[5], data[6], data[7]
            ]);
            
            // Format timestamps as readable dates
            let format_timestamp = |timestamp: i64| -> String {
                let dt = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(timestamp as u64);
                let datetime = dt.duration_since(UNIX_EPOCH).unwrap();
                
                let secs = datetime.as_secs();
                let days = secs / 86400;
                let hours = (secs % 86400) / 3600;
                let minutes = (secs % 3600) / 60;
                let seconds = secs % 60;
                
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                    1970 + (days / 365),              // Year (approximate)
                    ((days % 365) / 30) + 1,          // Month (approximate)
                    ((days % 365) % 30) + 1,          // Day (approximate)
                    hours,
                    minutes,
                    seconds
                )
            };
            
            // Display user profile information
            println!("\n==== USER PROFILE ====");
            println!("Username: {}", username);
            println!("Profile Image (hex): {}", if profile_image.is_empty() { "None" } else { &profile_image });
            if !profile_image.is_empty() {
                display_pixel_art(&profile_image);
            }
            println!("\n==== TOKEN STATISTICS ====");
            println!("Total Minted: {} tokens", total_minted);
            println!("Total Burned: {} tokens", total_burned);
            println!("Net Balance from Mint/Burn: {} tokens", (total_minted as i64 - total_burned as i64));
            println!("Mint Operations: {}", mint_count);
            println!("Burn Operations: {}", burn_count);
            println!("\n==== ACCOUNT INFO ====");
            println!("Owner: {}", pubkey);
            println!("Created: {}", format_timestamp(created_at));
            println!("Last Updated: {}", format_timestamp(last_updated));
            println!("Account Size: {} bytes", account.data.len());
            println!("Rent Exempt Balance: {} lamports", account.lamports);
        },
        Err(_) => {
            println!("No user profile found for {}.", target_pubkey);
            println!("To create a profile, use 'cargo run --bin init-user-profile <username> [profile_image_hex]'");
        }
    }
    
    Ok(())
}