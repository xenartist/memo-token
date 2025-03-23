use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

fn main() {
    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate top burn shard PDA
    let (top_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"top_burn_shard"],
        &program_id,
    );

    println!("Top Burn Shard PDA: {}", top_burn_shard_pda);

    // Get account data
    match client.get_account(&top_burn_shard_pda) {
        Ok(account) => {
            println!("\nAccount Data:");
            println!("Total size: {} bytes", account.data.len());
            
            // Print discriminator
            let discriminator = &account.data[0..8];
            println!("\nDiscriminator: {:?}", discriminator);
            
            // parse data - skip discriminator
            let data = &account.data[8..];
            
            let mut offset = 0;
            
            // parse records vector
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            println!("\nRecords ({}) - Top Burns Leaderboard (sorted by amount):", vec_len);
            
            for i in 0..vec_len {
                // parse pubkey (32 bytes)
                let pubkey = Pubkey::new(&data[offset..offset+32]);
                offset += 32;
                
                // read signature string
                let sig_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                let signature = String::from_utf8_lossy(&data[offset..offset+sig_len]);
                offset += sig_len;
                
                // read slot (8 bytes)
                let slot = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                offset += 8;
                
                // read blocktime (8 bytes)
                let blocktime = i64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                offset += 8;
                
                // read amount (8 bytes)
                let amount = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                offset += 8;
                
                println!("\nRank #{}: ", i + 1);
                println!("  Pubkey: {}", pubkey);
                println!("  Signature: {}", signature);
                println!("  Slot: {}", slot);
                println!("  Blocktime: {}", blocktime);
                println!("  Amount: {} ({} tokens)", amount, amount / 1_000_000_000);
            }

            // If the record list is not empty, show the minimum burn amount required to qualify
            if vec_len > 0 {
                // Go back to the last record's amount
                let min_amount_offset = 8 + 4 + ((vec_len - 1) * (32 + 4 + 88 + 8 + 8)) + 32 + 4 + 88 + 8 + 8;
                let min_amount = u64::from_le_bytes(data[min_amount_offset..min_amount_offset+8].try_into().unwrap());
                println!("\nMinimum burn amount to qualify for leaderboard: {} ({} tokens)", 
                         min_amount, min_amount / 1_000_000_000);
            } else {
                println!("\nNo records yet - any burn will qualify for the leaderboard.");
            }

            println!("\nAccount Info:");
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
        }
        Err(err) => println!("Failed to get account: {}", err),
    }
} 