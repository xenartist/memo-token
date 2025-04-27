use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use std::str::FromStr;
use borsh::{BorshSerialize, BorshDeserialize};
use std::io::Write;
use rand::Rng;
use flate2::Compression;
use flate2::write::DeflateEncoder;
use base64::{encode};

// memo-social initialize_social_profile discriminator
const INIT_SOCIAL_PROFILE_DISCRIMINATOR: [u8; 8] = [145, 188, 54, 68, 87, 82, 23, 87];

fn generate_random_pixel_art() -> String {
    let mut rng = rand::thread_rng();
    let mut pixel_data = Vec::with_capacity(1024); // 32x32 pixels
    
    // generate random pixel art
    for _ in 0..32 {
        for _ in 0..32 {
            pixel_data.push(rng.gen_bool(0.5));
        }
    }
    
    // convert to safe string
    let mut result = String::with_capacity(171);
    let mut current_bits = 0u8;
    let mut bit_count = 0;

    for &pixel in &pixel_data {
        current_bits = (current_bits << 1) | (pixel as u8);
        bit_count += 1;

        if bit_count == 6 {
            result.push(map_to_safe_char(current_bits));
            current_bits = 0;
            bit_count = 0;
        }
    }

    if bit_count > 0 {
        current_bits <<= (6 - bit_count);
        result.push(map_to_safe_char(current_bits));
    }

    // try to compress
    match compress_with_deflate(&result) {
        Ok(compressed) => {
            if compressed.len() + 2 < result.len() {
                format!("c:{}", compressed)
            } else {
                format!("n:{}", result)
            }
        }
        Err(_) => format!("n:{}", result)
    }
}

fn map_to_safe_char(value: u8) -> char {
    assert!(value < 64, "Value must be less than 64");
    let mut ascii = 35 + value;  // start from ASCII 35
    
    // skip ':' and '\'
    if ascii >= 58 { ascii += 1; }  // skip ':'
    if ascii >= 92 { ascii += 1; }  // skip '\'
    
    ascii as char
}

fn compress_with_deflate(input: &str) -> Result<String, Box<dyn std::error::Error>> {
    let bytes: Vec<u8> = input.chars()
        .map(|c| c as u8)
        .collect();
    
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&bytes)?;
    let compressed = encoder.finish()?;
    Ok(encode(compressed))
}

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
    
    // Parse username (required)
    if args.len() < 2 {
        println!("Usage: cargo run --bin init-social-profile <username> [profile_image_hex] [about_me]");
        println!("Example: cargo run --bin init-social-profile \"SolanaUser\" \"FF00FF00\" \"Hello, I'm a Solana developer\"");
        println!("Note: profile_image_hex should be a hex string representing pixel art (1=black, 0=white)");
        println!("You can use img2hex tool to convert images to hex format");
        println!("If no profile_image_hex is provided, a random pattern will be generated");
        println!("About me is optional and can be added to describe yourself (max 128 characters)");
        return Err("Username is required".into());
    }
    
    // Parse username
    let username = args[1].clone();
    if username.len() > 32 {
        return Err("Username too long. Maximum length is 32 characters.".into());
    }
    
    // Parse or generate profile image hex string
    let profile_image = if args.len() > 2 && !args[2].starts_with("-") {
        args[2].clone()
    } else {
        println!("No profile image provided, generating random pixel art...");
        let random_art = generate_random_pixel_art();
        println!("Generated random pixel art hex string: {}", random_art);
        random_art
    };
    
    // Check profile image hex string length
    if profile_image.len() > 256 {
        return Err("Profile image string too long. Maximum length is 256 characters.".into());
    }

    // Check format prefix
    if !profile_image.starts_with("n:") && !profile_image.starts_with("c:") {
        return Err("Profile image must start with 'n:' or 'c:' prefix.".into());
    }
    
    // Parse about_me (optional)
    let about_me = if args.len() > 3 {
        let about_text = args[3].clone();
        if about_text.len() > 128 {
            return Err("About me too long. Maximum length is 128 characters.".into());
        }
        Some(about_text)
    } else {
        None
    };
    
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program ID - memo-social program id
    let program_id = Pubkey::from_str("CamUGqtEX8knHJ9a4jBeo3hBmdE2pWonbiFjBgyEG92q")
        .expect("Invalid program ID");
    
    // Calculate social profile PDA
    let (social_profile_pda, bump) = Pubkey::find_program_address(
        &[b"social_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    println!("Social profile PDA: {}", social_profile_pda);
    
    // Check if social profile already exists
    match client.get_account(&social_profile_pda) {
        Ok(_) => {
            println!("Social profile already exists for {}.", payer.pubkey());
            println!("You can update it using 'cargo run --bin update-social-profile'.");
            return Ok(());
        },
        Err(_) => {
            println!("Creating new social profile for {}.", payer.pubkey());
        }
    }
    
    // Prepare instruction data using IDL discriminator
    let mut instruction_data = Vec::new();
    
    // Write discriminator
    instruction_data.extend_from_slice(&INIT_SOCIAL_PROFILE_DISCRIMINATOR);
    
    // Serialize username parameter (String)
    instruction_data.extend_from_slice(&(username.len() as u32).to_le_bytes());
    instruction_data.extend_from_slice(username.as_bytes());
    
    // Serialize profile_image parameter (String)
    instruction_data.extend_from_slice(&(profile_image.len() as u32).to_le_bytes());
    instruction_data.extend_from_slice(profile_image.as_bytes());
    
    // Serialize about_me parameter (Option<String>)
    if let Some(about_text) = &about_me {
        // Some variant (1)
        instruction_data.push(1);
        // String content
        instruction_data.extend_from_slice(&(about_text.len() as u32).to_le_bytes());
        instruction_data.extend_from_slice(about_text.as_bytes());
    } else {
        // None variant (0)
        instruction_data.push(0);
    }
    
    // Calculate required space for the account
    let space = 8 +    // discriminator
               32 +    // pubkey
               4 + username.len() + // username (String)
               4 + profile_image.len() + // profile_image (String)
               1 + (about_me.as_ref().map_or(0, |a| 4 + a.len())) + // about_me (Option<String>)
               8 +     // created_at
               8;      // last_updated
    
    // Calculate rent exempt minimum
    let rent = client.get_minimum_balance_for_rent_exemption(space)?;
    
    // Print initialization details
    println!("\nInitializing social profile with the following details:");
    println!("Username: {}", username);
    println!("Profile Image: {}", profile_image);
    if let Some(about_text) = &about_me {
        println!("About Me: {}", about_text);
    } else {
        println!("About Me: None");
    }
    println!("Account Space: {} bytes", space);
    println!("Required Rent (lamports): {}", rent);
    
    // Create initialize social profile instruction
    let init_social_profile_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true), // user (signer, writable)
            AccountMeta::new(social_profile_pda, false), // social_profile (writable)
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
        ],
    );
    
    // Set compute budget to avoid out of compute errors
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, init_social_profile_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    println!("Sending transaction...");
    
    match client.send_and_confirm_transaction_with_spinner(&transaction) {
        Ok(signature) => {
            println!("\nTransaction was successful!");
            println!("Signature: {}", signature);
            println!("Social profile created successfully for: {}", username);
            println!("Your social profile PDA: {}", social_profile_pda);
        },
        Err(err) => {
            println!("Error: {}", err);
            return Err(err.into());
        }
    }
    
    Ok(())
}
