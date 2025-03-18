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
    borsh::try_from_slice_unchecked,
};
use std::{str::FromStr, thread::sleep, time::Duration};
use borsh::ser::BorshSerialize;

fn main() {
    // Get zone from command line args
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} <zone>", args[0]);
        return;
    }
    let zone = args[1].clone();
    
    // Validate zone length
    if zone.len() > 32 {
        println!("Zone name too long. Maximum 32 bytes allowed.");
        return;
    }

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
    let (latest_burn_index_pda, _) = Pubkey::find_program_address(&[b"latest_burn_index"], &program_id);
    let (latest_burn_shard_pda, bump) = Pubkey::find_program_address(
        &[b"latest_burn_shard", zone.as_bytes()],
        &program_id
    );

    println!("Latest Burn Index PDA: {}", latest_burn_index_pda);
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
                32 + // zone
                1 + // current_index
                4 + // vec len
                (69 * (32 + 88 + 8 + 8)); // 69 records

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
        AccountMeta::new(latest_burn_index_pda, false),
        AccountMeta::new(latest_burn_shard_pda, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    // Prepare instruction data
    let mut data = vec![167,228,2,172,243,48,109,204]; // Discriminator for 'create_latest_burn_shard'
    data.extend((zone.len() as u32).to_le_bytes());
    data.extend(zone.as_bytes());

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    // Create and send transaction
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
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
            println!("Zone: {}", zone);
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