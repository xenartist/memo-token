use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use borsh::BorshDeserialize;
use std::error::Error;

// define SocialProfile struct for deserialization
#[derive(BorshDeserialize)]
struct SocialProfile {
    pub pubkey: Pubkey,           // 32 bytes - user pubkey
    pub username: String,         // username, max 32 characters
    pub profile_image: String,    // profile image, hex string
    pub about_me: Option<String>, // about me, max 128 characters, optional
    pub created_at: i64,          // created timestamp
    pub last_updated: i64,        // last updated timestamp
}

fn main() -> Result<(), Box<dyn Error>> {
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() != 2 {
        println!("Usage: cargo run --bin check-social-profile <PUBKEY>");
        println!("Example: cargo run --bin check-social-profile Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");
        return Ok(());
    }
    
    // parse user pubkey
    let user_pubkey = Pubkey::from_str(&args[1])?;
    
    // connect to Solana network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new_with_commitment(
        rpc_url.to_string(),
        CommitmentConfig::confirmed()
    );
    
    // memo-social program ID
    let program_id = Pubkey::from_str("CamUGqtEX8knHJ9a4jBeo3hBmdE2pWonbiFjBgyEG92q")?;
    
    // calculate social profile PDA
    let (social_profile_pda, _) = Pubkey::find_program_address(
        &[b"social_profile", user_pubkey.as_ref()],
        &program_id,
    );
    
    println!("Checking social profile for: {}", user_pubkey);
    println!("Social Profile PDA: {}", social_profile_pda);
    
    // get account data
    match client.get_account_data(&social_profile_pda) {
        Ok(data) => {
            // skip Anchor's 8-byte discriminator
            let social_profile = SocialProfile::deserialize(&mut &data[8..])?;
            
            // print social profile information
            println!("\nSocial Profile Details:");
            println!("------------------------");
            println!("Username: {}", social_profile.username);
            println!("Profile Image: {}", social_profile.profile_image);
            
            // display about_me (if exists)
            if let Some(about) = social_profile.about_me {
                println!("About Me: {}", about);
            } else {
                println!("About Me: Not set");
            }
            
            // convert and display timestamps
            let created_at = chrono::NaiveDateTime::from_timestamp_opt(social_profile.created_at, 0)
                .unwrap_or_default();
            let last_updated = chrono::NaiveDateTime::from_timestamp_opt(social_profile.last_updated, 0)
                .unwrap_or_default();
            
            println!("Created At: {}", created_at);
            println!("Last Updated: {}", last_updated);
            
            // if pixel art, try to display
            if social_profile.profile_image.starts_with("n:") || social_profile.profile_image.starts_with("c:") {
                display_pixel_art(&social_profile.profile_image);
            }
        },
        Err(err) => {
            println!("\nNo social profile found for this address.");
            println!("Error: {}", err);
        }
    }
    
    Ok(())
}

// display pixel art helper function
fn display_pixel_art(hex_string: &str) {
    if hex_string.is_empty() {
        return;
    }

    println!("\nPixel Art Representation:");
    
    // convert hex to binary
    let mut binary = String::new();
    for c in hex_string.chars() {
        if let Some(value) = c.to_digit(16) {
            binary.push_str(&format!("{:04b}", value));
        }
    }
    
    // calculate grid size (try to make it a square)
    let size = (binary.len() as f64).sqrt() as usize;
    
    // display grid
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
