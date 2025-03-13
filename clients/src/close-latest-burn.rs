use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
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

    // Calculate latest_burn PDA
    let (latest_burn_pda, _bump) = Pubkey::find_program_address(
        &[b"latest_burn"],
        &program_id,
    );

    println!("Latest Burn PDA to close: {}", latest_burn_pda);

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),      // recipient (writable, signer)
        AccountMeta::new(latest_burn_pda, false),    // latest_burn account (writable)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // close_latest_burn instruction discriminator
    let data = vec![138, 190, 54, 81, 161, 130, 84, 28];

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

    println!("Sending transaction to close latest burn account...");

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Latest burn account closed successfully!");
            println!("Transaction signature: {}", signature);
            
            // Verify account closure
            match client.get_account(&latest_burn_pda) {
                Ok(_) => println!("Warning: Account still exists"),
                Err(_) => println!("✓ Account successfully closed"),
            }
        }
        Err(err) => {
            println!("Failed to close latest burn account: {}", err);
        }
    }
}
