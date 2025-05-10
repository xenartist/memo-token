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
use std::thread::sleep;
use std::time::Duration;
use borsh::{BorshDeserialize};

// UserProfile
#[derive(BorshDeserialize)]
struct UserProfile {
    pubkey: Pubkey,
    total_minted: u64,
    total_burned: u64,
    mint_count: u64,
    burn_count: u64,
    created_at: i64,
    last_updated: i64,
    burn_history_index: Option<u64>,
}

fn main() {
    // rpc
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new_with_commitment(
        rpc_url.to_string(),
        CommitmentConfig::confirmed()
    );

    // wallet
    let wallet = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    println!("User: {}", wallet.pubkey());

    // program id
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // user profile pda
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", wallet.pubkey().as_ref()],
        &program_id,
    );
    
    println!("User profile PDA: {}", user_profile_pda);
    
    // check user has burn history records
    if let None = get_latest_burn_history_index(&client, &user_profile_pda) {
        println!("User has no burn history records. Nothing to close.");
        return;
    }
    
    // prepare to close all burn history records
    println!("Starting to close all burn history records...");
    
    // 'close_user_burn_history' method discriminator
    let close_discriminator = vec![208,153,10,179,27,50,158,161];
    
    // close all burn history records
    let mut iteration = 1;
    
    while let Some(current_index) = get_latest_burn_history_index(&client, &user_profile_pda) {
        println!("Iteration {}: Closing burn history with index: {}", iteration, current_index);
        
        // calculate current burn history pda
        let (burn_history_pda, _) = Pubkey::find_program_address(
            &[
                b"burn_history", 
                wallet.pubkey().as_ref(),
                &current_index.to_le_bytes()
            ],
            &program_id,
        );
        
        if close_burn_history(
            &client, 
            &program_id, 
            &wallet, 
            &user_profile_pda, 
            &burn_history_pda, 
            &close_discriminator
        ) {
            println!("Successfully closed burn history with index: {}", current_index);
        } else {
            println!("Failed to close burn history. Exiting.");
            return;
        }
        
        sleep(Duration::from_secs(2)); // Wait for state update
        iteration += 1;
    }
    
    println!("All burn history records have been closed!");
}

// get latest burn history index
fn get_latest_burn_history_index(client: &RpcClient, user_profile_pda: &Pubkey) -> Option<u64> {
    match client.get_account(user_profile_pda) {
        Ok(account) => {
            let mut data = &account.data[8..]; // skip discriminator
            match UserProfile::deserialize(&mut data) {
                Ok(profile) => profile.burn_history_index,
                Err(e) => {
                    println!("Failed to deserialize user profile: {}", e);
                    None
                }
            }
        },
        Err(_) => None
    }
}

// close single burn history record
fn close_burn_history(
    client: &RpcClient,
    program_id: &Pubkey,
    wallet: &solana_sdk::signature::Keypair,
    user_profile_pda: &Pubkey,
    burn_history_pda: &Pubkey,
    close_discriminator: &Vec<u8>
) -> bool {
    // Default compute units as fallback
    let initial_compute_units = 200_000;
    
    for attempt in 1..=3 {
        println!("Attempt {}/3 to close burn history...", attempt);
        
        // get latest blockhash
        let recent_blockhash = match client.get_latest_blockhash() {
            Ok(hash) => hash,
            Err(err) => {
                println!("Failed to get blockhash: {}", err);
                sleep(Duration::from_secs(2));
                continue;
            }
        };
        
        // build account metadata
        let accounts = vec![
            AccountMeta::new(wallet.pubkey(), true),
            AccountMeta::new(*user_profile_pda, false),
            AccountMeta::new(*burn_history_pda, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ];
        
        // build instruction
        let instruction = Instruction {
            program_id: *program_id,
            accounts,
            data: close_discriminator.clone(),
        };
        
        // Create transaction without compute budget instruction for simulation
        let sim_transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&wallet.pubkey()),
            &[wallet],
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
        
        // build transaction with compute budget instruction
        let transaction = Transaction::new_signed_with_payer(
            &[compute_budget_ix, instruction],
            Some(&wallet.pubkey()),
            &[wallet],
            recent_blockhash,
        );
        
        // send transaction
        match client.send_and_confirm_transaction(&transaction) {
            Ok(signature) => {
                println!("Transaction sent successfully! Signature: {}", signature);
                return true;
            },
            Err(err) => {
                println!("Failed to send transaction: {}", err);
                if attempt < 3 {
                    println!("Retrying in 2 seconds...");
                    sleep(Duration::from_secs(2));
                }
            }
        }
    }
    
    false
}
