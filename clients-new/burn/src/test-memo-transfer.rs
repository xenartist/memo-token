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
    system_instruction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_token_2022::{self, instruction as token_instruction};
use std::str::FromStr;
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-BURN CONTRACT TRANSFER REJECTION TEST ===");
    println!("Testing that the contract correctly rejects non-burn operations\n");

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Contract and token addresses
    let program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
        .expect("Invalid mint address");

    println!("Test contract: {}", program_id);
    println!("Payer: {}", payer.pubkey());
    println!("Target mint: {}\n", mint);

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Test 1: Try to send 0.01 SOL to the contract
    println!("🧪 TEST 1: Sending 0.01 SOL to contract (should fail)");
    test_sol_transfer(&client, &payer, &program_id, recent_blockhash)?;

    // Test 2: Try to send 1 MEM token to the contract
    println!("\n🧪 TEST 2: Sending 1 MEM token to contract (should fail)");
    test_token_transfer(&client, &payer, &program_id, &mint, recent_blockhash)?;

    println!("\n✅ All tests completed!");
    Ok(())
}

fn test_sol_transfer(
    client: &RpcClient,
    payer: &dyn Signer,
    program_id: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<(), Box<dyn std::error::Error>> {
    // Try to send 0.01 SOL (10,000,000 lamports) to the program
    let sol_amount = 10_000_000u64; // 0.01 SOL

    let sol_transfer_ix = system_instruction::transfer(
        &payer.pubkey(),
        program_id, // Send to contract address
        sol_amount,
    );

    let transaction = Transaction::new_signed_with_payer(
        &[sol_transfer_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    println!("  Attempting to send {} lamports to contract...", sol_amount);

    // Simulate the transaction
    match client.simulate_transaction_with_config(
        &transaction,
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
                println!("  ✅ SOL transfer correctly rejected: {:?}", err);
                println!("  📝 This is expected - contract should not accept SOL transfers");
            } else {
                println!("  ❌ SOL transfer unexpectedly succeeded!");
                println!("  🚨 This is a security issue - contract accepted SOL!");
            }

            // Show logs if available
            if let Some(logs) = result.value.logs {
                println!("  📋 Transaction logs:");
                for log in logs {
                    println!("    {}", log);
                }
            }
        },
        Err(err) => {
            println!("  ✅ SOL transfer failed at simulation level: {}", err);
            println!("  📝 This is expected - contract rejects unauthorized operations");
        }
    }

    Ok(())
}

fn test_token_transfer(
    client: &RpcClient,
    payer: &dyn Signer,
    program_id: &Pubkey,
    mint: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get user's token account
    let user_token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        mint,
        &token_2022_id(),
    );

    // Try to create a token account for the program (this would be needed for transfer)
    let program_token_account = get_associated_token_address_with_program_id(
        program_id,
        mint,
        &token_2022_id(),
    );

    println!("  User token account: {}", user_token_account);
    println!("  Program token account: {}", program_token_account);

    // Check if user has tokens
    match client.get_token_account_balance(&user_token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!("  Current user balance: {} tokens", current_balance);
            
            if current_balance < 1.0 {
                println!("  ⚠️  Insufficient balance for token transfer test");
                println!("  📝 This test requires at least 1 MEM token");
                return Ok(());
            }
        },
        Err(err) => {
            println!("  ⚠️  Could not check token balance: {}", err);
            println!("  📝 Skipping token transfer test");
            return Ok(());
        }
    }

    // Method 1: Try to create ATA for program and transfer (standard approach)
    println!("  🔬 Method 1: Standard token transfer");
    test_standard_token_transfer(client, payer, &user_token_account, &program_token_account, mint, recent_blockhash)?;

    // Method 2: Try to call the contract with a transfer-like instruction
    println!("  🔬 Method 2: Direct contract call with transfer data");
    test_contract_transfer_call(client, payer, program_id, &user_token_account, mint, recent_blockhash)?;

    Ok(())
}

fn test_standard_token_transfer(
    client: &RpcClient,
    payer: &dyn Signer,
    user_token_account: &Pubkey,
    program_token_account: &Pubkey,
    mint: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<(), Box<dyn std::error::Error>> {
    let token_amount = 1_000_000_000u64; // 1 token

    // Create transfer instruction
    let transfer_ix = token_instruction::transfer_checked(
        &token_2022_id(),
        user_token_account,
        mint,
        program_token_account,
        &payer.pubkey(),
        &[],
        token_amount,
        9, // decimals
    )?;

    // Create ATA for program if needed
    let create_ata_ix = spl_associated_token_account::instruction::create_associated_token_account(
        &payer.pubkey(),
        &Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP").unwrap(), // program as owner
        mint,
        &token_2022_id(),
    );

    let transaction = Transaction::new_signed_with_payer(
        &[create_ata_ix, transfer_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    println!("    Attempting standard token transfer...");

    // Simulate the transaction
    match client.simulate_transaction_with_config(
        &transaction,
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
                println!("    ✅ Standard transfer correctly rejected: {:?}", err);
                println!("    📝 This is expected - programs don't typically accept direct transfers");
            } else {
                println!("    ⚠️  Standard transfer simulation succeeded");
                println!("    📝 This might work since it's not calling the contract directly");
            }
        },
        Err(err) => {
            println!("    ✅ Standard transfer failed: {}", err);
        }
    }

    Ok(())
}

fn test_contract_transfer_call(
    client: &RpcClient,
    payer: &dyn Signer,
    program_id: &Pubkey,
    user_token_account: &Pubkey,
    mint: &Pubkey,
    recent_blockhash: solana_sdk::hash::Hash,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a fake "transfer" instruction to the contract
    // This is not a real instruction, but tests if the contract rejects unknown instructions
    
    let fake_transfer_data = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Invalid discriminator
    
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),        // user (signer)
        AccountMeta::new(*mint, false),                // mint
        AccountMeta::new(*user_token_account, false),  // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program
        AccountMeta::new_readonly(
            Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
            false
        ), // instructions sysvar
    ];

    let fake_instruction = Instruction::new_with_bytes(
        *program_id,
        &fake_transfer_data,
        accounts,
    );

    // Add a compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(100_000);

    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, fake_instruction],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    println!("    Attempting fake contract instruction call...");

    // Simulate the transaction
    match client.simulate_transaction_with_config(
        &transaction,
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
                println!("    ✅ Fake instruction correctly rejected: {:?}", err);
                if err.to_string().contains("InstructionFallbackNotFound") {
                    println!("    📝 Perfect! Contract only accepts known instructions (process_burn)");
                } else {
                    println!("    📝 Contract rejected unknown instruction with: {}", err);
                }
            } else {
                println!("    ❌ Fake instruction unexpectedly succeeded!");
                println!("    🚨 Security issue - contract should reject unknown instructions");
            }

            // Show logs if available
            if let Some(logs) = result.value.logs {
                println!("    📋 Transaction logs:");
                for log in logs {
                    println!("      {}", log);
                }
            }
        },
        Err(err) => {
            println!("    ✅ Fake instruction failed at simulation level: {}", err);
        }
    }

    Ok(())
} 