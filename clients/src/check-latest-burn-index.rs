use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::io::Write;
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct ShardInfo {
    category: String,     // shard category name (max 32 bytes)
    pubkey: Pubkey,      // shard account address
    record_count: u16,   // current record count
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct LatestBurnIndex {
    authority: Pubkey,    // creator's address
    shard_count: u8,      // current shard count
    shards: Vec<ShardInfo>, // shard info list
}

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate latest burn index PDA
    let (latest_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_index"],
        &program_id,
    );

    println!("Latest Burn Index PDA: {}", latest_burn_index_pda);

    // Get account data
    match client.get_account(&latest_burn_index_pda) {
        Ok(account) => {
            println!("\nAccount Data:");
            println!("Total size: {} bytes", account.data.len());
            
            // Print discriminator
            let discriminator = &account.data[0..8];
            println!("\nDiscriminator: {:?}", discriminator);
            
            // 手动解析数据
            let data = &account.data[8..];
            
            // 解析 authority (32 bytes)
            let authority = Pubkey::new(&data[0..32]);
            println!("\nAuthority: {}", authority);
            
            // 解析 shard_count (1 byte)
            let shard_count = data[32];
            println!("Shard count: {}", shard_count);
            
            // 解析 shards vector
            let mut offset = 33;
            println!("\nShards:");
            
            // 读取向量长度 (4 bytes)
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            for i in 0..vec_len {
                // 读取 category string
                let str_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                let category = String::from_utf8_lossy(&data[offset..offset+str_len]);
                offset += str_len;
                
                // 读取 pubkey
                let pubkey = Pubkey::new(&data[offset..offset+32]);
                offset += 32;
                
                // 读取 record_count
                let record_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap());
                offset += 2;
                
                println!("\nShard #{}:", i + 1);
                println!("  Category: {}", category);
                println!("  Pubkey: {}", pubkey);
                println!("  Record Count: {}", record_count);
            }

            println!("\nAccount Info:");
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
        }
        Err(err) => println!("Failed to get account: {}", err),
    }
}