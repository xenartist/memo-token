use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use memo_token_client::{get_rpc_url, get_program_id};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║    MEMO-BURN INITIALIZE SMOKE TEST (User Global Stats)      ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    
    // Connect to network
    let rpc_url = get_rpc_url();
    println!("─────────────────────────────────────────────────────────────────");
    println!("📋 Configuration");
    println!("─────────────────────────────────────────────────────────────────");
    println!("RPC URL:        {}", rpc_url);
    
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    
    println!("Payer:          {}", payer.pubkey());

    // Program ID
    let program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    
    println!("Program ID:     {}", program_id);

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _bump) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &program_id,
    );
    
    println!("Stats PDA:      {}", user_global_burn_stats_pda);
    println!();

    // Check if account already exists
    match client.get_account(&user_global_burn_stats_pda) {
        Ok(_) => {
            println!("─────────────────────────────────────────────────────────────────");
            println!("✅ Account Already Exists");
            println!("─────────────────────────────────────────────────────────────────");
            println!("The user global burn statistics account is already initialized.");
            println!("No action needed.");
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ✅ SMOKE TEST PASSED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            return Ok(());
        },
        Err(_) => {
            println!("─────────────────────────────────────────────────────────────────");
            println!("📝 Initializing Account");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Creating user global burn statistics account...");
        }
    }

    // Create instruction data for initialize_user_global_burn_stats
    // Discriminator for initialize_user_global_burn_stats
    let discriminator = [109, 178, 49, 106, 200, 87, 4, 107];
    let instruction_data = discriminator.to_vec();

    // Build accounts list
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),                      // user (signer, payer)
        AccountMeta::new(user_global_burn_stats_pda, false),         // user_global_burn_stats (to be created)
        AccountMeta::new_readonly(solana_sdk::system_program::id(), false), // system_program
    ];

    // Create initialize instruction
    let initialize_ix = Instruction::new_with_bytes(
        program_id,
        &instruction_data,
        accounts,
    );

    // Get latest blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    // Create transaction with compute budget
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    let transaction = Transaction::new_signed_with_payer(
        &[compute_budget_ix, initialize_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    println!("Sending transaction...");
    
    // Send and confirm transaction
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("✅ Initialization Successful");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Transaction:    {}", signature);
            println!("Stats Account:  {}", user_global_burn_stats_pda);
            println!();
            
            // Verify account was created
            match client.get_account(&user_global_burn_stats_pda) {
                Ok(account) => {
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("📊 Account Verification");
                    println!("─────────────────────────────────────────────────────────────────");
                    println!("Account Size:   {} bytes", account.data.len());
                    println!("Owner:          {}", account.owner);
                    println!("Lamports:       {}", account.lamports);
                    println!();
                    
                    println!("╔═══════════════════════════════════════════════════════════════╗");
                    println!("║                    ✅ SMOKE TEST PASSED                       ║");
                    println!("╚═══════════════════════════════════════════════════════════════╝");
                },
                Err(err) => {
                    println!("⚠️  Warning: Could not verify account: {}", err);
                    println!();
                    println!("╔═══════════════════════════════════════════════════════════════╗");
                    println!("║                    ⚠️  SMOKE TEST WARNING                     ║");
                    println!("╚═══════════════════════════════════════════════════════════════╝");
                }
            }
            
            Ok(())
        },
        Err(err) => {
            println!();
            println!("─────────────────────────────────────────────────────────────────");
            println!("❌ Initialization Failed");
            println!("─────────────────────────────────────────────────────────────────");
            println!("Error: {}", err);
            println!();
            println!("╔═══════════════════════════════════════════════════════════════╗");
            println!("║                    ❌ SMOKE TEST FAILED                       ║");
            println!("╚═══════════════════════════════════════════════════════════════╝");
            
            Err(err.into())
        }
    }
}

