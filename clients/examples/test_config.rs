/// Example: How to use the unified configuration module
/// 
/// This demonstrates how all client programs should read configuration
/// from Anchor.toml instead of hardcoding values.

use memo_token_client::{get_rpc_url, get_wallet_path, get_program_env, get_program_id, get_all_program_ids};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== UNIFIED CONFIGURATION TEST ===");
    println!();
    
    // 1. Get RPC URL
    println!("1. RPC Configuration:");
    let rpc_url = get_rpc_url();
    println!("   {}", rpc_url);
    println!();
    
    // 2. Get wallet path
    println!("2. Wallet Configuration:");
    let wallet_path = get_wallet_path();
    println!("   {}", wallet_path);
    println!();
    
    // 3. Get program environment
    println!("3. Program Environment:");
    let program_env = get_program_env();
    println!("   Current environment: {}", program_env);
    println!();
    
    // 4. Get specific program IDs
    println!("4. Individual Program IDs:");
    match get_program_id("memo_profile") {
        Ok(pubkey) => println!("   memo_profile: {}", pubkey),
        Err(e) => println!("   Error: {}", e),
    }
    
    match get_program_id("memo-burn") {  // Test dash format
        Ok(pubkey) => println!("   memo-burn: {}", pubkey),
        Err(e) => println!("   Error: {}", e),
    }
    println!();
    
    // 5. Get all program IDs
    println!("5. All Program IDs:");
    let all_programs = get_all_program_ids();
    for (name, pubkey) in all_programs.iter() {
        println!("   {} = {}", name, pubkey);
    }
    println!();
    
    println!("=== CONFIGURATION TEST COMPLETE ===");
    println!();
    println!("Configuration Summary:");
    println!("  - All values read from: Anchor.toml");
    println!("  - RPC cluster: {}", if rpc_url.contains("testnet") { "testnet" } else { "mainnet" });
    println!("  - Program environment: {}", program_env);
    println!("  - Total programs loaded: {}", all_programs.len());
    
    Ok(())
}

