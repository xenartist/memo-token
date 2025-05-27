use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use borsh::{BorshDeserialize, BorshSerialize};
use std::convert::TryInto;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
struct GlobalTopBurnIndex {
    top_burn_shard_total_count: u64,     // Total count of allocated shards
    top_burn_shard_current_index: Option<u64>,   // Current index with available space
}

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate global top burn index PDA
    let (global_top_burn_index_pda, _) = Pubkey::find_program_address(
        &[b"global_top_burn_index"],
        &program_id,
    );

    println!("Global Top Burn Index PDA: {}", global_top_burn_index_pda);

    // Get account data
    match client.get_account(&global_top_burn_index_pda) {
        Ok(account) => {
            println!("\nAccount Data:");
            println!("Total size: {} bytes", account.data.len());
            
            // Print discriminator
            let discriminator = &account.data[0..8];
            println!("\nDiscriminator: {:?}", discriminator);
            
            // Parse data
            let data = &account.data[8..];
            
            if data.len() >= 9 { // at least 8 bytes for total_count and 1 byte for option tag
                // Parse top_burn_shard_total_count (8 bytes for u64)
                let total_count_bytes = &data[0..8];
                let total_count = u64::from_le_bytes(total_count_bytes.try_into().unwrap());
                println!("Top Burn Shard Total Count: {}", total_count);
                
                // Parse top_burn_shard_current_index (Option<u64>) - read 1 byte for tag first
                let option_tag = data[8];
                let mut current_index_option = None;
                
                if option_tag == 0 {
                    println!("Top Burn Shard Current Index: None (No active shard)");
                } else if option_tag == 1 && data.len() >= 17 { // 1 byte for tag + 8 bytes for u64
                    let current_index_bytes = &data[9..17];
                    let current_index = u64::from_le_bytes(current_index_bytes.try_into().unwrap());
                    println!("Top Burn Shard Current Index: {}", current_index);
                    current_index_option = Some(current_index);
                } else {
                    println!("Invalid option tag or data format");
                }
                
                println!("\nTop Burn Shards:");
                
                // Try to fetch and display information about each shard
                for i in 0..total_count {
                    // Calculate the shard PDA
                    let (shard_pda, _) = Pubkey::find_program_address(
                        &[b"top_burn_shard", &i.to_le_bytes()], // Using the full 8 bytes for u64
                        &program_id,
                    );
                    
                    println!("\nShard #{} (index {})", i + 1, i);
                    println!("  PDA: {}", shard_pda);
                    
                    // Get the shard account data if it exists
                    match client.get_account(&shard_pda) {
                        Ok(shard_account) => {
                            println!("  Status: Exists");
                            
                            // Check if it has data
                            if shard_account.data.len() > 44 { // At least discriminator + index(8) + creator(32) + vec length(4)
                                // Skip discriminator
                                let shard_data = &shard_account.data[8..];
                                
                                // Parse index (8 bytes for u64)
                                let index_bytes = &shard_data[0..8];
                                let index = u64::from_le_bytes(index_bytes.try_into().unwrap());
                                println!("  Index: {}", index);
                                
                                // Parse creator (32 bytes)
                                let creator_bytes = &shard_data[8..40];
                                let creator = Pubkey::new(creator_bytes);
                                println!("  Creator: {}", creator);
                                
                                // Parse records vector length (4 bytes)
                                let record_count_bytes = &shard_data[40..44];
                                let record_count = u32::from_le_bytes(record_count_bytes.try_into().unwrap());
                                println!("  Record Count: {}", record_count);
                                println!("  Max Records: 69");
                                
                                // Is full?
                                println!("  Is Full: {}", record_count >= 69);
                            } else {
                                println!("  Data: Invalid or corrupted");
                            }
                            
                            println!("  Owner: {}", shard_account.owner);
                            println!("  Lamports: {}", shard_account.lamports);
                        },
                        Err(_) => {
                            println!("  Status: Does not exist or cannot be fetched");
                        }
                    }
                    
                    // Is this the current index?
                    if let Some(current_idx) = current_index_option {
                        if i == current_idx {
                            println!("  Current writing target: YES");
                        }
                    }
                }
            } else {
                println!("Account data is too small or invalid");
            }
            
            println!("\nAccount Info:");
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
        }
        Err(err) => println!("Failed to get account: {}", err),
    }
}
