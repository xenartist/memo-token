use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::time::Duration;
use std::thread::sleep;

// Test different memo lengths
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

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

    // Define test length ranges with custom test counts
    let test_ranges = vec![
        (69, 100, "Up to 100 bytes", 1, 2),     // 2 tests
        (101, 200, "101-200 bytes", 2, 4),      // 4 tests
        (201, 300, "201-300 bytes", 3, 8),      // 8 tests
        (301, 400, "301-400 bytes", 4, 10),     // 10 tests
        (401, 500, "401-500 bytes", 5, 12),     // 12 tests
        (501, 600, "501-600 bytes", 6, 16),     // 16 tests
        (601, 700, "601-700 bytes", 7, 20),     // 20 tests
    ];
    
    println!("Starting memo length token minting test");
    println!("======================================");
    
    // Get initial balance
    let initial_balance = get_token_balance(&client, &token_account)?;
    println!("Initial token balance: {}", initial_balance);
    
    // Calculate total tests
    let total_tests: usize = test_ranges.iter().map(|&(_, _, _, _, tests)| tests).sum();
    println!("Total planned tests: {}", total_tests);
    
    // Track completed tests
    let mut completed_tests = 0;
    
    // Test each range
    for &(min_len, max_len, description, max_possible, tests_per_range) in &test_ranges {
        println!("\nTesting range: {} (possible tokens: 1-{})", description, max_possible);
        println!("Running {} tests for this range", tests_per_range);
        
        // Collect results for this range
        let mut results = HashMap::new();
        let target_length = (min_len + max_len) / 2; // Use middle of range
        
        for i in 1..=tests_per_range {
            completed_tests += 1;
            println!("  Test #{}/{} (overall: {}/{}): Generating memo with length {}", 
                    i, tests_per_range, completed_tests, total_tests, target_length);
            
            // Generate memo of specified length
            let memo = generate_memo(target_length);
            
            // Get current balance
            let before_balance = get_token_balance(&client, &token_account)?;
            
            // Execute mint
            let signature = mint_with_memo(&client, &payer, &program_id, &mint, 
                                          &mint_authority_pda, &token_account, &memo)?;
            
            // Wait for confirmation and get new balance
            sleep(Duration::from_secs(2));
            let after_balance = get_token_balance(&client, &token_account)?;
            
            // Calculate tokens received
            let tokens_received = (after_balance - before_balance) as u64;
            
            // Update results statistics
            *results.entry(tokens_received).or_insert(0) += 1;
            
            println!("  Transaction signature: {}", signature);
            println!("  Tokens received: {}", tokens_received);
        }
        
        // Display statistics for this range
        println!("\nStatistics for range {}:", description);
        println!("------------------------");
        for (tokens, count) in results.iter() {
            let percentage = (count * 100) / tests_per_range;
            println!("  Received {} tokens: {} times ({}%)", 
                    tokens, count, percentage);
        }
        
        // Check if all possible token values were observed
        let unique_values = results.keys().count();
        let expected_values = max_possible as usize;
        if unique_values < expected_values {
            println!("  Note: Only observed {} out of {} possible token values", 
                    unique_values, expected_values);
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
    
    memo
}

// Get token balance
fn get_token_balance(client: &RpcClient, token_account: &Pubkey) -> Result<f64, Box<dyn std::error::Error>> {
    match client.get_token_account_balance(token_account) {
        Ok(balance) => Ok(balance.ui_amount.unwrap_or(0.0)),
        Err(e) => Err(Box::new(e)),
    }
}

// Mint tokens with specified memo
fn mint_with_memo(
    client: &RpcClient,
    payer: &solana_sdk::signer::keypair::Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    memo_text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

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

    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[memo_ix, mint_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    let signature = client.send_and_confirm_transaction(&transaction)?;
    
    Ok(signature.to_string())
} 