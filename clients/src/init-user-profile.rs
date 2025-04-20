// clients/src/init-user-profile.rs
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

// Using discriminator value from IDL
const INIT_USER_PROFILE_DISCRIMINATOR: [u8; 8] = [192, 144, 204, 140, 113, 25, 59, 102];

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
        println!("Usage: cargo run --bin init-user-profile <username> [profile_image_hex]");
        println!("Example: cargo run --bin init-user-profile \"SolanaUser\" \"FF00FF00\"");
        println!("Note: profile_image_hex should be a hex string representing pixel art (1=black, 0=white)");
        println!("You can use img2hex tool to convert images to hex format");
        println!("If no profile_image_hex is provided, a random pattern will be generated");
        return Err("Username is required".into());
    }
    
    let username = args[1].clone();
    if username.len() > 32 {
        return Err("Username too long. Maximum length is 32 characters.".into());
    }
    
    // Parse or generate profile image hex string
    let profile_image = if args.len() > 2 {
        args[2].clone()
    } else {
        println!("No profile image provided, generating random pixel art...");
        let random_art = generate_random_pixel_art();
        println!("Generated random pixel art hex string: {}", random_art);
        random_art
    };
    
    // check profile image hex string length
    if profile_image.len() > 256 {
        return Err("Profile image string too long. Maximum length is 256 characters.".into());
    }

    // check format prefix
    if !profile_image.starts_with("n:") && !profile_image.starts_with("c:") {
        return Err("Profile image must start with 'n:' or 'c:' prefix.".into());
    }
    
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    
    // Calculate user profile PDA
    let (user_profile_pda, bump) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    println!("User profile PDA: {}", user_profile_pda);
    
    // Check if user profile already exists
    match client.get_account(&user_profile_pda) {
        Ok(_) => {
            println!("User profile already exists for {}.", payer.pubkey());
            println!("You can update it using 'cargo run --bin update-user-profile'.");
            return Ok(());
        },
        Err(_) => {
            println!("Creating new user profile for {}.", payer.pubkey());
        }
    }
    
    // Prepare instruction data using IDL discriminator
    // 1. Create a buffer to store instruction data
    let mut instruction_data = Vec::with_capacity(
        INIT_USER_PROFILE_DISCRIMINATOR.len() + 
        4 + username.len() + 
        4 + profile_image.len()
    );
    
    // 2. Write discriminator
    instruction_data.extend_from_slice(&INIT_USER_PROFILE_DISCRIMINATOR);
    
    // 3. Serialize username parameter (String)
    instruction_data.extend_from_slice(&(username.len() as u32).to_le_bytes());
    instruction_data.extend_from_slice(username.as_bytes());
    
    // 4. Serialize profile_image parameter (String)
    instruction_data.extend_from_slice(&(profile_image.len() as u32).to_le_bytes());
    instruction_data.extend_from_slice(profile_image.as_bytes());
    
    // Calculate required space for the account
    let space = 8 + // discriminator
                32 + // pubkey
                4 + username.len() + // username (String)
                8 + // total_minted
                8 + // total_burned
                8 + // mint_count
                8 + // burn_count
                4 + profile_image.len() + // profile_image (String)
                8 + // created_at
                8 + // last_updated
                9;  // latest_burn_history_index (Option<u64>: 1 byte for Option + 8 bytes for u64)
    
    // Calculate rent exempt minimum
    let rent = client.get_minimum_balance_for_rent_exemption(space)?;
    
    // Print initialization details
    println!("\nInitializing user profile with the following details:");
    println!("Username: {}", username);
    println!("Profile Image: {}", profile_image);
    println!("Account Space: {} bytes", space);
    println!("Required Rent (lamports): {}", rent);
    println!("Burn History Index: None (will be set when first burn history is created)");
    
    // Create initialize user profile instruction
    let init_user_profile_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true), // user (signer, writable)
            AccountMeta::new(user_profile_pda, false), // user_profile (writable)
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
        ],
    );
    
    // Set compute budget to avoid out of compute errors
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, init_user_profile_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    match client.send_and_confirm_transaction_with_spinner(&transaction) {
        Ok(signature) => {
            println!("User profile initialized successfully!");
            println!("Transaction signature: {}", signature);
            println!("\nUser profile details:");
            println!("Owner: {}", payer.pubkey());
            println!("Username: {}", username);
            println!("Profile Image: {}", profile_image);
            println!("\nYou can now use your profile in mint and burn operations.");
            println!("The profile will automatically track your token statistics.");
        },
        Err(err) => {
            println!("Error initializing user profile: {}", err);
            println!("Common issues:");
            println!("1. Insufficient funds for account creation");
            println!("2. Network connectivity issues");
            return Err(err.into());
        }
    }
    
    Ok(())
}