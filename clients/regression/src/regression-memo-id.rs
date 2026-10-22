//! On-chain regression suite for memo-id (local validator only).
//!
//! Runs against X1 mainnet's builds of Metaplex Core and Token-2022 (dumped by
//! scripts/local-validator.sh), memo-mint, memo-burn and memo-home. Covers init (the MEMO ID
//! Collection), register (label rules, the 64-byte limit, prices by length against an
//! independent curve reference, the BurnMemo and its payload, URIs), Record SubIds, NFT SubIds
//! with the SubId Index and counters, forward records, Primary IDs, the one-level Vault rule,
//! lapsing on transfer, update_uri and signer checks. Every case asserts the exact outcome.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-id

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use harness::*;
use memo_token_client::get_program_id;
use sha2::{Digest, Sha256};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::instruction as alt_ix;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_client::rpc_config::RpcSendTransactionConfig;

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");

// memo-id error codes
const INVALID_LABEL: u32 = 6000;
const MEMO_ID_TOO_LONG: u32 = 6001;
const HASH_MISMATCH: u32 = 6002;
const INVALID_URI: u32 = 6003;
const URI_TOO_LONG: u32 = 6004;
const WRONG_COLLECTION: u32 = 6005;
const INVALID_MINT: u32 = 6006;
const WRONG_ASSET: u32 = 6007;
const NOT_CONTROLLER: u32 = 6008;
const NOT_IN_HOLDER_VAULT: u32 = 6009;
const PARENT_NOT_NFT: u32 = 6010;
const NOT_NFT: u32 = 6011;
const NOT_RECORD: u32 = 6012;
const WRONG_PARENT: u32 = 6013;
const WRONG_SUBID_INDEX: u32 = 6014;
const MEMO_REQUIRED: u32 = 6015;
const BURN_AMOUNT_MISMATCH: u32 = 6019;
const INVALID_CATEGORY: u32 = 6021;
const INVALID_OPERATION: u32 = 6022;
const USER_PUBKEY_MISMATCH: u32 = 6023;
const MEMO_ID_MISMATCH: u32 = 6024;
// memo-home
const HOME_INVALID_OWNER: u32 = 6020;

const URI: &str = "https://example.com/memo-id.json";
const MAX_URI: usize = 256;
/// Index of the memo-id instruction in [memo, compute budget, ix]
const IX: u64 = 2;

fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Address(Pubkey),
    Asset(Pubkey),
}
impl Target {
    fn key(&self) -> Pubkey { match self { Target::Address(k) | Target::Asset(k) => *k } }
    fn encode(&self, v: &mut Vec<u8>) {
        match self {
            Target::Address(_) => v.push(0),
            Target::Asset(_) => v.push(1),
        }
        v.extend(self.key().as_ref());
    }
}

/// Registry as read from chain
#[derive(Debug, PartialEq)]
struct Reg {
    kind: u8,
    asset: Pubkey,
    parent: Pubkey,
    nft_subids_issued: u32,
    record_subids: u32,
    target_set_by: Pubkey,
    target: Option<Target>,
    memo_id: String,
}

fn pk(d: &[u8], at: usize) -> Pubkey { Pubkey::new_from_array(d[at..at + 32].try_into().unwrap()) }
fn u32_at(d: &[u8], at: usize) -> u32 { u32::from_le_bytes(d[at..at + 4].try_into().unwrap()) }

fn parse_reg(d: &[u8]) -> Reg {
    let mut p = 122;
    let target = match d[p] {
        0 => {
            p += 1;
            None
        }
        _ => {
            let k = pk(d, p + 2);
            let t = if d[p + 1] == 0 { Target::Address(k) } else { Target::Asset(k) };
            p += 34;
            Some(t)
        }
    };
    let len = u32_at(d, p) as usize;
    Reg {
        kind: d[8],
        asset: pk(d, 9),
        parent: pk(d, 41),
        nft_subids_issued: u32_at(d, 81),
        record_subids: u32_at(d, 85),
        target_set_by: pk(d, 90),
        target,
        memo_id: String::from_utf8(d[p + 4..p + 4 + len].to_vec()).unwrap(),
    }
}

/// Core AssetV1: (owner, collection, name, uri)
fn parse_asset(d: &[u8]) -> (Pubkey, Option<Pubkey>, String, String) {
    let owner = pk(d, 1);
    let (col, mut p) = match d[33] {
        2 => (Some(pk(d, 34)), 66),
        1 => (None, 66),
        _ => (None, 34),
    };
    let mut s = || {
        let len = u32_at(d, p) as usize;
        let v = String::from_utf8(d[p + 4..p + 4 + len].to_vec()).unwrap();
        p += 4 + len;
        v
    };
    let name = s();
    let uri = s();
    (owner, col, name, uri)
}

struct Id<'a> {
    e: &'a Env,
    prog: Pubkey,
    home: Pubkey,
    authority: Pubkey,
    home_authority: Pubkey,
    burn_guard: Pubkey,
    collection: Pubkey,
    config: Pubkey,
}

impl<'a> Id<'a> {
    fn new(e: &'a Env) -> Id<'a> {
        let prog = get_program_id("memo_id").expect("memo_id in Anchor.toml");
        let home = get_program_id("memo_home").expect("memo_home in Anchor.toml");
        let authority = Pubkey::find_program_address(&[b"authority"], &prog).0;
        let collection = Pubkey::find_program_address(&[b"collection", authority.as_ref(), &sha(b"memo")], &home).0;
        Id {
            e,
            prog,
            home,
            authority,
            home_authority: Pubkey::find_program_address(&[b"authority"], &home).0,
            burn_guard: Pubkey::find_program_address(&[b"burn_guard"], &home).0,
            collection,
            config: Pubkey::find_program_address(&[b"collection_config", collection.as_ref()], &home).0,
        }
    }
    fn registry(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"memo_id", &sha(memo_id.as_bytes())], &self.prog).0
    }
    fn asset(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"asset", self.collection.as_ref(), &sha(memo_id.as_bytes())], &self.home).0
    }
    fn vault(&self, asset: &Pubkey) -> Pubkey { Pubkey::find_program_address(&[b"vault", asset.as_ref()], &self.home).0 }
    fn vault_state(&self, asset: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"vault_state", asset.as_ref()], &self.home).0
    }
    fn subid_index(&self, parent: &Pubkey, i: u32) -> Pubkey {
        Pubkey::find_program_address(&[b"subid", parent.as_ref(), &i.to_le_bytes()], &self.prog).0
    }
    fn primary(&self, target: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"primary_id", target.as_ref()], &self.prog).0
    }
    fn reg(&self, memo_id: &str) -> Option<Reg> { self.e.data(&self.registry(memo_id)).map(|d| parse_reg(&d)) }
    fn asset_data(&self, asset: &Pubkey) -> Option<(Pubkey, Option<Pubkey>, String, String)> {
        self.e.data(asset).map(|d| parse_asset(&d))
    }
    /// Price in units, from the independent curve reference: ceil(reward × centi / 100)
    fn price(&self, centi: u64) -> u64 {
        let reward = curve_reference(self.e.supply()).expect("reward") as u128;
        (reward * centi as u128).div_ceil(100) as u64
    }
    fn opt(&self, k: Option<&Pubkey>) -> AccountMeta {
        AccountMeta::new_readonly(k.copied().unwrap_or(self.prog), false)
    }
    fn ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.prog, &d, accounts)
    }

    fn init_burn_guard(&self, payer: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(self.home, &disc("init_burn_guard"), vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(self.burn_guard, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn init_with(&self, payer: &Pubkey, collection: &Pubkey) -> Instruction {
        self.ix("init", vec![], vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(self.burn_guard, false),
            AccountMeta::new(Pubkey::find_program_address(&[b"collection_config", collection.as_ref()], &self.home).0, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn home_accounts(&self, memo_id: &str) -> Vec<AccountMeta> {
        let asset = self.asset(memo_id);
        vec![
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.collection, false),
            AccountMeta::new(asset, false),
            AccountMeta::new(self.vault_state(&asset), false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
        ]
    }
    fn burn_accounts(&self, payer: &Pubkey, mint: &Pubkey, token_account: &Pubkey) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(self.e.stats_pda(payer), false),
            AccountMeta::new(self.e.gbs, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(self.e.burn_prog, false),
        ]
    }

    /// register with explicit hash, label and accounts
    fn register_raw(&self, payer: &Pubkey, hash: [u8; 32], label: &str, uri: &str, memo_id_for_accounts: &str,
                    mint: &Pubkey, token_account: &Pubkey) -> Instruction {
        let mut a = hash.to_vec();
        bstr(&mut a, label);
        bstr(&mut a, uri);
        let mut accounts = vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(self.registry(memo_id_for_accounts), false),
        ];
        accounts.extend(self.home_accounts(memo_id_for_accounts));
        accounts.extend(self.burn_accounts(payer, mint, token_account));
        accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
        accounts.push(AccountMeta::new_readonly(system_program(), false));
        self.ix("register", a, accounts)
    }
    fn register(&self, payer: &Pubkey, label: &str, uri: &str) -> Instruction {
        let memo_id = format!("{}.memo", label);
        self.register_raw(payer, sha(memo_id.as_bytes()), label, uri, &memo_id, &self.e.mint, &self.e.ata(payer))
    }

    fn payload(operation: &str, payer: &Pubkey, memo_id: &str) -> Vec<u8> {
        Self::payload_with(1, "memo_id", operation, &payer.to_string(), memo_id)
    }
    fn payload_with(version: u8, category: &str, operation: &str, user: &str, memo_id: &str) -> Vec<u8> {
        let mut p = vec![version];
        bstr(&mut p, category);
        bstr(&mut p, operation);
        bstr(&mut p, user);
        bstr(&mut p, memo_id);
        p
    }
    /// [BurnMemo, compute budget, ix]
    fn tx(&self, amount: u64, payload: &[u8], ix: Instruction) -> Vec<Instruction> {
        vec![app_burn_memo(amount, payload), cu(1_000_000), ix]
    }

    fn create_subid(&self, payer: &Pubkey, controller: &Pubkey, parent_id: &str, parent_asset: &Pubkey, holder: Option<&Pubkey>,
                    label: &str, target: Option<Target>) -> Instruction {
        let memo_id = format!("{}.{}", label, parent_id);
        let mut a = sha(memo_id.as_bytes()).to_vec();
        bstr(&mut a, label);
        match target {
            Some(t) => {
                a.push(1);
                t.encode(&mut a);
            }
            None => a.push(0),
        }
        self.ix("create_subid", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(self.registry(parent_id), false),
            AccountMeta::new_readonly(*parent_asset, false),
            self.opt(holder),
            AccountMeta::new(self.registry(&memo_id), false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn delete_subid(&self, controller: &Pubkey, parent_id: &str, parent_asset: &Pubkey, holder: Option<&Pubkey>, memo_id: &str) -> Instruction {
        self.ix("delete_subid", vec![], vec![
            AccountMeta::new(*controller, true),
            AccountMeta::new(self.registry(parent_id), false),
            AccountMeta::new_readonly(*parent_asset, false),
            self.opt(holder),
            AccountMeta::new(self.registry(memo_id), false),
        ])
    }
    #[allow(clippy::too_many_arguments)]
    fn issue_subid(&self, payer: &Pubkey, controller: &Pubkey, parent_id: &str, parent_asset: &Pubkey, holder: Option<&Pubkey>,
                   memo_id: &str, index: u32, new_owner: &Pubkey, uri: &str) -> Instruction {
        let parent = self.registry(parent_id);
        let mut a = index.to_le_bytes().to_vec();
        bstr(&mut a, uri);
        let mut accounts = vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(parent, false),
            AccountMeta::new_readonly(*parent_asset, false),
            self.opt(holder),
            AccountMeta::new(self.registry(memo_id), false),
            AccountMeta::new(self.subid_index(&parent, index), false),
            AccountMeta::new_readonly(*new_owner, false),
        ];
        accounts.extend(self.home_accounts(memo_id));
        accounts.extend(self.burn_accounts(payer, &self.e.mint, &self.e.ata(payer)));
        accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
        accounts.push(AccountMeta::new_readonly(system_program(), false));
        self.ix("issue_subid", a, accounts)
    }
    fn set_target(&self, controller: &Pubkey, memo_id: &str, parent_id: Option<&str>, asset: &Pubkey, holder: Option<&Pubkey>,
                  target: Option<Target>) -> Instruction {
        let accounts = vec![
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(self.registry(memo_id), false),
            self.opt(parent_id.map(|p| self.registry(p)).as_ref()),
            AccountMeta::new_readonly(*asset, false),
            self.opt(holder),
        ];
        match target {
            Some(t) => {
                let mut a = vec![];
                t.encode(&mut a);
                self.ix("set_target", a, accounts)
            }
            None => self.ix("clear_target", vec![], accounts),
        }
    }
    fn set_primary(&self, payer: &Pubkey, signer: &Pubkey, memo_id: &str, target: Target, target_asset: Option<&Pubkey>,
                   holder: Option<&Pubkey>) -> Instruction {
        let mut a = vec![];
        target.encode(&mut a);
        self.ix("set_primary", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*signer, true),
            AccountMeta::new_readonly(self.registry(memo_id), false),
            AccountMeta::new(self.primary(&target.key()), false),
            self.opt(target_asset),
            self.opt(holder),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn clear_primary(&self, signer: &Pubkey, target: &Pubkey, target_asset: Option<&Pubkey>, holder: Option<&Pubkey>) -> Instruction {
        self.ix("clear_primary", vec![], vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new(self.primary(target), false),
            self.opt(target_asset),
            self.opt(holder),
        ])
    }
    fn update_uri(&self, payer: &Pubkey, controller: &Pubkey, memo_id: &str, asset: &Pubkey, holder: Option<&Pubkey>, uri: &str) -> Instruction {
        let mut a = vec![];
        bstr(&mut a, uri);
        self.ix("update_uri", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new_readonly(self.registry(memo_id), false),
            AccountMeta::new(*asset, false),
            self.opt(holder),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(self.collection, false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    /// Primary ID: (target, registry, set_by)
    fn primary_data(&self, target: &Pubkey) -> Option<(Target, Pubkey, Pubkey)> {
        self.e.data(&self.primary(target)).map(|d| {
            let k = pk(&d, 9);
            let t = if d[8] == 0 { Target::Address(k) } else { Target::Asset(k) };
            (t, pk(&d, 41), pk(&d, 73))
        })
    }
}

impl Id<'_> {
    /// Accounts every register / issue_subid transaction needs, whoever sends it: the content of
    /// the frozen Address Lookup Table that clients use (programs invoked at the top level, such
    /// as memo-id and SPL Memo, must stay in the message itself)
    fn fixed_accounts(&self) -> Vec<Pubkey> {
        vec![self.authority, self.config, self.collection, self.home_authority, CORE, self.home, self.e.mint, self.e.gbs,
             t22(), self.e.burn_prog, IXS_SYSVAR, system_program()]
    }
}

/// Create the lookup table with the fixed accounts and freeze it (nobody can change it afterwards)
fn create_frozen_alt(e: &Env, accounts: Vec<Pubkey>) -> AddressLookupTableAccount {
    let w = &e.wallet;
    let slot = e.c.get_slot_with_commitment(CommitmentConfig::finalized()).expect("slot");
    #[allow(deprecated)]
    let (create, table) = alt_ix::create_lookup_table(w.pubkey(), w.pubkey(), slot);
    #[allow(deprecated)]
    let extend = alt_ix::extend_lookup_table(table, w.pubkey(), Some(w.pubkey()), accounts.clone());
    e.must("create and extend the lookup table", &[create, extend], w, &[]);
    #[allow(deprecated)]
    let freeze = alt_ix::freeze_lookup_table(table, w.pubkey());
    e.must("freeze the lookup table", &[freeze], w, &[]);
    // addresses become usable in the slot after the extension
    let after = e.c.get_slot().unwrap() + 2;
    while e.c.get_slot().unwrap() < after {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    AddressLookupTableAccount { key: table, addresses: accounts }
}

fn v0_tx(e: &Env, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair], alt: &AddressLookupTableAccount) -> VersionedTransaction {
    let bh = e.c.get_latest_blockhash().expect("blockhash");
    let msg = v0::Message::try_compile(&payer.pubkey(), ixs, std::slice::from_ref(alt), bh).expect("compile v0");
    let mut keys: Vec<&Keypair> = vec![payer];
    keys.extend(signers.iter().copied().filter(|k| k.pubkey() != payer.pubkey()));
    VersionedTransaction::try_new(VersionedMessage::V0(msg), &keys).expect("sign v0")
}

fn exec_v0(e: &Env, tx: &VersionedTransaction) -> Out {
    match e.c.send_transaction_with_config(tx, RpcSendTransactionConfig { skip_preflight: true, max_retries: Some(10), ..Default::default() }) {
        Ok(sig) => e.wait(&sig),
        Err(err) => Out { err: Some(serde_json::json!(format!("RPC rejected: {}", err))), ..Default::default() },
    }
}

/// TransferV1 of a Core Asset in `col`, signed by its owner (who also pays)
fn core_transfer(asset: &Pubkey, col: &Pubkey, owner: &Pubkey, new_owner: &Pubkey) -> Instruction {
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            AccountMeta::new_readonly(*col, false),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(*new_owner, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: vec![14, 0],
    }
}

/// A free label of `len` characters (the suite may run twice on one validator)
fn free_label(id: &Id, len: usize, salt: u64) -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    for n in salt.. {
        let mut x = n;
        let mut s = String::new();
        for _ in 0..len {
            s.push(CH[(x % 36) as usize] as char);
            x /= 36;
        }
        if id.reg(&format!("{}.memo", s)).is_none() {
            return s;
        }
    }
    unreachable!()
}

fn consumed(out: &Out, prog: &Pubkey) -> String {
    let p = format!("Program {} consumed", prog);
    out.logs.iter().filter(|l| l.starts_with(&p)).map(|l| l.split(' ').nth(3).unwrap_or("?").to_string()).collect::<Vec<_>>().join("+")
}

fn main() {
    let e = Env::new("regression-memo-id");
    let id = Id::new(&e);
    let w = &e.wallet;
    let wp = w.pubkey();
    println!("memo-id {}  memo-home {}  MEMO ID Collection {}", id.prog, id.home, id.collection);

    // ------------------------------------------------------------------ setup and init
    if e.data(&id.burn_guard).is_none() {
        e.must("init memo-home Burn Guard", &[id.init_burn_guard(&wp)], w, &[]);
    }
    let fresh = e.data(&id.collection).is_none();
    if fresh {
        let wrong = Pubkey::find_program_address(&[b"collection", id.authority.as_ref(), &sha(b"x1")], &id.home).0;
        e.reject("I01", "init at another Collection address", vec![id.init_with(&wp, &wrong)], w, &[],
                 Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
        let out = e.exec(&[id.init_with(&wp, &id.collection)], w, &[]);
        let col = e.data(&id.collection);
        let ok = out.ok() && col.as_ref().is_some_and(|d| {
            d[0] == 5 && pk(d, 1) == id.home_authority && {
                let len = u32_at(d, 33) as usize;
                &d[37..37 + len] == b"MEMO ID"
            }
        });
        let cfg_ok = e.data(&id.config).is_some_and(|d| pk(&d, 40) == id.authority && d[104..106] == [0, 0]);
        e.record("I02", "init creates the MEMO ID Collection (name, memo-home update authority, Authority = memo-id PDA, share 0)",
                 ok && cfg_ok, &out.brief());
    } else {
        println!("(MEMO ID Collection already exists: I01/I02 ran on an earlier pass)");
    }
    e.reject("I03", "init a second time", vec![id.init_with(&wp, &id.collection)], w, &[],
             Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &[], &[]);

    // ------------------------------------------------------------------ register
    let alice = e.new_user("alice", 1_000 * UNIT, true);
    let bob = e.new_user("bob", 1_000 * UNIT, true);
    let (ap, bp) = (alice.pubkey(), bob.pubkey());
    let a_ta = e.ata(&ap);

    let label = format!("alice{}", e.nonce());
    let a_id = format!("{}.memo", label);
    let price5 = id.price(6_900);
    let pl = Id::payload("register", &ap, &a_id);
    let lamports_before = e.c.get_balance(&ap).unwrap();
    let out = e.burn_ok("R01", "register a 5+ label burns exactly 69 mints", id.tx(price5, &pl, id.register(&ap, &label, URI)),
                        &alice, &[], &ap, price5, 1);
    println!("     register CU: {}, XNT spent (rent, Core fees, 5000 fee): {} lamports", consumed(&out, &id.prog),
             lamports_before - e.c.get_balance(&ap).unwrap());
    let a_asset = id.asset(&a_id);
    let reg = id.reg(&a_id);
    e.record("R02", "Registry: kind Nft, the Asset, no parent, no target, counters 0, the MEMO ID",
             reg == Some(Reg { kind: 0, asset: a_asset, parent: Pubkey::default(), nft_subids_issued: 0, record_subids: 0,
                               target_set_by: Pubkey::default(), target: None, memo_id: a_id.clone() }),
             &format!("{:?}", reg));
    let ad = id.asset_data(&a_asset);
    e.record("R03", "Asset: owner = payer, in the MEMO ID Collection, name = MEMO ID, URI as given",
             ad == Some((ap, Some(id.collection), a_id.clone(), URI.into())), &format!("{:?}", ad));
    e.record("R04", "Asset has its memo-home Vault State", e.data(&id.vault_state(&a_asset)).is_some_and(|d| d.len() == 17), "");

    // prices by length
    for (case, len, centi) in [("R05", 1, 42_069_000u64), ("R06", 2, 4_206_900), ("R07", 3, 420_690), ("R08", 4, 42_069)] {
        let l = free_label(&id, len, e.nonce());
        let m = format!("{}.memo", l);
        let price = id.price(centi);
        let pl = Id::payload("register", &wp, &m);
        if e.balance(&e.ata(&wp)).unwrap_or(0) < price {
            // only on a validator where the suite already ran: the wallet starts with 1,000,000 MEMO
            println!("SKIP [{}] the wallet holds less than {} units (run on a fresh validator)", case, price);
            continue;
        }
        e.burn_ok(case, &format!("register a {}-character label burns {} units", len, price),
                  id.tx(price, &pl, id.register(&wp, &l, "ar://abc")), w, &[], &wp, price, 1);
    }

    // rejections (alice's balance, supply and burn stats unchanged)
    let watch = [a_ta];
    let users = [ap];
    let rej = |case: &str, desc: &str, ixs: Vec<Instruction>, code: u32| {
        e.reject(case, desc, ixs, &alice, &[], Expect::Custom(IX, code), &watch, &users);
    };
    let pl_dup = Id::payload("register", &ap, &a_id);
    rej("R10", "register an existing MEMO ID", id.tx(price5, &pl_dup, id.register(&ap, &label, URI)), SYSTEM_ACCOUNT_IN_USE);
    for (case, bad) in [("R11", "Alice9x"), ("R12", "-alice9"), ("R13", "alice9-"), ("R14", "xn--alice"), ("R15", "ali_ce"), ("R16", "a.b-cd")] {
        let m = format!("{}.memo", bad);
        let pl = Id::payload("register", &ap, &m);
        rej(case, &format!("register label {:?}", bad), id.tx(price5, &pl, id.register(&ap, bad, URI)), INVALID_LABEL);
    }
    let long = "a".repeat(64);
    let m = format!("{}.memo", long);
    let pl = Id::payload("register", &ap, &m);
    rej("R17", "register a 64-character label", id.tx(price5, &pl, id.register(&ap, &long, URI)), INVALID_LABEL);
    let label2 = format!("bob{}", e.nonce());
    let id2 = format!("{}.memo", label2);
    let pl2 = Id::payload("register", &ap, &id2);
    rej("R18", "register with memo_id_hash of another ID",
        id.tx(price5, &pl2, id.register_raw(&ap, sha(b"other.memo"), &label2, URI, "other.memo", &e.mint, &a_ta)), HASH_MISMATCH);
    // a URI over 256 bytes does not fit in a register transaction; U06 checks the limit with update_uri
    for (case, uri, code) in [("R19", "http://example.com/a.json", INVALID_URI), ("R20", "https://", INVALID_URI)] {
        rej(case, &format!("register with URI {:.30}", uri), id.tx(price5, &pl2, id.register(&ap, &label2, uri)), code);
    }
    rej("R22", "memo burn_amount one unit short", id.tx(price5 - 1, &pl2, id.register(&ap, &label2, URI)), BURN_AMOUNT_MISMATCH);
    rej("R23", "memo burn_amount one unit over", id.tx(price5 + 1, &pl2, id.register(&ap, &label2, URI)), BURN_AMOUNT_MISMATCH);
    let wrong_op = Id::payload("issue_subid", &ap, &id2);
    rej("R24", "payload operation issue_subid", id.tx(price5, &wrong_op, id.register(&ap, &label2, URI)), INVALID_OPERATION);
    let wrong_id = Id::payload("register", &ap, &a_id);
    rej("R25", "payload names another MEMO ID", id.tx(price5, &wrong_id, id.register(&ap, &label2, URI)), MEMO_ID_MISMATCH);
    let wrong_user = Id::payload("register", &bp, &id2);
    rej("R26", "payload names another user", id.tx(price5, &wrong_user, id.register(&ap, &label2, URI)), USER_PUBKEY_MISMATCH);
    let wrong_cat = Id::payload_with(1, "profile", "register", &ap.to_string(), &id2);
    rej("R27", "payload category profile", id.tx(price5, &wrong_cat, id.register(&ap, &label2, URI)), INVALID_CATEGORY);
    rej("R28", "no memo at index 0", vec![cu(1_000_000), e.text_memo("x", 80), id.register(&ap, &label2, URI)], MEMO_REQUIRED);
    let other_mint = e.new_mint(&wp);
    let other_ta = ata_of_mint(&ap, &other_mint.pubkey());
    e.must("ATA of the second mint", &[e.create_ata_ix(&ap, &other_mint.pubkey())], w, &[]);
    rej("R29", "register with another mint",
        id.tx(price5, &pl2, id.register_raw(&ap, sha(id2.as_bytes()), &label2, URI, &id2, &other_mint.pubkey(), &other_ta)), INVALID_MINT);
    let poor = e.new_user("poor", price5 - 1, true);
    let pp = poor.pubkey();
    let plp = Id::payload("register", &pp, &id2);
    e.reject("R30", "register with too little MEMO", id.tx(price5, &plp, id.register(&pp, &label2, URI)), &poor, &[],
             Expect::Custom(IX, TOKEN_INSUFFICIENT_FUNDS), &[e.ata(&pp)], &[pp]);
    let mut wrong_col = id.register(&ap, &label2, URI);
    wrong_col.accounts[4].pubkey = Pubkey::find_program_address(&[b"collection", id.authority.as_ref(), &sha(b"x1")], &id.home).0;
    rej("R31", "register with another Collection", id.tx(price5, &pl2, wrong_col), WRONG_COLLECTION);
    e.record("R32", "rejected registrations left no Registry", id.reg(&id2).is_none(), "");

    // the largest legacy register transaction: a 63-character label (68-byte ID, the longest
    // second-level ID) and the longest URI that fits
    let big = free_label(&id, 63, e.nonce());
    let big_id = format!("{}.memo", big);
    let pl_big = Id::payload("register", &ap, &big_id);
    let size = |n: usize| {
        let ixs = id.tx(price5, &pl_big, id.register(&ap, &big, &format!("https://{}", "a".repeat(n - 8))));
        bincode::serialize(&e.build(&ixs, &alice, &[], Default::default())).unwrap().len()
    };
    let max_uri = (9..=MAX_URI).rev().find(|&n| size(n) <= 1232).unwrap();
    println!("     largest legacy register: 68-byte ID, URI up to {} bytes ({} bytes of 1232)", max_uri, size(max_uri));
    let uri = format!("https://{}", "a".repeat(max_uri - 8));
    e.burn_ok("R33", &format!("legacy: register a 68-byte ID with a {}-byte URI (the largest that fits)", max_uri),
              id.tx(price5, &pl_big, id.register(&ap, &big, &uri)), &alice, &[], &ap, price5, 1);

    // ------------------------------------------------------------------ Record SubIds
    let blog = format!("blog.{}", a_id);
    let out = e.exec(&[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "blog", Some(Target::Address(bp)))], &alice, &[]);
    let r = id.reg(&blog);
    e.record("S01", "create a Record SubId with a target (no burn)",
             out.ok() && r == Some(Reg { kind: 1, asset: Pubkey::default(), parent: id.registry(&a_id), nft_subids_issued: 0,
                                         record_subids: 0, target_set_by: ap, target: Some(Target::Address(bp)), memo_id: blog.clone() })
                 && e.data(&id.registry(&blog)).map(|d| d.len()) == Some(160 + blog.len()),
             &format!("{} {:?}", out.brief(), r));
    e.record("S02", "parent record_subids = 1", id.reg(&a_id).map(|r| r.record_subids) == Some(1), "");
    let rej0 = |case: &str, desc: &str, ix: Instruction, payer: &Keypair, signers: &[&Keypair], code: u32| {
        e.reject(case, desc, vec![ix], payer, signers, Expect::Custom(0, code), &[], &[]);
    };
    rej0("S03", "create a SubId as a non-controller", id.create_subid(&bp, &bp, &a_id, &a_asset, None, "shop", None), &bob, &[], NOT_CONTROLLER);
    rej0("S04", "create a SubId under a Record SubId", id.create_subid(&ap, &ap, &blog, &a_asset, None, "x", None), &alice, &[], PARENT_NOT_NFT);
    rej0("S05", "create a SubId with an invalid label", id.create_subid(&ap, &ap, &a_id, &a_asset, None, "Shop", None), &alice, &[], INVALID_LABEL);
    rej0("S06", "create an existing SubId", id.create_subid(&ap, &ap, &a_id, &a_asset, None, "blog", None), &alice, &[], SYSTEM_ACCOUNT_IN_USE);
    let other_asset = id.asset(&format!("{}.memo", free_label(&id, 4, 0)));
    rej0("S07", "create a SubId with another Asset", id.create_subid(&ap, &ap, &a_id, &other_asset, None, "shop", None), &alice, &[], WRONG_ASSET);
    rej0("S08", "create a SubId with a 64-character label", id.create_subid(&ap, &ap, &a_id, &a_asset, None, &"a".repeat(64), None), &alice, &[], INVALID_LABEL);

    let tmp = format!("tmp.{}", a_id);
    e.must("create tmp SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "tmp", None)], &alice, &[]);
    rej0("S09", "delete a SubId as a non-controller", id.delete_subid(&bp, &a_id, &a_asset, None, &tmp), &bob, &[], NOT_CONTROLLER);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.registry(&tmp)).unwrap();
    let out = e.exec(&[id.delete_subid(&ap, &a_id, &a_asset, None, &tmp)], &alice, &[]);
    let after = e.c.get_balance(&ap).unwrap();
    e.record("S10", "delete a Record SubId: account closed, rent to the controller, record_subids back to 1",
             out.ok() && id.reg(&tmp).is_none() && after + 5000 == before + rent && id.reg(&a_id).map(|r| r.record_subids) == Some(1),
             &format!("{} rent {} balance {} -> {}", out.brief(), rent, before, after));

    // ------------------------------------------------------------------ NFT SubIds
    let shop = format!("shop.{}", a_id);
    e.must("create shop SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "shop", None)], &alice, &[]);
    let parent_reg = id.registry(&a_id);
    let pl_shop = Id::payload("issue_subid", &ap, &shop);
    let issue = |index: u32, controller: &Pubkey, memo_id: &str, owner: &Pubkey| {
        id.issue_subid(&ap, controller, &a_id, &a_asset, None, memo_id, index, owner, URI)
    };
    e.reject("N01", "issue with index 1 (next is 0)", id.tx(price5, &pl_shop, issue(1, &ap, &shop, &bp)), &alice, &[],
             Expect::Custom(IX, WRONG_SUBID_INDEX), &watch, &users);
    e.reject("N02", "issue as a non-controller", id.tx(price5, &pl_shop, issue(0, &bp, &shop, &bp)), &alice, &[&bob],
             Expect::Custom(IX, NOT_CONTROLLER), &watch, &users);
    let pl_shop_op = Id::payload("register", &ap, &shop);
    e.reject("N03", "issue with a register payload", id.tx(price5, &pl_shop_op, issue(0, &ap, &shop, &bp)), &alice, &[],
             Expect::Custom(IX, INVALID_OPERATION), &watch, &users);
    e.reject("N04", "issue to Core's address", id.tx(price5, &pl_shop, issue(0, &ap, &shop, &CORE)), &alice, &[],
             Expect::Custom(IX, HOME_INVALID_OWNER), &watch, &users);
    {
        // the largest issue_subid transaction: a 64-byte SubId, a holder, a payer other than the controller
        let sub = format!("{}.{}", "s".repeat(64 - a_id.len() - 1), a_id);
        let pl = Id::payload("issue_subid", &ap, &sub);
        let size = |n: usize| {
            let ix = id.issue_subid(&ap, &bp, &a_id, &a_asset, Some(&id.asset(&shop)), &sub, 0, &bp, &format!("ar://{}", "a".repeat(n - 5)));
            bincode::serialize(&e.build(&id.tx(price5, &pl, ix), &alice, &[&bob], Default::default())).unwrap().len()
        };
        let fit = (6..=MAX_URI).rev().find(|&n| size(n) <= 1232);
        println!("     largest issue_subid: 64-byte ID, URI up to {:?} bytes (48-byte ar:// URI: {} bytes)", fit, size(48));
    }
    let lamports_before = e.c.get_balance(&ap).unwrap();
    let out = e.burn_ok("N05", "issue an NFT SubId to bob burns exactly 69 mints", id.tx(price5, &pl_shop, issue(0, &ap, &shop, &bp)),
                        &alice, &[], &ap, price5, 1);
    println!("     issue_subid CU: {}, XNT spent: {} lamports", consumed(&out, &id.prog), lamports_before - e.c.get_balance(&ap).unwrap());
    let shop_asset = id.asset(&shop);
    let r = id.reg(&shop);
    e.record("N06", "SubId becomes kind Nft with its Asset", r.as_ref().is_some_and(|r| r.kind == 0 && r.asset == shop_asset), &format!("{:?}", r));
    let ad = id.asset_data(&shop_asset);
    e.record("N07", "Asset owned by bob, named after the SubId", ad == Some((bp, Some(id.collection), shop.clone(), URI.into())), &format!("{:?}", ad));
    let idx0 = e.data(&id.subid_index(&parent_reg, 0));
    e.record("N08", "SubId Index entry 0 names the SubId's Registry", idx0.is_some_and(|d| d.len() == 41 && pk(&d, 8) == id.registry(&shop)), "");
    let p = id.reg(&a_id).unwrap();
    e.record("N09", "parent: nft_subids_issued 1, record_subids 1 (blog)", p.nft_subids_issued == 1 && p.record_subids == 1, &format!("{:?}", p));
    e.reject("N10", "issue the same SubId again", id.tx(price5, &pl_shop, issue(1, &ap, &shop, &bp)), &alice, &[],
             Expect::Custom(IX, NOT_RECORD), &watch, &users);
    rej0("N11", "delete an NFT SubId", id.delete_subid(&ap, &a_id, &a_asset, None, &shop), &alice, &[], NOT_RECORD);
    // the parent's controller no longer controls the issued SubId; its owner does
    rej0("N12", "parent's controller sets the target of an issued SubId",
         id.set_target(&ap, &shop, None, &shop_asset, None, Some(Target::Address(ap))), &alice, &[], NOT_CONTROLLER);
    let out = e.exec(&[id.create_subid(&bp, &bp, &shop, &shop_asset, None, "cart", None)], &bob, &[]);
    e.record("N13", "the NFT SubId's owner creates SubIds under it", out.ok() && id.reg(&format!("cart.{}", shop)).is_some(), &out.brief());
    // second issue gets index 1
    let wiki = format!("wiki.{}", a_id);
    e.must("create wiki SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "wiki", None)], &alice, &[]);
    let pl_wiki = Id::payload("issue_subid", &ap, &wiki);
    let out = e.exec(&id.tx(price5, &pl_wiki, issue(1, &ap, &wiki, &ap)), &alice, &[]);
    let p = id.reg(&a_id).unwrap();
    e.record("N14", "second NFT SubId: index 1, nft_subids_issued 2",
             out.ok() && p.nft_subids_issued == 2 && e.data(&id.subid_index(&parent_reg, 1)).is_some_and(|d| pk(&d, 8) == id.registry(&wiki)),
             &format!("{} {:?}", out.brief(), p));

    // ------------------------------------------------------------------ v0 transactions with the lookup table
    let alt = create_frozen_alt(&e, id.fixed_accounts());
    let frozen = e.data(&alt.key).is_some_and(|d| u64::from_le_bytes(d[4..12].try_into().unwrap()) == u64::MAX && d[22] == 0);
    e.record("A01", "lookup table with the fixed accounts, frozen (no authority)", frozen, &alt.key.to_string());
    let long_uri = format!("https://{}", "a".repeat(MAX_URI - 8));
    let big2 = free_label(&id, 63, e.nonce());
    let big2_id = format!("{}.memo", big2);
    let pl = Id::payload("register", &ap, &big2_id);
    let tx = v0_tx(&e, &id.tx(price5, &pl, id.register(&ap, &big2, &long_uri)), &alice, &[], &alt);
    let size = bincode::serialize(&tx).unwrap().len();
    let before = e.balance(&a_ta).unwrap();
    let out = exec_v0(&e, &tx);
    e.record("A02", &format!("v0: register a 68-byte ID with a 256-byte URI ({} bytes of 1232)", size),
             out.ok() && e.balance(&a_ta) == Some(before - price5)
                 && id.asset_data(&id.asset(&big2_id)).is_some_and(|a| a.0 == ap && a.2 == big2_id && a.3 == long_uri),
             &out.brief());
    let deep = format!("{}.{}", "d".repeat(64 - a_id.len() - 1), a_id);
    e.must("create a 64-byte Record SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, &"d".repeat(64 - a_id.len() - 1), None)], &alice, &[]);
    let pl = Id::payload("issue_subid", &bp, &deep);
    let n = id.reg(&a_id).unwrap().nft_subids_issued;
    // bob pays and alice (the controller) co-signs: two signatures, the largest message
    let ix = id.issue_subid(&bp, &ap, &a_id, &a_asset, None, &deep, n, &bp, &long_uri);
    let with_holder = {
        let ix = id.issue_subid(&bp, &ap, &a_id, &a_asset, Some(&shop_asset), &deep, n, &bp, &long_uri);
        bincode::serialize(&v0_tx(&e, &id.tx(price5, &pl, ix), &bob, &[&alice], &alt)).unwrap().len()
    };
    let tx = v0_tx(&e, &id.tx(price5, &pl, ix), &bob, &[&alice], &alt);
    let size = bincode::serialize(&tx).unwrap().len();
    let out = exec_v0(&e, &tx);
    e.record("A03", &format!("v0: issue a 64-byte SubId with a 256-byte URI, payer + controller ({} bytes; {} with a holder)", size, with_holder),
             out.ok() && with_holder <= 1232 && id.asset_data(&id.asset(&deep)).is_some_and(|a| a.0 == bp && a.3 == long_uri),
             &out.brief());

    // ------------------------------------------------------------------ 253-byte IDs
    // three NFT levels of 63-byte labels under alice's ID, then a 253-byte SubId at the fourth
    let ar_uri = format!("ar://{}", "a".repeat(43));
    let issue_v0 = |parent_id: &str, label: &str, owner: &Pubkey, payer: &Keypair| -> (String, Out) {
        let memo_id = format!("{}.{}", label, parent_id);
        let parent_asset = id.reg(parent_id).unwrap().asset;
        e.must("create a long Record SubId", &[id.create_subid(&ap, &ap, parent_id, &parent_asset, None, label, None)], &alice, &[]);
        let n = id.reg(parent_id).unwrap().nft_subids_issued;
        let pl = Id::payload("issue_subid", &payer.pubkey(), &memo_id);
        let ix = id.issue_subid(&payer.pubkey(), &ap, parent_id, &parent_asset, None, &memo_id, n, owner, &ar_uri);
        let signers: Vec<&Keypair> = if payer.pubkey() == ap { vec![] } else { vec![&alice] };
        (memo_id.clone(), exec_v0(&e, &v0_tx(&e, &id.tx(price5, &pl, ix), payer, &signers, &alt)))
    };
    let mut level = a_id.clone();
    let mut chain_ok = true;
    for c in ["l", "m", "n"] {
        let (next, out) = issue_v0(&level, &c.repeat(63), &ap, &alice);
        chain_ok &= out.ok();
        level = next;
    }
    e.record("L01", &format!("issue three NFT levels of 63-byte labels ({}-byte ID)", level.len()), chain_ok && level.len() == 192 + a_id.len(), &level);
    let last = 253 - level.len() - 1;
    let level_asset = id.reg(&level).unwrap().asset;
    rej0("L02", "create a 254-byte SubId", id.create_subid(&ap, &ap, &level, &level_asset, None, &"k".repeat(last + 1), None), &alice, &[], MEMO_ID_TOO_LONG);
    let (max_id, out) = issue_v0(&level, &"k".repeat(last), &bp, &bob);
    let max_asset = id.asset(&max_id);
    e.record("L03", "v0: issue a 253-byte SubId to bob (bob pays, alice co-signs, ar:// URI)",
             out.ok() && max_id.len() == 253 && id.asset_data(&max_asset).is_some_and(|a| a.0 == bp && a.2 == max_id)
                 && e.data(&id.registry(&max_id)).map(|d| d.len()) == Some(160 + 253),
             &out.brief());
    let out = e.exec(&[core_transfer(&max_asset, &id.collection, &bp, &ap)], &bob, &[]);
    e.record("L04", "an Asset with a 253-byte name transfers", out.ok() && id.asset_data(&max_asset).is_some_and(|a| a.0 == ap), &out.brief());
    let out = e.exec(&[cu(400_000), id.update_uri(&ap, &ap, &max_id, &max_asset, None, URI)], &alice, &[]);
    e.record("L05", "and its URI updates (name unchanged)", out.ok() && id.asset_data(&max_asset).is_some_and(|a| a.2 == max_id && a.3 == URI), &out.brief());

    // ------------------------------------------------------------------ forward records
    let out = e.exec(&[id.set_target(&ap, &a_id, None, &a_asset, None, Some(Target::Address(ap)))], &alice, &[]);
    let r = id.reg(&a_id).unwrap();
    e.record("T01", "controller sets the target", out.ok() && r.target == Some(Target::Address(ap)) && r.target_set_by == ap, &out.brief());
    rej0("T02", "non-controller sets the target", id.set_target(&bp, &a_id, None, &a_asset, None, Some(Target::Address(bp))), &bob, &[], NOT_CONTROLLER);
    rej0("T03", "set target with another Asset", id.set_target(&bp, &a_id, None, &shop_asset, None, Some(Target::Address(bp))), &bob, &[], WRONG_ASSET);
    let out = e.exec(&[id.set_target(&ap, &blog, Some(&a_id), &a_asset, None, Some(Target::Asset(shop_asset)))], &alice, &[]);
    let r = id.reg(&blog).unwrap();
    e.record("T04", "parent's controller sets a Record SubId's target", out.ok() && r.target == Some(Target::Asset(shop_asset)), &out.brief());
    rej0("T05", "Record SubId target without the parent",
         id.set_target(&ap, &blog, None, &a_asset, None, Some(Target::Address(ap))), &alice, &[], WRONG_PARENT);
    rej0("T06", "Record SubId target with another parent",
         id.set_target(&bp, &blog, Some(&shop), &shop_asset, None, Some(Target::Address(bp))), &bob, &[], WRONG_PARENT);
    let out = e.exec(&[id.set_target(&ap, &blog, Some(&a_id), &a_asset, None, None)], &alice, &[]);
    let r = id.reg(&blog).unwrap();
    e.record("T07", "clear_target", out.ok() && r.target.is_none() && r.target_set_by == Pubkey::default(), &out.brief());

    // ------------------------------------------------------------------ Primary IDs
    let out = e.exec(&[id.set_primary(&ap, &ap, &a_id, Target::Address(ap), None, None)], &alice, &[]);
    let pd = id.primary_data(&ap);
    e.record("P01", "wallet sets its Primary ID", out.ok() && pd == Some((Target::Address(ap), id.registry(&a_id), ap)), &format!("{} {:?}", out.brief(), pd));
    let r = id.reg(&a_id).unwrap();
    e.record("P02", "verified: forward and reverse match, both set by the current controllers",
             r.target == Some(Target::Address(ap)) && r.target_set_by == ap && pd.is_some_and(|p| p.1 == id.registry(&a_id) && p.2 == ap), "");
    rej0("P03", "set another wallet's Primary ID", id.set_primary(&bp, &bp, &a_id, Target::Address(ap), None, None), &bob, &[], NOT_CONTROLLER);
    let out = e.exec(&[id.set_primary(&ap, &ap, &wiki, Target::Address(ap), None, None)], &alice, &[]);
    e.record("P04", "set again: the Primary ID is replaced", out.ok() && id.primary_data(&ap).map(|p| p.1) == Some(id.registry(&wiki)), &out.brief());
    let out = e.exec(&[id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), Some(&shop_asset), None)], &bob, &[]);
    e.record("P05", "an Asset's owner sets its Primary ID", out.ok() && id.primary_data(&shop_asset) == Some((Target::Asset(shop_asset), id.registry(&shop), bp)), &out.brief());
    rej0("P06", "set an Asset's Primary ID as a non-owner",
         id.set_primary(&ap, &ap, &a_id, Target::Asset(shop_asset), Some(&shop_asset), None), &alice, &[], NOT_CONTROLLER);
    rej0("P07", "Asset target without its account", id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), None, None), &bob, &[], WRONG_ASSET);
    rej0("P08", "Asset target with another account", id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), Some(&a_asset), None), &bob, &[], WRONG_ASSET);
    rej0("P09", "clear another's Primary ID", id.clear_primary(&bp, &ap, None, None), &bob, &[], NOT_CONTROLLER);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.primary(&ap)).unwrap();
    let out = e.exec(&[id.clear_primary(&ap, &ap, None, None)], &alice, &[]);
    let after = e.c.get_balance(&ap).unwrap();
    e.record("P10", "clear_primary closes it, rent to the signer", out.ok() && id.primary_data(&ap).is_none() && after + 5000 == before + rent,
             &format!("{} rent {}", out.brief(), rent));

    // ------------------------------------------------------------------ transfer and the Vault rule
    // bob's NFT SubId moves into the Vault of alice's MEMO ID Asset: alice controls it through the holder
    let a_vault = id.vault(&a_asset);
    e.must("move shop into alice's Vault", &[core_transfer(&shop_asset, &id.collection, &bp, &a_vault)], &bob, &[]);
    let r = id.reg(&shop).unwrap();
    e.record("V01", "after the transfer bob's forward records lapse (target_set_by is not the controller)",
             r.target_set_by != a_vault && id.primary_data(&shop_asset).is_some_and(|p| p.2 == bp), "");
    rej0("V02", "the old owner can no longer set the target", id.set_target(&bp, &shop, None, &shop_asset, None, Some(Target::Address(bp))), &bob, &[], NOT_CONTROLLER);
    rej0("V03", "without the holder, the controller is the Vault (nobody can sign)",
         id.set_target(&ap, &shop, None, &shop_asset, None, Some(Target::Address(ap))), &alice, &[], NOT_CONTROLLER);
    let wiki_asset = id.asset(&wiki);
    rej0("V04", "a holder whose Vault does not hold the Asset",
         id.set_target(&ap, &shop, None, &shop_asset, Some(&wiki_asset), Some(Target::Address(ap))), &alice, &[], NOT_IN_HOLDER_VAULT);
    let out = e.exec(&[id.set_target(&ap, &shop, None, &shop_asset, Some(&a_asset), Some(Target::Address(ap)))], &alice, &[]);
    e.record("V05", "the holder's owner controls the held MEMO ID", out.ok() && id.reg(&shop).is_some_and(|r| r.target_set_by == ap), &out.brief());
    let out = e.exec(&[id.create_subid(&ap, &ap, &shop, &shop_asset, Some(&a_asset), "gift", None)], &alice, &[]);
    e.record("V06", "and creates SubIds under it", out.ok() && id.reg(&format!("gift.{}", shop)).is_some(), &out.brief());
    let out = e.exec(&[id.set_primary(&ap, &ap, &a_id, Target::Asset(shop_asset), Some(&shop_asset), Some(&a_asset))], &alice, &[]);
    e.record("V07", "and sets the held Asset's Primary ID", out.ok() && id.primary_data(&shop_asset).is_some_and(|p| p.2 == ap), &out.brief());

    // a sale of alice's ID: everything alice set lapses for bob
    e.must("transfer alice's ID to bob", &[core_transfer(&a_asset, &id.collection, &ap, &bp)], &alice, &[]);
    let r = id.reg(&a_id).unwrap();
    e.record("V08", "after a sale the forward record is void for the buyer", r.target_set_by == ap && r.target_set_by != bp, "");
    let out = e.exec(&[id.set_target(&bp, &blog, Some(&a_id), &a_asset, None, Some(Target::Address(bp)))], &bob, &[]);
    e.record("V09", "the buyer controls the Record SubIds", out.ok() && id.reg(&blog).is_some_and(|r| r.target_set_by == bp), &out.brief());
    rej0("V10", "the seller no longer does", id.set_target(&ap, &blog, Some(&a_id), &a_asset, None, None), &alice, &[], NOT_CONTROLLER);

    // ------------------------------------------------------------------ update_uri
    let out = e.exec(&[cu(400_000), id.update_uri(&bp, &bp, &a_id, &a_asset, None, "ipfs://bafynew")], &bob, &[]);
    e.record("U01", "controller updates the URI (name unchanged)",
             out.ok() && id.asset_data(&a_asset).is_some_and(|a| a.2 == a_id && a.3 == "ipfs://bafynew"), &out.brief());
    rej0("U02", "non-controller updates the URI", id.update_uri(&ap, &ap, &a_id, &a_asset, None, URI), &alice, &[], NOT_CONTROLLER);
    rej0("U03", "update to an invalid URI", id.update_uri(&bp, &bp, &a_id, &a_asset, None, "javascript:x"), &bob, &[], INVALID_URI);
    rej0("U04", "update the URI of a Record SubId", id.update_uri(&bp, &bp, &blog, &a_asset, None, URI), &bob, &[], NOT_NFT);
    rej0("U05", "update with another Asset", id.update_uri(&bp, &bp, &a_id, &shop_asset, None, URI), &bob, &[], WRONG_ASSET);
    let at_max = format!("https://{}", "a".repeat(MAX_URI - 8));
    let out = e.exec(&[cu(400_000), id.update_uri(&bp, &bp, &a_id, &a_asset, None, &at_max)], &bob, &[]);
    e.record("U06", "update to a 256-byte URI", out.ok() && id.asset_data(&a_asset).is_some_and(|a| a.3 == at_max), &out.brief());
    rej0("U07", "update to a 257-byte URI", id.update_uri(&bp, &bp, &a_id, &a_asset, None, &format!("{}a", at_max)), &bob, &[], URI_TOO_LONG);

    e.finish();
}

fn ata_of_mint(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address_with_program_id(owner, mint, &t22())
}
