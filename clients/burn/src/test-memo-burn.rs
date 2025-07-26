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
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use serde_json;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse compute units (default: 200_000)
    let initial_compute_units = if args.len() > 1 {
        args[1].parse().unwrap_or(200_000)
    } else {
        200_000
    };
    
    // Parse burn amount (in actual token units for decimal=0)
    let burn_amount_tokens = if args.len() > 2 {
        args[2].parse::<u64>().unwrap_or(1)
    } else {
        1
    };
    // For decimal=0, token amount equals the burn amount directly
    let burn_amount = burn_amount_tokens;

    // Parse custom message (optional) - ensure it's long enough
    let base_message = if args.len() > 3 {
        args[3].clone()
    } else {
        format!("Testing memo-burn contract with {} tokens on decimal=0 system", burn_amount_tokens)
    };

    // Build JSON format memo with enough content to meet 69-byte minimum
    let memo_json = serde_json::json!({
        "message": base_message,
        "amount": burn_amount,
        "operation": "burn",
        "timestamp": format!("{}", chrono::Utc::now().timestamp())
    });
    
    // Convert to string
    let mut memo_text = serde_json::to_string(&memo_json)
        .expect("Failed to serialize JSON");
    
    // Ensure memo meets minimum 69-byte requirement
    while memo_text.as_bytes().len() < 69 {
        let expanded_json = serde_json::json!({
            "message": format!("{} - extended for minimum length requirement", base_message),
            "amount": burn_amount,
            "operation": "burn",
            "timestamp": format!("{}", chrono::Utc::now().timestamp()),
            "padding": "x".repeat(69 - memo_text.as_bytes().len())
        });
        memo_text = serde_json::to_string(&expanded_json)
            .expect("Failed to serialize expanded JSON");
    }

    // Print detailed information
    println!("=== MEMO-BURN CONTRACT TEST (DECIMAL=0, MIN 69 BYTES) ===");
    println!("Burn amount: {} tokens (decimal=0, so {} units)", burn_amount_tokens, burn_amount);
    println!("Memo length: {} bytes (minimum required: 69)", memo_text.as_bytes().len());
    println!("Memo content:");
    println!("{}", memo_text);
    println!();

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and token addresses
    let program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("memoX1g5dtnxeN6zVdHMYWCCg3Qgre8WGFNs7YF2Mbc")
        .expect("Invalid mint address");

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );

    // Check token balance
    match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            let current_balance = balance.ui_amount.unwrap_or(0.0);
            println!("Current token balance: {} tokens", current_balance);
            
            if current_balance < burn_amount_tokens as f64 {
                println!("ERROR: Insufficient token balance!");
                println!("Requested burn: {} tokens", burn_amount_tokens);
                println!("Available balance: {} tokens", current_balance);
                return Ok(());
            }
        },
        Err(err) => {
            println!("Error checking token balance: {}", err);
            println!("Token account: {}", token_account);
            return Ok(());
        }
    }

    // Create instruction data for process_burn
    let discriminator = [220, 214, 24, 210, 116, 16, 167, 18];
    let mut instruction_data = discriminator.to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // Build accounts list
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),        // user (signer)
        AccountMeta::new(mint, false),                 // mint
        AccountMeta::new(token_account, false),        // token_account
        AccountMeta::new_readonly(token_2022_id(), false), // token_program
        AccountMeta::new_readonly(
            Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
            false
        ), // instructions sysvar
    ];

    // Create memo instruction
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
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

    // Create compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(initial_compute_units);
    
    // Final transaction: [compute_budget, memo, burn]
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, burn_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send transaction
    println!("Sending burn transaction...");
    println!("Instruction order: [compute_budget, memo, burn] - memo at index 1");
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🔥 BURN SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            println!("Burned {} tokens successfully", burn_amount_tokens);
            
            // Check new balance
            if let Ok(balance) = client.get_token_account_balance(&token_account) {
                println!("New token balance: {} tokens", balance.ui_amount.unwrap_or(0.0));
            }
            
            println!("\n✅ Decimal=0 memo-burn contract validation passed:");
            println!("  ✓ Memo length >= 69 bytes ({})", memo_text.as_bytes().len());
            println!("  ✓ Amount in memo matched burn amount (token count)");
            println!("  ✓ Decimal=0 token handling correct");
        },
        Err(err) => {
            println!("❌ BURN FAILED!");
            println!("Error: {}", err);
            print_error_guidance(&err.to_string());
        }
    }

    Ok(())
}

fn print_error_guidance(error_msg: &str) {
    println!("\n=== ERROR ANALYSIS ===");
    
    if error_msg.contains("Custom(6004)") || error_msg.contains("MemoTooShort") {
        println!("💡 Memo Too Short: Memo must be at least 69 bytes long.");
        println!("   Add more content to your memo JSON structure.");
    } else if error_msg.contains("Custom(6008)") || error_msg.contains("MemoTooLong") {
        println!("💡 Memo Too Long: Memo must not exceed 769 bytes.");
        println!("   Reduce the content in your memo.");
    } else if error_msg.contains("Custom(6009)") || error_msg.contains("AmountMismatch") {
        println!("💡 Amount Mismatch: The amount field in your memo doesn't match the burn amount.");
        println!("   For decimal=0 tokens: memo.amount should equal token count.");
    } else if error_msg.contains("Custom(6007)") || error_msg.contains("MissingAmountField") {
        println!("💡 Missing Amount: Your memo JSON must include an 'amount' field.");
    } else if error_msg.contains("Custom(6005)") || error_msg.contains("UnauthorizedMint") {
        println!("💡 Wrong Mint: Only the authorized mint can be used.");
        println!("   Expected: memoX1g5dtnxeN6zVdHMYWCCg3Qgre8WGFNs7YF2Mbc");
    } else {
        println!("💡 Error: {}", error_msg);
    }
    
    println!("\n=== TROUBLESHOOTING CHECKLIST ===");
    println!("1. ✓ Memo length between 69-769 bytes");
    println!("2. ✓ Memo contains valid JSON with 'amount' field");
    println!("3. ✓ Amount in memo matches burn amount (decimal=0)");
    println!("4. ✓ Using correct mint: memoX1g5dtnxeN6zVdHMYWCCg3Qgre8WGFNs7YF2Mbc");
    println!("5. ✓ Sufficient token balance");
} 