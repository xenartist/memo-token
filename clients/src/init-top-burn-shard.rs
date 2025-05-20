use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSendTransactionConfig, RpcSimulateTransactionConfig},
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
    commitment_config::CommitmentConfig,
    compute_budget,
};
use std::{str::FromStr, thread::sleep, time::Duration};

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(shellexpand::tilde("~/.config/solana/id.json").to_string())
        .expect("Failed to read keypair file");

    // Check payer balance
    let balance = client
        .get_balance(&payer.pubkey())
        .expect("Failed to get payer balance");
    println!("Payer balance: {} SOL", balance as f64 / 1_000_000_000.0);

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate PDAs
    let (global_top_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_top_burn_index"], 
        &program_id
    );

    // initalize total_count
    let mut shard_total_count: u64 = 0;

    // Check if global top burn index account exists
    match client.get_account(&global_top_burn_index_pda) {
        Ok(account) => {
            println!("Global top burn index account exists, continuing...");
            // parse account data to get current shard count
            if account.data.len() >= 16 {
                shard_total_count = u64::from_le_bytes(account.data[8..16].try_into().unwrap());
                println!("Current total count: {}", shard_total_count);
                
                if shard_total_count == 0 {
                    println!("Creating the first top burn shard");
                }
            }
        },
        Err(err) => {
            println!("Global top burn index account doesn't exist or cannot be fetched: {}", err);
            println!("Please initialize the global top burn index first.");
            return;
        }
    }

    // Calculate the TopBurnShard PDA using the total count
    let (top_burn_shard_pda, _) = Pubkey::find_program_address(
        &[
            b"top_burn_shard", 
            &shard_total_count.to_le_bytes()
        ],
        &program_id
    );

    println!("Global Top Burn Index PDA: {}", global_top_burn_index_pda);
    println!("Top Burn Shard PDA: {}", top_burn_shard_pda);
    println!("Creating shard with index: {}", shard_total_count);

    // Double-check shard account status
    match client.get_account(&top_burn_shard_pda) {
        Ok(account) => {
            println!(
                "Shard account already exists with {} bytes of data",
                account.data.len()
            );
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
            println!("Skipping initialization as account is already created");
            return;
        }
        Err(err) => {
            println!("Shard account does not exist or cannot be fetched: {}", err);
        }
    }

    // Calculate required space
    let space = 8 + // discriminator
                8 + // index
                32 + // creator pubkey
                4 + // vec len
                (69 * (32 + 88 + 8 + 8 + 8)); // 69 records (pubkey + signature + slot + blocktime + amount)

    // Calculate required lamports for rent exemption
    let rent = client
        .get_minimum_balance_for_rent_exemption(space)
        .expect("Failed to get rent exemption");

    println!("Account size: {} bytes", space);
    println!(
        "Required lamports for rent exemption: {} SOL",
        rent as f64 / 1_000_000_000.0
    );

    // read current index and get corresponding shard
    let mut current_top_burn_shard_pda = None;

    // read current index from global top burn index account
    match client.get_account(&global_top_burn_index_pda) {
        Ok(account) => {
            if account.data.len() >= 17 { // 8 bytes discriminator + 8 bytes total_count + 1 byte option tag
                let option_tag = account.data[16];
                if option_tag == 1 && account.data.len() >= 25 { // Option::Some
                    let current_index = u64::from_le_bytes(account.data[17..25].try_into().unwrap());
                    println!("Current top burn shard index: {}", current_index);
                    
                    // calculate current index's shard PDA
                    let (shard_pda, _) = Pubkey::find_program_address(
                        &[b"top_burn_shard", &current_index.to_le_bytes()],
                        &program_id,
                    );
                    current_top_burn_shard_pda = Some(shard_pda);
                    println!("Current top burn shard PDA: {}", shard_pda);
                }
            }
        },
        Err(err) => {
            println!("Error reading global top burn index: {}", err);
        }
    }

    // create accounts list
    let accounts = match current_top_burn_shard_pda {
        Some(current_shard_pda) => {
            // check if current index's shard exists
            match client.get_account(&current_shard_pda) {
                Ok(_) => {
                    println!("Current top burn shard account found, including it in transaction");
                    vec![
                        AccountMeta::new(payer.pubkey(), true),                  // user
                        AccountMeta::new(global_top_burn_index_pda, false),      // global_top_burn_index
                        AccountMeta::new(top_burn_shard_pda, false),             // top_burn_shard
                        AccountMeta::new_readonly(current_shard_pda, false),     // current_top_burn_shard
                        AccountMeta::new_readonly(system_program::id(), false),  // system_program
                    ]
                },
                Err(_) => {
                    println!("Current top burn shard account not found or cannot be fetched");
                    vec![
                        AccountMeta::new(payer.pubkey(), true),
                        AccountMeta::new(global_top_burn_index_pda, false),
                        AccountMeta::new(top_burn_shard_pda, false),
                        AccountMeta::new_readonly(system_program::id(), false),  // system_program
                    ]
                }
            }
        },
        None => {
            println!("No current top burn shard index found");
            vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(global_top_burn_index_pda, false),
                AccountMeta::new(top_burn_shard_pda, false),
                AccountMeta::new_readonly(system_program::id(), false),  // system_program
            ]
        }
    };

    // Prepare instruction data - Discriminator for 'initialize_top_burn_shard'
    // This is the correct discriminator from the IDL
    let data = vec![100, 156, 197, 248, 154, 101, 107, 185]; 

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    // Default compute units as fallback
    let initial_compute_units = 200_000;

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create transaction without compute budget instruction for simulation
    let sim_transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
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
    let compute_budget_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    println!("Setting compute budget: {} CUs", compute_units);

    // Create transaction with updated compute units
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    println!("Sending transaction to initialize top burn shard...");

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
            println!("Top burn shard initialized successfully!");
            println!("Transaction signature: {}", signature);

            // Print account info
            println!("\nTop Burn Shard Account Info:");
            println!("Program ID: {}", program_id);
            println!("Top Burn Shard PDA: {}", top_burn_shard_pda);
            println!("Creator: {}", payer.pubkey());
            println!("Account size: {} bytes", space);
            println!("Minimum burn amount to qualify: {} tokens", 420);

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
        }
        Err(err) => {
            println!("Failed to initialize top burn shard: {}", err);
            return;
        }
    }

    // Poll account status
    println!("Polling for account creation...");
    let max_attempts = 10;
    let delay = Duration::from_millis(10000);
    for attempt in 1..=max_attempts {
        match client.get_account(&top_burn_shard_pda) {
            Ok(account) => {
                println!(
                    "Account created successfully with {} bytes of data",
                    account.data.len()
                );
                println!("Owner: {}", account.owner);
                println!("Lamports: {}", account.lamports);
                return;
            }
            Err(err) => {
                println!(
                    "Attempt {}/{}: Account not yet available: {}",
                    attempt, max_attempts, err
                );
                if attempt == max_attempts {
                    println!("Failed to detect account after {} attempts", max_attempts);
                } else {
                    sleep(delay);
                }
            }
        }
    }
}