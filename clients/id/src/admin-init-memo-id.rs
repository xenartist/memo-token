// One-time setup after deploying memo-id: `init` creates the MEMO ID Collection through memo-home
// (Authority = the memo-id PDA, name and URI fixed in the program). register fails until it
// exists. The instruction is permissionless and its result is fixed; the deploy admin pays the
// rent. memo-home's Burn Guard must exist first (admin-init-burn-guard).
//
// Usage: cargo run --bin admin-init-memo-id [-- <memo-id program ID> [<RPC URL>]]
// Exit codes: 0 = created, or already present; 1 = failed

use memo_token_client::{get_program_id, get_rpc_url, memo_id::{MemoIdAddresses, CORE_PROGRAM_ID}, read_authority_keypair};
use sha2::{Digest, Sha256};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};
use solana_system_interface::program as system_program;

fn main() {
    if let Err(e) = run() {
        println!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-ID INITIALIZE (MEMO ID COLLECTION) ===");
    let rpc_url = std::env::args().nth(2).unwrap_or_else(get_rpc_url);
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    let program_id = match std::env::args().nth(1) {
        Some(id) => id.parse::<Pubkey>().map_err(|e| format!("invalid program ID {}: {}", id, e))?,
        None => get_program_id("memo_id")?,
    };
    let a = MemoIdAddresses::new(program_id, get_program_id("memo_home")?);
    println!("memo-id: {}", a.program);
    println!("memo-home: {}", a.home);
    println!("MEMO ID Collection: {}", a.collection);
    println!("Collection Config: {}", a.config);

    // Idempotent: memo-home rejects a second create_collection, so existing accounts are final
    if let Ok(collection) = client.get_account(&a.collection) {
        check(&client, &a, &collection.owner)?;
        println!("✅ MEMO ID Collection already exists. No action needed.");
        return Ok(());
    }
    if client.get_account(&a.burn_guard).is_err() {
        return Err(format!("memo-home Burn Guard {} does not exist: run admin-init-burn-guard first", a.burn_guard).into());
    }

    let payer = read_authority_keypair("deploy_admin")?;
    println!("Payer (deploy admin): {}", payer.pubkey());
    let ix = Instruction::new_with_bytes(a.program, &Sha256::digest(b"global:init")[..8], vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new_readonly(a.authority, false),
        AccountMeta::new(a.collection, false),
        AccountMeta::new_readonly(a.home_authority, false),
        AccountMeta::new_readonly(a.burn_guard, false),
        AccountMeta::new(a.config, false),
        AccountMeta::new_readonly(CORE_PROGRAM_ID, false),
        AccountMeta::new_readonly(a.home, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ]);

    // X1 charges by the requested CU limit: simulate, then send with the simulated units + margin
    let blockhash = client.get_latest_blockhash()?;
    let sim_tx = Transaction::new_signed_with_payer(
        &[ix.clone(), ComputeBudgetInstruction::set_compute_unit_limit(400_000)], Some(&payer.pubkey()), &[&payer], blockhash);
    let sim = client.simulate_transaction_with_config(&sim_tx, RpcSimulateTransactionConfig {
        sig_verify: false,
        commitment: Some(CommitmentConfig::confirmed()),
        ..RpcSimulateTransactionConfig::default()
    })?;
    if let Some(err) = sim.value.err {
        for log in sim.value.logs.unwrap_or_default() {
            println!("   {}", log);
        }
        return Err(format!("simulation failed: {:?}", err).into());
    }
    let units = sim.value.units_consumed.unwrap_or(100_000) as u32;
    let limit = units + units / 10 + 50;
    println!("Simulated {} CU, sending with limit {}", units, limit);
    let tx = Transaction::new_signed_with_payer(
        &[ix, ComputeBudgetInstruction::set_compute_unit_limit(limit)], Some(&payer.pubkey()), &[&payer], blockhash);
    let signature = client.send_and_confirm_transaction(&tx).map_err(|e| format!("initialization failed: {}", e))?;
    println!("Transaction: {}", signature);

    let collection = client.get_account(&a.collection)?;
    check(&client, &a, &collection.owner)?;
    println!("🎉 MEMO ID Collection created. memo-id is ready for register.");
    Ok(())
}

/// The Collection is a Core account updated by memo-home's Authority PDA, and memo-home's
/// Collection Config names the memo-id Authority PDA as the Collection's authority
fn check(client: &RpcClient, a: &MemoIdAddresses, owner: &Pubkey) -> Result<(), String> {
    let data = client.get_account_data(&a.collection).map_err(|e| e.to_string())?;
    // Core CollectionV1: key (1 byte) = 5, then update_authority
    if *owner != CORE_PROGRAM_ID || data.len() < 33 || data[0] != 5 || data[1..33] != a.home_authority.to_bytes() {
        return Err(format!("unexpected MEMO ID Collection account: owner {}, {} bytes", owner, data.len()));
    }
    let config = client.get_account(&a.config).map_err(|e| format!("Collection Config {} missing: {}", a.config, e))?;
    // CollectionConfig: discriminator, collection, authority
    let d = &config.data;
    if config.owner != a.home || d.len() < 72 || d[8..40] != a.collection.to_bytes() || d[40..72] != a.authority.to_bytes() {
        return Err(format!("unexpected Collection Config {}: owner {}, {} bytes", a.config, config.owner, d.len()));
    }
    println!("   Collection: Core, update authority = memo-home Authority {}", a.home_authority);
    println!("   Config: memo-home, authority = memo-id Authority {}", a.authority);
    Ok(())
}
