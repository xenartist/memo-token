use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_program,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
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

    // Calculate PDA
    let (latest_burn_shard_pda, _bump) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id,
    );

    println!("Latest Burn Shard PDA to close: {}", latest_burn_shard_pda);

    // Create instruction
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),      // recipient (writable, signer)
        AccountMeta::new(latest_burn_shard_pda, false),    // latest_burn_shard account (writable)
        AccountMeta::new_readonly(system_program::id(), false), // system program
    ];

    // Prepare instruction data - Discriminator for 'close_latest_burn_shard'
    let data = vec![93,129,3,152,194,180,0,53]; 

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
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    println!("Setting compute budget: {} CUs", compute_units);

    // Create transaction with updated compute units
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Sending transaction to close latest burn shard account...");

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Latest burn shard account closed successfully!");
            println!("Transaction signature: {}", signature);
            
            // Verify account closure
            match client.get_account(&latest_burn_shard_pda) {
                Ok(_) => println!("Warning: Account still exists"),
                Err(_) => println!("✓ Account successfully closed"),
            }
        }
        Err(err) => {
            println!("Failed to close latest burn shard account: {}", err);
        }
    }
} 