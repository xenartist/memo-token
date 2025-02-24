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

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Program and mint addresses
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap").expect("Invalid program ID");
    let mint = Pubkey::from_str("9AkxbAh31apMdjbG467VymHWJXtX92LCUz29nvtRYURS").expect("Invalid mint address");

    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Calculate Anchor instruction sighash
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();

    // Create mint instruction
    let mint_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),         // from
            AccountMeta::new(mint, false),                  // mint
            AccountMeta::new(payer.pubkey(), true),         // mint_authority
            AccountMeta::new(token_account, false),         // token_account
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
        ],
    );

    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[mint_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send and confirm transaction
    let signature = client
        .send_and_confirm_transaction(&transaction)
        .expect("Failed to send transaction");

    println!("Mint successful! Signature: {}", signature);

    // Print token balance
    match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            println!("New token balance: {}", balance.ui_amount.unwrap());
        }
        Err(_) => {
            println!("Failed to get token balance");
        }
    }
} 