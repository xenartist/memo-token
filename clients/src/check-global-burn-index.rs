use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::io::Write;
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct ShardInfo {
    pubkey: Pubkey,      // shard account address
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct GlobalBurnIndex {
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
            
            // parse shard_count (1 byte)
            let shard_count = data[0];
            println!("Shard count: {}", shard_count);
            
            // parse shards vector
            let mut offset = 1;
            println!("\nShards:");
            
            // parse vector length (4 bytes)
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            for i in 0..vec_len {
                // parse pubkey
                let pubkey = Pubkey::new(&data[offset..offset+32]);
                offset += 32;
                
                // No more record_count parsing needed
                
                println!("\nShard #{}:", i + 1);
                println!("  Pubkey: {}", pubkey);
                
                // Check if this is the latest burn shard
                let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
                    &[b"latest_burn_shard"],
                    &program_id,
                );
                
                if pubkey == latest_burn_shard_pda {
                    println!("  Type: Latest Burn Shard");
                    
                    // Try to get actual record count from the shard
                    match client.get_account(&latest_burn_shard_pda) {
                        Ok(shard_account) => {
                            // Check if it has data
                            if shard_account.data.len() > 9 { // At least discriminator + current_index + vec length
                                // Skip discriminator and go to records vector length
                                let record_data = &shard_account.data[8+1..]; // Skip discriminator and current_index
                                let record_count = u32::from_le_bytes(record_data[0..4].try_into().unwrap());
                                println!("  Current Records: {}", record_count);
                                println!("  Max Records: 69");
                                
                                // Get current index
                                let current_index = shard_account.data[8];
                                println!("  Current Index: {}", current_index);
                            } else {
                                println!("  Empty shard or invalid data");
                            }
                        },
                        Err(err) => {
                            println!("  Could not fetch shard data: {}", err);
                        }
                    }
                }
            }

            println!("\nAccount Info:");
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
        }
        Err(err) => println!("Failed to get account: {}", err),
    }
}