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

    // Calculate global burn index PDA
    let (global_burn_index_pda, _bump) = Pubkey::find_program_address(
        &[b"global_burn_index"],
        &program_id,
    );

    println!("Global Burn Index PDA to close: {}", global_burn_index_pda);

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

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),      // recipient (writable, signer)
        AccountMeta::new(global_burn_index_pda, false),    // global_burn_index account (writable)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // close_global_burn_index instruction discriminator
    let data = vec![250,224,134,80,71,240,143,218];

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

    println!("Sending transaction to close global burn index account...");

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Global burn index account closed successfully!");
            println!("Transaction signature: {}", signature);
            
            // Verify account closure
            match client.get_account(&global_burn_index_pda) {
                Ok(_) => println!("Warning: Account still exists"),
                Err(_) => println!("✓ Account successfully closed"),
            }
        }
        Err(err) => {
            println!("Failed to close global burn index account: {}", err);
        }
    }
} 