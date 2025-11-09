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

const BURN_MEMO_VERSION: u8 = 1;
const BURN_AMOUNT_TOKENS: u64 = 1; // Burn 1 token
const DECIMAL_FACTOR: u64 = 1_000_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         MEMO-BURN SMOKE TEST (Single Transaction)           ║");
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

    // Check if user global burn statistics account exists
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("✅ User global burn statistics account found");
        },
        Err(_) => {
            println!("❌ User global burn statistics account not found");
            println!("💡 Please run smoke-test-memo-burn-initialize first");
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ❌ SMOKE TEST FAILED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            return Err("User global burn statistics account not initialized".into());
        }
    }

    // Check token balance
    let balance_before = match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("💰 Initial Balance");
            println!("─────────────────────────────────────────────────────────────────");
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

    println!("─────────────────────────────────────────────────────────────────");
    println!("🔥 Burning Tokens");
    println!("─────────────────────────────────────────────────────────────────");
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
                println!("💰 Final Balance");
                println!("─────────────────────────────────────────────────────────────────");
                println!("Balance Before: {} tokens", balance_before);
                println!("Balance After:  {} tokens", balance_after);
                println!("Burned:         {} tokens", burned);
                println!();
                
                // Verify burn amount
                let expected_burned = BURN_AMOUNT_TOKENS as f64;
                let tolerance = 0.000001; // Allow small floating point difference
                
                if (burned - expected_burned).abs() < tolerance {
                    println!("✅ Burn amount verified: {} tokens", burned);
                } else {
                    println!("⚠️  Warning: Burn amount mismatch");
                    println!("   Expected: {} tokens", expected_burned);
                    println!("   Actual:   {} tokens", burned);
                }
            }
            
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ✅ SMOKE TEST PASSED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            
            Ok(())
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

