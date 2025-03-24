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

// Using discriminator value from IDL
const UPDATE_USER_PROFILE_DISCRIMINATOR: [u8; 8] = [79, 75, 114, 130, 68, 123, 180, 11];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 {
        println!("Usage: cargo run --bin update-user-profile <field> <value>");
        println!("Fields: username, profile_image");
        println!("Example: cargo run --bin update-user-profile username \"NewName\"");
        println!("Example: cargo run --bin update-user-profile profile_image \"https://example.com/new-avatar.png\"");
        return Err("Field and value are required".into());
    }
    
    let field = args[1].to_lowercase();
    if field != "username" && field != "profile_image" {
        return Err("Invalid field. Must be 'username' or 'profile_image'".into());
    }
    
    let value = args[2].clone();
    
    // Validate input values
    if field == "username" && value.len() > 32 {
        return Err("Username too long. Maximum length is 32 characters.".into());
    }
    
    if field == "profile_image" && value.len() > 128 {
        return Err("Profile image URL too long. Maximum length is 128 characters.".into());
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
            println!("\nTo view your profile, run: cargo run --bin check-user-profile");
        },
        Err(err) => {
            println!("Error updating user profile: {}", err);
            return Err(err.into());
        }
    }
    
    Ok(())
}