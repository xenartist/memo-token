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

// Using discriminator value from IDL
const INIT_USER_PROFILE_DISCRIMINATOR: [u8; 8] = [192, 144, 204, 140, 113, 25, 59, 102];

fn generate_random_pixel_art() -> String {
    let mut rng = rand::thread_rng();
    let mut hex_string = String::with_capacity(256);
    
    for _ in 0..256 {
        let hex_char = format!("{:X}", rng.gen_range(0..16));
        hex_string.push_str(&hex_char);
    }
    
    hex_string
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
        return Err("Profile image hex string too long. Maximum length is 256 characters.".into());
    }

    // check profile image hex string format
    if !profile_image.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Profile image must be a valid hexadecimal string (0-9, A-F).".into());
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
                8; // last_updated
    
    // Calculate rent exempt minimum
    let rent = client.get_minimum_balance_for_rent_exemption(space)?;
    
    // Print initialization details
    println!("\nInitializing user profile with the following details:");
    println!("Username: {}", username);
    println!("Profile Image (hex): {}", if profile_image.is_empty() { "None" } else { &profile_image });
    display_pixel_art(&profile_image);
    println!("Account Space: {} bytes", space);
    println!("Required Rent (lamports): {}", rent);
    
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
            println!("Profile Image: {}", if profile_image.is_empty() { "None" } else { &profile_image });
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