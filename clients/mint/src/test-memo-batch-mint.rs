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
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};
use std::str::FromStr;
use sha2::{Sha256, Digest};
use borsh::{BorshSerialize, BorshDeserialize};
use rand::{thread_rng, Rng};
use chrono::Utc;

// Import token-2022 program id
use spl_token_2022::id as token_2022_id;

// simplified borsh data structure - only one string field
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct SimpleMintMemoData {
    /// single string field, used to control the total length of the memo
    pub content: String,
}

// simplified random borsh memo generation function - use ASCII, no Base64
fn create_random_valid_borsh_memo() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut rng = thread_rng();
    
    // generate random length ASCII content
    let content_length = rng.gen_range(50..500);
    let content = format!("Batch mint test at {} - {}", 
                         Utc::now().to_rfc3339(),
                         "x".repeat(content_length));
    
    let borsh_data = SimpleMintMemoData { content };
    
    // serialize to Borsh, directly use (no Base64)
    let borsh_bytes = borsh_data.try_to_vec()?;
    
    // check if the length is reasonable
    if borsh_bytes.len() < 69 {
        return Err("Generated memo too short".into());
    }
    if borsh_bytes.len() > 800 {
        return Err("Generated memo too long".into());
    }
    
    Ok(borsh_bytes)
}

// create exact length memo - use ASCII, no Base64
fn create_exact_length_memo(target_length: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // directly calculate the required string length
    // Borsh serialization: 4 bytes (string length) + string content
    if target_length < 4 {
        return Err("Target length too small for Borsh string".into());
    }
    
    let content_length = target_length - 4; // subtract 4 bytes length prefix
    let content = "x".repeat(content_length);
    
    let borsh_data = SimpleMintMemoData { content };
    let borsh_bytes = borsh_data.try_to_vec()?;
    
    println!("Generated memo: content_length={}, borsh_length={}, target={}", 
            content_length, borsh_bytes.len(), target_length);
    
    if borsh_bytes.len() == target_length {
        println!("🎯 Found exact target length: {} bytes", target_length);
        Ok(borsh_bytes)
    } else {
        println!("⚠️  Length mismatch: got {} bytes (target: {})", borsh_bytes.len(), target_length);
        Ok(borsh_bytes) // return the closest result
    }
}

// Constants matching the contract patterns
const BATCH_MINT_MEMO_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "mint";
const EXPECTED_OPERATION: &str = "batch_mint";

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Token batch mint test client (BORSH FORMAT) ===\n");
    
    // Get command line arguments - support direct parameter format
    let args: Vec<String> = std::env::args().collect();
    
    // Parse command line arguments - the first parameter is the mode
    let test_mode = if args.len() > 1 {
        args[1].clone()
    } else {
        "valid-memo".to_string() // default mode
    };

    // Show execution plan
    match test_mode.as_str() {
        "valid-memo" => {
            println!("Execution plan: infinite mint operation with random memo lengths");
        },
        "memo-69" => {
            println!("Execution plan: infinite mint operation testing minimum memo length (69 bytes)");
        },
        "memo-800" => {
            println!("Execution plan: infinite mint operation testing maximum memo length (800 bytes)");
        },
        _ => {
            print_usage();
            return Ok(());
        }
    }
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Start batch mint operation (infinite loop)
    let mut completed_mints = 0u64;
    let mut successful_mints = 0u64;
    let mut total_tokens_minted = 0u64;
    
    loop {
        completed_mints += 1;
        
        // Get current token balance (raw lamports)
        let balance_before = get_token_balance_raw(&client, &token_account);
        
        // Generate memo based on test mode
        let memo_bytes = match test_mode.as_str() {
            "valid-memo" => create_random_valid_borsh_memo()?,
            "memo-69" => create_exact_length_memo(69)?,
            "memo-800" => create_exact_length_memo(800)?,
            _ => return Err("Invalid test mode".into())
        };
        
        println!("\n🔄 Execute the {}th mint operation", completed_mints);
        println!("Memo length: {} bytes (Borsh format)", memo_bytes.len());
        
        // Debug: Show first 50 bytes of memo as ASCII (no Base64) 
        if memo_bytes.len() <= 100 {
            if let Ok(ascii_str) = std::str::from_utf8(&memo_bytes) {
                println!("Memo preview: {}", ascii_str);
            } else {
                println!("Memo preview: [binary data, {} bytes]", memo_bytes.len());
            }
        } else {
            if let Ok(ascii_str) = std::str::from_utf8(&memo_bytes[..50]) {
                println!("Memo preview: {}...", ascii_str);
            } else {
                println!("Memo preview: [binary data, {} bytes]", memo_bytes.len());
            }
        }
        
        // Create memo instruction
        let memo_ix = spl_memo::build_memo(&memo_bytes, &[&payer.pubkey()]);
        
        // Create mint instruction
        let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
        
        // Execute transaction
        match execute_transaction(&client, &payer, vec![memo_ix, mint_ix], &format!("batch mint #{}", completed_mints)) {
            Ok(signature) => {
                successful_mints += 1;
                
                // Check token balance change
                let balance_after = get_token_balance_raw(&client, &token_account);
                let raw_minted = if balance_after >= balance_before {
                    balance_after - balance_before
                } else {
                    println!("   ⚠️  Warning: balance_after ({}) < balance_before ({})", balance_after, balance_before);
                    0
                };
                
                total_tokens_minted += raw_minted;
                
                println!("✅ Transaction successful!");
                println!("   Signature: {}", signature);
                println!("   Token balance change: {} -> {} lamports", balance_before, balance_after);
                println!("   Tokens minted this time: {} lamports ({})", raw_minted, format_token_amount(raw_minted));
                
                // Show mint stage information
                let (is_valid, description) = validate_mint_amount(raw_minted);
                if is_valid {
                    println!("   📊 Mint stage: {}", description);
                } else {
                    println!("   ⚠️  {}", description);
                }
                
                println!("   Cumulative successful: {}/{}", successful_mints, completed_mints);
                println!("   Total tokens accumulated: {} lamports ({})", total_tokens_minted, format_token_amount(total_tokens_minted));
            },
            Err(e) => {
                println!("❌ Transaction failed!");
                println!("   Error: {}", e);
                
                // Check for specific errors
                if e.to_string().contains("SupplyLimitReached") {
                    println!("   🚫 SUPPLY LIMIT REACHED!");
                    println!("   📊 Maximum supply of 10 trillion tokens has been reached");
                    println!("   🛑 Stopping batch mint operation");
                    println!("   ✅ Contract protection is working correctly");
                    break;
                }
                
                println!("   Cumulative successful: {}/{}", successful_mints, completed_mints);
            }
        }
        
        // Add a small delay to avoid overwhelming the network
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // Show final statistics
    println!("\n📊 Batch mint execution statistics:");
    println!("   Test mode: {}", test_mode);
    println!("   Total execution times: {}", completed_mints);
    println!("   Successful times: {}", successful_mints);
    println!("   Failed times: {}", completed_mints - successful_mints);
    println!("   Success rate: {:.2}%", (successful_mints as f64 / completed_mints as f64) * 100.0);
    println!("   Total tokens minted: {} lamports ({})", total_tokens_minted, format_token_amount(total_tokens_minted));
    
    // Final balance check
    let final_balance = get_token_balance_raw(&client, &token_account);
    println!("   Final token balance: {} lamports ({})", final_balance, format_token_amount(final_balance));
    
    Ok(())
}

fn print_usage() {
    println!("Usage: cargo run --bin test-memo-batch-mint [mode]");
    println!();
    println!("Parameters:");
    println!("  mode   - Test mode:");
    println!("           valid-memo - Random memo lengths (default)");
    println!("           memo-69    - Test minimum length (69 bytes)");
    println!("           memo-800   - Test maximum length (800 bytes)");
    println!();
    println!("Examples:");
    println!("  cargo run --bin test-memo-batch-mint                # Random lengths");
    println!("  cargo run --bin test-memo-batch-mint valid-memo     # Random lengths");
    println!("  cargo run --bin test-memo-batch-mint memo-69        # 69 bytes");
    println!("  cargo run --bin test-memo-batch-mint memo-800       # 800 bytes");
}

fn get_token_balance_raw(client: &RpcClient, token_account: &Pubkey) -> u64 {
    // Try multiple times to ensure consistency
    for attempt in 0..3 {
        match client.get_account(token_account) {
            Ok(account) => {
                // Parse the token account data to get the raw amount (in lamports)
                if account.data.len() >= 72 { // SPL Token account is 165 bytes, amount is at offset 64-72
                    let amount_bytes = &account.data[64..72];
                    let balance = u64::from_le_bytes(amount_bytes.try_into().unwrap_or([0; 8]));
                    return balance;
                } else {
                    println!("   ⚠️  Warning: Token account data too short (attempt {})", attempt + 1);
                }
            },
            Err(e) => {
                println!("   ⚠️  Warning: Failed to get token account balance (attempt {}): {}", attempt + 1, e);
            }
        }
        
        // If failed, wait a little bit and try again
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }
    
    println!("   ❌ Failed to read token balance after 3 attempts, returning 0");
    0
}

fn format_token_amount(raw_amount: u64) -> String {
    // Convert raw lamports to tokens with 6 decimal places
    match raw_amount {
        1_000_000 => "1.0".to_string(),
        100_000 => "0.1".to_string(),
        10_000 => "0.01".to_string(),
        1_000 => "0.001".to_string(),
        100 => "0.0001".to_string(),
        10 => "0.00001".to_string(),
        1 => "0.000001".to_string(),
        0 => "0".to_string(),
        _ => {
            let tokens = raw_amount as f64 / 1_000_000.0;
            format!("{:.6}", tokens)
        }
    }
}

fn validate_mint_amount(raw_amount: u64) -> (bool, String) {
    match raw_amount {
        1_000_000 => (true, "1.0 token (stage 1: 0-100M supply)".to_string()),
        100_000 => (true, "0.1 token (stage 2: 100M-1B supply)".to_string()),
        10_000 => (true, "0.01 token (stage 3: 1B-10B supply)".to_string()),
        1_000 => (true, "0.001 token (stage 4: 10B-100B supply)".to_string()),
        100 => (true, "0.0001 token (stage 5: 100B-1T supply)".to_string()),
        1 => (true, "0.000001 token (stage 6: 1T+ supply)".to_string()),
        0 => (false, "No tokens minted - supply limit reached".to_string()),
        _ => (false, format!("Unexpected amount: {} lamports ({:.6} tokens)", raw_amount, raw_amount as f64 / 1_000_000.0)),
    }
}

fn create_rpc_client() -> RpcClient {
    let rpc_url = get_rpc_url();
    println!("Connect to: {}", rpc_url);
    RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed())
}

fn load_payer_keypair() -> solana_sdk::signature::Keypair {
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Cannot read payer keypair file");
    println!("Use payer: {}", payer.pubkey());
    payer
}

fn get_program_addresses() -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let program_id = get_program_id("memo_mint").expect("Failed to get memo_mint program ID");
    let mint_address = get_token_mint("memo_token").expect("Failed to get memo_token mint address");
    
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Cannot read keypair file");
    
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_address,
        &token_2022_id(),
    );
    
    println!("Program ID: {}", program_id);
    println!("Mint address: {}", mint_address);
    println!("Mint authority PDA: {}", mint_authority_pda);
    println!("Token account: {}", token_account);
    println!();
    
    (program_id, mint_address, mint_authority_pda, token_account)
}

fn ensure_token_account_exists(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    mint_address: &Pubkey,
    token_account: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    match client.get_account(token_account) {
        Ok(_) => {
            println!("✅ Token account exists: {}", token_account);
        },
        Err(_) => {
            println!("⚠️  Token account does not exist, creating...");
            
            let create_ata_ix = create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                mint_address,
                &token_2022_id(),
            );
            
            let recent_blockhash = client.get_latest_blockhash()?;
            
            let transaction = Transaction::new_signed_with_payer(
                &[create_ata_ix],
                Some(&payer.pubkey()),
                &[payer],
                recent_blockhash,
            );
            
            match client.send_and_confirm_transaction(&transaction) {
                Ok(signature) => {
                    println!("✅ Token account created successfully!");
                    println!("   Signature: {}", signature);
                    println!("   Account: {}", token_account);
                },
                Err(e) => {
                    return Err(format!("Failed to create token account: {}", e).into());
                }
            }
        }
    }
    
    Ok(())
}

fn create_mint_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
) -> Instruction {
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_mint");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let accounts = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false),
    ];
    
    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn execute_transaction(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
    _test_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create transaction for simulation with correct instruction order
    // IMPORTANT: Memo must be at index 0 for contract validation
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
    let mut sim_instructions = instructions.clone();
    sim_instructions.push(dummy_compute_budget_ix);
    
    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_instructions,
        Some(&payer.pubkey()),
        &[payer],
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
            if let Some(_err) = result.value.err {
                let default_cu = 300_000u32;
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                optimal_cu
            } else {
                300_000u32
            }
        },
        Err(_) => 300_000u32
    };
    
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    
    // Create final transaction with memo first (index 0), then mints, then compute budget
    // IMPORTANT: Contract now requires memo at index 0
    let mut final_instructions = instructions;
    final_instructions.push(compute_budget_ix);
    
    let transaction = Transaction::new_signed_with_payer(
        &final_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => Ok(signature.to_string()),
        Err(e) => Err(e.into()),
    }
} 