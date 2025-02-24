use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Use fixed mint address
    let mint = Pubkey::from_str("9AkxbAh31apMdjbG467VymHWJXtX92LCUz29nvtRYURS").expect("Invalid mint address");

    // Get user's token account address
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    // Check if token account exists
    match client.get_account(&token_account) {
        Ok(_) => {
            println!("Token account already exists");
        }
        Err(_) => {
            // Create instruction for token account
            let create_token_account_ix = spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint,
                &spl_token::id(),
            );

            // Get latest blockhash
            let recent_blockhash = client
                .get_latest_blockhash()
                .expect("Failed to get recent blockhash");

            // Create and send transaction
            let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
                &[create_token_account_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );

            let signature = client
                .send_and_confirm_transaction(&transaction)
                .expect("Failed to create token account");

            println!("Created token account: {}", signature);
        }
    }

    println!("\nDeployment Info:");
    println!("Program ID: {}", "68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap");
    println!("Mint address: {}", mint);
    println!("Your wallet: {}", payer.pubkey());
    println!("Your token account: {}", token_account);

    // Print token balance
    match client.get_token_account_balance(&token_account) {
        Ok(balance) => {
            println!("Token balance: {}", balance.ui_amount.unwrap());
        }
        Err(_) => {
            println!("Failed to get token balance");
        }
    }
} 