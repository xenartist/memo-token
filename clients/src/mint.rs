use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_memo::build_memo;
use sha2::{Sha256, Digest};
use std::str::FromStr;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and mint addresses
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("HgJGY6N9R1JcF7VHa6tkc7zQPWCD3ZrhuDeXFwnHnU7Y")  // Get from create_token output
        .expect("Invalid mint address");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account - Note we're using the Token-2022 program ID
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &spl_token_2022::id(), // Use Token-2022 program ID
    );

    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

    // Create mint instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(mint, false),
        AccountMeta::new_readonly(mint_authority_pda, false),
        AccountMeta::new(token_account, false),
        AccountMeta::new_readonly(spl_token_2022::id(), false), // Use Token-2022 program ID
    ];

    let process_transfer_ix = Instruction {
        program_id,
        accounts,
        data: instruction_data,
    };

    // Create memo instruction
    let memo_ix = build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .await?;

    // Create and send transaction
    let mut instructions = vec![process_transfer_ix];
    
    // Add memo instruction
    instructions.push(memo_ix);

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let signature = client
        .send_and_confirm_transaction(&transaction)
        .await?;

    println!("Transaction signature: {}", signature);
    println!("Token minted successfully with memo: {}", memo_text);

    // Print token balance
    match client.get_token_account_balance(&token_account).await {
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