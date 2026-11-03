// memo-id update_uri: change the metadata URI of a MEMO ID whose Asset the configured wallet
// owns (for example an ID that smoke-test-memo-id registered). The wallet is the controller and
// the payer. Use it to try metadata JSON drafts in wallets before COLLECTION_URI is fixed.
//
// Usage: cargo run --bin memo-id-update-uri -- <memo_id> <uri>
//        e.g. memo-id-update-uri smoke261028041132.memo https://ardrive.net/<txid>
// The transaction is simulated first and sent with the simulated compute units plus a margin.

use sha2::{Digest, Sha256};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use solana_system_interface::program as system_program;
use memo_token_client::{get_rpc_url, get_wallet_path, memo_id::{MemoIdAddresses, CORE_PROGRAM_ID}};

fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }

/// Core AssetV1: key, owner, update authority (tag + key), name, uri
fn core_uri(d: &[u8]) -> Option<String> {
    let mut at = 1 + 32 + 1 + 32;
    let name_len = u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize;
    at += 4 + name_len;
    let uri_len = u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize;
    String::from_utf8(d.get(at + 4..at + 4 + uri_len)?.to_vec()).ok()
}

fn main() {
    if let Err(e) = run() {
        println!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [memo_id, uri] = args.as_slice() else {
        return Err("usage: memo-id-update-uri <memo_id> <uri>".into());
    };
    let wallet = get_wallet_path();
    let payer = read_keypair_file(&wallet).map_err(|e| format!("cannot read wallet {}: {}", wallet, e))?;
    let client = RpcClient::new_with_commitment(get_rpc_url(), CommitmentConfig::confirmed());
    let a = MemoIdAddresses::from_config()?;

    let hash = sha(memo_id.as_bytes());
    let registry = Pubkey::find_program_address(&[b"memo_id", &hash], &a.program).0;
    let asset = Pubkey::find_program_address(&[b"asset", a.collection.as_ref(), &hash], &a.home).0;
    let data = client.get_account(&asset).map_err(|e| format!("Asset {} of {}: {}", asset, memo_id, e))?.data;
    println!("memo-id: {}", a.program);
    println!("ID:      {}", memo_id);
    println!("Asset:   {}", asset);
    println!("wallet:  {}", payer.pubkey());
    println!("old URI: {}", core_uri(&data).unwrap_or_default());

    let mut d = Sha256::digest(b"global:update_uri")[..8].to_vec();
    d.extend((uri.len() as u32).to_le_bytes());
    d.extend(uri.as_bytes());
    let ix = Instruction::new_with_bytes(a.program, &d, vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new_readonly(payer.pubkey(), true),
        AccountMeta::new_readonly(registry, false),
        AccountMeta::new(asset, false),
        AccountMeta::new_readonly(a.program, false), // holder: none
        AccountMeta::new_readonly(a.program, false), // holder_config: none
        AccountMeta::new_readonly(a.authority, false),
        AccountMeta::new_readonly(a.config, false),
        AccountMeta::new_readonly(a.collection, false),
        AccountMeta::new_readonly(a.home_authority, false),
        AccountMeta::new_readonly(CORE_PROGRAM_ID, false),
        AccountMeta::new_readonly(a.home, false),
        AccountMeta::new_readonly(system_program::ID, false),
    ]);
    let build = |limit: u32| -> Result<Transaction, String> {
        let bh = client.get_latest_blockhash().map_err(|e| e.to_string())?;
        Ok(Transaction::new_signed_with_payer(
            &[ix.clone(), ComputeBudgetInstruction::set_compute_unit_limit(limit)], Some(&payer.pubkey()), &[&payer], bh))
    };
    let sim = client.simulate_transaction_with_config(
        &build(1_400_000)?,
        RpcSimulateTransactionConfig { sig_verify: false, commitment: Some(CommitmentConfig::confirmed()), ..Default::default() },
    ).map_err(|e| e.to_string())?;
    if let Some(err) = sim.value.err {
        for l in sim.value.logs.unwrap_or_default() {
            println!("     {}", l);
        }
        return Err(format!("simulation failed: {:?}", err));
    }
    let units = sim.value.units_consumed.unwrap_or(0);
    let sig = client.send_and_confirm_transaction(&build((units + units / 10 + 50) as u32)?).map_err(|e| e.to_string())?;
    let data = client.get_account(&asset).map_err(|e| e.to_string())?.data;
    println!("new URI: {}", core_uri(&data).unwrap_or_default());
    println!("✅ update_uri: {} CU, {}", units, sig);
    Ok(())
}
