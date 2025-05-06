use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Function to display pixel art in console with emoji square pixels
fn display_pixel_art(hex_string: &str) {
    println!("\nPixel Art Representation:");
    
    // Convert hex to binary
    let mut binary_grid = vec![vec![0; 50]; 50];
    let mut hex_index = 0;
    
    for row in 0..50 {
        for col in 0..50 {
            if col % 4 == 0 {
                if hex_index >= hex_string.len() {
                    break;
                }
                
                // Get next hex character
                if let Some(hex_char) = hex_string.chars().nth(hex_index) {
                    hex_index += 1;
                    
                    // Convert hex to decimal
                    if let Ok(decimal) = u8::from_str_radix(&hex_char.to_string(), 16) {
                        // Convert to 4 bits
                        let bits = format!("{:04b}", decimal);
                        
                        // Set the 4 pixels
                        for (i, bit) in bits.chars().enumerate() {
                            if col + i < 50 {
                                binary_grid[row][col + i] = if bit == '1' { 1 } else { 0 };
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Display the grid with emoji squares
    for row in &binary_grid {
        for &cell in row {
            // Use black square emoji for filled pixels, white square for empty
            print!("{}", if cell == 1 { "⬛" } else { "⬜" });
        }
        println!();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse compute units from args (default: 440_000)
    // Now it's the first parameter
    let compute_units = if args.len() > 1 {
        match args[1].parse::<u32>() {
            Ok(cu) => cu,
            Err(_) => {
                // If first arg can't be parsed as a number, assume it's a memo
                // and use default CU
                440_000
            }
        }
    } else {
        440_000 // Default CU limit
    };
    
    // default fake signature
    let default_signature = "2ZaXvNKVY8DbqZitNHAYRmqvqD6cBupCJmYY6rDnP5XzY7FPPpyVzKGdNhfXUWnz2J2zU6SK8J2WZPTdJA5eSNoK";

    // Parse memo from args
    let (message, signature) = if args.len() > 1 {
        // Check if first arg is a number (CU)
        if args[1].parse::<u32>().is_ok() {
            // First arg is CU, check if there's a second arg for memo
            if args.len() > 2 {
                // check if signature separator "|" is included
                if args[2].contains("|") {
                    let parts: Vec<&str> = args[2].split("|").collect();
                    (parts[0].to_string(), parts[1].to_string())
                } else {
                    (args[2].clone(), default_signature.to_string())
                }
            } else {
                // No memo provided, use default
                (String::from("Default message for memo"), default_signature.to_string())
            }
        } else {
            // First arg is not a number, assume it's a memo
            if args[1].contains("|") {
                let parts: Vec<&str> = args[1].split("|").collect();
                (parts[0].to_string(), parts[1].to_string())
            } else {
                (args[1].clone(), default_signature.to_string())
            }
        }
    } else {
        // No args provided, use default memo
        (String::from("Default message for memo"), default_signature.to_string())
    };

    // build JSON format memo
    let memo_json = serde_json::json!({
        "signature": signature,
        "message": message
    });
    
    let memo_text = memo_json.to_string();

    // ensure memo length is at least 69 bytes
    let memo_text = ensure_min_length(memo_text, 69);

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and mint addresses
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
        .expect("Invalid mint address");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Use token-2022 version of get_associated_token_address
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),  // Use token-2022 program ID
    );

    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );

    // Check if user profile exists
    let user_profile_exists = match client.get_account(&user_profile_pda) {
        Ok(_) => {
            println!("User profile found at: {}", user_profile_pda);
            true
        },
        Err(_) => {
            println!("No user profile found. The mint will succeed but won't track statistics.");
            println!("To create a profile, use 'cargo run --bin init-user-profile <username> [profile_image_url]'");
            false
        }
    };

    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

    // Print memo length and CU information
    let memo_length = memo_text.as_bytes().len();
    println!("Memo length: {} bytes", memo_length);
    println!("Memo content: {}", memo_text);
    println!("Setting compute budget: {} CUs", compute_units);

    // Create compute budget instruction to set CU limit
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // Create mint instruction - use token-2022 program ID
    let mut accounts = vec![
        AccountMeta::new(payer.pubkey(), true),         // user
        AccountMeta::new(mint, false),                  // mint
        AccountMeta::new(mint_authority_pda, false),    // mint_authority (PDA)
        AccountMeta::new(token_account, false),         // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program (use token-2022)
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
    ];
    
    // Add user profile PDA to account list if it exists
    if user_profile_exists {
        accounts.push(AccountMeta::new(user_profile_pda, false)); // user_profile
    }
    
    let mint_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // Create memo instruction with JSON content
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create and send transaction with new instruction order:
    // 1. Compute budget instruction
    // 2. Memo instruction
    // 3. Mint instruction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, mint_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Mint successful! Signature: {}", signature);
            println!("Memo: {}", memo_text);

            // Print token balance
            match client.get_token_account_balance(&token_account) {
                Ok(balance) => {
                    println!("New token balance: {}", balance.ui_amount.unwrap());
                    
                    // Check user profile stats if a profile exists
                    if user_profile_exists {
                        println!("\nYour mint statistics have been updated in your user profile.");
                        println!("To view your profile stats, run: cargo run --bin check-user-profile");
                    }
                }
                Err(_) => {
                    println!("Failed to get token balance");
                }
            }
            
            // Display pixel art if memo contains pixel art
            if memo_text.starts_with("pixel:") {
                let hex_string = memo_text.trim_start_matches("pixel:").trim();
                display_pixel_art(hex_string);
            }
        },
        Err(err) => {
            println!("Error: {}", err);
            println!("\nCommon issues:");
            println!("1. If you've created a user profile, make sure to include it in the transaction");
            println!("2. If you don't have a user profile, the contract expects the account list to be exactly 6 accounts");
            println!("3. Memo length must be at least 69 bytes");
            
            // Provide more specific advice based on error
            if err.to_string().contains("AccountNotEnoughKeys") {
                println!("\nThe contract is expecting more accounts than provided.");
                println!("To fix this, either create a user profile or update this script to include a dummy user profile account.");
            }
            
            return Err(err.into());
        }
    }

    Ok(())
}

// modify ensure_min_length function to keep JSON format
fn ensure_min_length(text: String, min_length: usize) -> String {
    if text.as_bytes().len() >= min_length {
        return text;
    }
    
    // parse existing JSON
    let mut json: serde_json::Value = serde_json::from_str(&text)
        .expect("Failed to parse JSON");
    
    // get existing message
    let message = json["message"].as_str().unwrap_or("");
    
    // calculate padding length needed
    let current_length = text.as_bytes().len();
    let padding_needed = min_length - current_length;
    let padding = ".".repeat(padding_needed);
    
    // update message field
    let new_message = format!("{}{}", message, padding);
    json["message"] = serde_json::Value::String(new_message);
    
    // convert back to string
    let result = json.to_string();
    println!("Memo was padded to meet minimum length requirement of {} bytes", min_length);
    
    result
}