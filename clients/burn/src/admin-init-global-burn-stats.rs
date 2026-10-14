use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use std::path::PathBuf;
use sha2::{Sha256, Digest};
use solana_system_interface::program as system_program;
use memo_token_client::{get_rpc_url, get_program_id};

// Get admin authority keypair path (unified for all environments)
fn get_admin_authority_keypair_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    PathBuf::from(home)
        .join(".config/solana/memo-token/authority/deploy_admin-keypair.json")
}

// Exit codes: 0 = initialized or already initialized, 1 = failed
fn main() {
    if let Err(e) = run() {
        println!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-BURN INITIALIZE GLOBAL BURN STATS (ADMIN ONLY) ===");
    println!("One-time setup: creates the global burn statistics account.");
    println!("Run it right after deploying memo-burn: every burn fails until this account exists.");
    println!();

    // Connect to network: optional second argument overrides the RPC URL from Anchor.toml
    // (the deploy script passes the cluster it just deployed to)
    let rpc_url = std::env::args().nth(2).unwrap_or_else(get_rpc_url);
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load admin wallet from unified authority keypair location
    let admin_keypair_path = get_admin_authority_keypair_path();
    println!("Loading admin keypair from: {}", admin_keypair_path.display());
    let admin = read_keypair_file(&admin_keypair_path)
        .map_err(|e| format!("Failed to read admin keypair file {:?}: {}. Run setup-keypairs.sh first.", admin_keypair_path, e))?;
    println!("   Admin address: {}", admin.pubkey());
    println!();

    // Optional argument: memo-burn program ID (the deploy script passes the ID it just deployed);
    // otherwise read from Anchor.toml
    let memo_burn_program_id = match std::env::args().nth(1) {
        Some(id) => id.parse::<Pubkey>().map_err(|e| format!("invalid program ID {}: {}", id, e))?,
        None => get_program_id("memo_burn")?,
    };
    let (global_burn_stats_pda, bump) = Pubkey::find_program_address(
        &[b"global_burn_stats"],
        &memo_burn_program_id,
    );

    println!("Program addresses:");
    println!("  Memo-burn program: {}", memo_burn_program_id);
    println!("  Global burn stats PDA: {}", global_burn_stats_pda);
    println!("  PDA bump: {}", bump);
    println!();

    // Idempotent: nothing to do if the account already exists
    if let Ok(account) = client.get_account(&global_burn_stats_pda) {
        println!("✅ Global burn stats already exists.");
        println!("   Owner: {}", account.owner);
        if account.owner != memo_burn_program_id {
            return Err(format!("account is owned by {}, expected memo-burn", account.owner).into());
        }
        print_global_burn_stats(&account.data);
        println!("No action needed.");
        return Ok(());
    }
    println!("ℹ️  Global burn stats not found. Initializing...");

    let admin_balance = client.get_balance(&admin.pubkey())?;
    println!("Admin balance: {:.4} XNT", admin_balance as f64 / 1_000_000_000.0);
    if admin_balance < 1_000_000 {
        return Err("insufficient balance for rent and fees".into());
    }

    let recent_blockhash = client.get_latest_blockhash()?;
    let init_ix = create_initialize_global_burn_stats_instruction(
        &memo_burn_program_id,
        &admin.pubkey(),
        &global_burn_stats_pda,
    );

    // Simulate to get the compute unit limit (X1 charges by requested CU limit)
    let sim_transaction = Transaction::new_signed_with_payer(
        &[init_ix.clone(), ComputeBudgetInstruction::set_compute_unit_limit(200_000)],
        Some(&admin.pubkey()),
        &[&admin],
        recent_blockhash,
    );
    let sim = client.simulate_transaction_with_config(
        &sim_transaction,
        RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: false,
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None,
            accounts: None,
            min_context_slot: None,
            inner_instructions: false,
        },
    )?;
    if let Some(err) = sim.value.err {
        if let Some(logs) = sim.value.logs {
            for log in logs {
                println!("   {}", log);
            }
        }
        return Err(format!("simulation failed: {:?}", err).into());
    }
    let units = sim.value.units_consumed.unwrap_or(50_000);
    let cu_limit = ((units as f64) * 1.1) as u32;
    println!("Simulation consumed {} CUs, setting limit to {}", units, cu_limit);

    let transaction = Transaction::new_signed_with_payer(
        &[init_ix, ComputeBudgetInstruction::set_compute_unit_limit(cu_limit)],
        Some(&admin.pubkey()),
        &[&admin],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("🎉 INITIALIZATION SUCCESSFUL!");
            println!("Transaction signature: {}", signature);
            let account = client.get_account(&global_burn_stats_pda)?;
            print_global_burn_stats(&account.data);
            println!("✅ memo-burn is now ready to accept burns (direct and via app CPI).");
            Ok(())
        }
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("UnauthorizedAdmin") || msg.contains("0x177c") {
                println!("💡 Only AUTHORIZED_ADMIN_PUBKEY in memo-burn can initialize the global burn stats.");
            } else if msg.contains("already in use") {
                println!("💡 The account already exists (initialized concurrently?).");
            }
            Err(format!("initialization failed: {}", msg).into())
        }
    }
}

fn create_initialize_global_burn_stats_instruction(
    program_id: &Pubkey,
    admin: &Pubkey,
    global_burn_stats: &Pubkey,
) -> Instruction {
    // Anchor instruction sighash for "initialize_global_burn_stats"
    let mut hasher = Sha256::new();
    hasher.update(b"global:initialize_global_burn_stats");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec(); // Only the sighash, no parameters

    let accounts = vec![
        AccountMeta::new(*admin, true),                            // admin (signer, must be authorized)
        AccountMeta::new(*global_burn_stats, false),               // global_burn_stats (PDA to be created)
        AccountMeta::new_readonly(system_program::id(), false),    // system_program
    ];

    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

// GlobalBurnStats layout: 8 discriminator + total_burned u128 + burn_count u64 + last_burn_time i64 + bump u8 + reserved [u8; 64]
fn print_global_burn_stats(data: &[u8]) {
    if data.len() < 8 + 16 + 8 + 8 + 1 {
        println!("   ⚠️  Unexpected data length {}, cannot decode", data.len());
        return;
    }
    let total_burned = u128::from_le_bytes(data[8..24].try_into().unwrap());
    let burn_count = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let last_burn_time = i64::from_le_bytes(data[32..40].try_into().unwrap());
    println!("   total_burned: {} units ({} MEMO)", total_burned, total_burned / 1_000_000);
    println!("   burn_count: {}", burn_count);
    println!("   last_burn_time: {}", last_burn_time);
}
