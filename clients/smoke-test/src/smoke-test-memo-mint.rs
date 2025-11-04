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
use sha2::{Sha256, Digest};
use rand::{thread_rng, Rng};
use chrono::Utc;

// Import token-2022 program id
use spl_token_2022::id as token_2022_id;

use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

/// Generate exact 69-byte ASCII memo for smoke test
fn create_69_byte_ascii_memo() -> Vec<u8> {
    let mut rng = thread_rng();
    
    // Create a template with timestamp
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let prefix = format!("SMOKE_TEST_{}_", timestamp);
    
    // Calculate remaining space
    let target_length = 69;
    if prefix.len() >= target_length {
        // If prefix is too long, truncate and pad
        let mut memo = prefix.chars().take(target_length - 4).collect::<String>();
        memo.push_str("_END");
        return memo.as_bytes().to_vec();
    }
    
    // Fill remaining space with random ASCII characters
    let remaining = target_length - prefix.len();
    let ascii_chars: Vec<char> = (65u8..91).chain(97..123)
        .map(|b| b as char)
        .collect(); // A-Z, a-z
    let random_part: String = (0..remaining)
        .map(|_| ascii_chars[rng.gen_range(0..ascii_chars.len())])
        .collect();
    
    let memo_content = format!("{}{}", prefix, random_part);
    let memo_bytes = memo_content.as_bytes().to_vec();
    
    // Ensure exactly 69 bytes
    if memo_bytes.len() != 69 {
        panic!("Failed to generate exactly 69 bytes memo: got {} bytes", memo_bytes.len());
    }
    
    memo_bytes
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         MEMO-MINT SMOKE TEST (Single Transaction)           ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    
    // Setup client and accounts
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    println!("─────────────────────────────────────────────────────────────────");
    println!("📋 Configuration");
    println!("─────────────────────────────────────────────────────────────────");
    println!("RPC URL:        {}", get_rpc_url());
    println!("Payer:          {}", payer.pubkey());
    println!("Program ID:     {}", program_id);
    println!("Mint Address:   {}", mint_address);
    println!("Token Account:  {}", token_account);
    println!();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // Get initial balance
    let balance_before = get_token_balance_raw(&client, &token_account);
    println!("─────────────────────────────────────────────────────────────────");
    println!("💰 Initial Balance");
    println!("─────────────────────────────────────────────────────────────────");
    println!("Token Balance:  {} lamports ({} tokens)", balance_before, format_token_amount(balance_before));
    println!();
    
    // Generate exactly 69-byte ASCII memo
    let memo_bytes = create_69_byte_ascii_memo();
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Memo Details");
    println!("─────────────────────────────────────────────────────────────────");
    println!("Memo Length:    {} bytes (EXACT)", memo_bytes.len());
    println!("Memo Format:    Pure ASCII");
    if let Ok(memo_str) = std::str::from_utf8(&memo_bytes) {
        println!("Memo Content:   {}", memo_str);
    }
    println!();
    
    // Create memo instruction (MUST BE AT INDEX 0)
    let memo_ix = spl_memo::build_memo(&memo_bytes, &[&payer.pubkey()]);
    
    // Create mint instruction
    let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
    
    // Execute transaction with NO BUFFER on compute units
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔧 Transaction Execution");
    println!("─────────────────────────────────────────────────────────────────");
    
    match execute_transaction_exact_cu(&client, &payer, vec![memo_ix, mint_ix]) {
        Ok((signature, cu_used, cu_set)) => {
            println!("✅ Transaction SUCCESSFUL");
            println!();
            println!("Signature:      {}", signature);
            println!("CU Simulated:   {}", cu_used);
            println!("CU Set (exact): {}", cu_set);
            println!("CU Buffer:      0% (NO BUFFER - Exact CU)");
            println!();
            
            // Get final balance
            let balance_after = get_token_balance_raw(&client, &token_account);
            let tokens_minted = if balance_after >= balance_before {
                balance_after - balance_before
            } else {
                0
            };
            
            println!("─────────────────────────────────────────────────────────────────");
            println!("💰 Final Balance");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Balance Before: {} lamports", balance_before);
            println!("Balance After:  {} lamports", balance_after);
            println!("Tokens Minted:  {} lamports ({} tokens)", tokens_minted, format_token_amount(tokens_minted));
            println!();
            
            // Validate mint amount
            let (is_valid, description) = validate_mint_amount(tokens_minted);
            if is_valid {
                println!("✅ Mint Amount: {}", description);
            } else {
                println!("⚠️  Unexpected:  {}", description);
            }
            println!();
            
            println!("─────────────────────────────────────────────────────────────────");
            println!("✅ SMOKE TEST PASSED");
            println!("─────────────────────────────────────────────────────────────────");
            
            Ok(())
        },
        Err(e) => {
            println!("❌ Transaction FAILED");
            println!();
            println!("Error: {}", e);
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("❌ SMOKE TEST FAILED");
            println!("─────────────────────────────────────────────────────────────────");
            
            Err(e)
        }
    }
}

fn create_rpc_client() -> RpcClient {
    let rpc_url = get_rpc_url();
    RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed())
}

fn load_payer_keypair() -> solana_sdk::signature::Keypair {
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Cannot read payer keypair file");
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
            println!("✅ Token account exists");
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
                    println!("✅ Token account created: {}", signature);
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

/// Execute transaction with EXACT compute units (NO BUFFER)
fn execute_transaction_exact_cu(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
) -> Result<(String, u64, u32), Box<dyn std::error::Error>> {
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Step 1: Simulate to get exact CU requirement
    println!("🔍 Simulating transaction to determine exact CU...");
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
    let mut sim_instructions = instructions.clone();
    sim_instructions.push(dummy_compute_budget_ix);
    
    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let simulation_result = client.simulate_transaction_with_config(
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
    )?;
    
    // Get exact CU from simulation (NO BUFFER ADDED)
    let exact_cu = if let Some(err) = simulation_result.value.err {
        return Err(format!("Simulation failed: {:?}", err).into());
    } else if let Some(units_consumed) = simulation_result.value.units_consumed {
        // CRITICAL: NO BUFFER - Use exact simulated value
        units_consumed as u32
    } else {
        return Err("Simulation did not return units consumed".into());
    };
    
    println!("✅ Simulation complete");
    println!("   CU Required: {} (exact, no buffer)", exact_cu);
    println!();
    
    // Step 2: Create final transaction with EXACT CU
    println!("📤 Sending transaction with exact CU...");
    
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(exact_cu);
    
    // IMPORTANT: Memo at index 0, mint at index 1, compute budget at index 2
    let mut final_instructions = instructions;
    final_instructions.push(compute_budget_ix);
    
    let transaction = Transaction::new_signed_with_payer(
        &final_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => Ok((signature.to_string(), simulation_result.value.units_consumed.unwrap_or(0), exact_cu)),
        Err(e) => Err(e.into()),
    }
}

fn get_token_balance_raw(client: &RpcClient, token_account: &Pubkey) -> u64 {
    match client.get_account(token_account) {
        Ok(account) => {
            if account.data.len() >= 72 {
                let amount_bytes = &account.data[64..72];
                u64::from_le_bytes(amount_bytes.try_into().unwrap_or([0; 8]))
            } else {
                0
            }
        },
        Err(_) => 0,
    }
}

fn format_token_amount(raw_amount: u64) -> String {
    let tokens = raw_amount as f64 / 1_000_000.0;
    format!("{:.6}", tokens)
}

fn validate_mint_amount(raw_amount: u64) -> (bool, String) {
    match raw_amount {
        1_000_000 => (true, "1.0 token (Tier 1: 0-100M supply)".to_string()),
        100_000 => (true, "0.1 token (Tier 2: 100M-1B supply)".to_string()),
        10_000 => (true, "0.01 token (Tier 3: 1B-10B supply)".to_string()),
        1_000 => (true, "0.001 token (Tier 4: 10B-100B supply)".to_string()),
        100 => (true, "0.0001 token (Tier 5: 100B-1T supply)".to_string()),
        1 => (true, "0.000001 token (Tier 6: 1T+ supply)".to_string()),
        0 => (false, "No tokens minted - supply limit reached".to_string()),
        _ => (false, format!("Unexpected amount: {} lamports", raw_amount)),
    }
}
