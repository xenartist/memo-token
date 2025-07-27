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
use serde_json;
use rand::{thread_rng, Rng};
use chrono::Utc;

// import token-2022 program id
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Token batch mint test client ===\n");
    
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // parse mint count
    let mint_count = if args.len() > 1 {
        args[1].parse::<u64>().unwrap_or(0)
    } else {
        0 // default to 0 means infinite execution
    };

    // show execution plan
    if mint_count == 0 {
        println!("execution plan: infinite mint operation");
    } else {
        println!("execution plan: mint {} times", mint_count);
    }
    
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    // start batch mint operation
    let mut completed_mints = 0u64;
    let mut successful_mints = 0u64;
    
    loop {
        // check if reached the specified number of times
        if mint_count > 0 && completed_mints >= mint_count {
            break;
        }
        
        completed_mints += 1;
        
        // get current token balance
        let balance_before = get_token_balance(&client, &token_account);
        
        // generate random length valid memo (69-800 bytes)
        let memo_text = create_random_valid_memo();
        println!("\n🔄 execute the {}th mint operation", completed_mints);
        println!("memo length: {} bytes", memo_text.len());
        
        // create memo instruction
        let memo_ix = spl_memo::build_memo(memo_text.as_bytes(), &[&payer.pubkey()]);
        
        // create mint instruction
        let mint_ix = create_mint_instruction(&program_id, &payer.pubkey(), &mint_address, &mint_authority_pda, &token_account);
        
        // execute transaction
        match execute_transaction(&client, &payer, vec![memo_ix, mint_ix], &format!("batch mint #{}", completed_mints)) {
            Ok(signature) => {
                successful_mints += 1;
                
                // check token balance change
                let balance_after = get_token_balance(&client, &token_account);
                println!("✅ transaction successful!");
                println!("   signature: {}", signature);
                println!("   token balance change: {} -> {}", balance_before, balance_after);
                println!("   cumulative successful: {}/{}", successful_mints, completed_mints);
            },
            Err(e) => {
                println!("❌ transaction failed!");
                println!("   error: {}", e);
                println!("   cumulative successful: {}/{}", successful_mints, completed_mints);
            }
        }
    }
    
    // show final statistics
    println!("\n📊 batch mint execution statistics:");
    println!("   total execution times: {}", completed_mints);
    println!("   successful times: {}", successful_mints);
    println!("   failed times: {}", completed_mints - successful_mints);
    println!("   success rate: {:.2}%", (successful_mints as f64 / completed_mints as f64) * 100.0);
    
    Ok(())
}

// generate random length valid memo (69-800 bytes)
fn create_random_valid_memo() -> String {
    let mut rng = thread_rng();
    let target_length = rng.gen_range(69..=800);
    
    let message = format!("Batch mint test at {}", Utc::now().to_rfc3339());
    let random_data = (0..target_length)
        .map(|_| rng.gen_range(b'a'..=b'z') as char)
        .collect::<String>();
    
    let memo_json = serde_json::json!({
        "message": message,
        "timestamp": Utc::now().to_rfc3339(),
        "random_data": random_data
    });
    
    let mut memo_text = memo_json.to_string();
    
    // fine-tune to reach the target length
    while memo_text.len() < target_length {
        memo_text.push('x');
    }
    while memo_text.len() > target_length {
        memo_text.pop();
    }
    
    memo_text
}

fn create_rpc_client() -> RpcClient {
    let rpc_url = "https://rpc-testnet.x1.wiki";
    println!("connect to: {}", rpc_url);
    RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed())
}

fn load_payer_keypair() -> solana_sdk::signature::Keypair {
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("cannot read payer keypair file");
    println!("use payer: {}", payer.pubkey());
    payer
}

fn get_program_addresses() -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let program_id = Pubkey::from_str("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy")
        .expect("invalid program id");
    let mint_address = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("invalid mint address");
    
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("cannot read keypair file");
    
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_address,
        &token_2022_id(),
    );
    
    println!("program id: {}", program_id);
    println!("mint address: {}", mint_address);
    println!("mint authority pda: {}", mint_authority_pda);
    println!("token account: {}", token_account);
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
            println!("✅ token account exists: {}", token_account);
        },
        Err(_) => {
            println!("⚠️  token account does not exist, creating...");
            
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
                    println!("✅ token account created successfully!");
                    println!("   signature: {}", signature);
                    println!("   account: {}", token_account);
                },
                Err(e) => {
                    return Err(format!("failed to create token account: {}", e).into());
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
    test_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("execute {}...", test_name);
    
    let recent_blockhash = client.get_latest_blockhash()?;
    
    let dummy_compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
    let mut sim_instructions = vec![dummy_compute_budget_ix];
    sim_instructions.extend(instructions.clone());
    
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
            if let Some(err) = result.value.err {
                println!("simulate shows expected error: {:?}", err);
                let default_cu = 300_000u32;
                println!("use default compute unit: {}", default_cu);
                default_cu
            } else if let Some(units_consumed) = result.value.units_consumed {
                let optimal_cu = ((units_consumed as f64) * 1.1) as u32;
                println!("simulate consume {} CUs, set limit to {} CUs (+10%)", 
                    units_consumed, optimal_cu);
                optimal_cu
            } else {
                let default_cu = 300_000u32;
                println!("simulate success but no CU data, use default value: {}", default_cu);
                default_cu
            }
        },
        Err(err) => {
            println!("simulate failed: {}, use default CU", err);
            300_000u32
        }
    };
    
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(optimal_cu);
    
    let mut final_instructions = vec![compute_budget_ix];
    final_instructions.extend(instructions);
    
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

fn get_token_balance(client: &RpcClient, token_account: &Pubkey) -> u64 {
    match client.get_token_account_balance(token_account) {
        Ok(balance) => {
            // For decimal=6 tokens, ui_amount represents the actual token count
            // The contract mints 1,000,000 units = 1 token (displayed as ui_amount)
            balance.ui_amount.unwrap_or(0.0) as u64
        },
        Err(_) => 0,
    }
} 