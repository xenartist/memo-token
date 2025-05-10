// clients/src/close-user-profile.rs
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
};
use std::str::FromStr;
use std::io;

// Using discriminator value from IDL
const CLOSE_USER_PROFILE_DISCRIMINATOR: [u8; 8] = [242, 80, 248, 79, 81, 251, 65, 113];

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    
    println!("User: {}", payer.pubkey());
    println!("User profile PDA: {}", user_profile_pda);
    
    // Check if user profile exists
    match client.get_account(&user_profile_pda) {
        Ok(account) => {
            println!("Found user profile at: {}", user_profile_pda);
            println!("Account rent-exempt balance: {} lamports", account.lamports);
            println!("Note: This will permanently delete your token profile!");
            println!("Are you sure you want to continue? (y/n)");

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Operation cancelled.");
                return Ok(());
            }
            
            // Create instruction data
            let mut instruction_data = Vec::new();
            instruction_data.extend_from_slice(&CLOSE_USER_PROFILE_DISCRIMINATOR);
            
            // Create the close instruction
            let accounts = vec![
                AccountMeta::new(payer.pubkey(), true),        // user
                AccountMeta::new(user_profile_pda, false),     // user_profile
                AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
            ];
            
            let close_ix = Instruction::new_with_bytes(
                program_id,
                &instruction_data,
                accounts,
            );
            
            // Default compute units as fallback
            let initial_compute_units = 200_000;
            
            // Get recent blockhash
            let recent_blockhash = client.get_latest_blockhash()?;
            
            // Create transaction without compute budget instruction for simulation
            let sim_transaction = Transaction::new_signed_with_payer(
                &[close_ix.clone()],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );
            
            // Simulate transaction to determine required compute units
            println!("Simulating transaction to determine required compute units...");
            let compute_units = match client.simulate_transaction_with_config(
                &sim_transaction,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: false,
                    commitment: Some(CommitmentConfig::confirmed()),
                    encoding: None,
                    accounts: None,
                    min_context_slot: None,
                    inner_instructions: true,
                },
            ) {
                Ok(result) => {
                    if let Some(err) = result.value.err {
                        println!("Warning: Transaction simulation failed: {:?}", err);
                        println!("Using default compute units: {}", initial_compute_units);
                        initial_compute_units
                    } else if let Some(units_consumed) = result.value.units_consumed {
                        // Add 10% safety margin
                        let required_cu = (units_consumed as f64 * 1.1) as u32;
                        println!("Simulation consumed {} CUs, requesting {} CUs with 10% safety margin", 
                            units_consumed, required_cu);
                        required_cu
                    } else {
                        println!("Simulation didn't return units consumed, using default: {}", initial_compute_units);
                        initial_compute_units
                    }
                },
                Err(err) => {
                    println!("Failed to simulate transaction: {}", err);
                    println!("Using default compute units: {}", initial_compute_units);
                    initial_compute_units
                }
            };
            
            // Create compute budget instruction with dynamically calculated CU
            let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
            println!("Setting compute budget: {} CUs", compute_units);
            
            // Create transaction with updated compute units
            let transaction = Transaction::new_signed_with_payer(
                &[compute_budget_ix, close_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );
            
            println!("Sending transaction to close user profile...");
            
            // Send transaction with spinner
            let signature = client.send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::confirmed(),
                solana_client::rpc_config::RpcSendTransactionConfig {
                    skip_preflight: false,
                    preflight_commitment: None,
                    encoding: None,
                    max_retries: Some(5),
                    min_context_slot: None,
                },
            )?;
            
            println!("User profile closed successfully!");
            println!("Transaction signature: {}", signature);
            println!("The SOL from this account has been returned to your wallet.");
            println!("Note: This only closed your token profile.");
            println!("If you have a social profile, you'll need to close it separately with memo-social client.");
        },
        Err(_) => {
            println!("No user profile found for this wallet.");
            println!("There is nothing to close.");
        }
    }
    
    Ok(())
}