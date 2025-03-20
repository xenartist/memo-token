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

    // Calculate latest burn shard PDA
    let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_shard"],
        &program_id,
    );

    println!("Latest Burn Shard PDA: {}", latest_burn_shard_pda);

    // Get account data
    match client.get_account(&latest_burn_shard_pda) {
        Ok(account) => {
            println!("\nAccount Data:");
            println!("Total size: {} bytes", account.data.len());
            
            // Print discriminator
            let discriminator = &account.data[0..8];
            println!("\nDiscriminator: {:?}", discriminator);
            
            // parse data - skip discriminator
            let data = &account.data[8..];
            
            // No more authority field, start directly with current_index
            let mut offset = 0; // No authority field anymore
            
            // parse current_index (1 byte)
            let current_index = data[offset];
            println!("Current Index: {}", current_index);
            offset += 1;
            
            // parse records vector
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            println!("\nRecords ({})", vec_len);
            
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
                
                println!("\nRecord #{}:", i + 1);
                println!("  Pubkey: {}", pubkey);
                println!("  Signature: {}", signature);
                println!("  Slot: {}", slot);
                println!("  Blocktime: {}", blocktime);
            }

            println!("\nAccount Info:");
            println!("Owner: {}", account.owner);
            println!("Lamports: {}", account.lamports);
        }
        Err(err) => println!("Failed to get account: {}", err),
    }
}