use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;

fn main() {
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

    // Calculate latest burn index PDA
    let (latest_burn_index_pda, _bump) = Pubkey::find_program_address(
        &[b"latest_burn_index"],
        &program_id,
    );

    println!("Latest Burn Index PDA to close: {}", latest_burn_index_pda);

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),      // recipient (writable, signer)
        AccountMeta::new(latest_burn_index_pda, false),    // latest_burn_index account (writable)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // close_latest_burn_index instruction discriminator
    let data = vec![165,60,102,151,142,96,120,43]; // You'll need to replace this with the actual discriminator

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

    println!("Sending transaction to close latest burn index account...");

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Latest burn index account closed successfully!");
            println!("Transaction signature: {}", signature);
            
            // Verify account closure
            match client.get_account(&latest_burn_index_pda) {
                Ok(_) => println!("Warning: Account still exists"),
                Err(_) => println!("✓ Account successfully closed"),
            }
        }
        Err(err) => {
            println!("Failed to close latest burn index account: {}", err);
        }
    }
} 