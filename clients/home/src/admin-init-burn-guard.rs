// One-time setup after deploying memo-home: creates the global Burn Guard (the Core Oracle that
// rejects every Asset burn). create_collection fails until it exists. The instruction is
// permissionless and the content is fixed; the deploy admin pays the rent.
//
// Usage: cargo run --bin admin-init-burn-guard [-- <memo-home program ID> [<RPC URL>]]
// Exit codes: 0 = created, or already present with the expected content; 1 = failed

use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};
use sha2::{Digest, Sha256};
use solana_system_interface::program as system_program;
use memo_token_client::{get_program_id, get_rpc_url, read_authority_keypair};

/// Anchor discriminator + OracleValidation::V1 { create: Pass, transfer: Pass, burn: Rejected, update: Pass } + bump
const BURN_GUARD_SPACE: usize = 14;
const EXPECTED_VALIDATION: [u8; 5] = [1, 2, 2, 1, 2];

fn main() {
    if let Err(e) = run() {
        println!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-HOME INITIALIZE BURN GUARD ===");
    let rpc_url = std::env::args().nth(2).unwrap_or_else(get_rpc_url);
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    let program_id = match std::env::args().nth(1) {
        Some(id) => id.parse::<Pubkey>().map_err(|e| format!("invalid program ID {}: {}", id, e))?,
        None => get_program_id("memo_home")?,
    };
    let (burn_guard, _) = Pubkey::find_program_address(&[b"burn_guard"], &program_id);
    println!("memo-home: {}", program_id);
    println!("Burn Guard: {}", burn_guard);

    // Idempotent: an existing Burn Guard must have the fixed content
    if let Ok(account) = client.get_account(&burn_guard) {
        check(&account.owner, &account.data, &program_id)?;
        println!("✅ Burn Guard already exists. No action needed.");
        return Ok(());
    }

    let payer = read_authority_keypair("deploy_admin")?;
    println!("Payer (deploy admin): {}", payer.pubkey());
    let ix = Instruction::new_with_bytes(program_id, &Sha256::digest(b"global:init_burn_guard")[..8], vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(burn_guard, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ]);

    // X1 charges by the requested CU limit: simulate, then send with the simulated units + margin
    let blockhash = client.get_latest_blockhash()?;
    let sim_tx = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(200_000), ix.clone()], Some(&payer.pubkey()), &[&payer], blockhash);
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
    let units = sim.value.units_consumed.unwrap_or(20_000) as u32;
    let limit = units + units / 10 + 50;
    println!("Simulated {} CU, sending with limit {}", units, limit);
    let tx = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(limit), ix], Some(&payer.pubkey()), &[&payer], blockhash);
    let signature = client.send_and_confirm_transaction(&tx).map_err(|e| format!("initialization failed: {}", e))?;
    println!("Transaction: {}", signature);

    let account = client.get_account(&burn_guard)?;
    check(&account.owner, &account.data, &program_id)?;
    println!("🎉 Burn Guard created. memo-home is ready for create_collection.");
    Ok(())
}

fn check(owner: &Pubkey, data: &[u8], program_id: &Pubkey) -> Result<(), String> {
    let discriminator = &Sha256::digest(b"account:BurnGuard")[..8];
    if owner != program_id || data.len() != BURN_GUARD_SPACE || &data[..8] != discriminator || data[8..13] != EXPECTED_VALIDATION {
        return Err(format!("unexpected Burn Guard account: owner {}, {} bytes, data {:?}", owner, data.len(), data));
    }
    println!("   owner memo-home, OracleValidation::V1 {{ create Pass, transfer Pass, burn Rejected, update Pass }}");
    Ok(())
}
