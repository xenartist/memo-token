// memo-id smoke test: the main path on a live network, with the configured wallet as the payer,
// the ID's owner and the SubId's owner.
//
//   Collection and frozen lookup table present → MEMO for two burns (mints the shortfall) →
//   register <label>.memo (v0 + lookup table) → set_primary → create_subid www → issue_subid
//   (v0 + lookup table) → check the Assets, the Registries and the Primary ID
//
// Each run registers a new ID: two burns of 69 mints each, plus about 0.02 XNT in rent and fees.
// Every transaction is simulated first and sent with the simulated compute units plus a margin.
// Run admin-init-memo-id and admin-init-memo-id-alt (the deploy script does) before this.

use base64::{engine::general_purpose, Engine as _};
use sha2::{Digest, Sha256};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig, rpc_request::RpcRequest};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::state::AddressLookupTable;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    sysvar,
    transaction::{Transaction, VersionedTransaction},
};
use solana_system_interface::program as system_program;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id as ata_of, instruction::create_associated_token_account_idempotent,
};
use spl_token_2022::{extension::StateWithExtensions, id as token_2022_id, state::Account as TokenAccount};
use memo_token_client::{
    expected_mint_amount, get_lookup_table, get_program_id, get_rpc_url, get_token_mint, get_wallet_path,
    global_burn_stats_pda, memo_id::{MemoIdAddresses, CORE_PROGRAM_ID, SUFFIX}, mint_state_pda,
};

/// Burn prices in hundredths of a mint (memo-id PRICE_CENTI_MINTS_5_PLUS / _ISSUE_SUBID)
const PRICE_CENTI_REGISTER_5_PLUS: u64 = 6_900;
const PRICE_CENTI_ISSUE_SUBID: u64 = 6_900;
const URI: &str = "https://example.com/memo-id-smoke.json";
const SUBID_LABEL: &str = "www";
const MAX_TX_BYTES: usize = 1232;

fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }
fn disc(name: &str) -> Vec<u8> { Sha256::digest(format!("global:{}", name).as_bytes())[..8].to_vec() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}
fn pk(d: &[u8], at: usize) -> Pubkey { Pubkey::new_from_array(d[at..at + 32].try_into().unwrap()) }
fn u32_at(d: &[u8], at: usize) -> u32 { u32::from_le_bytes(d[at..at + 4].try_into().unwrap()) }

/// Base64(Borsh(BurnMemo { version: 1, burn_amount, payload })) with the memo-id payload
fn burn_memo(amount: u64, operation: &str, user: &Pubkey, memo_id: &str) -> Instruction {
    let mut payload = vec![1u8];
    bstr(&mut payload, "memo_id");
    bstr(&mut payload, operation);
    bstr(&mut payload, &user.to_string());
    bstr(&mut payload, memo_id);
    let mut raw = vec![1u8];
    raw.extend(amount.to_le_bytes());
    raw.extend((payload.len() as u32).to_le_bytes());
    raw.extend(payload);
    spl_memo::build_memo(general_purpose::STANDARD.encode(raw).as_bytes(), &[user])
}

/// What one sent transaction cost
struct Sent {
    units: u64,
    bytes: usize,
    fee: u64,
}

struct Ctx {
    client: RpcClient,
    payer: Keypair,
    a: MemoIdAddresses,
    mint: Pubkey,
    memo_burn: Pubkey,
    alt: AddressLookupTableAccount,
    mint_state: Pubkey,
    gbs: Pubkey,
}

impl Ctx {
    fn w(&self) -> Pubkey { self.payer.pubkey() }
    fn registry(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"memo_id", &sha(memo_id.as_bytes())], &self.a.program).0
    }
    fn asset(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"asset", self.a.collection.as_ref(), &sha(memo_id.as_bytes())], &self.a.home).0
    }
    fn primary(&self, target: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"primary_id", target.as_ref()], &self.a.program).0
    }
    fn subid_index(&self, parent: &Pubkey, i: u32) -> Pubkey {
        Pubkey::find_program_address(&[b"subid", parent.as_ref(), &i.to_le_bytes()], &self.a.program).0
    }
    fn stats(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"user_global_burn_stats", self.w().as_ref()], &self.memo_burn).0
    }
    fn ata(&self) -> Pubkey { ata_of(&self.w(), &self.mint, &token_2022_id()) }
    fn ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.a.program, &d, accounts)
    }
    fn data(&self, k: &Pubkey) -> Option<Vec<u8>> {
        self.client.get_account_with_commitment(k, CommitmentConfig::confirmed()).ok()?.value.map(|a| a.data)
    }
    fn balance(&self) -> u64 {
        self.data(&self.ata()).and_then(|d| StateWithExtensions::<TokenAccount>::unpack(&d).ok().map(|s| s.base.amount)).unwrap_or(0)
    }
    fn supply(&self) -> Result<u64, String> {
        self.client.get_token_supply_with_commitment(&self.mint, CommitmentConfig::confirmed()).map_err(|e| e.to_string())?
            .value.amount.parse().map_err(|e: std::num::ParseIntError| e.to_string())
    }
    /// memo-id burn_price: ceil(reward at the current supply × centi / 100)
    fn price(&self, centi: u64) -> Result<u64, String> {
        Ok((expected_mint_amount(self.supply()?) as u128 * centi as u128).div_ceil(100) as u64)
    }

    // ---------------------------------------------------------------- instructions
    fn home_accounts(&self, memo_id: &str) -> Vec<AccountMeta> {
        let asset = self.asset(memo_id);
        let vault_state = Pubkey::find_program_address(&[b"vault_state", asset.as_ref()], &self.a.home).0;
        vec![
            AccountMeta::new_readonly(self.a.authority, false),
            AccountMeta::new_readonly(self.a.config, false),
            AccountMeta::new(self.a.collection, false),
            AccountMeta::new(asset, false),
            AccountMeta::new(vault_state, false),
            AccountMeta::new_readonly(self.a.home_authority, false),
            AccountMeta::new_readonly(CORE_PROGRAM_ID, false),
            AccountMeta::new_readonly(self.a.home, false),
        ]
    }
    fn burn_accounts(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.mint, false),
            AccountMeta::new(self.ata(), false),
            AccountMeta::new(self.stats(), false),
            AccountMeta::new(self.gbs, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(self.memo_burn, false),
        ]
    }
    fn register(&self, label: &str) -> Instruction {
        let memo_id = format!("{}.{}", label, SUFFIX);
        let mut a = sha(memo_id.as_bytes()).to_vec();
        bstr(&mut a, label);
        bstr(&mut a, URI);
        let mut accounts = vec![AccountMeta::new(self.w(), true), AccountMeta::new(self.registry(&memo_id), false)];
        accounts.extend(self.home_accounts(&memo_id));
        accounts.extend(self.burn_accounts());
        accounts.push(AccountMeta::new_readonly(sysvar::instructions::ID, false));
        accounts.push(AccountMeta::new_readonly(system_program::id(), false));
        self.ix("register", a, accounts)
    }
    fn set_primary_address(&self, memo_id: &str) -> Instruction {
        let mut a = vec![0u8];
        a.extend(self.w().as_ref());
        self.ix("set_primary", a, vec![
            AccountMeta::new(self.w(), true),
            AccountMeta::new_readonly(self.w(), true),
            AccountMeta::new_readonly(self.registry(memo_id), false),
            AccountMeta::new(self.primary(&self.w()), false),
            AccountMeta::new_readonly(self.a.program, false),
            AccountMeta::new_readonly(self.a.program, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ])
    }
    fn create_subid(&self, parent_id: &str, parent_asset: &Pubkey, label: &str) -> Instruction {
        let memo_id = format!("{}.{}", label, parent_id);
        let mut a = sha(memo_id.as_bytes()).to_vec();
        bstr(&mut a, label);
        a.push(0); // no target
        self.ix("create_subid", a, vec![
            AccountMeta::new(self.w(), true),
            AccountMeta::new_readonly(self.w(), true),
            AccountMeta::new(self.registry(parent_id), false),
            AccountMeta::new_readonly(*parent_asset, false),
            AccountMeta::new_readonly(self.a.program, false),
            AccountMeta::new(self.registry(&memo_id), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ])
    }
    fn issue_subid(&self, parent_id: &str, parent_asset: &Pubkey, memo_id: &str, index: u32) -> Instruction {
        let parent = self.registry(parent_id);
        let mut a = index.to_le_bytes().to_vec();
        bstr(&mut a, URI);
        let mut accounts = vec![
            AccountMeta::new(self.w(), true),
            AccountMeta::new_readonly(self.w(), true),
            AccountMeta::new(parent, false),
            AccountMeta::new_readonly(*parent_asset, false),
            AccountMeta::new_readonly(self.a.program, false),
            AccountMeta::new(self.registry(memo_id), false),
            AccountMeta::new(self.subid_index(&parent, index), false),
            AccountMeta::new_readonly(self.w(), false),
        ];
        accounts.extend(self.home_accounts(memo_id));
        accounts.extend(self.burn_accounts());
        accounts.push(AccountMeta::new_readonly(sysvar::instructions::ID, false));
        accounts.push(AccountMeta::new_readonly(system_program::id(), false));
        self.ix("issue_subid", a, accounts)
    }
    fn process_mint(&self, memo_mint: &Pubkey) -> Instruction {
        let mint_authority = Pubkey::find_program_address(&[b"mint_authority"], memo_mint).0;
        Instruction::new_with_bytes(*memo_mint, &disc("process_mint"), vec![
            AccountMeta::new(self.w(), true),
            AccountMeta::new(self.mint, false),
            AccountMeta::new_readonly(mint_authority, false),
            AccountMeta::new(self.ata(), false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(sysvar::instructions::ID, false),
            AccountMeta::new(self.mint_state, false),
        ])
    }

    // ---------------------------------------------------------------- sending
    /// Legacy or v0 (with the lookup table) message with a compute unit limit appended last
    fn build(&self, ixs: &[Instruction], limit: u32, v0_alt: bool) -> Result<VersionedTransaction, String> {
        let mut all = ixs.to_vec();
        all.push(ComputeBudgetInstruction::set_compute_unit_limit(limit));
        let bh = self.client.get_latest_blockhash().map_err(|e| e.to_string())?;
        if v0_alt {
            let msg = v0::Message::try_compile(&self.w(), &all, std::slice::from_ref(&self.alt), bh).map_err(|e| e.to_string())?;
            VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&self.payer]).map_err(|e| e.to_string())
        } else {
            Ok(Transaction::new_signed_with_payer(&all, Some(&self.w()), &[&self.payer], bh).into())
        }
    }
    /// Simulate, then send with the simulated units + 10% + 50 (the memo, if any, stays at index 0)
    fn send(&self, label: &str, ixs: Vec<Instruction>, v0_alt: bool) -> Result<Sent, String> {
        let sim = self.client.simulate_transaction_with_config(
            &self.build(&ixs, 1_400_000, v0_alt)?,
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
        let tx = self.build(&ixs, limit, v0_alt)?;
        let bytes = bincode::serialize(&tx).map_err(|e| e.to_string())?.len();
        let sig = self.client.send_and_confirm_transaction(&tx).map_err(|e| format!("{}: {}", label, e))?;
        let fee = self.fee(&sig);
        println!("  ✅ {}: {} CU (limit {}), {} bytes{}, fee {} XNT, {}", label, units, limit, bytes,
                 if v0_alt { " v0" } else { "" }, fee as f64 / 1e9, sig);
        Ok(Sent { units, bytes, fee })
    }
    fn fee(&self, sig: &Signature) -> u64 {
        let params = serde_json::json!([sig.to_string(), { "encoding": "base64", "commitment": "confirmed", "maxSupportedTransactionVersion": 0 }]);
        self.client.send::<serde_json::Value>(RpcRequest::GetTransaction, params).ok()
            .and_then(|t| t["meta"]["fee"].as_u64()).unwrap_or(0)
    }
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    MEMO-ID SMOKE TEST                         ║");
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
    let wallet = get_wallet_path();
    let payer = read_keypair_file(&wallet).map_err(|e| format!("cannot read wallet {}: {}", wallet, e))?;
    let client = RpcClient::new_with_commitment(get_rpc_url(), CommitmentConfig::confirmed());
    let a = MemoIdAddresses::from_config()?;
    let memo_mint = get_program_id("memo_mint")?;
    let memo_burn = get_program_id("memo_burn")?;
    let mint = get_token_mint("memo_token").map_err(|e| e.to_string())?;

    println!("\n[1] Deployment");
    println!("  memo-id: {}", a.program);
    println!("  wallet:  {}", payer.pubkey());
    if client.get_account(&a.collection).is_err() {
        return Err(format!("MEMO ID Collection {} missing: run admin-init-memo-id", a.collection));
    }
    println!("  ✅ MEMO ID Collection {}", a.collection);
    let table = get_lookup_table("memo_id")?.ok_or("no [lookup_tables.<env>] memo_id in Anchor.toml: run admin-init-memo-id-alt")?;
    let account = client.get_account(&table).map_err(|e| format!("lookup table {}: {}", table, e))?;
    #[allow(deprecated)]
    let state = AddressLookupTable::deserialize(&account.data).map_err(|e| e.to_string())?;
    let expected = a.lookup_table_addresses_from_config()?;
    if state.meta.authority.is_some() || state.addresses.as_ref() != expected.as_slice() {
        return Err(format!("lookup table {} is not frozen or does not hold the expected addresses", table));
    }
    println!("  ✅ Lookup table {}: frozen, {} addresses", table, expected.len());
    let cx = Ctx { client, payer, a, mint, memo_burn, alt: AddressLookupTableAccount { key: table, addresses: expected },
                  mint_state: mint_state_pda(), gbs: global_burn_stats_pda() };
    let w = cx.w();

    println!("\n[2] MEMO for two burns");
    if cx.data(&cx.stats()).is_none() {
        let init = Instruction::new_with_bytes(memo_burn, &disc("initialize_user_global_burn_stats"), vec![
            AccountMeta::new(w, true),
            AccountMeta::new(cx.stats(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ]);
        cx.send("initialize UserGlobalBurnStats", vec![init], false)?;
    }
    // Minting raises the supply and may lower the reward, so check the price again after each mint
    let mut mints = 0;
    loop {
        let need = cx.price(PRICE_CENTI_REGISTER_5_PLUS)? + cx.price(PRICE_CENTI_ISSUE_SUBID)?;
        let have = cx.balance();
        if have >= need {
            println!("  ✅ balance {} units, two burns need {} ({} mints sent)", have, need, mints);
            break;
        }
        let memo = format!("memo-id smoke test: MEMO for the registration burn, mint {} at {}", mints + 1, chrono::Utc::now().timestamp());
        let mut ixs = vec![spl_memo::build_memo(memo.as_bytes(), &[&w])];
        if mints == 0 {
            ixs.push(create_associated_token_account_idempotent(&w, &w, &mint, &token_2022_id()));
        }
        ixs.push(cx.process_mint(&memo_mint));
        let label = format!("mint {} ({} of {} units)", mints + 1, have, need);
        if mints % 20 == 0 {
            cx.send(&label, ixs, false)?;
        } else {
            // quiet: only failures print
            let tx = cx.build(&ixs, 60_000, false)?;
            cx.client.send_and_confirm_transaction(&tx).map_err(|e| format!("{}: {}", label, e))?;
        }
        mints += 1;
    }

    println!("\n[3] register (v0 + lookup table)");
    let label = format!("smoke{}", chrono::Utc::now().format("%y%m%d%H%M%S"));
    let memo_id = format!("{}.{}", label, SUFFIX);
    let asset = cx.asset(&memo_id);
    let before = cx.balance();
    let price = cx.price(PRICE_CENTI_REGISTER_5_PLUS)?;
    let legacy = bincode::serialize(&cx.build(&[burn_memo(price, "register", &w, &memo_id), cx.register(&label)], 200_000, false)?)
        .map_err(|e| e.to_string())?.len();
    let reg = cx.send(&format!("register {}", memo_id), vec![burn_memo(price, "register", &w, &memo_id), cx.register(&label)], true)?;
    let d = cx.data(&cx.registry(&memo_id)).ok_or("Registry missing")?;
    let asset_data = cx.data(&asset).ok_or("Asset missing")?;
    if pk(&d, 9) != asset || pk(&asset_data, 1) != w || cx.balance() != before - price {
        return Err(format!("register: unexpected state (asset {}, owner {}, burned {})", pk(&d, 9), pk(&asset_data, 1), before - cx.balance()));
    }
    println!("  ✅ {} → Asset {} owned by the wallet, burned {} units (legacy would be {} bytes)", memo_id, asset, price, legacy);

    println!("\n[4] set_primary (wallet address → {})", memo_id);
    cx.send("set_primary", vec![cx.set_primary_address(&memo_id)], false)?;
    let p = cx.data(&cx.primary(&w)).ok_or("Primary ID missing")?;
    if p[8] != 0 || pk(&p, 9) != w || pk(&p, 41) != cx.registry(&memo_id) {
        return Err("Primary ID does not point to the registered ID".into());
    }
    println!("  ✅ Primary ID of {} = {}", w, memo_id);

    println!("\n[5] NFT SubId {}.{}", SUBID_LABEL, memo_id);
    let sub = format!("{}.{}", SUBID_LABEL, memo_id);
    cx.send("create_subid (Record)", vec![cx.create_subid(&memo_id, &asset, SUBID_LABEL)], false)?;
    let index = u32_at(&cx.data(&cx.registry(&memo_id)).ok_or("parent Registry missing")?, 81);
    let price = cx.price(PRICE_CENTI_ISSUE_SUBID)?;
    let ixs = vec![burn_memo(price, "issue_subid", &w, &sub), cx.issue_subid(&memo_id, &asset, &sub, index)];
    let legacy = bincode::serialize(&cx.build(&ixs, 200_000, false)?).map_err(|e| e.to_string())?.len();
    let iss = cx.send("issue_subid", ixs, true)?;
    let sub_asset = cx.data(&cx.asset(&sub)).ok_or("SubId Asset missing")?;
    let parent = cx.data(&cx.registry(&memo_id)).ok_or("parent Registry missing")?;
    if pk(&sub_asset, 1) != w || u32_at(&parent, 81) != index + 1 {
        return Err("issue_subid: unexpected state".into());
    }
    println!("  ✅ {} → Asset {}, nft_subids_issued {} (legacy would be {} bytes, limit {})",
             sub, cx.asset(&sub), index + 1, legacy, MAX_TX_BYTES);

    println!("\n[6] Summary");
    println!("  register:    {} bytes, {} CU, fee {} XNT", reg.bytes, reg.units, reg.fee as f64 / 1e9);
    println!("  issue_subid: {} bytes, {} CU, fee {} XNT", iss.bytes, iss.units, iss.fee as f64 / 1e9);
    println!("  Explorer: https://explorer.testnet.x1.xyz/address/{}", asset);
    Ok(())
}
