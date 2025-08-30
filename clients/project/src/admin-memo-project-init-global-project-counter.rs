use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use sha2::{Sha256, Digest};
use solana_system_interface::program as system_program;

// This should match the AUTHORIZED_ADMIN in the contract
const AUTHORIZED_ADMIN: &str = "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-PROJECT INITIALIZE GLOBAL COUNTER (ADMIN ONLY) ===");
    println!("This is a one-time setup operation to initialize the global project counter.");
    println!("Only the authorized admin can perform this operation.");
    println!();

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet (must be the authorized admin)
    let admin = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Verify that the loaded keypair is the authorized admin
    let expected_admin = Pubkey::from_str(AUTHORIZED_ADMIN)
        .expect("Invalid AUTHORIZED_ADMIN address");
    
    if admin.pubkey() != expected_admin {
        println!("❌ AUTHORIZATION ERROR!");
        println!("   Current wallet: {}", admin.pubkey());
        println!("   Required admin:  {}", expected_admin);
        println!();
        println!("This operation requires the authorized admin wallet.");
        println!("Please ensure you're using the correct admin keypair file.");
        return Ok(());
    }

    println!("✅ Admin authorization verified!");
    println!("   Admin address: {}", admin.pubkey());
    println!();

    // Program address
    let memo_project_program_id = Pubkey::from_str("ENVapgjzzMjbRhLJ279yNsSgaQtDYYVgWq98j54yYnyx")
        .expect("Invalid memo-project program ID");

    // Calculate global counter PDA
    let (global_counter_pda, bump) = Pubkey::find_program_address(
        &[b"global_counter"],
        &memo_project_program_id,
    );

    println!("Program addresses:");
    println!("  Memo-project program: {}", memo_project_program_id);
    println!("  Admin: {}", admin.pubkey());
    println!("  Global counter PDA: {}", global_counter_pda);
    println!("  PDA bump: {}", bump);
    println!();

    // Check if global counter already exists
    match client.get_account(&global_counter_pda) {
        Ok(account) => {
            println!("✅ Global project counter already exists!");
            println!("   Account owner: {}", account.owner);
            println!("   Data length: {} bytes", account.data.len());
            
            // Verify it's owned by the correct program
            if account.owner == memo_project_program_id {
                println!("   ✅ Owned by memo-project program");
            } else {
                println!("   ⚠️  Owned by different program: {}", account.owner);
            }
            
            if account.data.len() >= 16 { // 8 bytes discriminator + 8 bytes u64
                let total_projects_bytes = &account.data[8..16];
                let total_projects = u64::from_le_bytes(total_projects_bytes.try_into().unwrap());
                println!("   Current total_projects: {}", total_projects);
            }
            
            println!();
            println!("No action needed. The global project counter is already initialized.");
            return Ok(());
        },
        Err(_) => {
            println!("ℹ️  Global project counter not found. Proceeding with initialization...");
        }
    }

    // Check admin SOL balance
    let admin_balance = client.get_balance(&admin.pubkey())?;
    let admin_sol = admin_balance as f64 / 1_000_000_000.0; // Convert lamports to SOL
    println!("Admin SOL balance: {:.4} SOL", admin_sol);
    
    if admin_sol < 0.01 {
        println!("⚠️  Warning: Low SOL balance. You may need more SOL to pay for transaction fees.");
        if admin_sol < 0.001 {
            println!("❌ ERROR: Insufficient SOL balance for transaction fees.");
            return Ok(());
        }
    }
    println!();

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create initialize_global_counter instruction
    let init_counter_ix = create_initialize_global_counter_instruction(
        &memo_project_program_id,
        &admin.pubkey(),
        &global_counter_pda,
    );

    // Simulate transaction to get optimal CU limit
    println!("Simulating transaction to calculate optimal compute units...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(500_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, init_counter_ix.clone()],
        Some(&admin.pubkey()),
        &[&admin],
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
                println!("Simulation shows expected error: {:?}", err);
                200_000u32
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% margin
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("Simulation consumed {} CUs, setting limit to {} CUs (+10% margin)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 200_000u32;
                println!("Simulation successful but no CU data, using default: {}", default_cu);
                default_cu
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
        &[compute_budget_ix, init_counter_ix],
        Some(&admin.pubkey()),
        &[&admin],
        recent_blockhash,
    );

    println!("Sending initialize global project counter transaction with {} compute units...", optimal_cu);
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 INITIALIZATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!();
            
            // Verify the account was created correctly
            match client.get_account(&global_counter_pda) {
                Ok(account) => {
                    println!("✅ Global project counter account created:");
                    println!("   PDA: {}", global_counter_pda);
                    println!("   Owner: {}", account.owner);
                    println!("   Data length: {} bytes", account.data.len());
                    
                    if account.data.len() >= 16 {
                        let total_projects_bytes = &account.data[8..16];
                        let total_projects = u64::from_le_bytes(total_projects_bytes.try_into().unwrap());
                        println!("   Initial total_projects: {}", total_projects);
                    }
                    
                    println!();
                    println!("✅ The memo-project program is now ready to accept project creations!");
                    println!("   Next project created will have ID: 0");
                    println!("   Users can now create projects by burning tokens.");
                },
                Err(e) => {
                    println!("⚠️  Could not fetch created account: {}", e);
                }
            }
        },
        Err(err) => {
            println!("❌ INITIALIZATION FAILED!");
            println!("Error: {}", err);
            
            // Provide helpful error analysis
            let error_msg = err.to_string();
            if error_msg.contains("UnauthorizedAdmin") {
                println!("💡 Authorization Error: Only the authorized admin can initialize the global counter.");
                println!("   Expected admin: {}", expected_admin);
                println!("   Current wallet: {}", admin.pubkey());
            } else if error_msg.contains("already in use") {
                println!("💡 The global counter account already exists. This is normal if initialization was run before.");
            } else if error_msg.contains("insufficient funds") {
                println!("💡 Insufficient SOL balance. Please add more SOL to the admin wallet.");
            } else if error_msg.contains("Invalid program id") {
                println!("💡 Check that the memo-project program is deployed and the program ID is correct.");
            } else {
                println!("💡 Unexpected error. Please check the program deployment and network connection.");
            }
        }
    }

    Ok(())
}

fn create_initialize_global_counter_instruction(
    program_id: &Pubkey,
    admin: &Pubkey,
    global_counter: &Pubkey,
) -> Instruction {
    // Calculate Anchor instruction sighash for "initialize_global_counter"
    let mut hasher = Sha256::new();
    hasher.update(b"global:initialize_global_counter");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // Only the sighash, no additional parameters

    let accounts = vec![
        AccountMeta::new(*admin, true),                            // admin (signer, must be authorized)
        AccountMeta::new(*global_counter, false),                  // global_counter (PDA to be created)
        AccountMeta::new_readonly(system_program::id(), false),    // system_program
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}
