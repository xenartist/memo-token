use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use sha2::{Sha256, Digest};

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
    // Get command line arguments for memo
    let args: Vec<String> = std::env::args().collect();
    
    let memo = if args.len() > 1 {
        args[1].clone()
    } else {
        String::from("Default memo message")
    };
    
    // Check if memo starts with "pixel:" prefix
    let (memo_text, has_pixel_art) = if memo.starts_with("pixel:") {
        let hex_part = memo.trim_start_matches("pixel:").trim();
        // Validate that the remaining part is a valid hex string
        if hex_part.chars().all(|c| c.is_digit(16)) {
            (memo.clone(), true)
        } else {
            (memo, false)
        }
    } else {
        (memo, false)
    };

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
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")  // Get from create_token output
        .expect("Invalid mint address");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

    // Create mint instruction
    let mint_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),         // user
            AccountMeta::new(mint, false),                  // mint
            AccountMeta::new(mint_authority_pda, false),    // mint_authority (PDA)
            AccountMeta::new(token_account, false),         // token_account
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
        ],
    );

    // Create memo instruction
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create and send transaction with both mint and memo instructions
    let transaction = Transaction::new_signed_with_payer(
        &[mint_ix, memo_ix],
        Some(&payer.pubkey()),
        &[&payer],  // Only user needs to sign, PDA signs in program
        recent_blockhash,
    );

    // Send and confirm transaction
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Mint successful! Signature: {}", signature);
    println!("Memo: {}", memo_text);

    // Print token balance
    match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            println!("New token balance: {}", balance.ui_amount.unwrap());
        }
        Err(_) => {
            println!("Failed to get token balance");
        }
    }
    
    // Display pixel art if memo contains pixel art
    if has_pixel_art {
        let hex_string = memo_text.trim_start_matches("pixel:").trim();
        display_pixel_art(hex_string);
    }

    Ok(())
}