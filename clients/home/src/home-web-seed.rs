// Seed the home:// extension prototype on the local validator: a memo-home Asset owned by the
// wallet, a MEMO ID pointing at it, and every file of a site folder stored on-chain through the
// throwaway home_web_test program (clients/regression/test-programs/home-web).
//
//   ./scripts/local-validator.sh start
//   cd clients && MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=../target/local-validator/wallet.json \
//       cargo run --bin home-web-seed -- [--name xen-artist.memo] [--site home-extension/demo-site]
//
// Idempotent: existing Burn Guard, Collection, Asset and name are reused; a file whose content
// changed is closed and written again. Local validator only (the program is never deployed).

use sha2::{Digest, Sha256};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    transaction::Transaction,
};
use solana_system_interface::program as system_program;
use memo_token_client::{get_program_id, get_rpc_url, get_wallet_path};

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const WEB: Pubkey = solana_sdk::pubkey!("6uknhjicEvg7B7oAvZ3y7TWojo8YfvKicn7HFVDPtiip");
const FILE_HEADER: usize = 156;
const MAX_FILE_SIZE: usize = 9 * 1024;
const CHUNK: usize = 900;

fn disc(name: &str) -> Vec<u8> { Sha256::digest(format!("global:{}", name).as_bytes())[..8].to_vec() }
fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

struct Ctx {
    client: RpcClient,
    payer: Keypair,
}

impl Ctx {
    /// Simulate, then send with the simulated units + 10% + 50
    fn send(&self, label: &str, ixs: Vec<Instruction>) -> Result<(), String> {
        let p = self.payer.pubkey();
        let build = |limit: u32| {
            let mut all = ixs.clone();
            all.push(ComputeBudgetInstruction::set_compute_unit_limit(limit));
            all
        };
        let bh = self.client.get_latest_blockhash().map_err(|e| e.to_string())?;
        let sim = self.client.simulate_transaction_with_config(
            &Transaction::new_signed_with_payer(&build(1_400_000), Some(&p), &[&self.payer], bh),
            RpcSimulateTransactionConfig { sig_verify: false, commitment: Some(CommitmentConfig::confirmed()), ..Default::default() },
        ).map_err(|e| e.to_string())?;
        if let Some(err) = sim.value.err {
            for l in sim.value.logs.unwrap_or_default() {
                println!("     {}", l);
            }
            return Err(format!("{}: simulation failed: {:?}", label, err));
        }
        let units = sim.value.units_consumed.unwrap_or(0);
        let limit = (units + units / 10 + 50) as u32;
        self.client
            .send_and_confirm_transaction(&Transaction::new_signed_with_payer(&build(limit), Some(&p), &[&self.payer], bh))
            .map_err(|e| format!("{}: {}", label, e))?;
        println!("  ✅ {} ({} CU)", label, units);
        Ok(())
    }
    fn data(&self, key: &Pubkey) -> Option<Vec<u8>> {
        self.client.get_account_with_commitment(key, CommitmentConfig::confirmed()).ok().and_then(|r| r.value).map(|a| a.data)
    }
}

fn arg(args: &[String], flag: &str, default: &str) -> String {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
}

fn main() {
    if let Err(e) = run() {
        println!("\n❌ {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let name = arg(&args, "--name", "xen-artist.memo");
    let site = arg(&args, "--site", "home-extension/demo-site");
    let rpc = get_rpc_url();
    if !rpc.contains("127.0.0.1") && !rpc.contains("localhost") {
        return Err(format!("{} is not a local validator: home_web_test only runs locally (set MEMO_RPC_URL)", rpc));
    }
    let wallet = shellexpand::tilde(&get_wallet_path()).to_string();
    let payer = read_keypair_file(&wallet).map_err(|e| format!("cannot read wallet {}: {}", wallet, e))?;
    let cx = Ctx { client: RpcClient::new_with_commitment(rpc.clone(), CommitmentConfig::confirmed()), payer };
    let home = get_program_id("memo_home")?;
    let w = cx.payer.pubkey();
    if cx.data(&WEB).is_none() {
        return Err(format!("home_web_test {} is not loaded: build it (run-regression-tests.sh home) and restart the validator", WEB));
    }
    println!("RPC {}  wallet {}  memo-home {}  home_web_test {}", rpc, w, home, WEB);
    let hp = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &home).0;
    let wp = |seeds: &[&[u8]]| Pubkey::find_program_address(seeds, &WEB).0;

    // Burn Guard (permissionless, once per memo-home deployment)
    let burn_guard = hp(&[b"burn_guard"]);
    if cx.data(&burn_guard).is_none() {
        cx.send("init_burn_guard", vec![Instruction::new_with_bytes(home, &disc("init_burn_guard"), vec![
            AccountMeta::new(w, true), AccountMeta::new(burn_guard, false), AccountMeta::new_readonly(system_program::id(), false),
        ])])?;
    }

    // Collection "MEMO Web demo" with the wallet as Authority
    let program_authority = hp(&[b"authority"]);
    let cseed = sha(b"home-web-demo");
    let col = hp(&[b"collection", w.as_ref(), &cseed]);
    let config = hp(&[b"collection_config", col.as_ref()]);
    if cx.data(&col).is_none() {
        let mut a = cseed.to_vec();
        bstr(&mut a, "MEMO Web demo");
        bstr(&mut a, "https://example.com/home-web-demo.json");
        a.extend(0u16.to_le_bytes());
        a.extend([0, 0]);
        cx.send("create_collection", vec![Instruction::new_with_bytes(home, &[disc("create_collection"), a].concat(), vec![
            AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new(col, false),
            AccountMeta::new_readonly(program_authority, false), AccountMeta::new_readonly(burn_guard, false),
            AccountMeta::new(config, false), AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program::id(), false),
        ])])?;
    }

    // The site's Asset, seeded by the name as memo-id will do
    let name_hash = sha(name.as_bytes());
    let asset = hp(&[b"asset", col.as_ref(), &name_hash]);
    if cx.data(&asset).is_none() {
        let vault_state = hp(&[b"vault_state", asset.as_ref()]);
        let mut a = name_hash.to_vec();
        bstr(&mut a, &name);
        bstr(&mut a, "https://example.com/home-web-demo-asset.json");
        cx.send("create_asset", vec![Instruction::new_with_bytes(home, &[disc("create_asset"), a].concat(), vec![
            AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(config, false),
            AccountMeta::new(col, false), AccountMeta::new(asset, false), AccountMeta::new(vault_state, false),
            AccountMeta::new_readonly(w, false), AccountMeta::new_readonly(program_authority, false),
            AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program::id(), false),
        ])])?;
    }
    match cx.data(&asset) {
        Some(d) if d.len() >= 33 && d[0] == 1 && d[1..33] == w.to_bytes() => {}
        _ => return Err(format!("Asset {} is not owned by the wallet", asset)),
    }

    // Name → Asset
    let record = wp(&[b"name", &name_hash]);
    match cx.data(&record) {
        Some(d) if d.len() >= 40 && d[8..40] != asset.to_bytes() => {
            return Err(format!("{} is already registered to another Asset", name));
        }
        Some(_) => {}
        None => {
            let mut a = name_hash.to_vec();
            bstr(&mut a, &name);
            cx.send(&format!("register_name {}", name), vec![Instruction::new_with_bytes(WEB, &[disc("register_name"), a].concat(), vec![
                AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(asset, false),
                AccountMeta::new(record, false), AccountMeta::new_readonly(system_program::id(), false),
            ])])?;
        }
    }

    // Files
    let mut paths = Vec::new();
    collect(std::path::Path::new(&site), std::path::Path::new(&site), &mut paths)?;
    if paths.is_empty() {
        return Err(format!("no files in {}", site));
    }
    paths.sort();
    for (path, full) in paths {
        let content = std::fs::read(&full).map_err(|e| format!("{}: {}", full.display(), e))?;
        if content.len() > MAX_FILE_SIZE {
            return Err(format!("{}: {} bytes, the prototype allows {}", path, content.len(), MAX_FILE_SIZE));
        }
        let hash = sha(&content);
        let file = wp(&[b"file", asset.as_ref(), &sha(path.as_bytes())]);
        if let Some(d) = cx.data(&file) {
            if d.len() == FILE_HEADER + content.len() && d[72..104] == hash && d[FILE_HEADER..] == content[..] {
                println!("  ·  {} unchanged", path);
                continue;
            }
            cx.send(&format!("close_file {}", path), vec![Instruction::new_with_bytes(WEB, &disc("close_file"), vec![
                AccountMeta::new(w, true), AccountMeta::new_readonly(asset, false), AccountMeta::new(file, false),
            ])])?;
        }
        let ct = content_type(&path);
        let mut a = sha(path.as_bytes()).to_vec();
        a.extend((content.len() as u32).to_le_bytes());
        a.extend(hash);
        bstr(&mut a, ct);
        cx.send(&format!("init_file {} ({} bytes, {})", path, content.len(), ct), vec![Instruction::new_with_bytes(WEB, &[disc("init_file"), a].concat(), vec![
            AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(asset, false),
            AccountMeta::new(file, false), AccountMeta::new_readonly(system_program::id(), false),
        ])])?;
        for (i, chunk) in content.chunks(CHUNK).enumerate() {
            let mut a = ((i * CHUNK) as u32).to_le_bytes().to_vec();
            a.extend((chunk.len() as u32).to_le_bytes());
            a.extend(chunk);
            cx.send(&format!("write_file {} @{}", path, i * CHUNK), vec![Instruction::new_with_bytes(WEB, &[disc("write_file"), a].concat(), vec![
                AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(asset, false), AccountMeta::new(file, false),
            ])])?;
        }
    }

    println!("\nAsset {}\nOpen  home://{}/  or  home://{}/", asset, name, asset);
    Ok(())
}

/// Files under `dir` as (path relative to `root` with '/' separators, full path); hidden files skipped
fn collect(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, std::path::PathBuf)>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(true) {
            continue;
        }
        if p.is_dir() {
            collect(root, &p, out)?;
        } else {
            let rel = p.strip_prefix(root).map_err(|e| e.to_string())?.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            out.push((rel, p));
        }
    }
    Ok(())
}
