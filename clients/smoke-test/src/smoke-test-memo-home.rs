// memo-home smoke test: the main path on a live network, with the configured wallet as both the
// Collection's Authority and the Asset's owner.
//
//   Burn Guard present → create_collection → create_asset → Core burn rejected (simulated) →
//   mine with a 30% share → withdraw → Vault burn → withdraw everything
//
// Each run creates a new Collection and Asset (about 0.012 XNT in rent and fees). Every
// transaction is simulated first and sent with the simulated compute units plus a margin.
// The wallet needs XNT and, for the Vault burn, at least one mint's worth of MEMO.

use base64::{engine::general_purpose, Engine as _};
use borsh::BorshSerialize;
use chrono::Utc;
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
use spl_associated_token_account::{
    get_associated_token_address_with_program_id as ata_of, instruction::create_associated_token_account_idempotent,
};
use spl_token_2022::{extension::StateWithExtensions, id as token_2022_id, state::Account as TokenAccount};
use memo_token_client::{expected_mint_amount, fetch_min_burn_amount, get_program_id, get_rpc_url, get_token_mint, get_wallet_path};

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const SHARE_BPS: u16 = 3000;

#[derive(BorshSerialize)]
struct BurnMemo {
    version: u8,
    burn_amount: u64,
    payload: Vec<u8>,
}

fn disc(name: &str) -> Vec<u8> { Sha256::digest(format!("global:{}", name).as_bytes())[..8].to_vec() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}

struct Ctx {
    client: RpcClient,
    payer: Keypair,
    home: Pubkey,
    mint: Pubkey,
}

impl Ctx {
    /// Simulate, then send with the simulated units + 10% + 50 (the memo, if any, stays at index 0)
    fn send(&self, label: &str, ixs: Vec<Instruction>) -> Result<u64, String> {
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
        let sig = self.client
            .send_and_confirm_transaction(&Transaction::new_signed_with_payer(&build(limit), Some(&p), &[&self.payer], bh))
            .map_err(|e| format!("{}: {}", label, e))?;
        println!("  ✅ {}: {} CU (limit {}), {}", label, units, limit, sig);
        Ok(units)
    }
    /// Simulation only; Ok(()) when the transaction fails
    fn must_fail(&self, label: &str, ixs: Vec<Instruction>) -> Result<(), String> {
        let bh = self.client.get_latest_blockhash().map_err(|e| e.to_string())?;
        let mut all = ixs;
        all.push(ComputeBudgetInstruction::set_compute_unit_limit(400_000));
        let sim = self.client.simulate_transaction_with_config(
            &Transaction::new_signed_with_payer(&all, Some(&self.payer.pubkey()), &[&self.payer], bh),
            RpcSimulateTransactionConfig { sig_verify: false, commitment: Some(CommitmentConfig::confirmed()), ..Default::default() },
        ).map_err(|e| e.to_string())?;
        match sim.value.err {
            Some(err) => {
                println!("  ✅ {}: rejected as expected ({:?})", label, err);
                Ok(())
            }
            None => Err(format!("{}: simulation succeeded, expected a rejection", label)),
        }
    }
    fn pda(&self, seeds: &[&[u8]]) -> Pubkey { Pubkey::find_program_address(seeds, &self.home).0 }
    fn balance(&self, ta: &Pubkey) -> u64 {
        self.client.get_account_with_commitment(ta, CommitmentConfig::confirmed()).ok().and_then(|r| r.value)
            .and_then(|a| StateWithExtensions::<TokenAccount>::unpack(&a.data).ok().map(|s| s.base.amount)).unwrap_or(0)
    }
    fn nonce(&self, asset: &Pubkey) -> Option<u64> {
        let state = self.pda(&[b"vault_state", asset.as_ref()]);
        self.client.get_account_with_commitment(&state, CommitmentConfig::confirmed()).ok().and_then(|r| r.value)
            .filter(|a| a.owner == self.home && a.data.len() == 17)
            .map(|a| u64::from_le_bytes(a.data[8..16].try_into().unwrap()))
    }
    fn supply(&self) -> Result<u64, String> {
        self.client.get_token_supply_with_commitment(&self.mint, CommitmentConfig::confirmed()).map_err(|e| e.to_string())?
            .value.amount.parse().map_err(|e: std::num::ParseIntError| e.to_string())
    }
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                   MEMO-HOME SMOKE TEST                        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    match run() {
        Ok(()) => println!("\n✅ SMOKE TEST PASSED"),
        Err(e) => {
            println!("\n❌ SMOKE TEST FAILED: {}", e);
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let wallet = shellexpand::tilde(&get_wallet_path()).to_string();
    let payer = read_keypair_file(&wallet).map_err(|e| format!("cannot read wallet {}: {}", wallet, e))?;
    let cx = Ctx {
        client: RpcClient::new_with_commitment(get_rpc_url(), CommitmentConfig::confirmed()),
        payer,
        home: get_program_id("memo_home")?,
        mint: get_token_mint("memo_token").map_err(|e| e.to_string())?,
    };
    let memo_mint = get_program_id("memo_mint")?;
    let memo_burn = get_program_id("memo_burn")?;
    let w = cx.payer.pubkey();
    let t22 = token_2022_id();
    println!("RPC {}  wallet {}  memo-home {}", get_rpc_url(), w, cx.home);

    let program_authority = cx.pda(&[b"authority"]);
    let burn_guard = cx.pda(&[b"burn_guard"]);
    match cx.client.get_account(&burn_guard) {
        Ok(a) if a.owner == cx.home && a.data.len() == 14 && a.data[8..13] == [1, 2, 2, 1, 2] => println!("  ✅ Burn Guard {} present", burn_guard),
        _ => return Err(format!("Burn Guard {} missing or wrong: run admin-init-burn-guard", burn_guard)),
    }

    // Collection with the wallet as Authority (a fresh seed per run)
    let stamp = Utc::now().format("%Y%m%d%H%M%S%3f").to_string();
    let seed: [u8; 32] = Sha256::digest(format!("smoke-test-memo-home:{}", stamp).as_bytes()).into();
    let col = cx.pda(&[b"collection", w.as_ref(), &seed]);
    let config = cx.pda(&[b"collection_config", col.as_ref()]);
    let mut a = seed.to_vec();
    bstr(&mut a, &format!("Smoke {}", stamp));
    bstr(&mut a, "https://example.com/smoke-collection.json");
    a.extend(SHARE_BPS.to_le_bytes());
    a.extend([0, 0]); // max_assets None, royalty None
    cx.send("create_collection", vec![Instruction::new_with_bytes(cx.home, &[disc("create_collection"), a].concat(), vec![
        AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new(col, false),
        AccountMeta::new_readonly(program_authority, false), AccountMeta::new_readonly(burn_guard, false),
        AccountMeta::new(config, false), AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program::id(), false),
    ])])?;

    let aseed: [u8; 32] = Sha256::digest(b"room #1").into();
    let asset = cx.pda(&[b"asset", col.as_ref(), &aseed]);
    let vault = cx.pda(&[b"vault", asset.as_ref()]);
    let vault_state = cx.pda(&[b"vault_state", asset.as_ref()]);
    let mut a = aseed.to_vec();
    bstr(&mut a, "Smoke Room #1");
    bstr(&mut a, "https://example.com/smoke-asset.json");
    cx.send("create_asset", vec![Instruction::new_with_bytes(cx.home, &[disc("create_asset"), a].concat(), vec![
        AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(config, false),
        AccountMeta::new(col, false), AccountMeta::new(asset, false), AccountMeta::new(vault_state, false),
        AccountMeta::new_readonly(w, false), AccountMeta::new_readonly(program_authority, false),
        AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program::id(), false),
    ])])?;
    println!("     Collection {}  Asset {}  Vault {}", col, asset, vault);
    if cx.nonce(&asset) != Some(0) {
        return Err("Vault State missing or nonce not 0".into());
    }

    // Core BurnV1 by the owner, with the Burn Guard account: rejected by the Oracle
    cx.must_fail("Core burn of the Asset", vec![Instruction { program_id: CORE, data: vec![12, 0], accounts: vec![
        AccountMeta::new(asset, false), AccountMeta::new(col, false), AccountMeta::new(w, true),
        AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(burn_guard, false),
    ]}])?;

    // mine: the wallet signs as Authority and as the memo-mint caller
    let w_ta = ata_of(&w, &cx.mint, &t22);
    let v_ta = ata_of(&vault, &cx.mint, &t22);
    cx.send("create the wallet's MEMO token account (idempotent)", vec![create_associated_token_account_idempotent(&w, &w, &cx.mint, &t22)])?;
    let (w0, supply) = (cx.balance(&w_ta), cx.supply()?);
    let memo = spl_memo::build_memo(format!("memo-home smoke test {}: mine into the Vault with a 30% share", stamp).as_bytes(), &[]);
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &memo_mint);
    let (mint_state, _) = Pubkey::find_program_address(&[b"mint_state"], &memo_mint);
    cx.send("mine (30% share)", vec![memo, Instruction::new_with_bytes(cx.home, &[disc("mine"), SHARE_BPS.to_le_bytes().to_vec()].concat(), vec![
        AccountMeta::new(w, true), AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(config, false),
        AccountMeta::new_readonly(asset, false), AccountMeta::new_readonly(vault, false), AccountMeta::new(v_ta, false),
        AccountMeta::new(w_ta, false), AccountMeta::new(cx.mint, false), AccountMeta::new_readonly(mint_authority, false),
        AccountMeta::new(mint_state, false), AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        AccountMeta::new_readonly(t22, false), AccountMeta::new_readonly(spl_associated_token_account::id(), false),
        AccountMeta::new_readonly(memo_mint, false), AccountMeta::new_readonly(system_program::id(), false),
    ])])?;
    let reward = expected_mint_amount(supply);
    let share = (reward as u128 * SHARE_BPS as u128 / 10_000) as u64;
    let (vault_bal, w_got) = (cx.balance(&v_ta), cx.balance(&w_ta).saturating_sub(w0));
    println!("     reward {} (expected at this supply), Vault {} , share to the wallet {}", reward, vault_bal, w_got);
    if vault_bal != reward - share || w_got != share {
        return Err(format!("mine split: Vault {} (expected {}), share {} (expected {})", vault_bal, reward - share, w_got, share));
    }

    let withdraw = |amount: u64| Instruction::new_with_bytes(cx.home, &[disc("withdraw"), amount.to_le_bytes().to_vec()].concat(), vec![
        AccountMeta::new_readonly(w, true), AccountMeta::new_readonly(config, false), AccountMeta::new_readonly(asset, false),
        AccountMeta::new_readonly(vault, false), AccountMeta::new(vault_state, false), AccountMeta::new(v_ta, false),
        AccountMeta::new(w_ta, false), AccountMeta::new_readonly(cx.mint, false), AccountMeta::new_readonly(t22, false),
    ]);
    cx.send("withdraw 1 unit", vec![withdraw(1)])?;
    if cx.nonce(&asset) != Some(1) {
        return Err(format!("nonce after withdraw: {:?}, expected 1", cx.nonce(&asset)));
    }

    // Vault burn of one mint's worth (+1%), topped up from the wallet when the Vault lacks it
    let min = fetch_min_burn_amount(&cx.client, &cx.mint, 1).map_err(|e| e.to_string())?;
    let amount = min + min / 100;
    let have = cx.balance(&v_ta);
    if have < amount {
        // one unit more than needed, so the final withdraw-all has something to move
        let top_up = amount - have + 1;
        if cx.balance(&w_ta) < top_up {
            return Err(format!("the wallet needs {} MEMO units for the Vault burn", top_up));
        }
        cx.send("fund the Vault for the burn", vec![spl_token_2022::instruction::transfer_checked(
            &t22, &w_ta, &cx.mint, &v_ta, &w, &[], top_up, 6).map_err(|e| e.to_string())?])?;
    }
    let burn_memo = BurnMemo { version: 1, burn_amount: amount, payload: format!("memo-home smoke test {}: Vault burn", stamp).into_bytes() };
    let memo = spl_memo::build_memo(general_purpose::STANDARD.encode(borsh::to_vec(&burn_memo).map_err(|e| e.to_string())?).as_bytes(), &[]);
    let vault_stats = Pubkey::find_program_address(&[b"user_global_burn_stats", vault.as_ref()], &memo_burn).0;
    let global_stats = Pubkey::find_program_address(&[b"global_burn_stats"], &memo_burn).0;
    let before = cx.balance(&v_ta);
    cx.send("Vault burn (first use: Vault burn stats)", vec![memo, Instruction::new_with_bytes(cx.home, &[disc("burn"), amount.to_le_bytes().to_vec()].concat(), vec![
        AccountMeta::new_readonly(w, true), AccountMeta::new(w, true), AccountMeta::new_readonly(config, false),
        AccountMeta::new_readonly(asset, false), AccountMeta::new(vault, false), AccountMeta::new(vault_state, false),
        AccountMeta::new(v_ta, false), AccountMeta::new(vault_stats, false), AccountMeta::new(global_stats, false),
        AccountMeta::new(cx.mint, false), AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        AccountMeta::new_readonly(t22, false), AccountMeta::new_readonly(memo_burn, false), AccountMeta::new_readonly(system_program::id(), false),
    ])])?;
    if before - cx.balance(&v_ta) != amount || cx.nonce(&asset) != Some(2) {
        return Err(format!("burn: Vault {} -> {}, nonce {:?}", before, cx.balance(&v_ta), cx.nonce(&asset)));
    }

    cx.send("withdraw everything (u64::MAX)", vec![withdraw(u64::MAX)])?;
    if cx.balance(&v_ta) != 0 || cx.nonce(&asset) != Some(3) {
        return Err(format!("withdraw all: Vault {}, nonce {:?}", cx.balance(&v_ta), cx.nonce(&asset)));
    }
    println!("     Vault emptied, nonce 3");
    Ok(())
}
