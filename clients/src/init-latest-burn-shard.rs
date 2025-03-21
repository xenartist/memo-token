use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSendTransactionConfig,
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

    // Add admin wallet verification logic
    // Check admin pubkey
    let admin_pubkey = Pubkey::from_str("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn")
        .expect("Invalid admin pubkey string");

    // Check if current wallet matches admin pubkey
    if payer.pubkey() != admin_pubkey {
        println!("Warning: Current wallet is not the admin wallet.");
        println!("Current wallet: {}", payer.pubkey());
        println!("Admin pubkey: {}", admin_pubkey);
        println!("Continue? (y/n)");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Failed to read input");
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Operation cancelled");
            return;
        }
    } else {
        println!("Confirmed: Current wallet is the admin wallet");
    }

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate PDAs
    let (global_burn_index_pda, _) = Pubkey::find_program_address(&[b"global_burn_index"], &program_id);
    let (latest_burn_shard_pda, bump) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id
    );

    println!("Global Burn Index PDA: {}", global_burn_index_pda);
    println!("Latest Burn Shard PDA: {}", latest_burn_shard_pda);
    println!("Shard bump seed: {}", bump);

    // Double-check shard account status
    match client.get_account(&latest_burn_shard_pda) {
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
                1 + // current_index
                4 + // vec len
                (69 * (32 + 88 + 8 + 8 + 8)); // 69 records

    // Calculate required lamports for rent exemption
    let rent = client
        .get_minimum_balance_for_rent_exemption(space)
        .expect("Failed to get rent exemption");

    println!("Account size: {} bytes", space);
    println!(
        "Required lamports for rent exemption: {} SOL",
        rent as f64 / 1_000_000_000.0
    );

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(global_burn_index_pda, false),
        AccountMeta::new(latest_burn_shard_pda, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    // Prepare instruction data - Discriminator for 'initialize_latest_burn_shard'
    let data = vec![150,220,2,213,30,67,33,31]; 

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    // Add compute budget instruction
    let compute_budget_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(200_000);

    // Create and send transaction
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    println!("Sending transaction to initialize latest burn shard...");

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
            println!("Latest burn shard initialized successfully!");
            println!("Transaction signature: {}", signature);

            // Print account info
            println!("\nLatest Burn Shard Account Info:");
            println!("Program ID: {}", program_id);
            println!("Latest Burn Shard PDA: {}", latest_burn_shard_pda);
            println!("Your wallet (payer): {}", payer.pubkey());

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
            println!("Failed to initialize latest burn shard: {}", err);
            return;
        }
    }

    // Poll account status
    println!("Polling for account creation...");
    let max_attempts = 10;
    let delay = Duration::from_millis(10000);
    for attempt in 1..=max_attempts {
        match client.get_account(&latest_burn_shard_pda) {
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