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
    
    // Parse compute units (default: 200_000 - simplified contract needs less)
    let initial_compute_units = if args.len() > 1 {
        args[1].parse().unwrap_or(200_000)
    } else {
        200_000
    };
    
    // Parse burn amount (in actual token units)
    let burn_amount_tokens = if args.len() > 2 {
        args[2].parse::<u64>().unwrap_or(1)
    } else {
        1
    };
    let burn_amount = burn_amount_tokens * 1_000_000_000; // Convert to lamports

    // Parse custom message (optional)
    let message = if args.len() > 3 {
        args[3].clone()
    } else {
        format!("Testing memo-burn contract with {} tokens", burn_amount_tokens)
    };

    // Build simplified JSON format memo - only amount field is required
    let memo_json = serde_json::json!({
        "message": message,
        "amount": burn_amount.to_string() // Required field - must match burn amount
    });
    
    // Convert to string
    let memo_text = serde_json::to_string(&memo_json)
        .expect("Failed to serialize JSON");

    // Print detailed information
    println!("=== MEMO-BURN CONTRACT TEST (SIMPLIFIED) ===");
    println!("Burn amount: {} tokens ({} lamports)", burn_amount_tokens, burn_amount);
    println!("Memo JSON structure (no signature required):");
    println!("{:#}", memo_json);
    println!("\nFinal memo text (length: {} bytes):", memo_text.as_bytes().len());
    println!("{}", memo_text);
    println!();

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and token addresses - UPDATE THESE WHEN YOU DEPLOY THE NEW CONTRACT
    let program_id = Pubkey::from_str("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP")
        .expect("Invalid program ID - update this with your deployed contract ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
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

    // Create instruction data for process_burn using correct discriminator from IDL
    // Discriminator from IDL: [220, 214, 24, 210, 116, 16, 167, 18]
    let discriminator = [220, 214, 24, 210, 116, 16, 167, 18];
    
    let mut instruction_data = discriminator.to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // Build accounts list (simplified for memo-burn contract)
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

    // Print account information
    println!("Transaction accounts:");
    for (i, account) in accounts.iter().enumerate() {
        println!("  {}: {} (signer: {}, writable: {})",
               i, account.pubkey, account.is_signer, account.is_writable);
    }
    println!();

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

    // Simulate with proper instruction order
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(100_000);
    let sim_transaction = Transaction::new_signed_with_payer(
        &[dummy_compute_budget_ix, memo_ix.clone(), burn_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Simulating transaction...");
    let compute_units = match client.simulate_transaction_with_config(
        &sim_transaction,
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
                println!("Simulation failed: {:?}", err);
                print_error_guidance(&err.to_string());
                
                // Show logs if available
                if let Some(logs) = result.value.logs {
                    println!("\n=== TRANSACTION LOGS ===");
                    for log in logs {
                        println!("  {}", log);
                    }
                }
                return Ok(());
            } else if let Some(units_consumed) = result.value.units_consumed {
                let required_cu = (units_consumed as f64 * 1.1) as u32; // 10% safety margin
                println!("Simulation successful! Consumed {} CUs, requesting {} CUs (10% safety margin)", 
                    units_consumed, required_cu);
                required_cu
            } else {
                println!("Simulation successful! Using default compute units: {}", initial_compute_units);
                initial_compute_units
            }
        },
        Err(err) => {
            println!("Failed to simulate: {}", err);
            println!("Using default compute units: {}", initial_compute_units);
            initial_compute_units
        }
    };

    // Create final transaction with proper compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // Final transaction: [compute_budget, memo, burn] - memo at index 1 ✅
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
            
            println!("\n✅ Simplified memo-burn contract validation passed:");
            println!("  ✓ Amount in memo matched burn amount");
            println!("  ✓ Only burn operation was allowed");
            println!("  ✓ No signature field required");
            println!("  ✓ No length limit on memo");
            println!("  ✓ Instruction order correct (memo at index 1)");
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
    
    if error_msg.contains("ProgramFailedToComplete") {
        println!("💡 Program Failed to Complete: The contract encountered an internal error or panic.");
        println!("   This could be due to:");
        println!("   - JSON parsing error in memo validation");
        println!("   - Number overflow in amount conversion");
        println!("   - Invalid UTF-8 in memo data");
        println!("   - Unexpected data format");
    } else if error_msg.contains("Custom(6009)") || error_msg.contains("AmountMismatch") {
        println!("💡 Amount Mismatch: The amount field in your memo doesn't match the burn amount.");
        println!("   Make sure memo.amount equals the exact lamports being burned.");
    } else if error_msg.contains("Custom(6007)") || error_msg.contains("MissingAmountField") {
        println!("💡 Missing Amount: Your memo JSON must include an 'amount' field.");
        println!("   Example: {{\"amount\": \"1000000000\", \"message\":\"...\"}}");
    } else if error_msg.contains("Custom(6006)") || error_msg.contains("UnauthorizedTokenAccount") {
        println!("💡 Unauthorized: Only the token account owner can burn tokens.");
        println!("   This prevents transfers from other accounts.");
    } else if error_msg.contains("Custom(6000)") || error_msg.contains("MemoRequired") {
        println!("💡 Missing Memo: This contract requires a memo instruction.");
        println!("   Make sure memo instruction is at index 1 in the transaction.");
    } else if error_msg.contains("Custom(6001)") || error_msg.contains("InvalidMemoFormat") {
        println!("💡 Invalid Memo: Memo must be valid JSON format.");
    } else if error_msg.contains("Custom(6002)") || error_msg.contains("BurnAmountTooSmall") {
        println!("💡 Amount Too Small: Must burn at least 1 token (1,000,000,000 lamports).");
    } else if error_msg.contains("Custom(6003)") || error_msg.contains("InvalidBurnAmount") {
        println!("💡 Invalid Amount: Burn amount must be a multiple of 1 token (1,000,000,000 lamports).");
    } else if error_msg.contains("Custom(6005)") || error_msg.contains("UnauthorizedMint") {
        println!("💡 Wrong Mint: Only the authorized mint can be used.");
    } else if error_msg.contains("InsufficientFunds") {
        println!("💡 Insufficient Balance: You don't have enough tokens to burn.");
    } else if error_msg.contains("InvalidAccountData") {
        println!("💡 Account Issue: Check that the token account exists and belongs to the right mint.");
    } else if error_msg.contains("ProgramError") {
        println!("💡 Program Error: The memo-burn contract encountered an issue.");
        println!("   Check that you're using the correct program ID.");
    } else {
        println!("💡 Unknown Error: {}", error_msg);
    }
    
    println!("\n=== TROUBLESHOOTING CHECKLIST ===");
    println!("1. ✓ Memo contains required 'amount' field as string");
    println!("2. ✓ Memo is valid JSON format");
    println!("3. ✓ No signature field required (removed)");
    println!("4. ✓ Burn amount >= 1 token and is multiple of 1 token");
    println!("5. ✓ Sufficient token balance");
    println!("6. ✓ Using correct program ID for memo-burn contract");
    println!("7. ✓ Token account belongs to the signer");
    println!("8. ✓ Instruction order: [compute_budget, memo, burn]");
} 