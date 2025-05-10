use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Fixed addresses
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")  // Get from create_token output
        .expect("Invalid mint address");

    // Calculate PDA (for information only)
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Create token account if it doesn't exist
    if client.get_account(&token_account).is_err() {
        println!("Creating token account...");
        
        let create_token_account_ix = 
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint,
                &spl_token::id(),
            );

        // Default compute units as fallback
        let initial_compute_units = 200_000;

        // Get recent blockhash
        let recent_blockhash = client
            .get_latest_blockhash()
            .expect("Failed to get recent blockhash");

        // Create transaction without compute budget instruction for simulation
        let sim_transaction = Transaction::new_signed_with_payer(
            &[create_token_account_ix.clone()],
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
            &[compute_budget_ix, create_token_account_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        client
            .send_and_confirm_transaction(&transaction)
            .expect("Failed to create token account");

        println!("Token account created successfully");
    } else {
        println!("Token account already exists");
    }

    // Print account info
    println!("\nAccount Info:");
    println!("Program ID: {}", program_id);
    println!("Mint: {}", mint);
    println!("Mint Authority (PDA): {}", mint_authority_pda);
    println!("Your wallet: {}", payer.pubkey());
    println!("Your token account: {}", token_account);
} 