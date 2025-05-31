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

    println!("=== Top Burn Shards Status ===\n");

    // First, get the global top burn index
    let (global_index_pda, _) = Pubkey::find_program_address(
        &[b"global_top_burn_index"],
        &program_id,
    );

    println!("Global Top Burn Index PDA: {}", global_index_pda);

    let (total_count, current_index) = match client.get_account(&global_index_pda) {
        Ok(account) => {
            println!("Global Index Account found\n");
            
            // Parse global index data
            let data = &account.data[8..]; // Skip discriminator
            
            // Read top_burn_shard_total_count (8 bytes)
            let total_count = u64::from_le_bytes(data[0..8].try_into().unwrap());
            
            // Read top_burn_shard_current_index (Option<u64>: 1 byte + 8 bytes)
            let has_current = data[8] == 1;
            let current_index = if has_current {
                Some(u64::from_le_bytes(data[9..17].try_into().unwrap()))
            } else {
                None
            };
            
            println!("Total Shards Count: {}", total_count);
            match current_index {
                Some(idx) => println!("Current Active Shard Index: {}", idx),
                None => println!("Current Active Shard Index: None (no shards available)"),
            }
            println!();
            
            (total_count, current_index)
        }
        Err(err) => {
            println!("❌ Failed to get global index account: {}", err);
            println!("The top burn system is not initialized. Please run init-global-top-burn-index first.");
            return;
        }
    };

    // Check each top burn shard
    if total_count > 0 {
        for shard_index in 0..total_count {
            check_top_burn_shard(&client, &program_id, shard_index, current_index);
        }
        
        // Summary
        println!("\n=== Summary ===");
        println!("Total Shards: {}", total_count);
        match current_index {
            Some(idx) => {
                if idx < total_count {
                    println!("Current Active Shard: #{} (accepting new high-value burns)", idx);
                } else {
                    println!("Warning: Current index {} exceeds total count {}", idx, total_count);
                }
            }
            None => println!("No active shard (all full or none exist)"),
        }
        println!("Minimum burn to qualify for top burns: 420 tokens");
    } else {
        println!("No shards have been created yet.");
        println!("Use init-top-burn-shard to create the first shard.");
    }
}

fn check_top_burn_shard(client: &RpcClient, program_id: &Pubkey, shard_index: u64, current_index: Option<u64>) {
    // Calculate top burn shard PDA for this index
    let (top_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"top_burn_shard", &shard_index.to_le_bytes()],
        program_id,
    );

    let is_current = current_index == Some(shard_index);
    let status_indicator = if is_current { " 🔥 ACTIVE" } else { "" };
    
    println!("--- Top Burn Shard #{}{} ---", shard_index, status_indicator);
    println!("PDA: {}", top_burn_shard_pda);

    match client.get_account(&top_burn_shard_pda) {
        Ok(account) => {
            let data = &account.data[8..]; // Skip discriminator
            let mut offset = 0;
            
            // Read index (8 bytes)
            let stored_index = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            offset += 8;
            
            // Read creator (32 bytes)
            let creator = Pubkey::new(&data[offset..offset+32]);
            offset += 32;
            
            // Read records vector length
            let records_count = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            println!("Stored Index: {}", stored_index);
            println!("Creator: {}", creator);
            println!("Records Count: {}/69", records_count);
            
            if records_count == 69 {
                println!("Status: FULL ❌");
            } else if is_current {
                println!("Status: ACTIVE - Accepting burns ✅");
            } else {
                println!("Status: Available but not current");
            }
            
            if records_count > 0 {
                println!("\nTop Burns in this shard:");
                
                for i in 0..records_count {
                    // Parse each record
                    let pubkey = Pubkey::new(&data[offset..offset+32]);
                    offset += 32;
                    
                    let sig_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
                    offset += 4;
                    let signature = String::from_utf8_lossy(&data[offset..offset+sig_len]);
                    offset += sig_len;
                    
                    let slot = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    offset += 8;
                    
                    let blocktime = i64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    offset += 8;
                    
                    let amount = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    offset += 8;
                    
                    println!("  {}. {} burned {} tokens (sig: {}...)", 
                        i + 1, 
                        pubkey, 
                        amount / 1_000_000_000,
                        &signature[..8]
                    );
                }
            }
            
            println!("Account Size: {} bytes", account.data.len());
            println!("Lamports: {}\n", account.lamports);
        }
        Err(err) => {
            println!("❌ Shard not found: {}\n", err);
        }
    }
} 