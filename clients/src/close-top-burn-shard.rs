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
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct OptionU64 {
    tag: u8,
    value: Option<u64>,
}

impl OptionU64 {
    fn from_bytes(data: &[u8]) -> Self {
        let tag = data[0];
        if tag == 0 {
            OptionU64 { tag: 0, value: None }
        } else {
            let value = u64::from_le_bytes(data[1..9].try_into().unwrap());
            OptionU64 { tag: 1, value: Some(value) }
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct GlobalTopBurnIndex {
    top_burn_shard_total_count: u64,
    top_burn_shard_current_index: OptionU64,
}

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

    println!("Global Top Burn Index PDA: {}", global_top_burn_index_pda);

    // Get global top burn index account
    let global_top_burn_index_account = match client.get_account(&global_top_burn_index_pda) {
        Ok(account) => account,
        Err(err) => {
            println!("Error fetching global top burn index account: {}", err);
            return;
        }
    };

    // Parse global top burn index data
    if global_top_burn_index_account.data.len() < 17 {
        println!("Global top burn index account data is too small");
        return;
    }

    // Skip the 8-byte discriminator
    let data = &global_top_burn_index_account.data[8..];
    let total_count = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let current_index_option = OptionU64::from_bytes(&data[8..]);

    println!("Total top burn shard count: {}", total_count);
    println!("Current top burn shard index: {:?}", current_index_option.value);

    if total_count == 0 {
        println!("No top burn shards to close");
        return;
    }

    // Get recent blockhash for all transactions
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Default compute units
    let compute_units = 200_000;

    // Close top burn shards from newest to oldest
    for i in (0..total_count).rev() {
        let (top_burn_shard_pda, _) = Pubkey::find_program_address(
            &[b"top_burn_shard", &i.to_le_bytes()],
            &program_id
        );

        println!("\nProcessing top burn shard with index {}", i);
        println!("PDA address: {}", top_burn_shard_pda);

        // Check if the account exists
        match client.get_account(&top_burn_shard_pda) {
            Ok(account) => {
                println!("Shard exists, preparing to close");
                // Prepare close instruction
                let accounts = vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(global_top_burn_index_pda, false),
                    AccountMeta::new(top_burn_shard_pda, false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ];

                // Prepare instruction data - Discriminator for 'close_top_burn_shard'
                let data = vec![252, 203, 86, 232, 209, 69, 97, 14]; // Replace with the actual discriminator from your IDL

                let close_instruction = Instruction {
                    program_id,
                    accounts,
                    data,
                };

                // Create the compute budget instruction inside the loop
                let compute_budget_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(compute_units);

                // Create transaction
                let transaction = Transaction::new_signed_with_payer(
                    &[compute_budget_ix, close_instruction],
                    Some(&payer.pubkey()),
                    &[&payer],
                    recent_blockhash,
                );

                // Send transaction
                println!("Sending close transaction for shard index {}...", i);
                match client.send_and_confirm_transaction_with_spinner_and_config(
                    &transaction,
                    CommitmentConfig::confirmed(),
                    RpcSendTransactionConfig {
                        skip_preflight: true,
                        preflight_commitment: None,
                        encoding: None,
                        max_retries: Some(3),
                        min_context_slot: None,
                    },
                ) {
                    Ok(signature) => {
                        println!("Successfully closed shard with index {}!", i);
                        println!("Transaction signature: {}", signature);

                        // Get transaction logs for more detailed info
                        if let Ok(tx_data) = client.get_transaction_with_config(
                            &signature,
                            solana_client::rpc_config::RpcTransactionConfig {
                                encoding: None,
                                commitment: Some(CommitmentConfig::confirmed()),
                                max_supported_transaction_version: None,
                            },
                        ) {
                            if let Some(meta) = tx_data.transaction.meta {
                                println!("Transaction logs:");
                                if let solana_transaction_status::option_serializer::OptionSerializer::Some(logs) = meta.log_messages {
                                    for log in logs {
                                        println!("  {}", log);
                                    }
                                }
                            }
                        }

                        // Wait a bit before proceeding to the next shard
                        sleep(Duration::from_millis(1000));
                    }
                    Err(err) => {
                        println!("Failed to close shard with index {}: {}", i, err);
                        // Continue with the next shard anyway
                    }
                }
            }
            Err(err) => {
                println!("Shard with index {} does not exist or cannot be fetched: {}", i, err);
                // Continue with the next shard
            }
        }
    }

    // Final check of the global top burn index
    match client.get_account(&global_top_burn_index_pda) {
        Ok(account) => {
            // Skip the 8-byte discriminator
            let data = &account.data[8..];
            let total_count = u64::from_le_bytes(data[0..8].try_into().unwrap());
            let current_index_option = OptionU64::from_bytes(&data[8..]);

            println!("\nFinal global top burn index state:");
            println!("Total top burn shard count: {}", total_count);
            println!("Current top burn shard index: {:?}", current_index_option.value);
        }
        Err(err) => {
            println!("Error fetching final global top burn index state: {}", err);
        }
    }

    println!("\nOperation completed! All top burn shards have been processed.");
}