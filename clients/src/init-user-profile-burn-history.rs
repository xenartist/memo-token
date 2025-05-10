use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSendTransactionConfig, RpcSimulateTransactionConfig},
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
    system_program,
    signer::keypair::Keypair,
};
use std::str::FromStr;

// discriminator and max signatures per burn history
const INIT_BURN_HISTORY_DISCRIMINATOR: [u8; 8] = [40, 163, 144, 239, 40, 5, 88, 119];
const MAX_SIGNATURES_PER_BURN_HISTORY: usize = 100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // program address
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );

    // Check if user profile exists and get burn_history_index
    match client.get_account(&user_profile_pda) {
        Ok(account) => {
            println!("User profile found at: {}", user_profile_pda);
            
            // skip discriminator
            let mut data = &account.data[8..];
            
            // skip owner pubkey
            data = &data[32..];
            
            // skip total_minted, total_burned, mint_count, burn_count
            data = &data[32..];
            
            // skip timestamps
            data = &data[16..];
            
            // read burn_history_index
            let has_burn_history = data[0] == 1;
            let burn_history_index = if has_burn_history {
                let current_index = u64::from_le_bytes([
                    data[1], data[2], data[3], data[4],
                    data[5], data[6], data[7], data[8]
                ]);
                Some(current_index)
            } else {
                None
            };

            // check if current burn history exists
            match burn_history_index {
                None => {
                    // if no burn history, create a new one (index 0)
                    println!("No burn history found. Creating the first burn history (index 0).");
                    
                    let (burn_history_pda, _) = Pubkey::find_program_address(
                        &[
                            b"burn_history",
                            payer.pubkey().as_ref(),
                            &0u64.to_le_bytes()
                        ],
                        &program_id,
                    );
                    initialize_burn_history(&client, &payer, &program_id, user_profile_pda, burn_history_pda)?;
                },
                Some(current_index) => {
                    // get current burn history PDA
                    let (current_burn_history_pda, _) = Pubkey::find_program_address(
                        &[
                            b"burn_history",
                            payer.pubkey().as_ref(),
                            &current_index.to_le_bytes()
                        ],
                        &program_id,
                    );
                    
                    // check if current burn history exists
                    match client.get_account(&current_burn_history_pda) {
                        Ok(burn_history_account) => {
                            // parse burn history data, check signature count
                            let burn_history_data = &burn_history_account.data[8..]; // skip discriminator
                            
                            // skip owner and index
                            let data = &burn_history_data[40..]; // 32 bytes owner + 8 bytes index
                            
                            // read signature array length
                            let signatures_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                            
                            println!("Current burn history (index {}) has {} signatures.", current_index, signatures_len);
                            
                            // check if signature count is full
                            if signatures_len >= MAX_SIGNATURES_PER_BURN_HISTORY {
                                // if full, create a new burn history
                                let new_index = current_index + 1;
                                println!("Current burn history is full. Creating a new burn history (index {}).", new_index);
                                
                                let (new_burn_history_pda, _) = Pubkey::find_program_address(
                                    &[
                                        b"burn_history",
                                        payer.pubkey().as_ref(),
                                        &new_index.to_le_bytes()
                                    ],
                                    &program_id,
                                );
                                initialize_burn_history(&client, &payer, &program_id, user_profile_pda, new_burn_history_pda)?;
                            } else {
                                // if not full, no need to create a new one
                                println!("Current burn history is not full ({}/{} signatures). No need to create a new one.",
                                    signatures_len, MAX_SIGNATURES_PER_BURN_HISTORY);
                                println!("You can continue to add burn signatures to the current burn history.");
                            }
                        },
                        Err(_) => {
                            // if current index burn history doesn't exist, recreate it
                            println!("Burn history with index {} doesn't exist. Creating it now.", current_index);
                            
                            let (burn_history_pda, _) = Pubkey::find_program_address(
                                &[
                                    b"burn_history",
                                    payer.pubkey().as_ref(),
                                    &current_index.to_le_bytes()
                                ],
                                &program_id,
                            );
                            initialize_burn_history(&client, &payer, &program_id, user_profile_pda, burn_history_pda)?;
                        }
                    }
                }
            }
        },
        Err(_) => {
            println!("No user profile found. Please create a profile first using:");
            println!("cargo run --bin init-user-profile <username> [profile_image_url]");
            return Ok(());
        }
    }

    Ok(())
}

// initialize burn history helper function
fn initialize_burn_history(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    user_profile_pda: Pubkey,
    burn_history_pda: Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing burn history at: {}", burn_history_pda);
    
    // construct instruction data: only discriminator
    let instruction_data = INIT_BURN_HISTORY_DISCRIMINATOR.to_vec();

    // create instruction
    let ix = Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),         // user (signer, writable)
            AccountMeta::new(user_profile_pda, false),      // user_profile (NOT writable)
            AccountMeta::new(burn_history_pda, false),      // burn_history (NOT writable)
            AccountMeta::new_readonly(system_program::id(), false), // system_program
        ],
        data: instruction_data,
    };

    // Default compute units as fallback
    let initial_compute_units = 300_000;

    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;

    // Create transaction without compute budget instruction for simulation
    let sim_transaction = Transaction::new_signed_with_payer(
        &[ix.clone()],
        Some(&payer.pubkey()),
        &[payer],
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
        &[compute_budget_ix, ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        preflight_commitment: None,
        encoding: None,
        max_retries: Some(3),
        min_context_slot: None,
    };

    match client.send_and_confirm_transaction_with_spinner_and_config(
        &transaction,
        CommitmentConfig::confirmed(),
        config,
    ) {
        Ok(signature) => {
            println!("Successfully initialized burn history account!");
            println!("Transaction signature: {}", signature);
            
            // Get transaction logs
            if let Ok(tx_data) = client.get_transaction_with_config(
                &signature,
                solana_client::rpc_config::RpcTransactionConfig {
                    encoding: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: None,
                },
            ) {
                if let Some(meta) = tx_data.transaction.meta {
                    println!("\nTransaction logs:");
                    match meta.log_messages {
                        solana_transaction_status::option_serializer::OptionSerializer::Some(logs) => {
                            for log in logs {
                                println!("{}", log);
                            }
                        }
                        solana_transaction_status::option_serializer::OptionSerializer::None => {
                            println!("No logs available");
                        }
                        solana_transaction_status::option_serializer::OptionSerializer::Skip => {
                            println!("Transaction logs skipped");
                        }
                    }
                }
            }
            Ok(())
        },
        Err(err) => {
            println!("Failed to initialize burn history account: {}", err);
            Err(Box::new(err))
        }
    }
}
