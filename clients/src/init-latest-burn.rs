use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig},
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
    commitment_config::CommitmentConfig,
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

    // Calculate latest burn PDA
    let (latest_burn_pda, bump) = Pubkey::find_program_address(&[b"latest_burn"], &program_id);

    println!("Latest Burn PDA: {}", latest_burn_pda);
    println!("Bump seed: {}", bump);

    // Double-check account status
    match client.get_account(&latest_burn_pda) {
        Ok(account) => {
            println!(
                "Account already exists with {} bytes of data",
                account.data.len()
            );
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
            println!("Skipping initialization as account is already created");
            return;
        }
        Err(err) => {
            println!("Account does not exist or cannot be fetched: {}", err);
        }
    }

    // Calculate required space (adjusted for 69 records)
    let space = 8 + // discriminator
                1 + // current_index (u8)
                4 + // vec len
                (69 * ( // 69 records
                    32 + // pubkey
                    88 + // signature string
                    8 +  // slot
                    8    // blocktime
                ));

    // Calculate required lamports for rent exemption
    let rent = client
        .get_minimum_balance_for_rent_exemption(space)
        .expect("Failed to get rent exemption");

    println!("Account size: {} bytes", space);
    println!(
        "Required lamports for rent exemption: {} SOL",
        rent as f64 / 1_000_000_000.0
    );

    // Estimate transaction fee (CU * 10, max 200,000 CU per instruction)
    let max_cu_per_instruction = 200_000;
    let estimated_fee = max_cu_per_instruction * 10; // 2,000,000 lamports
    println!(
        "Estimated transaction fee: {} SOL",
        estimated_fee as f64 / 1_000_000_000.0
    );

    // Check if payer has enough balance
    let total_required = rent + estimated_fee;
    if balance < total_required {
        println!("Insufficient balance to initialize account!");
        println!(
            "Required: {} SOL, Available: {} SOL",
            total_required as f64 / 1_000_000_000.0,
            balance as f64 / 1_000_000_000.0
        );
        return;
    }

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true), // payer (writable, signer)
        AccountMeta::new(latest_burn_pda, false), // latest burn account (writable, NOT signer)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // Initialize latest burn instruction (only discriminator)
    let data = vec![207, 56, 114, 145, 214, 56, 168, 234]; // Anchor discriminator for 'initialize_latest_burn'

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    // Create and send transaction
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    println!("Recent blockhash: {}", recent_blockhash);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    println!("Sending transaction to initialize latest burn...");

    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        preflight_commitment: None,
        encoding: None,
        max_retries: Some(3), // Retry up to 3 times
        min_context_slot: None,
    };

    match client.send_and_confirm_transaction_with_spinner_and_config(
        &transaction,
        CommitmentConfig::confirmed(),
        config,
    ) {
        Ok(signature) => {
            println!("Latest burn initialized successfully!");
            println!("Transaction signature: {}", signature);

            // Print account info
            println!("\nLatest Burn Account Info:");
            println!("Program ID: {}", program_id);
            println!("Latest Burn PDA: {}", latest_burn_pda);
            println!("Your wallet (payer): {}", payer.pubkey());

            // Get transaction logs
            if let Ok(tx_data) = client.get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
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
            println!("Failed to initialize latest burn: {}", err);
            if err.to_string().contains("already in use") {
                println!("Note: This error might mean the latest burn account is already initialized.");
            }
            return; // Exit early if transaction fails
        }
    }

    // Poll account status with retries
    println!("Polling for account creation...");
    let max_attempts = 10;
    let delay = Duration::from_millis(10000); // Wait 10s between attempts
    for attempt in 1..=max_attempts {
        match client.get_account(&latest_burn_pda) {
            Ok(account) => {
                println!(
                    "Post-transaction: Account now exists with {} bytes of data",
                    account.data.len()
                );
                println!("Owner: {}", account.owner);
                println!("Lamports: {}", account.lamports);
                return; // Exit once account is found
            }
            Err(err) => {
                println!(
                    "Attempt {}/{}: Account not yet available: {}",
                    attempt, max_attempts, err
                );
                if attempt == max_attempts {
                    println!("Failed to detect account after {} attempts", max_attempts);
                } else {
                    sleep(delay); // Wait before next attempt
                }
            }
        }
    }
}