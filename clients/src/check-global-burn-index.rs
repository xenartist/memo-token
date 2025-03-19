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

    // Calculate global burn index PDA
    let (global_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_burn_index"],
        &program_id,
    );

    println!("Global Burn Index PDA: {}", global_burn_index_pda);

    // Get account data
    match client.get_account(&global_burn_index_pda) {
        Ok(account) => {
            println!("\nAccount Data:");
            println!("Total size: {} bytes", account.data.len());
            
            // Print discriminator
            let discriminator = &account.data[0..8];
            println!("\nDiscriminator: {:?}", discriminator);
            
            // parse data
            let data = &account.data[8..];
            
            // parse authority (32 bytes)
            let authority = Pubkey::new(&data[0..32]);
            println!("\nAuthority: {}", authority);
            
            // parse shard_count (1 byte)
            let shard_count = data[32];
            println!("Shard count: {}", shard_count);
            
            // parse shards vector
            let mut offset = 33;
            println!("\nShards:");
            
            // parse vector length (4 bytes)
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            for i in 0..vec_len {
                // parse pubkey
                let pubkey = Pubkey::new(&data[offset..offset+32]);
                offset += 32;
                
                // parse record_count
                let record_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap());
                offset += 2;
                
                println!("\nShard #{}:", i + 1);
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