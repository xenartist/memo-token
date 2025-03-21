use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::time::Duration;
use std::thread::sleep;

// Test different memo lengths using maximum length in each range
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to network with confirmed commitment for better transaction info
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let commitment_config = CommitmentConfig::confirmed();
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), commitment_config);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and token addresses
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")
        .expect("Invalid mint address");

    // Calculate PDA
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Define test length ranges with custom test counts and required CU
    let test_ranges = vec![
        (69, 100, "Up to 100 bytes", 1, 2, 120_000),     // 2 tests, 120k CU
        (101, 200, "101-200 bytes", 2, 4, 160_000),      // 4 tests, 160k CU
        (201, 300, "201-300 bytes", 3, 8, 200_000),      // 8 tests, 200k CU
        (301, 400, "301-400 bytes", 4, 10, 250_000),     // 10 tests, 250k CU
        (401, 500, "401-500 bytes", 5, 12, 300_000),     // 12 tests, 300k CU
        (501, 600, "501-600 bytes", 6, 16, 350_000),     // 16 tests, 350k CU
        (601, 700, "601-700 bytes", 7, 20, 400_000),     // 20 tests, 400k CU
    ];
    
    println!("Starting memo length token minting test");
    println!("======================================");
    
    // Get initial balance
    let initial_balance = get_token_balance(&client, &token_account)?;
    println!("Initial token balance: {}", initial_balance);
    
    // Calculate total tests
    let total_tests: usize = test_ranges.iter().map(|&(_, _, _, _, tests, _)| tests).sum();
    println!("Total planned tests: {}", total_tests);
    
    // Track completed tests
    let mut completed_tests = 0;
    
    // Test each range
    for &(_, max_len, description, max_possible, tests_per_range, required_cu) in &test_ranges {
        println!("\nTesting range: {} (possible tokens: 1-{})", description, max_possible);
        println!("Running {} tests with memo length of {} bytes", tests_per_range, max_len);
        println!("Required CU for this range: {}", required_cu);
        
        // Collect results for this range
        let mut results = HashMap::new();
        let target_length = max_len; // Use maximum length in the range
        
        for i in 1..=tests_per_range {
            completed_tests += 1;
            println!("  Test #{}/{} (overall: {}/{}): Using memo with length {}", 
                    i, tests_per_range, completed_tests, total_tests, target_length);
            
            // Generate memo of specified length
            let memo = generate_memo(target_length);
            
            // Get current balance
            let before_balance = get_token_balance(&client, &token_account)?;
            
            // Execute mint with appropriate CU limit
            let signature = mint_with_memo(
                &client, &payer, &program_id, &mint, 
                &mint_authority_pda, &token_account, &memo, required_cu
            )?;
            
            println!("  Transaction signature: {}", signature);
            
            // Wait for transaction to be finalized
            wait_for_finalized_transaction(&client, &signature)?;
            
            // Get new balance with retry mechanism
            let after_balance = get_token_balance_with_retry(&client, &token_account, 5)?;
            
            // Calculate tokens received
            let tokens_received = (after_balance - before_balance) as u64;
            
            // Update results statistics
            *results.entry(tokens_received).or_insert(0) += 1;
            
            println!("  Tokens received: {}", tokens_received);
            
            // If no tokens received, print warning
            if tokens_received == 0 {
                println!("  WARNING: No tokens received. Please check transaction on explorer: https://explorer.x1.testnet.solana.com/tx/{}", signature);
            }
            
            // Add additional delay between transactions
            println!("  Waiting before next transaction...");
            sleep(Duration::from_secs(3));
        }
        
        // Display statistics for this range
        println!("\nStatistics for range {}:", description);
        println!("------------------------");
        for (tokens, count) in results.iter() {
            let percentage = (count * 100) / tests_per_range;
            println!("  Received {} tokens: {} times ({}%)", 
                    tokens, count, percentage);
        }
    }
    
    // Display summary
    let final_balance = get_token_balance(&client, &token_account)?;
    let total_received = final_balance - initial_balance;
    
    println!("\nTest Summary");
    println!("======================================");
    println!("Initial balance: {}", initial_balance);
    println!("Final balance: {}", final_balance);
    println!("Total tokens received: {}", total_received);
    println!("Average tokens per mint: {:.2}", total_received / total_tests as f64);
    println!("Total mint operations: {}", total_tests);
    
    Ok(())
}

// Generate memo of specified length
fn generate_memo(length: usize) -> String {
    // Use random characters to generate memo, ensuring each is different
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let charset: Vec<char> = charset.chars().collect();
    
    let mut memo = String::with_capacity(length);
    let mut hasher = Sha256::new();
    hasher.update(format!("{:?}", std::time::SystemTime::now()).as_bytes());
    let hash = hasher.finalize();
    
    // Use hash as seed for pseudo-random characters
    for i in 0..length {
        let index = (hash[i % 32] as usize) % charset.len();
        memo.push(charset[index]);
    }
    
    // Ensure exact length
    while memo.len() < length {
        memo.push('X');
    }
    
    memo
}

// Get token balance
fn get_token_balance(client: &RpcClient, token_account: &Pubkey) -> Result<f64, Box<dyn std::error::Error>> {
    match client.get_token_account_balance(token_account) {
        Ok(balance) => Ok(balance.ui_amount.unwrap_or(0.0)),
        Err(e) => Err(Box::new(e)),
    }
}

// Wait for transaction to be finalized
fn wait_for_finalized_transaction(client: &RpcClient, signature: &str) -> Result<(), Box<dyn std::error::Error>> {
    let sig = signature.parse()?;
    
    // Maximum wait time: 60 seconds
    let timeout = Duration::from_secs(60);
    let start = std::time::Instant::now();
    
    while start.elapsed() < timeout {
        match &client.get_signature_statuses(&[sig])?.value[0] {
            Some(status) => {
                if let Some(conf_status) = &status.confirmation_status {
                    // Compare with formatted string
                    let status_debug = format!("{:?}", conf_status);
                    if status_debug.contains("Finalized") {
                        return Ok(());
                    }
                }
            },
            None => {
            }
        }
        
        // Wait before checking again
        sleep(Duration::from_secs(2));
    }
    
    Err("Transaction did not reach finalized status within timeout".into())
}

// Get token balance with retry mechanism
fn get_token_balance_with_retry(client: &RpcClient, token_account: &Pubkey, max_retries: u8) -> Result<f64, Box<dyn std::error::Error>> {
    let mut retries = 0;
    let mut last_balance = 0.0;
    
    while retries < max_retries {
        match client.get_token_account_balance(token_account) {
            Ok(balance) => {
                let current_balance = balance.ui_amount.unwrap_or(0.0);
                
                // If balance changed from last check, wait a bit more to ensure it's stable
                if retries > 0 && current_balance != last_balance {
                    println!("    Balance changed from {} to {}, waiting to stabilize...", last_balance, current_balance);
                    sleep(Duration::from_secs(2));
                    last_balance = current_balance;
                    continue;
                }
                
                return Ok(current_balance);
            },
            Err(e) => {
                println!("    Error getting balance (retry {}/{}): {}", retries + 1, max_retries, e);
                retries += 1;
                if retries >= max_retries {
                    return Err(Box::new(e));
                }
                sleep(Duration::from_secs(2));
            }
        }
    }
    
    Ok(last_balance)
}

// Mint tokens with specified memo and CU limit
fn mint_with_memo(
    client: &RpcClient,
    payer: &solana_sdk::signer::keypair::Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    memo_text: &str,
    compute_units: u32,
) -> Result<String, Box<dyn std::error::Error>> {
    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

    // Create compute budget instruction to request specific CU amount
    let compute_budget_ix = solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(compute_units);

    // Create mint instruction
    let mint_ix = Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),         // user
            AccountMeta::new(*mint, false),                  // mint
            AccountMeta::new(*mint_authority_pda, false),    // mint_authority (PDA)
            AccountMeta::new(*token_account, false),         // token_account
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
            AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        ],
    );

    // Create memo instruction
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create and send transaction with compute budget instruction first
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, mint_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send transaction
    let signature = client.send_transaction(&transaction)?;
    
    // Don't wait for confirmation here, we'll do that separately
    
    Ok(signature.to_string())
} 