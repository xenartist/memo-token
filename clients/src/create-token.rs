use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction,
    transaction::Transaction,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
};
use spl_token_2022::{
    extension::{
        ExtensionType,
        metadata_pointer,
    },
    instruction as token_instruction,
};
use anchor_lang::{prelude::*, AnchorSerialize};
use std::str::FromStr;

// 自定义程序的 ID
const PROGRAM_ID: &str = "TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw";

// 使用 Anchor 的方式定义指令数据结构
#[derive(AnchorSerialize)]
struct SetMetadataArgs {
    name: String,
    symbol: String,
    uri: String,
}

fn main() {
    // 1. 设置 RPC 客户端
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url.to_string());

    // 2. 加载支付账户（payer）的密钥对
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    println!("支付账户公钥: {}", payer.pubkey());

    // 检查余额
    let balance = client.get_balance(&payer.pubkey()).expect("获取余额失败");
    println!("支付账户余额: {} lamports", balance);

    // 3. 生成新的 mint 账户密钥对
    let mint_keypair = Keypair::new();
    println!("新 Token Mint 地址: {}", mint_keypair.pubkey());

    // 4. 推导 Mint Authority 的 PDA
    let program_id = Pubkey::from_str(PROGRAM_ID).unwrap();
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    println!("Mint Authority PDA: {}", mint_authority_pda);

    // 5. 初始化 Mint（客户端完成）
    // 计算包含元数据扩展的 mint 大小
    let extensions = vec![
        ExtensionType::MetadataPointer,  // 只需要 MetadataPointer 扩展
    ];
    let mint_len = ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&extensions)
        .expect("Failed to calculate mint len");
    
    let mint_rent = client.get_minimum_balance_for_rent_exemption(mint_len).unwrap();
    println!("Mint 租金: {} lamports", mint_rent);

    // 创建 mint 账户
    let create_mint_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_keypair.pubkey(),
        mint_rent,
        mint_len as u64,
        &spl_token_2022::id(),
    );

    // 初始化 mint
    let init_mint_ix = token_instruction::initialize_mint(
        &spl_token_2022::id(),
        &mint_keypair.pubkey(),
        &mint_authority_pda,
        None,
        9,
    ).unwrap();

    // 创建并初始化 mint
    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let mint_tx = Transaction::new_signed_with_payer(
        &[
            create_mint_ix,
            init_mint_ix,
        ],
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        recent_blockhash,
    );

    let mint_signature = client.send_and_confirm_transaction(&mint_tx).unwrap();
    println!("Mint 初始化成功，签名: {}", mint_signature);

    // 6. 设置元数据（调用合约的 set_metadata）
    let args = SetMetadataArgs {
        name: "My Token".to_string(),
        symbol: "MTK".to_string(),
        uri: "https://my-token-metadata.json".to_string(),
    };

    // 使用正确的 discriminator
    let mut data = vec![]; 
    data.extend_from_slice(&[78, 157, 75, 242, 151, 20, 121, 144]); // set_metadata discriminator
    args.try_to_vec().unwrap().iter().for_each(|byte| data.push(*byte));

    let set_metadata_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true), // user (签名者)
            AccountMeta::new(mint_keypair.pubkey(), true), // mint (需要是可写的)
            AccountMeta::new_readonly(mint_authority_pda, false), // mint_authority
            AccountMeta::new_readonly(spl_token_2022::id(), false), // token_program
        ],
        data,
    };

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let metadata_tx = Transaction::new_signed_with_payer(
        &[set_metadata_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let metadata_signature = client.send_and_confirm_transaction(&metadata_tx).unwrap();
    println!("元数据设置成功，签名: {}", metadata_signature);

    // 7. 创建并初始化 Token Account
    let token_account_keypair = Keypair::new();
    let token_account_rent = client.get_minimum_balance_for_rent_exemption(spl_token_2022::state::Account::LEN).unwrap();
    println!("Token Account 租金: {} lamports", token_account_rent);

    let create_token_account_ix = system_instruction::create_account(
        &payer.pubkey(),
        &token_account_keypair.pubkey(),
        token_account_rent,
        spl_token_2022::state::Account::LEN as u64,
        &spl_token_2022::id(),
    );

    let init_token_account_ix = token_instruction::initialize_account(
        &spl_token_2022::id(),
        &token_account_keypair.pubkey(),
        &mint_keypair.pubkey(),
        &payer.pubkey(),
    ).unwrap();

    // 8. 铸造代币（调用合约的 process_transfer）
    let process_transfer_data = {
        let mut data = vec![];
        data.extend_from_slice(&[212, 115, 192, 211, 191, 149, 132, 69]); // process_transfer discriminator
        data  // 这个指令不需要参数，所以只需要 discriminator
    };

    let process_transfer_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true), // user (签名者)
            AccountMeta::new(mint_keypair.pubkey(), false), // mint
            AccountMeta::new_readonly(mint_authority_pda, false), // mint_authority
            AccountMeta::new(token_account_keypair.pubkey(), false), // token_account
            AccountMeta::new_readonly(spl_token_2022::id(), false), // token_program
        ],
        data: process_transfer_data,
    };

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let transfer_tx = Transaction::new_signed_with_payer(
        &[create_token_account_ix, init_token_account_ix, process_transfer_ix],
        Some(&payer.pubkey()),
        &[&payer, &token_account_keypair],
        recent_blockhash,
    );

    let transfer_signature = client.send_and_confirm_transaction(&transfer_tx).unwrap();
    println!("代币铸造成功，签名: {}", transfer_signature);

    // 9. 输出最终信息
    println!("\n代币信息:");
    println!("Mint 地址: {}", mint_keypair.pubkey());
    println!("Mint Authority PDA: {}", mint_authority_pda);
    println!("Token Account 地址: {}", token_account_keypair.pubkey());
    println!("名称: My Token");
    println!("符号: MTK");
    println!("URI: https://my-token-metadata.json");
}