use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use sha2::{Sha256, Digest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // parse compute units (default: 440_000)
    let compute_units = if args.len() > 1 {
        args[1].parse().unwrap_or(440_000)
    } else {
        440_000
    };
    
    // parse burn amount (in actual token units)
    let burn_amount = if args.len() > 2 {
        args[2].parse::<u64>().unwrap_or(1) * 1_000_000_000 // convert to lamports
    } else {
        1_000_000_000 // default burn 1 token
    };

    // parse memo
    let memo = if args.len() > 3 {
        args[3].clone()
    } else {
        String::from("This is a default memo message that is at least 69 bytes long for the minimum requirement.")
    };

    // ensure memo length is at least 69 bytes
    let memo_text = ensure_min_length(memo, 69);

    // connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // program and token address
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz")
        .expect("Invalid mint address");

    // get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // calculate storage PDA
    let (storage_pda, _) = Pubkey::find_program_address(
        &[b"test_onchain_storage"],
        &program_id,
    );

    // calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_burn");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    
    // add burn amount parameter
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    // create compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
    
    // print information
    println!("Memo length: {} bytes", memo_text.as_bytes().len());
    println!("Setting compute budget: {} CUs", compute_units);
    println!("Burning {} tokens", burn_amount / 1_000_000_000);

    // create burn instruction
    let burn_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),         // user
            AccountMeta::new(mint, false),                  // mint
            AccountMeta::new(token_account, false),         // token_account
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
            AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
            AccountMeta::new(storage_pda, false),          // storage account
        ],
    );

    // create memo instruction
    let memo_ix = spl_memo::build_memo(
        memo_text.as_bytes(),
        &[&payer.pubkey()],
    );
    
    // get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, memo_ix, burn_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // send and confirm transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("Burn successful! Signature: {}", signature);
            println!("Memo: {}", memo_text);

            // print token balance
            if let Ok(balance) = client.get_token_account_balance(&token_account) {
                println!("New token balance: {}", balance.ui_amount.unwrap());
            }
        }
        Err(err) => {
            println!("Failed to burn tokens: {}", err);
        }
    }

    Ok(())
}

// ensure string has at least minimum length
fn ensure_min_length(text: String, min_length: usize) -> String {
    if text.as_bytes().len() >= min_length {
        return text;
    }
    
    let padding_needed = min_length - text.as_bytes().len();
    let padding = ".".repeat(padding_needed);
    
    format!("{}{}", text, padding)
}
