// clients/src/update-user-profile.rs
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use std::str::FromStr;
use std::io::Write;
use rand::Rng;
use flate2::Compression;
use flate2::write::DeflateEncoder;
use base64::{encode};

// Using discriminator value from IDL
const UPDATE_USER_PROFILE_DISCRIMINATOR: [u8; 8] = [79, 75, 114, 130, 68, 123, 180, 11];

// generate random username
fn generate_random_username() -> String {
    let mut rng = rand::thread_rng();
    let number = rng.gen_range(0..10); // generate 0-9 random number
    format!("test{}", number)
}

fn generate_random_pixel_art() -> String {
    let mut rng = rand::thread_rng();
    let mut pixel_data = Vec::with_capacity(1024); // 32x32 pixels
    
    // generate random pixels
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
    
    let (field, value) = if args.len() < 3 {
        println!("No parameters provided, generating random values...");
        println!("Usage: cargo run --bin update-user-profile <field> <value>");
        println!("Fields: username, profile_image");
        println!("Example: cargo run --bin update-user-profile username \"NewName\"");
        println!("Example: cargo run --bin update-user-profile profile_image \"FF00FF00\"");
        println!("Note: profile_image should be a hex string representing pixel art (1=black, 0=white)");
        println!("You can use img2hex tool to convert images to hex format");
        
        // Randomly choose between username and profile_image
        let mut rng = rand::thread_rng();
        if rng.gen_bool(0.5) {
            let random_username = generate_random_username();
            println!("\nGenerating random username: {}", random_username);
            ("username".to_string(), random_username)
        } else {
            let random_art = generate_random_pixel_art();
            println!("\nGenerating random pixel art...");
            println!("Generated hex string: {}", random_art);
            ("profile_image".to_string(), random_art)
        }
    } else {
        (args[1].to_lowercase(), args[2].clone())
    };
    
    if field != "username" && field != "profile_image" {
        return Err("Invalid field. Must be 'username' or 'profile_image'".into());
    }
    
    // Validate input values
    if field == "username" && value.len() > 32 {
        return Err("Username too long. Maximum length is 32 characters.".into());
    }
    
    if field == "profile_image" {
        if value.len() > 256 {
            return Err("Profile image string too long. Maximum length is 256 characters.".into());
        }
        if !value.starts_with("n:") && !value.starts_with("c:") {
            return Err("Profile image must start with 'n:' or 'c:' prefix.".into());
        }
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
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    // Check if user profile exists
    match client.get_account(&user_profile_pda) {
        Ok(_) => {
            println!("Updating user profile for {}.", payer.pubkey());
        },
        Err(_) => {
            println!("User profile does not exist for {}.", payer.pubkey());
            println!("Please create a profile first using 'cargo run --bin init-user-profile'.");
            return Ok(());
        }
    }
    
    // Prepare instruction data using IDL discriminator
    let mut instruction_data = Vec::new();
    
    // Add discriminator
    instruction_data.extend_from_slice(&UPDATE_USER_PROFILE_DISCRIMINATOR);
    
    if field == "username" {
        // Username is Some(String)
        instruction_data.push(1); // Some variant
        instruction_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
        instruction_data.extend_from_slice(value.as_bytes());
        
        // Profile image is None
        instruction_data.push(0); // None variant
    } else {
        // Username is None
        instruction_data.push(0); // None variant
        
        // Profile image is Some(String)
        instruction_data.push(1); // Some variant
        instruction_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
        instruction_data.extend_from_slice(value.as_bytes());
    }
    
    println!("Updating {} to: {}", field, value);
    
    // Create update user profile instruction
    let update_user_profile_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true), // user (signer, writable)
            AccountMeta::new(user_profile_pda, false), // user_profile (writable)
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
        ],
    );
    
    // Set compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, update_user_profile_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    match client.send_and_confirm_transaction_with_spinner(&transaction) {
        Ok(signature) => {
            println!("User profile updated successfully!");
            println!("Transaction signature: {}", signature);
            println!("\nUpdated {} to: '{}'", field, value);
            if field == "profile_image" && !value.is_empty() {
                display_pixel_art(&value);
            }
            println!("\nTo view your profile, run: cargo run --bin check-user-profile");
        },
        Err(err) => {
            println!("Error updating user profile: {}", err);
            return Err(err.into());
        }
    }
    
    Ok(())
}