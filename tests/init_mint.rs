use anchor_client::{
    solana_sdk::{
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair},
        system_program,
    },
    Client, Program,
};
use std::str::FromStr;

fn main() {
    // 设置连接
    let url = "https://rpc.testnet.x1.xyz".to_string();
    
    // 读取本地密钥对
    let payer = read_keypair_file(&*shellexpand::tilde("~/.config/solana/id.json"))
        .expect("读取密钥对失败");
    
    // 设置程序 ID
    let program_id = Pubkey::from_str("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap")
        .expect("无效的程序 ID");
    
    // 创建客户端
    let client = Client::new_with_options(
        cluster,
        Rc::new(payer),
        CommitmentConfig::confirmed(),
    );
    
    // 获取程序
    let program = client.program(program_id);
    
    // 计算 PDA
    let seeds = b"memo_mint";
    let (mint_pda, _bump) = Pubkey::find_program_address(&[seeds], &program_id);
    
    println!("Mint PDA: {}", mint_pda);
    
    // 调用初始化指令
    let signature = program
        .request()
        .accounts(memo_token::accounts::InitializeMint {
            mint: mint_pda,
            payer: payer.pubkey(),
            system_program: system_program::ID,
            token_program: spl_token::ID,
            rent: solana_sdk::sysvar::rent::ID,
        })
        .args(memo_token::instruction::InitializeMint)
        .send()
        .expect("初始化 mint 失败");
    
    println!("交易签名: {}", signature);
}
