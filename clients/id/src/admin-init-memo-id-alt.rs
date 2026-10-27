// One-time setup after deploying and initializing memo-id: the frozen Address Lookup Table that
// register / issue_subid transactions use (they exceed the 1232-byte limit as legacy
// transactions). It holds the accounts every such transaction needs, whoever sends it
// (memo_id::MemoIdAddresses::lookup_table_addresses), and is frozen right after it is filled:
// nobody can change, deactivate or close it afterwards. The deploy admin pays the rent.
//
// The table is recorded in Anchor.toml [lookup_tables.<program_env>] memo_id = "...", which
// clients read. If it is already recorded, this only verifies it on chain. With --no-record
// (a Prod Staging cluster, which is not the network program_env names) it creates a table and
// only prints it.
//
// Usage: cargo run --bin admin-init-memo-id-alt [-- <memo-id program ID> [<RPC URL>]] [--no-record]
// Exit codes: 0 = created and recorded, or the recorded table is correct; 1 = failed

use memo_token_client::{
    get_lookup_table, get_program_env, get_program_id, get_rpc_url, memo_id::MemoIdAddresses, read_authority_keypair,
    record_lookup_table,
};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::{instruction as alt_ix, state::AddressLookupTable};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::time::Duration;

fn main() {
    if let Err(e) = run() {
        println!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== MEMO-ID ADDRESS LOOKUP TABLE ===");
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let record = match args.iter().position(|a| a == "--no-record") {
        Some(i) => {
            args.remove(i);
            false
        }
        None => true,
    };
    let rpc_url = args.get(1).cloned().unwrap_or_else(get_rpc_url);
    println!("🔍 Connecting to: {}", rpc_url);
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let env = get_program_env();

    let program_id = match args.first() {
        Some(id) => id.parse::<Pubkey>().map_err(|e| format!("invalid program ID {}: {}", id, e))?,
        None => get_program_id("memo_id")?,
    };
    let a = MemoIdAddresses::new(program_id, get_program_id("memo_home")?);
    let addresses = a.lookup_table_addresses_from_config()?;
    println!("memo-id: {}", a.program);

    // Idempotent: a recorded table is only verified
    if let Some(table) = get_lookup_table("memo_id")?.filter(|_| record) {
        println!("Recorded in Anchor.toml [lookup_tables.{}]: {}", env, table);
        check(&client, &table, &addresses)?;
        println!("✅ Lookup table is frozen with the expected addresses. No action needed.");
        return Ok(());
    }

    // The table lists the Collection; create it only once the Collection exists
    if client.get_account(&a.collection).is_err() {
        return Err(format!("MEMO ID Collection {} does not exist: run admin-init-memo-id first", a.collection).into());
    }

    let payer = read_authority_keypair("deploy_admin")?;
    println!("Payer and temporary authority (deploy admin): {}", payer.pubkey());
    let slot = client.get_slot_with_commitment(CommitmentConfig::finalized())?;
    #[allow(deprecated)]
    let (create, table) = alt_ix::create_lookup_table(payer.pubkey(), payer.pubkey(), slot);
    #[allow(deprecated)]
    let extend = alt_ix::extend_lookup_table(table, payer.pubkey(), Some(payer.pubkey()), addresses.clone());
    println!("Lookup table: {}", table);

    send(&client, &payer, "create and fill the lookup table", vec![create, extend])?;
    #[allow(deprecated)]
    let freeze = alt_ix::freeze_lookup_table(table, payer.pubkey());
    send(&client, &payer, "freeze the lookup table", vec![freeze])?;

    // Addresses can be loaded from the slot after the extension
    let after = client.get_slot()? + 1;
    while client.get_slot()? <= after {
        std::thread::sleep(Duration::from_millis(400));
    }
    check(&client, &table, &addresses)?;

    if record {
        let path = record_lookup_table(&env, "memo_id", &table)?;
        println!("Recorded in {}: [lookup_tables.{}] memo_id = \"{}\"", path.display(), env, table);
    } else {
        println!("Not recorded (--no-record): memo-id lookup table on {} = {}", client.url(), table);
    }
    println!("🎉 Lookup table ready. Clients compile register / issue_subid as v0 messages with it.");
    Ok(())
}

/// X1 charges by the requested CU limit: simulate, then send with the simulated units + margin
fn send(client: &RpcClient, payer: &Keypair, what: &str, mut ixs: Vec<Instruction>) -> Result<(), String> {
    let blockhash = client.get_latest_blockhash().map_err(|e| e.to_string())?;
    let mut sim_ixs = ixs.clone();
    sim_ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(200_000));
    let sim_tx = Transaction::new_signed_with_payer(&sim_ixs, Some(&payer.pubkey()), &[payer], blockhash);
    let sim = client
        .simulate_transaction_with_config(&sim_tx, RpcSimulateTransactionConfig {
            sig_verify: false,
            commitment: Some(CommitmentConfig::confirmed()),
            ..RpcSimulateTransactionConfig::default()
        })
        .map_err(|e| e.to_string())?;
    if let Some(err) = sim.value.err {
        for log in sim.value.logs.unwrap_or_default() {
            println!("   {}", log);
        }
        return Err(format!("{}: simulation failed: {:?}", what, err));
    }
    let units = sim.value.units_consumed.unwrap_or(20_000) as u32;
    ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(units + units / 10 + 50));
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &[payer], blockhash);
    let signature = client.send_and_confirm_transaction(&tx).map_err(|e| format!("{}: {}", what, e))?;
    println!("{} ({} CU): {}", what, units, signature);
    Ok(())
}

/// Frozen (no authority), active, and exactly the expected addresses in order
fn check(client: &RpcClient, table: &Pubkey, expected: &[Pubkey]) -> Result<(), String> {
    let account = client.get_account(table).map_err(|e| format!("lookup table {}: {}", table, e))?;
    if account.owner != solana_sdk::address_lookup_table::program::id() {
        return Err(format!("{} is owned by {}, not the Address Lookup Table program", table, account.owner));
    }
    let state = AddressLookupTable::deserialize(&account.data).map_err(|e| format!("lookup table {}: {}", table, e))?;
    if let Some(authority) = state.meta.authority {
        return Err(format!("lookup table {} is not frozen (authority {})", table, authority));
    }
    if state.meta.deactivation_slot != u64::MAX {
        return Err(format!("lookup table {} is deactivated", table));
    }
    if state.addresses.as_ref() != expected {
        return Err(format!("lookup table {} holds {:?}, expected {:?}", table, state.addresses, expected));
    }
    println!("   frozen, active, {} addresses:", expected.len());
    for (i, k) in expected.iter().enumerate() {
        println!("   {:>3}  {}", i, k);
    }
    Ok(())
}
