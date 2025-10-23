use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file},
    signer::Signer,
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use sha2::{Sha256, Digest};
use std::str::FromStr;
use solana_system_interface::program as system_program;
use memo_token_client::get_rpc_url;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-BURN USER GLOBAL BURN STATISTICS INITIALIZATION ===");
    println!();

    // Connect to network
    let rpc_url = get_rpc_url();
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let user = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    println!("User: {}", user.pubkey());

    // Program ID
    let memo_burn_program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid memo-burn program ID");

    // Derive user global burn statistics PDA
    let (user_global_burn_stats_pda, bump) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", user.pubkey().as_ref()],
        &memo_burn_program_id,
    );

    println!("User Global Burn Statistics PDA: {}", user_global_burn_stats_pda);
    println!("PDA Bump: {}", bump);
    println!();

    // Check if account already exists
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("✅ User global burn statistics account already exists!");
            println!("PDA: {}", user_global_burn_stats_pda);
            println!();
            println!("🔥 IMPORTANT: UserGlobalBurnStats is now REQUIRED for all burn operations!");
            println!("   This account is already initialized and ready to use.");
            return Ok(());
        },
        Err(_) => {
            println!("Account does not exist, proceeding with initialization...");
            println!();
            println!("🔥 IMPORTANT: UserGlobalBurnStats is now REQUIRED for all burn operations!");
            println!("   You must initialize this account before performing any burns.");
        }
    }

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create initialize instruction
    let init_ix = create_initialize_user_global_burn_stats_instruction(
        &memo_burn_program_id,
        &user.pubkey(),
        &user_global_burn_stats_pda,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, init_ix.clone()],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    let optimal_cu = match client.simulate_transaction_with_config(
        &sim_transaction,
        RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: false,
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None,
            accounts: None,
            min_context_slot: None,
            inner_instructions: false,
        },
    ) {
        Ok(result) => {
            if let Some(err) = result.value.err {
                println!("Simulation failed: {:?}", err);
                200_000u32 // Default fallback
            } else if let Some(units_consumed) = result.value.units_consumed {
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32; // 10% margin
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                println!("Simulation successful but no CU data, using default");
                200_000u32
            }
        },
        Err(err) => {
            println!("Simulation failed: {}, using default CU", err);
            200_000u32
        }
    };

    // Create final transaction with optimal compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, init_ix],
        Some(&user.pubkey()),
        &[&user],
        recent_blockhash,
    );

    println!("Sending initialize user global burn statistics transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 INITIALIZATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            println!("User Global Burn Statistics Account Details:");
            println!("  User: {}", user.pubkey());
            println!("  PDA: {}", user_global_burn_stats_pda);
            println!("  Initial total_burned: 0 tokens");
            println!("  Initial burn_count: 0");
            println!();
            println!("✅ You can now use memo-burn operations with global burn statistics tracking!");
            println!();
            println!("🔥 REMINDER: UserGlobalBurnStats is now REQUIRED for all burn operations.");
            println!("   All future burn transactions will automatically update these statistics.");
        },
        Err(err) => {
            println!("❌ INITIALIZATION FAILED!");
            println!("Error: {}", err);
            
            // Provide helpful error guidance
            if err.to_string().contains("already in use") {
                println!("💡 The account may already exist. Check if initialization was already completed.");
            } else if err.to_string().contains("insufficient funds") {
                println!("💡 Insufficient SOL balance. You need SOL to pay for account creation.");
            } else {
                println!("💡 Check network connection and program deployment status.");
            }
        }
    }

    Ok(())
}

fn create_initialize_user_global_burn_stats_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
) -> Instruction {
    // Calculate Anchor instruction sighash for "initialize_user_global_burn_stats"
    let mut hasher = Sha256::new();
    hasher.update(b"global:initialize_user_global_burn_stats");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // Only the sighash, no additional parameters
    
    let accounts = vec![
        AccountMeta::new(*user, true),                                    // user (signer)
        AccountMeta::new(*user_global_burn_stats_pda, false),            // user_global_burn_stats
        AccountMeta::new_readonly(system_program::id(), false),          // system_program
    ];

    Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        accounts,
    )
}
