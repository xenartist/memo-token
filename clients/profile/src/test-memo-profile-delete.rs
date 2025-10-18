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

#[derive(Debug, Clone)]
struct DeleteParams {
    pub should_succeed: bool,             // Whether the test should succeed
    pub test_description: String,         // Description of what this test validates
}

// Get RPC URL from environment or use default testnet
fn get_rpc_url() -> String {
    std::env::var("X1_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let test_case = &args[1];
    
    // Define test cases
    let test_params = match test_case.as_str() {
        "delete-existing" => DeleteParams {
            should_succeed: true,
            test_description: "Delete an existing profile".to_string(),
        },
        "delete-nonexistent" => DeleteParams {
            should_succeed: false,
            test_description: "Try to delete a non-existent profile".to_string(),
        },
        _ => {
            println!("Unknown test case: {}", test_case);
            print_usage();
            return Ok(());
        }
    };

    println!("=== MEMO PROFILE DELETE TEST ===");
    println!("Test case: {}", test_case);
    println!("Description: {}", test_params.test_description);
    println!();

    // Constants
    let rpc_url = get_rpc_url();
    let wallet_path = std::env::var("WALLET_PATH").unwrap_or_else(|_| {
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        format!("{}/.config/solana/id.json", home)
    });

    println!("RPC URL: {}", rpc_url);
    println!("Wallet: {}", wallet_path);

    // Create RPC client
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load wallet
    let wallet_path_expanded = shellexpand::tilde(&wallet_path).to_string();
    let payer = read_keypair_file(&wallet_path_expanded)
        .map_err(|e| format!("Failed to read keypair from {}: {}", wallet_path_expanded, e))?;

    println!("User: {}", payer.pubkey());

    // Program ID
    let memo_profile_program_id = Pubkey::from_str("BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US")?;

    println!("Memo Profile Program: {}", memo_profile_program_id);

    // Derive profile PDA
    let (profile_pda, profile_bump) = Pubkey::find_program_address(
        &[b"profile", payer.pubkey().as_ref()],
        &memo_profile_program_id,
    );

    println!("Profile PDA: {}", profile_pda);
    println!("Profile Bump: {}", profile_bump);

    // Check current state
    println!();
    println!("=== PRE-DELETE STATE ===");
    
    let profile_exists = check_profile_exists(&client, &profile_pda);
    let initial_balance = client.get_balance(&payer.pubkey())?;

    println!("Profile exists: {}", profile_exists);
    println!("User balance: {:.6} SOL", initial_balance as f64 / 1_000_000_000.0);

    // Handle test case logic
    if test_case == "delete-nonexistent" && profile_exists {
        println!("⚠️  Profile exists, but test expects non-existent profile.");
        println!("   Please delete the profile first or use a different wallet for this test.");
        return Ok(());
    }

    if test_case == "delete-existing" && !profile_exists {
        println!("❌ Profile does not exist. Create a profile first using test-memo-profile-create.");
        return Ok(());
    }

    // Create the profile deletion instruction
    let delete_instruction = create_delete_profile_instruction(
        &memo_profile_program_id,
        &payer.pubkey(),
        &profile_pda,
    )?;

    // Prepare instructions for simulation
    let sim_instructions = vec![delete_instruction.clone()];

    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;

    // Create simulation transaction with high CU limit
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    let mut sim_transaction_instructions = vec![compute_budget_ix];
    sim_transaction_instructions.extend(sim_instructions.clone());

    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_transaction_instructions,
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!();
    println!("=== COMPUTE UNIT OPTIMIZATION ===");
    println!("Simulating transaction to calculate optimal compute units...");

    // Simulate to get optimal CU
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
                if !test_params.should_succeed {
                    println!("✅ EXPECTED FAILURE: Simulation failed as expected");
                    analyze_expected_error(&format!("{:?}", err), &test_params);
                    return Ok(());
                }
                let default_cu = 200_000u32;
                println!("Using default compute units: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add 10% safety margin to actual consumption
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

    println!();
    println!("=== TRANSACTION EXECUTION ===");
    println!("Operation: Delete Profile");
    println!("Expected to succeed: {}", test_params.should_succeed);
    println!("Compute Units: {}", optimal_cu);

    // Create final transaction with optimized CU
    let optimized_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    let final_transaction = Transaction::new_signed_with_payer(
        &[
            optimized_compute_budget_ix,
            delete_instruction,
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send transaction
    println!();
    println!("Sending delete transaction...");
    
    match client.send_and_confirm_transaction(&final_transaction) {
        Ok(signature) => {
            println!("✅ TRANSACTION SUCCESS!");
            println!("Signature: {}", signature);

            // Verify the deletion
            println!();
            println!("=== POST-DELETE VERIFICATION ===");
            
            let profile_exists_after = check_profile_exists(&client, &profile_pda);
            let final_balance = client.get_balance(&payer.pubkey())?;
            
            println!("Profile exists after delete: {}", profile_exists_after);
            println!("User balance after: {:.6} SOL", final_balance as f64 / 1_000_000_000.0);
            
            // Calculate rent reclaimed
            let rent_reclaimed = final_balance.saturating_sub(initial_balance);
            if rent_reclaimed > 0 {
                println!("✅ Rent reclaimed: {:.6} SOL", rent_reclaimed as f64 / 1_000_000_000.0);
            }
            
            // Verify profile was actually deleted
            if !profile_exists_after {
                println!("✅ Profile successfully deleted");
            } else {
                println!("❌ Profile still exists after deletion");
            }

            if !test_params.should_succeed {
                println!("❌ UNEXPECTED SUCCESS: Test should have failed");
            }
        },
        Err(err) => {
            println!("❌ TRANSACTION FAILED!");
            println!("Error: {}", err);
            
            if !test_params.should_succeed {
                println!("✅ EXPECTED FAILURE: Test failed as expected");
                analyze_expected_error(&err.to_string(), &test_params);
            } else {
                println!("❌ UNEXPECTED FAILURE: Test should have succeeded");
                analyze_unexpected_error(&err.to_string());
            }
        }
    }

    Ok(())
}

fn create_delete_profile_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    profile: &Pubkey,
) -> Result<Instruction, Box<dyn std::error::Error>> {
    // Calculate Anchor instruction sighash for "delete_profile"
    let mut hasher = Sha256::new();
    hasher.update(b"global:delete_profile");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // No additional parameters needed
    
    let accounts = vec![
        AccountMeta::new(*user, true),      // user (signer)
        AccountMeta::new(*profile, false),  // profile (will be closed)
    ];
    
    Ok(Instruction::new_with_bytes(*program_id, &instruction_data, accounts))
}

fn check_profile_exists(client: &RpcClient, profile_pda: &Pubkey) -> bool {
    match client.get_account(profile_pda) {
        Ok(_) => true,
        Err(_) => false,
    }
}

fn analyze_expected_error(error_msg: &str, params: &DeleteParams) {
    if error_msg.contains("Account does not exist") && params.test_description.contains("non-existent") {
        println!("✅ Correct: Non-existent profile deletion failed as expected");
    } else if error_msg.contains("UnauthorizedProfileAccess") {
        println!("✅ Correct: Unauthorized access detected");
    } else {
        println!("⚠️  Unexpected error type: {}", error_msg);
    }
}

fn analyze_unexpected_error(error_msg: &str) {
    println!("💡 Error analysis:");
    if error_msg.contains("Account does not exist") {
        println!("   Profile does not exist - create it first");
    } else if error_msg.contains("UnauthorizedProfileAccess") {
        println!("   Profile access authorization failed");
    } else if error_msg.contains("constraint was violated") {
        println!("   Account constraint validation failed");
    } else {
        println!("   Unexpected error type");
    }
}

fn print_usage() {
    println!("Usage: test-memo-profile-delete <test_case>");
    println!();
    println!("Test Cases:");
    println!("  delete-existing     - Delete an existing profile");
    println!("  delete-nonexistent  - Try to delete a non-existent profile (should fail)");
    println!();
    println!("Environment Variables:");
    println!("  RPC_URL      - Solana RPC endpoint (default: testnet)");
    println!("  WALLET_PATH  - Path to wallet keypair file");
    println!();
    println!("Prerequisites:");
    println!("  For 'delete-existing': A profile must exist for the user");
    println!("  For 'delete-nonexistent': No profile should exist for the user");
    println!();
    println!("Effects:");
    println!("  - Profile account will be closed");
    println!("  - Rent will be reclaimed to user's wallet");
}
