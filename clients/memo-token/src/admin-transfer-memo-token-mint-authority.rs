use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Keypair, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
};
use spl_token_2022::instruction as token_instruction;
use std::{str::FromStr, env, process};

// Token-2022 program ID constant
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

use memo_token_client::get_rpc_url;

fn main() {
    // Default paths
    let mint_keypair_path = shellexpand::tilde("~/.config/solana/memo-token/authority/memo_token_mint-keypair.json").to_string();
    let program_keypair_path = "target/deploy/memo_mint-keypair.json";
    
    // Read command line arguments for optional overrides
    let args: Vec<String> = env::args().collect();
    
    // Check if user wants to see usage
    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        println!("Usage: {} [OPTIONS]", args[0]);
        println!("\nThis tool transfers mint authority to the memo-mint program's PDA.");
        println!("\nDefault paths:");
        println!("  Mint keypair: ~/.config/solana/memo-token/authority/memo_token_mint-keypair.json");
        println!("  Program keypair: target/deploy/memo_mint-keypair.json");
        println!("\nOptions:");
        println!("  --mint-keypair <path>     Override mint keypair path");
        println!("  --program-keypair <path>  Override program keypair path");
        println!("  --help, -h                Show this help message");
        return;
    }
    
    // Parse optional arguments
    let mut custom_mint_path = None;
    let mut custom_program_path = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mint-keypair" => {
                if i + 1 < args.len() {
                    custom_mint_path = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    println!("Error: --mint-keypair requires a path argument");
                    process::exit(1);
                }
            },
            "--program-keypair" => {
                if i + 1 < args.len() {
                    custom_program_path = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    println!("Error: --program-keypair requires a path argument");
                    process::exit(1);
                }
            },
            _ => {
                println!("Unknown argument: {}", args[i]);
                println!("Use --help to see available options");
                process::exit(1);
            }
        }
    }
    
    // Use custom paths or defaults
    let final_mint_path = custom_mint_path.as_ref().map(|s| s.as_str()).unwrap_or(&mint_keypair_path);
    let final_program_path = custom_program_path.as_ref().map(|s| s.as_str()).unwrap_or(program_keypair_path);
    
    println!("=== Memo Token Mint Authority Transfer ===");
    println!("Mint keypair path: {}", final_mint_path);
    println!("Program keypair path: {}", final_program_path);
    println!();
    
    // Load mint keypair and get address
    let mint_address = match read_keypair_file(final_mint_path) {
        Ok(keypair) => {
            let pubkey = keypair.pubkey();
            println!("✓ Loaded mint keypair");
            println!("  Mint address: {}", pubkey);
            pubkey
        },
        Err(e) => {
            println!("✗ Error: Could not load mint keypair from: {}", final_mint_path);
            println!("  Error details: {}", e);
            println!("\nPlease ensure:");
            println!("  1. The file exists at the specified path");
            println!("  2. The file is a valid Solana keypair JSON file");
            process::exit(1);
        }
    };
    
    // Load program keypair and get program ID
    let program_id = match read_keypair_file(final_program_path) {
        Ok(keypair) => {
            let pubkey = keypair.pubkey();
            println!("✓ Loaded program keypair");
            println!("  Program ID: {}", pubkey);
            pubkey
        },
        Err(e) => {
            println!("✗ Error: Could not load program keypair from: {}", final_program_path);
            println!("  Error details: {}", e);
            println!("\nPlease ensure:");
            println!("  1. The file exists at the specified path");
            println!("  2. You have built the program (run: anchor build)");
            println!("  3. The file is a valid Solana keypair JSON file");
            process::exit(1);
        }
    };
    
    // Use network URL from environment or default to testnet X1
    let rpc_url = get_rpc_url();
    
    println!("\nConnecting to network: {}", rpc_url);
    let client = RpcClient::new_with_commitment(
        rpc_url.to_string(),
        CommitmentConfig::confirmed(),
    );

    // Load payer keypair (wallet that will pay for transaction)
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/memo-token/authority/deploy_admin-keypair.json").to_string()
    ).expect("Failed to read payer keypair file");
    
    println!("✓ Using payer: {}", payer.pubkey());

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    println!("✓ Calculated mint authority PDA: {}", mint_authority_pda);
    println!();

    // First, check if the mint actually exists and verify it's a Token-2022 mint
    match client.get_account(&mint_address) {
        Ok(account) => {
            let owner = account.owner;
            let token_2022_id = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();
            
            println!("Mint account verification:");
            println!("  Owner: {}", owner);
            
            // Only support Token-2022
            if owner == token_2022_id {
                println!("  ✓ This is a Token-2022 token mint.");
                println!();
                transfer_token_2022_authority(&client, &mint_address, &mint_authority_pda, &payer);
            } else {
                println!("  ✗ Error: This tool only supports Token-2022 mints!");
                println!("  Expected owner: Token-2022 ({})", token_2022_id);
                println!("  Actual owner: {}", owner);
                println!("\nIf you need to transfer authority for a legacy SPL token, please use the spl-token CLI tool:");
                println!("spl-token authorize {} mint {}", mint_address, mint_authority_pda);
                process::exit(1);
            }
        },
        Err(e) => {
            println!("✗ Error: Could not find mint account. Make sure:");
            println!("  1. The mint address is correct");
            println!("  2. You are connected to the correct network");
            println!("  3. The account exists on this network");
            println!("  Error details: {}", e);
            process::exit(1);
        }
    }
}

fn transfer_token_2022_authority(
    client: &RpcClient,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    payer: &Keypair
) {
    let token_2022_id = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();

    // Create instruction to transfer mint authority using Token-2022
    let set_authority_ix = match token_instruction::set_authority(
        &token_2022_id,
        mint_address,
        Some(mint_authority_pda),
        token_instruction::AuthorityType::MintTokens,
        &payer.pubkey(),
        &[&payer.pubkey()],
    ) {
        Ok(ix) => ix,
        Err(e) => {
            println!("Error creating set_authority instruction: {}", e);
            println!("This could be because you don't have the right to transfer this mint's authority.");
            process::exit(1);
        }
    };
    
    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    
    // Create transaction without compute budget instruction for simulation
    let sim_transaction = Transaction::new_signed_with_payer(
        &[set_authority_ix.clone()],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Simulate transaction to determine required compute units
    println!("Simulating transaction to determine required compute units...");
    let mut compute_units = 10_000u32; // Minimum safe value
    
    match client.simulate_transaction_with_config(
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
                println!("Warning: Transaction simulation failed: {:?}", err);
                compute_units = 10_000; // Use safe default
            } else if let Some(units_consumed) = result.value.units_consumed {
                // Add significant safety margin (50% more)
                compute_units = ((units_consumed as f64 * 1.5) as u32).max(5000);
                println!("Simulation consumed {} CUs, requesting {} CUs with 50% safety margin", 
                    units_consumed, compute_units);
            } else {
                println!("Simulation didn't return units consumed, using safe default: {}", compute_units);
            }
        },
        Err(err) => {
            println!("Failed to simulate transaction: {}", err);
            compute_units = 10_000; // Use safe default
        }
    };
    
    println!("Setting compute budget: {} CUs", compute_units);
    
    // Create compute budget instruction with calculated CU
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // Get fresh blockhash for the actual transaction
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    
    // Create and sign transaction with compute budget
    let transfer_auth_transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, set_authority_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    println!("\nTransferring mint authority to PDA using Token-2022 program...");
    match client.send_and_confirm_transaction_with_spinner(&transfer_auth_transaction) {
        Ok(sig) => {
            println!("\n✓ Mint authority transferred to PDA successfully!");
            println!("  Transaction signature: {}", sig);
            println!("\nToken Info Summary:");
            println!("  Mint address: {}", mint_address);
            println!("  Mint authority (PDA): {}", mint_authority_pda);
            println!("\nSave these addresses for future use!");
            
            // Optional: Create a token account for the current wallet
            println!("\nTip: You can create a token account for your wallet using:");
            println!("spl-token create-account {} --program-id {}", mint_address, TOKEN_2022_PROGRAM_ID);
        },
        Err(e) => {
            println!("✗ Error transferring mint authority: {}", e);
            println!("  Detailed error: {:?}", e);
            
            println!("\nYou can try using the spl-token CLI tool instead:");
            println!("spl-token authorize {} mint {} --program-id {}", 
                mint_address, mint_authority_pda, TOKEN_2022_PROGRAM_ID);
        }
    }
}