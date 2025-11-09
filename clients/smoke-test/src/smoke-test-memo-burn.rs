use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use borsh::{BorshSerialize, BorshDeserialize};
use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;
use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

// Borsh memo structure (must match the contract)
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BurnMemo {
    pub version: u8,
    pub burn_amount: u64,
    pub payload: Vec<u8>,
}

// User global burn statistics structure (must match the contract)
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct UserGlobalBurnStats {
    pub user: Pubkey,
    pub total_burned: u64,
    pub burn_count: u64,
    pub last_burn_time: i64,
    pub bump: u8,
}

const BURN_MEMO_VERSION: u8 = 1;
const BURN_AMOUNT_TOKENS: u64 = 1; // Burn 1 token
const DECIMAL_FACTOR: u64 = 1_000_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║    MEMO-BURN SMOKE TEST (Initialize + Burn + Verify)        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    
    let burn_amount = BURN_AMOUNT_TOKENS * DECIMAL_FACTOR;
    
    // Connect to network
    let rpc_url = get_rpc_url();
    println!("─────────────────────────────────────────────────────────────────");
    println!("📋 Configuration");
    println!("─────────────────────────────────────────────────────────────────");
    println!("RPC URL:        {}", rpc_url);
    
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    
    println!("Payer:          {}", payer.pubkey());

    // Program and token addresses
    let program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    let mint = get_token_mint("memo_token")
        .expect("Failed to get memo_token mint address");
    
    println!("Program ID:     {}", program_id);
    println!("Mint Address:   {}", mint);

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );
    
    println!("Token Account:  {}", token_account);

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &program_id,
    );
    
    println!("Stats PDA:      {}", user_global_burn_stats_pda);
    println!();

    // Step 1: Check and initialize if needed
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔧 Step 1: Initialize User Global Burn Statistics");
    println!("─────────────────────────────────────────────────────────────────");
    
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("✅ Account already exists, skipping initialization");
        },
        Err(_) => {
            println!("📝 Account not found, initializing...");
            initialize_burn_stats(&client, &payer, &program_id, &user_global_burn_stats_pda)?;
            println!("✅ Initialization successful");
        }
    }
    println!();

    // Get stats before burn
    let stats_before = get_burn_stats(&client, &user_global_burn_stats_pda)?;
    println!("─────────────────────────────────────────────────────────────────");
    println!("📊 Statistics Before Burn");
    println!("─────────────────────────────────────────────────────────────────");
    println!("Total Burned:   {} tokens ({} units)", stats_before.total_burned / DECIMAL_FACTOR, stats_before.total_burned);
    println!("Burn Count:     {}", stats_before.burn_count);
    println!("Last Burn Time: {}", if stats_before.last_burn_time == 0 { "Never".to_string() } else { format!("{}", stats_before.last_burn_time) });
    println!();

    // Step 2: Check token balance and burn
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔥 Step 2: Burn Tokens");
    println!("─────────────────────────────────────────────────────────────────");
    
    let balance_before = match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!("Token Balance:  {} tokens", current_balance);
            
            if current_balance < BURN_AMOUNT_TOKENS as f64 {
                println!();
                println!("❌ Insufficient token balance!");
                println!("Required:       {} tokens", BURN_AMOUNT_TOKENS);
                println!("Available:      {} tokens", current_balance);
                println!();
                println!("╔═══════════════════════════════════════════════════════════════╗");
                println!("║                    ❌ SMOKE TEST FAILED                       ║");
                println!("╚═══════════════════════════════════════════════════════════════╝");
                return Err("Insufficient token balance".into());
            }
            current_balance
        },
        Err(err) => {
            println!();
            println!("❌ Error checking token balance: {}", err);
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ❌ SMOKE TEST FAILED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            return Err(err.into());
        }
    };

    // Create Borsh memo with timestamp
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let payload = format!("SMOKE_TEST_BURN_{}_amount_{}_tokens", timestamp, BURN_AMOUNT_TOKENS).into_bytes();
    
    let memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };
    
    let borsh_data = borsh::to_vec(&memo)?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    let memo_bytes = base64_encoded.into_bytes();
    
    println!();
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Memo Details");
    println!("─────────────────────────────────────────────────────────────────");
    println!("Memo Format:    Borsh + Base64");
    println!("Memo Version:   {}", BURN_MEMO_VERSION);
    println!("Burn Amount:    {} tokens ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    println!("Payload Size:   {} bytes", memo.payload.len());
    println!("Borsh Size:     {} bytes", borsh_data.len());
    println!("Base64 Size:    {} bytes", memo_bytes.len());
    println!();

    // Create memo instruction (must be at index 0)
    let memo_ix = spl_memo::build_memo(
        &memo_bytes,
        &[&payer.pubkey()],
    );

    // Create burn instruction data
    let discriminator = [220, 214, 24, 210, 116, 16, 167, 18];
    let mut instruction_data = discriminator.to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // Build accounts list for burn
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),                      // user (signer)
        AccountMeta::new(mint, false),                               // mint
        AccountMeta::new(token_account, false),                      // token_account
        AccountMeta::new(user_global_burn_stats_pda, false),         // user_global_burn_stats
        AccountMeta::new_readonly(token_2022_id(), false),           // token_program
        AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false), // instructions sysvar
    ];

    // Create burn instruction
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create transaction with proper instruction order:
    // [0] memo, [1] burn, [2] compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
    let transaction = Transaction::new_signed_with_payer(
        &[memo_ix, burn_ix, compute_budget_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Sending burn transaction...");
    
    // Send and confirm transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("✅ Burn Successful");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Transaction:    {}", signature);
            println!("Burned Amount:  {} tokens", BURN_AMOUNT_TOKENS);
            println!();
            
            // Check new balance
            if let Ok(balance) = client.get_token_account_balance(&token_account) {
                let balance_after = balance.ui_amount.unwrap_or(0.0);
                let burned = balance_before - balance_after;
                
                println!("─────────────────────────────────────────────────────────────────");
                println!("💰 Token Balance Verification");
                println!("─────────────────────────────────────────────────────────────────");
                println!("Balance Before: {} tokens", balance_before);
                println!("Balance After:  {} tokens", balance_after);
                println!("Burned:         {} tokens", burned);
                
                // Verify burn amount
                let expected_burned = BURN_AMOUNT_TOKENS as f64;
                let tolerance = 0.000001;
                
                if (burned - expected_burned).abs() < tolerance {
                    println!("✅ Token balance verified");
                } else {
                    println!("⚠️  Warning: Burn amount mismatch");
                    println!("   Expected: {} tokens", expected_burned);
                    println!("   Actual:   {} tokens", burned);
                }
            }
            println!();
            
            // Step 3: Verify burn statistics
            println!("─────────────────────────────────────────────────────────────────");
            println!("🔍 Step 3: Verify Burn Statistics");
            println!("─────────────────────────────────────────────────────────────────");
            
            let stats_after = get_burn_stats(&client, &user_global_burn_stats_pda)?;
            
            println!("Statistics After Burn:");
            println!("  Total Burned:   {} tokens ({} units)", stats_after.total_burned / DECIMAL_FACTOR, stats_after.total_burned);
            println!("  Burn Count:     {}", stats_after.burn_count);
            println!("  Last Burn Time: {}", stats_after.last_burn_time);
            println!();
            
            // Verify statistics changes
            let mut all_checks_passed = true;
            
            println!("Verification:");
            
            // Check total burned increased
            let expected_total = stats_before.total_burned + burn_amount;
            if stats_after.total_burned == expected_total {
                println!("  ✅ Total burned increased correctly: {} -> {} (+{} tokens)",
                    stats_before.total_burned / DECIMAL_FACTOR,
                    stats_after.total_burned / DECIMAL_FACTOR,
                    BURN_AMOUNT_TOKENS);
            } else {
                println!("  ❌ Total burned mismatch:");
                println!("     Expected: {} units", expected_total);
                println!("     Actual:   {} units", stats_after.total_burned);
                all_checks_passed = false;
            }
            
            // Check burn count increased
            let expected_count = stats_before.burn_count + 1;
            if stats_after.burn_count == expected_count {
                println!("  ✅ Burn count increased correctly: {} -> {}",
                    stats_before.burn_count, stats_after.burn_count);
            } else {
                println!("  ❌ Burn count mismatch:");
                println!("     Expected: {}", expected_count);
                println!("     Actual:   {}", stats_after.burn_count);
                all_checks_passed = false;
            }
            
            // Check last burn time updated
            if stats_after.last_burn_time > stats_before.last_burn_time {
                println!("  ✅ Last burn time updated: {} -> {}",
                    stats_before.last_burn_time, stats_after.last_burn_time);
            } else {
                println!("  ❌ Last burn time not updated");
                all_checks_passed = false;
            }
            
            // Check user pubkey matches
            if stats_after.user == payer.pubkey() {
                println!("  ✅ User pubkey verified: {}", stats_after.user);
            } else {
                println!("  ❌ User pubkey mismatch:");
                println!("     Expected: {}", payer.pubkey());
                println!("     Actual:   {}", stats_after.user);
                all_checks_passed = false;
            }
            
            println!();
            
            if all_checks_passed {
                println!("╔═══════════════════════════════════════════════════════════════╗");
                println!("║                    ✅ SMOKE TEST PASSED                       ║");
                println!("║                                                               ║");
                println!("║  All verifications passed:                                    ║");
                println!("║  ✓ Account initialization                                     ║");
                println!("║  ✓ Token burn execution                                       ║");
                println!("║  ✓ Balance verification                                       ║");
                println!("║  ✓ Statistics verification                                    ║");
                println!("╚═══════════════════════════════════════════════════════════════╝");
                Ok(())
            } else {
                println!("╔═══════════════════════════════════════════════════════════════╗");
                println!("║                    ⚠️  SMOKE TEST WARNING                     ║");
                println!("║                                                               ║");
                println!("║  Burn succeeded but some verifications failed                 ║");
                println!("╚═══════════════════════════════════════════════════════════════╝");
                Err("Statistics verification failed".into())
            }
        },
        Err(err) => {
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("❌ Burn Failed");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Error: {}", err);
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ❌ SMOKE TEST FAILED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            
            Err(err.into())
        }
    }
}

/// Initialize user global burn statistics account
fn initialize_burn_stats(
    client: &RpcClient,
    payer: &dyn Signer,
    program_id: &Pubkey,
    stats_pda: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create instruction data for initialize_user_global_burn_stats
    let discriminator = [109, 178, 49, 106, 200, 87, 4, 107];
    let instruction_data = discriminator.to_vec();

    // Build accounts list
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*stats_pda, false),
        AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
    ];

    let initialize_ix = Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        accounts,
    );

    let recent_blockhash = client.get_latest_blockhash()?;
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, initialize_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    client.send_and_confirm_transaction(&transaction)?;
    Ok(())
}

/// Get and deserialize burn statistics
fn get_burn_stats(
    client: &RpcClient,
    stats_pda: &Pubkey,
) -> Result<UserGlobalBurnStats, Box<dyn std::error::Error>> {
    let account = client.get_account(stats_pda)?;
    
    // Skip 8-byte discriminator
    if account.data.len() < 8 {
        return Err("Account data too short".into());
    }
    
    let stats = UserGlobalBurnStats::try_from_slice(&account.data[8..])?;
    Ok(stats)
}

