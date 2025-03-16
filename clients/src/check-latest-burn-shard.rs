use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

fn main() {
    // Get category from command line args
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} <category>", args[0]);
        return;
    }
    let category = args[1].clone();

    // Connect to network
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let client = RpcClient::new(rpc_url);

    // Program ID
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");

    // Calculate latest burn shard PDA
    let (latest_burn_shard_pda, _) = Pubkey::find_program_address(
        &[b"latest_burn_shard", category.as_bytes()],
        &program_id,
    );

    println!("Latest Burn Shard PDA: {}", latest_burn_shard_pda);
    println!("Category: {}", category);

    // Get account data
    match client.get_account(&latest_burn_shard_pda) {
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
            
            // 解析 category string
            let mut offset = 32;
            let category_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            let category = String::from_utf8_lossy(&data[offset..offset+category_len]);
            offset += category_len;
            println!("Category: {}", category);
            
            // 解析 current_index (1 byte)
            let current_index = data[offset];
            println!("Current Index: {}", current_index);
            offset += 1;
            
            // 解析 records vector
            let vec_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            println!("\nRecords ({})", vec_len);
            
            for i in 0..vec_len {
                // 读取 pubkey (32 bytes)
                let pubkey = Pubkey::new(&data[offset..offset+32]);
                offset += 32;
                
                // 读取 signature string
                let sig_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
                offset += 4;
                let signature = String::from_utf8_lossy(&data[offset..offset+sig_len]);
                offset += sig_len;
                
                // 读取 slot (8 bytes)
                let slot = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                offset += 8;
                
                // 读取 blocktime (8 bytes)
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