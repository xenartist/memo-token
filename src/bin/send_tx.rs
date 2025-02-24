use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    message::Message,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction,
    transaction::Transaction,
    instruction::{Instruction, AccountMeta},
    pubkey::Pubkey,
};
use spl_token::{instruction as token_instruction};
use std::str::FromStr;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Amount of SOL to send (in lamports)
    #[arg(short, long, default_value = "1")]
    amount: u64,
}

fn main() {
    let args = Args::parse();

    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    let keypair = read_keypair_file(shellexpand::tilde("~/.config/solana/id.json").to_string())
        .expect("Failed to read keypair file");

    // 程序 ID 和 mint 地址（从部署脚本输出获取）
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("9AkxbAh31apMdjbG467VymHWJXtX92LCUz29nvtRYURS")
        .expect("Invalid mint address");

    // 获取用户的代币账户
    let token_account = spl_associated_token_account::get_associated_token_address(
        &keypair.pubkey(),
        &mint
    );

    // 创建转账指令
    let transfer_ix = system_instruction::transfer(
        &keypair.pubkey(),
        &Pubkey::from_str("oamuAfbJADHKwmePoYKCyHwANs329FKhhwHFtNLUyzN").unwrap(),
        args.amount,
    );

    // Anchor 指令标识符 "process_transfer"
    let anchor_sighash = {
        use sha2::{Sha256, Digest};
        let preimage = b"global:process_transfer";
        let mut hasher = Sha256::new();
        hasher.update(preimage);
        let result = hasher.finalize();
        result[..8].to_vec()
    };

    // 创建程序调用指令
    let process_transfer_ix = Instruction::new_with_bytes(
        program_id,
        &anchor_sighash,  // Anchor 指令标识符
        vec![
            AccountMeta::new(keypair.pubkey(), true),      // from
            AccountMeta::new(mint, false),                 // mint
            AccountMeta::new(keypair.pubkey(), true),      // mint_authority
            AccountMeta::new(token_account, false),        // token_account
            AccountMeta::new_readonly(spl_token::id(), false), // token_program
        ],
    );

    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let message = Message::new(
        &[transfer_ix, process_transfer_ix],
        Some(&keypair.pubkey()),
    );
    let mut transaction = Transaction::new(&[&keypair], message, recent_blockhash);

    let signature = client
        .send_transaction(&transaction)
        .expect("Failed to send transaction");

    println!("Transaction sent! Signature: {}", signature);

    client.confirm_transaction(&signature)
        .expect("Failed to confirm transaction");

    println!("Transaction confirmed!");

    // 检查代币余额
    let balance = client
        .get_token_account_balance(&token_account)
        .expect("Failed to get token balance");
    
    println!("Token balance: {}", balance.ui_amount.unwrap());
}