//! On-chain regression suite for memo-id (local validator only).
//!
//! Runs against X1 mainnet's builds of Metaplex Core and Token-2022 (dumped by
//! scripts/local-validator.sh), memo-mint, memo-burn and memo-home. Covers init (the MEMO ID
//! Collection), register (label rules, the 253-byte limit, prices by length against an
//! independent curve reference, the BurnMemo and its payload, the memo-id instruction's index,
//! URIs, pre-funded PDAs, the Registry's size), Record SubIds (hash binding, parent links), NFT
//! SubIds with the SubId Index and counters, forward records, Primary IDs, the one-level Vault
//! rule, lapsing on transfer, update_uri, the signature of every controller account (G) and the
//! program accounts memo-id calls by CPI (I04, R52-R54, U10). Successes assert the exact state,
//! including created_at, bumps and the events; rejections assert the exact error code, the
//! program that raised it and its Anchor error name (System and Token-2022 log none), and that
//! no account the transaction references changed (except the fee payer's fee).
//!
//! scripts/run-regression-tests.sh runs it twice: at a supply of 5M (reward 1 MEMO) and at 2.5T
//! (reward 8 units), where prices are fractions of a MEMO, so ceil rounding and the supply input
//! of the price are visible. Cases that need a fresh validator (I02, R05-R08 once the wallet is
//! drained) are skipped elsewhere, and a skip fails the suite unless MEMO_REGRESSION_ALLOW_SKIP=1.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-id

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use base64::{engine::general_purpose, Engine as _};
use harness::*;
use memo_token_client::{get_program_id, memo_id::MemoIdAddresses};
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
use solana_system_interface::instruction as sys_ix;

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
const MEMO_TOO_SHORT: u32 = 6016;
const INVALID_MEMO_FORMAT: u32 = 6017;
const UNSUPPORTED_MEMO_VERSION: u32 = 6018;
const BURN_AMOUNT_MISMATCH: u32 = 6019;
const UNSUPPORTED_PAYLOAD_VERSION: u32 = 6020;
const INVALID_CATEGORY: u32 = 6021;
const INVALID_OPERATION: u32 = 6022;
const USER_PUBKEY_MISMATCH: u32 = 6023;
const MEMO_ID_MISMATCH: u32 = 6024;
/// memo-id's ErrorCode names, in order from 6000 (the name tells memo-id's errors apart from
/// memo-home's and memo-burn's, which use the same numbers)
const ERROR_NAMES: [&str; 26] = [
    "InvalidLabel", "MemoIdTooLong", "HashMismatch", "InvalidUri", "UriTooLong", "WrongCollection", "InvalidMint",
    "WrongAsset", "NotController", "NotInHolderVault", "ParentNotNft", "NotNft", "NotRecord", "WrongParent",
    "WrongSubidIndex", "MemoRequired", "MemoTooShort", "InvalidMemoFormat", "UnsupportedMemoVersion",
    "BurnAmountMismatch", "UnsupportedPayloadVersion", "InvalidCategory", "InvalidOperation", "UserPubkeyMismatch",
    "MemoIdMismatch", "ArithmeticOverflow",
];
// memo-home (raised in memo-home's frame, under memo-id's instruction index)
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

/// Independent reference of the forward-record rule (Registry::forward, which only runs off
/// chain): the target is valid only while the controller that set it is still the controller
fn forward(r: &Reg, controller: &Pubkey) -> Option<Target> { r.target.filter(|_| r.target_set_by == *controller) }

/// Borsh reader for event fields
struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
    fn pk(&mut self) -> Option<Pubkey> { Some(Pubkey::new_from_array(self.take(32)?.try_into().ok()?)) }
    fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    fn u64(&mut self) -> Option<u64> { Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
    fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn target(&mut self) -> Option<Target> {
        let kind = self.take(1)?[0];
        let k = self.pk()?;
        match kind {
            0 => Some(Target::Address(k)),
            1 => Some(Target::Asset(k)),
            _ => None,
        }
    }
}

/// The Borsh fields of every event `name` the transaction emitted ("Program data:" logs)
fn event_data(out: &Out, name: &str) -> Vec<Vec<u8>> {
    let disc = &Sha256::digest(format!("event:{}", name).as_bytes())[..8];
    out.logs.iter().filter_map(|l| l.strip_prefix("Program data: "))
        .filter_map(|b| general_purpose::STANDARD.decode(b).ok())
        .filter(|v| v.len() >= 8 && &v[..8] == disc)
        .map(|v| v[8..].to_vec())
        .collect()
}

/// The one event `name` the transaction emitted, read by `f`, which must consume all of its
/// fields; None when it was not emitted exactly once
fn event<T>(out: &Out, name: &str, f: impl Fn(&mut Rd) -> Option<T>) -> Option<T> {
    let all = event_data(out, name);
    let [data] = all.as_slice() else { return None };
    let mut r = Rd { d: data, p: 0 };
    let v = f(&mut r)?;
    (r.p == r.d.len()).then_some(v)
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
    /// Primary ID of a Target: [b"primary_id", kind (0 Address, 1 Asset), key]
    fn primary(&self, target: &Target) -> Pubkey {
        let kind = match target { Target::Address(_) => 0u8, Target::Asset(_) => 1 };
        Pubkey::find_program_address(&[b"primary_id", &[kind], target.key().as_ref()], &self.prog).0
    }
    fn reg(&self, memo_id: &str) -> Option<Reg> { self.e.data(&self.registry(memo_id)).map(|d| parse_reg(&d)) }
    /// The Registry fields parse_reg leaves out, created_at (offset 73) and bump (89), must be the
    /// block time of the transaction `out` and the canonical bump of the Registry PDA
    fn stamped(&self, memo_id: &str, out: &Out) -> bool {
        let bump = Pubkey::find_program_address(&[b"memo_id", &sha(memo_id.as_bytes())], &self.prog).1;
        let meta = self.e.data(&self.registry(memo_id)).map(|d| (i64::from_le_bytes(d[73..81].try_into().unwrap()), d[89]));
        out.block_time.is_some() && meta == out.block_time.map(|t| (t, bump))
    }
    /// The memo-id error `code` (its ErrorCode from 6000, or an Anchor account error) raised in
    /// memo-id's own frame at instruction `ix`; System's AccountAlreadyInUse raised by System
    fn err(&self, ix: u64, code: u32) -> Expect<'static> {
        let name = match code {
            SYSTEM_ACCOUNT_IN_USE => return Expect::RaisedBy(ix, code, system_program()),
            6000..=6025 => ERROR_NAMES[(code - 6000) as usize],
            ANCHOR_CONSTRAINT_SEEDS => "ConstraintSeeds",
            ANCHOR_CONSTRAINT_ADDRESS => "ConstraintAddress",
            ANCHOR_ACCOUNT_NOT_SIGNER => "AccountNotSigner",
            ANCHOR_DISCRIMINATOR_MISMATCH => "AccountDiscriminatorMismatch",
            ANCHOR_INVALID_PROGRAM_ID => "InvalidProgramId",
            _ => panic!("no expectation for code {}", code),
        };
        Expect::Raised(ix, code, self.prog, name)
    }
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
            AccountMeta::new(self.primary(&target), false),
            self.opt(target_asset),
            self.opt(holder),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn clear_primary(&self, signer: &Pubkey, target: &Target, target_asset: Option<&Pubkey>, holder: Option<&Pubkey>) -> Instruction {
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
    fn primary_data(&self, target: &Target) -> Option<(Target, Pubkey, Pubkey)> {
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
        // the list admin-init-memo-id-alt deploys
        let shared = MemoIdAddresses::new(self.prog, self.home);
        assert_eq!((shared.authority, shared.collection, shared.config, shared.home_authority),
                   (self.authority, self.collection, self.config, self.home_authority));
        shared.lookup_table_addresses(self.e.mint, self.e.burn_prog, self.e.gbs)
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

/// A v0 transaction that must fail with `expect` and change none of the watched state
#[allow(clippy::too_many_arguments)]
fn reject_v0(e: &Env, case: &str, desc: &str, tx: &VersionedTransaction, expect: Expect, tokens: &[Pubkey], users: &[Pubkey],
             accounts: &[Pubkey]) {
    let (before, raw_before) = (e.snap(tokens, users), e.raw(accounts));
    let out = exec_v0(e, tx);
    let (after, raw_after) = (e.snap(tokens, users), e.raw(accounts));
    let pass = expect.matches(&out) && before == after && raw_before == raw_after;
    e.record(case, desc, pass, &format!("expected {} and no state change; got {} ({:?} in {:?}); {}; accounts unchanged {}",
                                        expect.describe(), out.brief(), out.error_name(), out.failed_program(), before.diff(&after),
                                        raw_before == raw_after));
    if !pass {
        println!("     logs:\n       {}", out.logs.join("\n       "));
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

/// CreateV1 of a Core Asset at keypair address `asset`, owned by `owner`, in no collection
fn core_create_plain(asset: &Pubkey, payer: &Pubkey, owner: &Pubkey) -> Instruction {
    let mut d = vec![0u8, 0];
    bstr(&mut d, "plain core asset");
    bstr(&mut d, "https://example.com/plain.json");
    d.push(0);
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, true),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
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
    // the Collection address is checked before the handler, so I01 runs on any validator
    let wrong_col = Pubkey::find_program_address(&[b"collection", id.authority.as_ref(), &sha(b"x1")], &id.home).0;
    e.reject_untouched("I01", "init at another Collection address", vec![id.init_with(&wp, &wrong_col)], w, &[],
                       id.err(0, WRONG_COLLECTION), &[], &[]);
    // Anchor 0.32 CPIs go to the program account the caller passes, signed by the memo-id
    // Authority: only the program checks keep that signature away from other programs
    let mut fake_home = id.init_with(&wp, &id.collection);
    fake_home.accounts[7].pubkey = spl_memo::ID;
    e.reject_untouched("I04", "init with SPL Memo in place of memo-home", vec![fake_home], w, &[],
                       id.err(0, ANCHOR_INVALID_PROGRAM_ID), &[], &[]);
    let fresh = e.data(&id.collection).is_none();
    if fresh {
        let out = e.exec(&[id.init_with(&wp, &id.collection)], w, &[]);
        let col = e.data(&id.collection);
        let ok = out.ok() && col.as_ref().is_some_and(|d| {
            d[0] == 5 && pk(d, 1) == id.home_authority && {
                let len = u32_at(d, 33) as usize;
                &d[37..37 + len] == b"MEMO ID"
            }
        });
        let cfg_ok = e.data(&id.config).is_some_and(|d| pk(&d, 40) == id.authority && d[104..106] == [0, 0]);
        let ev = event(&out, "Initialized", |r| r.pk());
        e.record("I02", "init creates the MEMO ID Collection (name, memo-home update authority, Authority = memo-id PDA, share 0)",
                 ok && cfg_ok && ev == Some(id.collection), &format!("{} event {:?}", out.brief(), ev));
    } else {
        e.skip("I02", "the MEMO ID Collection already exists (init ran on an earlier pass)");
    }
    e.reject_untouched("I03", "init a second time", vec![id.init_with(&wp, &id.collection)], w, &[],
                       id.err(0, SYSTEM_ACCOUNT_IN_USE), &[], &[]);

    // ------------------------------------------------------------------ register
    let alice = e.new_user("alice", 3_000 * UNIT, true);
    let bob = e.new_user("bob", 2_000 * UNIT, true);
    let (ap, bp) = (alice.pubkey(), bob.pubkey());
    let a_ta = e.ata(&ap);

    let label = format!("alice{}", e.nonce());
    let a_id = format!("{}.memo", label);
    let price5 = id.price(6_900);
    println!("     supply {} units, reward {:?} units: 69 mints = {} units", e.supply(), curve_reference(e.supply()), price5);
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
    let ev = event(&out, "Registered", |r| Some((r.string()?, r.pk()?, r.pk()?, r.pk()?, r.u64()?)));
    e.record("R35", "Registry created_at = block time, canonical bump; Registered event (ID, Registry, Asset, owner, burned)",
             id.stamped(&a_id, &out) && ev == Some((a_id.clone(), id.registry(&a_id), a_asset, ap, price5)), &format!("{:?}", ev));
    let raw = e.raw(&[id.registry(&a_id)]);
    let reg_len = 160 + a_id.len();
    e.record("R50", "the Registry is sized for its own ID (160 + ID bytes) and holds exactly its rent",
             raw[0].as_ref().is_some_and(|(owner, lamports, d)| {
                 *owner == id.prog && d.len() == reg_len && *lamports == e.c.get_minimum_balance_for_rent_exemption(reg_len).unwrap()
             }),
             &format!("{:?}", raw[0].as_ref().map(|r| (r.0, r.1, r.2.len()))));

    // prices by length
    for (case, len, centi) in [("R05", 1, 42_069_000u64), ("R06", 2, 4_206_900), ("R07", 3, 420_690), ("R08", 4, 42_069)] {
        let l = free_label(&id, len, e.nonce());
        let m = format!("{}.memo", l);
        let price = id.price(centi);
        let pl = Id::payload("register", &wp, &m);
        if e.balance(&e.ata(&wp)).unwrap_or(0) < price {
            // only on a validator where the suite already ran: the wallet starts with 1,000,000 MEMO
            e.skip(case, &format!("the wallet holds less than {} units (run on a fresh validator)", price));
            continue;
        }
        e.burn_ok(case, &format!("register a {}-character label burns {} units", len, price),
                  id.tx(price, &pl, id.register(&wp, &l, "ar://abc")), w, &[], &wp, price, 1);
    }

    // rejections (alice's balance, supply, burn stats and every account the transaction names unchanged)
    let watch = [a_ta];
    let users = [ap];
    let rej = |case: &str, desc: &str, ixs: Vec<Instruction>, code: u32| {
        e.reject_untouched(case, desc, ixs, &alice, &[], id.err(IX, code), &watch, &users);
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
    let pl_suffix = Id::payload("register", &ap, "memo.memo");
    rej("R34", "register the label \"memo\" (the suffix is not a label)", id.tx(price5, &pl_suffix, id.register(&ap, "memo", URI)), INVALID_LABEL);
    let label2 = format!("bob{}", e.nonce());
    let id2 = format!("{}.memo", label2);
    let pl2 = Id::payload("register", &ap, &id2);
    rej("R18", "register with memo_id_hash of another ID",
        id.tx(price5, &pl2, id.register_raw(&ap, sha(b"other.memo"), &label2, URI, "other.memo", &e.mint, &a_ta)), HASH_MISMATCH);
    // a URI over 256 bytes does not fit in a legacy register transaction: A04 sends one as v0 with
    // the lookup table, and U07 checks the limit with update_uri
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
    e.reject_untouched("R30", "register with too little MEMO", id.tx(price5, &plp, id.register(&pp, &label2, URI)), &poor, &[],
                       Expect::RaisedBy(IX, TOKEN_INSUFFICIENT_FUNDS, t22()), &[e.ata(&pp)], &[pp]);
    let mut wrong_col_ix = id.register(&ap, &label2, URI);
    wrong_col_ix.accounts[4].pubkey = wrong_col;
    rej("R31", "register with another Collection", id.tx(price5, &pl2, wrong_col_ix), WRONG_COLLECTION);
    // the programs memo-id calls are checked by memo-id itself: a CPI goes to the account passed
    // (with memo-id's Authority signing for memo-home), and memo-burn could not check itself
    for (case, at, what, program) in [("R52", 9, "memo-home", spl_memo::ID), ("R53", 15, "memo-burn", spl_memo::ID),
                                      ("R54", 14, "Token-2022", SPL_TOKEN_LEGACY)] {
        let mut ix = id.register(&ap, &label2, URI);
        ix.accounts[at].pubkey = program;
        rej(case, &format!("register with {} in place of {}", if program == spl_memo::ID { "SPL Memo" } else { "SPL Token" }, what),
            id.tx(price5, &pl2, ix), ANCHOR_INVALID_PROGRAM_ID);
    }

    // the BurnMemo, decoded by memo-id before memo-burn sees it: memo-burn repeats the outer checks
    // with its own numbers and never reads the payload, so only memo-id's codes show these checks
    rej("R36", "memo of 68 bytes (one short)", vec![e.text_memo("short", 68), cu(1_000_000), id.register(&ap, &label2, URI)], MEMO_TOO_SHORT);
    rej("R51", "memo of exactly 69 bytes passes the length check (not Base64: InvalidMemoFormat)",
        vec![e.text_memo("min", 69), cu(1_000_000), id.register(&ap, &label2, URI)], INVALID_MEMO_FORMAT);
    rej("R37", "memo that is not Base64", vec![memo_ix(&[b'!'; 80]), cu(1_000_000), id.register(&ap, &label2, URI)], INVALID_MEMO_FORMAT);
    let memo_raw = |raw: Vec<u8>| vec![memo_ix(&b64(&raw)), cu(1_000_000), id.register(&ap, &label2, URI)];
    rej("R38", "BurnMemo payload length prefix one byte too long",
        memo_raw(burn_memo_raw(1, price5, pl2.len() as u32 + 1, &pl2)), INVALID_MEMO_FORMAT);
    let mut extra = burn_memo_raw(1, price5, pl2.len() as u32, &pl2);
    extra.push(0);
    rej("R39", "a byte after the BurnMemo", memo_raw(extra), INVALID_MEMO_FORMAT);
    rej("R40", "BurnMemo version 2", memo_raw(burn_memo_raw(2, price5, pl2.len() as u32, &pl2)), UNSUPPORTED_MEMO_VERSION);
    let pl_v2 = Id::payload_with(2, "memo_id", "register", &ap.to_string(), &id2);
    rej("R41", "payload version 2 (memo-id's 6020, not memo-home's InvalidOwner)", id.tx(price5, &pl_v2, id.register(&ap, &label2, URI)),
        UNSUPPORTED_PAYLOAD_VERSION);
    let mut pl_trailing = pl2.clone();
    pl_trailing.push(0);
    rej("R42", "payload with a trailing byte (the BurnMemo length covers it)", id.tx(price5, &pl_trailing, id.register(&ap, &label2, URI)),
        INVALID_MEMO_FORMAT);
    e.reject_untouched("R43", "register at instruction index 0, the BurnMemo at index 1",
                       vec![id.register(&ap, &label2, URI), app_burn_memo(price5, &pl2)], &alice, &[], id.err(0, MEMO_REQUIRED), &watch, &users);
    let mut fake_sysvar = id.register(&ap, &label2, URI);
    fake_sysvar.accounts[16].pubkey = wp;
    rej("R44", "register with a wallet in place of the instructions sysvar", id.tx(price5, &pl2, fake_sysvar), ANCHOR_CONSTRAINT_ADDRESS);
    // memo-home binds the Asset to the name: memo-id burns first, then memo-home refuses
    let foreign_asset = id.asset(&format!("{}.memo", free_label(&id, 7, e.nonce())));
    let mut other_pdas = id.register(&ap, &label2, URI);
    other_pdas.accounts[5].pubkey = foreign_asset;
    other_pdas.accounts[6].pubkey = id.vault_state(&foreign_asset);
    e.reject_untouched("R45", "register with the Asset and Vault State PDAs of another name (memo-home ConstraintSeeds)",
                       id.tx(price5, &pl2, other_pdas), &alice, &[], Expect::Raised(IX, ANCHOR_CONSTRAINT_SEEDS, id.home, "ConstraintSeeds"),
                       &watch, &users);
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

    // the layout clients send: [BurnMemo, register, compute budget], memo-id at index 1
    let l1 = format!("first{}", e.nonce());
    let id1 = format!("{}.memo", l1);
    let out = e.burn_ok("R46", "register at instruction index 1 (memo, register, compute budget) burns exactly 69 mints",
                        vec![app_burn_memo(price5, &Id::payload("register", &ap, &id1)), id.register(&ap, &l1, URI), cu(1_000_000)],
                        &alice, &[], &ap, price5, 1);
    let r = id.reg(&id1);
    e.record("R47", "and creates the Registry and the Asset",
             r == Some(Reg { kind: 0, asset: id.asset(&id1), parent: Pubkey::default(), nft_subids_issued: 0, record_subids: 0,
                             target_set_by: Pubkey::default(), target: None, memo_id: id1.clone() })
                 && id.stamped(&id1, &out) && id.asset_data(&id.asset(&id1)) == Some((ap, Some(id.collection), id1.clone(), URI.into())),
             &format!("{:?}", r));

    // anyone can send lamports to the Registry and Asset PDAs of a name before it is registered
    let lp = format!("prefunded{}", e.nonce());
    let idp = format!("{}.memo", lp);
    let prefund = 1_000_000;
    e.must("pre-fund the Registry and Asset PDAs", &[sys_ix::transfer(&wp, &id.registry(&idp), prefund),
                                                     sys_ix::transfer(&wp, &id.asset(&idp), prefund)], w, &[]);
    let out = e.burn_ok("R48", "register over a pre-funded Registry and Asset burns exactly 69 mints",
                        id.tx(price5, &Id::payload("register", &ap, &idp), id.register(&ap, &lp, URI)), &alice, &[], &ap, price5, 1);
    let raw = e.raw(&[id.registry(&idp), id.asset(&idp), id.asset(&id1)]);
    let rent_min = |len: usize| e.c.get_minimum_balance_for_rent_exemption(len).unwrap();
    // an Asset holds its rent plus Core's create fee; id1's Asset (R46, not pre-funded) gives the fee
    let core_fee = raw[2].as_ref().map(|(_, lamports, d)| lamports - rent_min(d.len()));
    let reg_ok = raw[0].as_ref().is_some_and(|(owner, lamports, d)| *owner == id.prog && *lamports == rent_min(d.len()) && parse_reg(d).memo_id == idp);
    let asset_ok = raw[1].as_ref().is_some_and(|(owner, lamports, d)| {
        *owner == CORE && Some(*lamports) == core_fee.map(|f| rent_min(d.len()) + f) && parse_asset(d).0 == ap
    });
    e.record("R49", "the Registry is topped up to its rent (no more); the Asset's pre-fund was swept (it holds what an Asset holds)",
             out.ok() && reg_ok && asset_ok && id.stamped(&idp, &out),
             &format!("registry {:?} asset {:?} Core create fee {:?}", raw[0].as_ref().map(|r| (r.1, r.2.len())),
                      raw[1].as_ref().map(|r| (r.1, r.2.len())), core_fee));

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
    let created = |out: &Out| event(out, "SubidCreated", |r| Some((r.string()?, r.pk()?, r.pk()?)));
    let target_set = |out: &Out| event(out, "TargetSet", |r| Some((r.pk()?, r.target()?, r.pk()?)));
    let (ev_c, ev_t) = (created(&out), target_set(&out));
    e.record("S12", "created_at = block time, canonical bump; SubidCreated and TargetSet (set_by = the controller) events",
             id.stamped(&blog, &out) && ev_c == Some((blog.clone(), id.registry(&blog), id.registry(&a_id)))
                 && ev_t == Some((id.registry(&blog), Target::Address(bp), ap)),
             &format!("{:?} {:?}", ev_c, ev_t));
    // rejections of single-instruction transactions: nothing they name may change
    let rej0 = |case: &str, desc: &str, ix: Instruction, payer: &Keypair, signers: &[&Keypair], code: u32| {
        e.reject_untouched(case, desc, vec![ix], payer, signers, id.err(0, code), &[], &[]);
    };
    rej0("S03", "create a SubId as a non-controller", id.create_subid(&bp, &bp, &a_id, &a_asset, None, "shop", None), &bob, &[], NOT_CONTROLLER);
    rej0("S04", "create a SubId under a Record SubId", id.create_subid(&ap, &ap, &blog, &a_asset, None, "x", None), &alice, &[], PARENT_NOT_NFT);
    rej0("S05", "create a SubId with an invalid label", id.create_subid(&ap, &ap, &a_id, &a_asset, None, "Shop", None), &alice, &[], INVALID_LABEL);
    rej0("S06", "create an existing SubId", id.create_subid(&ap, &ap, &a_id, &a_asset, None, "blog", None), &alice, &[], SYSTEM_ACCOUNT_IN_USE);
    let other_asset = id.asset(&format!("{}.memo", free_label(&id, 4, 0)));
    rej0("S07", "create a SubId with another Asset", id.create_subid(&ap, &ap, &a_id, &other_asset, None, "shop", None), &alice, &[], WRONG_ASSET);
    rej0("S08", "create a SubId with a 64-character label", id.create_subid(&ap, &ap, &a_id, &a_asset, None, &"a".repeat(64), None), &alice, &[], INVALID_LABEL);
    rej0("S11", "create a SubId with the label \"memo\" (no ID contains .memo.)", id.create_subid(&ap, &ap, &a_id, &a_asset, None, "memo", None), &alice, &[], INVALID_LABEL);
    // memo_id_hash is an argument and the Registry address follows it: only the hash check keeps a
    // SubId's Registry at the address of its own name (each case also watches that address)
    let with_hash = |hash_of: &str, registry_of: &str| {
        let mut ix = id.create_subid(&ap, &ap, &a_id, &a_asset, None, "x", None);
        ix.data[8..40].copy_from_slice(&sha(hash_of.as_bytes()));
        ix.accounts[5].pubkey = id.registry(registry_of);
        ix
    };
    let victim = format!("victim{}.memo", e.nonce());
    let x_sub = format!("x.{}", a_id);
    rej0("S13", "create SubId x with the hash and Registry PDA of a second-level name (no Registry appears there)",
         with_hash(&victim, &victim), &alice, &[], HASH_MISMATCH);
    rej0("S14", "create SubId x with the hash and Registry PDA of another SubId of the parent",
         with_hash(&format!("y.{}", a_id), &format!("y.{}", a_id)), &alice, &[], HASH_MISMATCH);
    rej0("S15", "create SubId x with its own hash but another name's Registry PDA", with_hash(&x_sub, &victim), &alice, &[], ANCHOR_CONSTRAINT_SEEDS);

    let tmp = format!("tmp.{}", a_id);
    let out = e.must("create tmp SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "tmp", None)], &alice, &[]);
    let r = id.reg(&tmp);
    let ev_c = created(&out);
    e.record("S16", "a SubId created without a target: target_set_by stays zero, created_at = block time, SubidCreated only",
             r == Some(Reg { kind: 1, asset: Pubkey::default(), parent: id.registry(&a_id), nft_subids_issued: 0, record_subids: 0,
                             target_set_by: Pubkey::default(), target: None, memo_id: tmp.clone() })
                 && id.stamped(&tmp, &out) && ev_c == Some((tmp.clone(), id.registry(&tmp), id.registry(&a_id)))
                 && event_data(&out, "TargetSet").is_empty(),
             &format!("{:?} {:?}", r, ev_c));
    rej0("S09", "delete a SubId as a non-controller", id.delete_subid(&bp, &a_id, &a_asset, None, &tmp), &bob, &[], NOT_CONTROLLER);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.registry(&tmp)).unwrap();
    let out = e.exec(&[id.delete_subid(&ap, &a_id, &a_asset, None, &tmp)], &alice, &[]);
    let after = e.c.get_balance(&ap).unwrap();
    e.record("S10", "delete a Record SubId: account closed, rent to the controller, record_subids back to 1",
             out.ok() && id.reg(&tmp).is_none() && after + 5000 == before + rent && id.reg(&a_id).map(|r| r.record_subids) == Some(1),
             &format!("{} rent {} balance {} -> {}", out.brief(), rent, before, after));
    // re-create the deleted name with bob paying (fee and rent) and alice controlling
    let (a0, b0) = (e.c.get_balance(&ap).unwrap(), e.c.get_balance(&bp).unwrap());
    let out = e.exec(&[id.create_subid(&bp, &ap, &a_id, &a_asset, None, "tmp", Some(Target::Address(ap)))], &bob, &[&alice]);
    let rent = e.c.get_balance(&id.registry(&tmp)).unwrap();
    let (a1, b1) = (e.c.get_balance(&ap).unwrap(), e.c.get_balance(&bp).unwrap());
    let r = id.reg(&tmp);
    e.record("S17", "re-create it, bob paying and alice controlling: a fresh Record, target stamped by the controller, rent from the payer",
             out.ok() && r == Some(Reg { kind: 1, asset: Pubkey::default(), parent: id.registry(&a_id), nft_subids_issued: 0, record_subids: 0,
                                         target_set_by: ap, target: Some(Target::Address(ap)), memo_id: tmp.clone() })
                 && id.stamped(&tmp, &out) && a1 == a0 && b1 + rent + 10_000 == b0 && id.reg(&a_id).map(|r| r.record_subids) == Some(2),
             &format!("{} {:?} rent {} alice {} -> {} bob {} -> {}", out.brief(), r, rent, a0, a1, b0, b1));
    let out = e.exec(&[id.delete_subid(&ap, &a_id, &a_asset, None, &tmp)], &alice, &[]);
    let (a2, b2) = (e.c.get_balance(&ap).unwrap(), e.c.get_balance(&bp).unwrap());
    let ev = event(&out, "SubidDeleted", |r| Some((r.string()?, r.pk()?)));
    e.record("S18", "delete it: the rent goes to the controller, not to the payer who funded it; SubidDeleted event",
             out.ok() && id.reg(&tmp).is_none() && a2 + 5000 == a1 + rent && b2 == b1 && id.reg(&a_id).map(|r| r.record_subids) == Some(1)
                 && ev == Some((tmp.clone(), id.registry(&tmp))),
             &format!("{} alice {} -> {} {:?}", out.brief(), a1, a2, ev));
    // only the xn-- prefix is reserved (under id1, so the counters of alice's first ID stay as they are)
    let xn = format!("a-xn--b.{}", id1);
    let out = e.exec(&[id.create_subid(&ap, &ap, &id1, &id.asset(&id1), None, "a-xn--b", None)], &alice, &[]);
    let r = id.reg(&xn);
    e.record("S22", "a label with xn-- after its first character is valid (a-xn--b)",
             out.ok() && r == Some(Reg { kind: 1, asset: Pubkey::default(), parent: id.registry(&id1), nft_subids_issued: 0, record_subids: 0,
                                         target_set_by: Pubkey::default(), target: None, memo_id: xn.clone() })
                 && id.stamped(&xn, &out) && id.reg(&id1).map(|r| r.record_subids) == Some(1),
             &format!("{} {:?}", out.brief(), r));

    // ------------------------------------------------------------------ NFT SubIds
    let shop = format!("shop.{}", a_id);
    e.must("create shop SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "shop", None)], &alice, &[]);
    let parent_reg = id.registry(&a_id);
    let pl_shop = Id::payload("issue_subid", &ap, &shop);
    let issue = |index: u32, controller: &Pubkey, memo_id: &str, owner: &Pubkey| {
        id.issue_subid(&ap, controller, &a_id, &a_asset, None, memo_id, index, owner, URI)
    };
    // rejections (alice's balance, supply, burn stats and every account the transaction names unchanged)
    let rej_n = |case: &str, desc: &str, ixs: Vec<Instruction>, signers: &[&Keypair], expect: Expect| {
        e.reject_untouched(case, desc, ixs, &alice, signers, expect, &watch, &users);
    };
    rej_n("N01", "issue with index 1 (next is 0)", id.tx(price5, &pl_shop, issue(1, &ap, &shop, &bp)), &[], id.err(IX, WRONG_SUBID_INDEX));
    rej_n("N02", "issue as a non-controller", id.tx(price5, &pl_shop, issue(0, &bp, &shop, &bp)), &[&bob], id.err(IX, NOT_CONTROLLER));
    let pl_shop_op = Id::payload("register", &ap, &shop);
    rej_n("N03", "issue with a register payload", id.tx(price5, &pl_shop_op, issue(0, &ap, &shop, &bp)), &[], id.err(IX, INVALID_OPERATION));
    rej_n("N04", "issue to Core's address (memo-home InvalidOwner)", id.tx(price5, &pl_shop, issue(0, &ap, &shop, &CORE)), &[],
          Expect::Raised(IX, HOME_INVALID_OWNER, id.home, "InvalidOwner"));
    // memo-home checks only the URI's length, so the scheme rule is memo-id's alone
    let issue_uri = |uri: &str| id.issue_subid(&ap, &ap, &a_id, &a_asset, None, &shop, 0, &bp, uri);
    rej_n("N15", "issue with an http:// URI", id.tx(price5, &pl_shop, issue_uri("http://example.com/x.json")), &[], id.err(IX, INVALID_URI));
    rej_n("N16", "issue with an empty URI", id.tx(price5, &pl_shop, issue_uri("")), &[], id.err(IX, INVALID_URI));
    rej_n("N17", "issue with a payload naming another user", id.tx(price5, &Id::payload("issue_subid", &bp, &shop), issue(0, &ap, &shop, &bp)),
          &[], id.err(IX, USER_PUBKEY_MISMATCH));
    rej_n("N18", "issue with a payload naming another MEMO ID", id.tx(price5, &Id::payload("issue_subid", &ap, &blog), issue(0, &ap, &shop, &bp)),
          &[], id.err(IX, MEMO_ID_MISMATCH));
    let id1_asset = id.asset(&id1);
    rej_n("N19", "issue with another Asset as the parent's",
          id.tx(price5, &pl_shop, id.issue_subid(&ap, &ap, &a_id, &id1_asset, None, &shop, 0, &bp, URI)), &[], id.err(IX, WRONG_ASSET));
    rej_n("N20", "issue through a holder whose Vault does not hold the parent's Asset",
          id.tx(price5, &pl_shop, id.issue_subid(&ap, &ap, &a_id, &a_asset, Some(&id1_asset), &shop, 0, &bp, URI)), &[], id.err(IX, NOT_IN_HOLDER_VAULT));
    rej_n("N21", "issue at instruction index 0, the BurnMemo at index 1", vec![issue(0, &ap, &shop, &bp), app_burn_memo(price5, &pl_shop)],
          &[], id.err(0, MEMO_REQUIRED));
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
    let issued = |out: &Out| event(out, "SubidIssued", |r| Some((r.string()?, r.pk()?, r.pk()?, r.u32()?, r.pk()?, r.pk()?, r.u64()?)));
    let index_bump = |i: u32| Pubkey::find_program_address(&[b"subid", parent_reg.as_ref(), &i.to_le_bytes()], &id.prog).1;
    let ev = issued(&out);
    e.record("N22", "SubidIssued event (ID, Registry, parent, index 0, Asset, owner, burned); SubId Index entry has the canonical bump",
             ev == Some((shop.clone(), id.registry(&shop), parent_reg, 0, shop_asset, bp, price5))
                 && e.data(&id.subid_index(&parent_reg, 0)).is_some_and(|d| d[40] == index_bump(0)),
             &format!("{:?}", ev));
    rej_n("N10", "issue the same SubId again", id.tx(price5, &pl_shop, issue(1, &ap, &shop, &bp)), &[], id.err(IX, NOT_RECORD));
    rej0("N11", "delete an NFT SubId", id.delete_subid(&ap, &a_id, &a_asset, None, &shop), &alice, &[], NOT_RECORD);
    // the parent's controller no longer controls the issued SubId; its owner does
    rej0("N12", "parent's controller sets the target of an issued SubId",
         id.set_target(&ap, &shop, None, &shop_asset, None, Some(Target::Address(ap))), &alice, &[], NOT_CONTROLLER);
    let out = e.exec(&[id.create_subid(&bp, &bp, &shop, &shop_asset, None, "cart", None)], &bob, &[]);
    e.record("N13", "the NFT SubId's owner creates SubIds under it", out.ok() && id.reg(&format!("cart.{}", shop)).is_some(), &out.brief());
    // bob controls the NFT ID shop: he names it as the parent of alice's Record blog (only the
    // registry.parent constraints stop him)
    let n_shop = id.reg(&shop).unwrap().nft_subids_issued;
    e.reject_untouched("N23", "issue another parent's Record SubId (bob names his own NFT ID as its parent)",
                       id.tx(price5, &Id::payload("issue_subid", &bp, &blog), id.issue_subid(&bp, &bp, &shop, &shop_asset, None, &blog, n_shop, &bp, URI)),
                       &bob, &[], id.err(IX, WRONG_PARENT), &[e.ata(&bp)], &[bp]);
    rej0("S19", "delete another parent's Record SubId (bob names his own NFT ID as its parent)",
         id.delete_subid(&bp, &shop, &shop_asset, None, &blog), &bob, &[], WRONG_PARENT);
    rej0("S20", "delete a Record SubId with another Asset as the parent's", id.delete_subid(&ap, &a_id, &shop_asset, None, &blog), &alice, &[], WRONG_ASSET);
    // second issue gets index 1
    let wiki = format!("wiki.{}", a_id);
    e.must("create wiki SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "wiki", None)], &alice, &[]);
    let pl_wiki = Id::payload("issue_subid", &ap, &wiki);
    let out = e.exec(&id.tx(price5, &pl_wiki, issue(1, &ap, &wiki, &ap)), &alice, &[]);
    let p = id.reg(&a_id).unwrap();
    e.record("N14", "second NFT SubId: index 1, nft_subids_issued 2",
             out.ok() && p.nft_subids_issued == 2 && e.data(&id.subid_index(&parent_reg, 1)).is_some_and(|d| pk(&d, 8) == id.registry(&wiki)),
             &format!("{} {:?}", out.brief(), p));
    let wiki_asset = id.asset(&wiki);
    // the next index is 2; fwd is a Record with a target
    let fwd = format!("fwd.{}", a_id);
    let fwd_out = e.must("create fwd SubId with a target", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "fwd", Some(Target::Address(bp)))],
                         &alice, &[]);
    let pl_fwd = Id::payload("issue_subid", &ap, &fwd);
    // below the counter the SubId Index entry already exists, so its init fails before the index
    // check (an index rule of <= would behave the same)
    rej_n("N24", "issue with index 0, below the counter: the entry exists", id.tx(price5, &pl_fwd, issue(0, &ap, &fwd, &bp)), &[],
          id.err(IX, SYSTEM_ACCOUNT_IN_USE));
    rej_n("N25", "issue with index 3, above the counter (2)", id.tx(price5, &pl_fwd, issue(3, &ap, &fwd, &bp)), &[], id.err(IX, WRONG_SUBID_INDEX));
    let mut fake_sysvar = issue(2, &ap, &fwd, &bp);
    fake_sysvar.accounts[22].pubkey = wp;
    rej_n("N30", "issue with a wallet in place of the instructions sysvar", id.tx(price5, &pl_fwd, fake_sysvar), &[],
          id.err(IX, ANCHOR_CONSTRAINT_ADDRESS));
    rej0("S21", "delete a Record SubId through a holder whose Vault does not hold the parent's Asset",
         id.delete_subid(&ap, &a_id, &a_asset, Some(&wiki_asset), &fwd), &alice, &[], NOT_IN_HOLDER_VAULT);
    // the layout clients send: [BurnMemo, issue_subid, compute budget], memo-id at index 1
    let out = e.burn_ok("N26", "issue at instruction index 1 (memo, issue_subid, compute budget) burns exactly 69 mints",
                        vec![app_burn_memo(price5, &pl_fwd), issue(2, &ap, &fwd, &bp), cu(1_000_000)], &alice, &[], &ap, price5, 1);
    let r = id.reg(&fwd);
    let ev = issued(&out);
    e.record("N27", "an issued Record keeps its target, target_set_by (alice, no longer the controller: not valid for bob) and created_at",
             r == Some(Reg { kind: 0, asset: id.asset(&fwd), parent: parent_reg, nft_subids_issued: 0, record_subids: 0, target_set_by: ap,
                             target: Some(Target::Address(bp)), memo_id: fwd.clone() })
                 && r.as_ref().is_some_and(|r| forward(r, &bp).is_none()) && id.stamped(&fwd, &fwd_out)
                 && id.asset_data(&id.asset(&fwd)).is_some_and(|a| a.0 == bp)
                 && ev == Some((fwd.clone(), id.registry(&fwd), parent_reg, 2, id.asset(&fwd), bp, price5)),
             &format!("{:?} {:?}", r, ev));
    // anyone can send lamports to the next SubId Index entry
    let pf = format!("pf.{}", a_id);
    e.must("create pf SubId", &[id.create_subid(&ap, &ap, &a_id, &a_asset, None, "pf", None)], &alice, &[]);
    let idx3 = id.subid_index(&parent_reg, 3);
    e.must("pre-fund SubId Index entry 3", &[sys_ix::transfer(&wp, &idx3, prefund)], w, &[]);
    e.burn_ok("N28", "issue over a pre-funded SubId Index entry burns exactly 69 mints",
              id.tx(price5, &Id::payload("issue_subid", &ap, &pf), issue(3, &ap, &pf, &ap)), &alice, &[], &ap, price5, 1);
    let raw = e.raw(&[idx3]);
    e.record("N29", "the entry is topped up to its rent (no more) and names the SubId's Registry",
             raw[0].as_ref().is_some_and(|(owner, lamports, d)| *owner == id.prog && *lamports == rent_min(41) && d.len() == 41
                                         && pk(d, 8) == id.registry(&pf) && d[40] == index_bump(3)),
             &format!("{:?}", raw[0].as_ref().map(|r| (r.1, r.2.len()))));

    // ------------------------------------------------------------------ v0 transactions with the lookup table
    let alt = create_frozen_alt(&e, id.fixed_accounts());
    let frozen = e.data(&alt.key).is_some_and(|d| u64::from_le_bytes(d[4..12].try_into().unwrap()) == u64::MAX && d[22] == 0);
    e.record("A01", "lookup table with the fixed accounts, frozen (no authority)", frozen, &alt.key.to_string());
    let long_uri = format!("https://{}", "a".repeat(MAX_URI - 8));
    // a 257-byte URI fits in a v0 register: memo-id refuses it (memo-home's own limit has another number)
    let too_long_uri = format!("{}a", long_uri);
    let big3 = free_label(&id, 63, e.nonce());
    let big3_id = format!("{}.memo", big3);
    let tx = v0_tx(&e, &id.tx(price5, &Id::payload("register", &ap, &big3_id), id.register(&ap, &big3, &too_long_uri)), &alice, &[], &alt);
    reject_v0(&e, "A04", &format!("v0: register a 68-byte ID with a 257-byte URI ({} bytes of 1232)", bincode::serialize(&tx).unwrap().len()),
              &tx, id.err(IX, URI_TOO_LONG), &[a_ta], &[ap], &[id.registry(&big3_id), id.asset(&big3_id), id.vault_state(&id.asset(&big3_id))]);
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
    let tx = v0_tx(&e, &id.tx(price5, &pl, id.issue_subid(&bp, &ap, &a_id, &a_asset, None, &deep, n, &bp, &too_long_uri)), &bob, &[&alice], &alt);
    reject_v0(&e, "A05", "v0: issue a SubId with a 257-byte URI", &tx, id.err(IX, URI_TOO_LONG), &[e.ata(&bp)], &[bp],
              &[id.registry(&deep), id.registry(&a_id), id.subid_index(&id.registry(&a_id), n), id.asset(&deep)]);
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
    let ev = target_set(&out);
    e.record("T08", "TargetSet event (Registry, target, set_by)", ev == Some((id.registry(&a_id), Target::Address(ap), ap)), &format!("{:?}", ev));
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
    e.must("wiki's owner sets its target", &[id.set_target(&ap, &wiki, None, &wiki_asset, None, Some(Target::Address(bp)))], &alice, &[]);
    let out = e.exec(&[id.set_target(&ap, &wiki, None, &wiki_asset, None, None)], &alice, &[]);
    let r = id.reg(&wiki).unwrap();
    let ev = event(&out, "TargetCleared", |r| r.pk());
    e.record("T09", "clear_target on an NFT ID: no target, target_set_by zero; TargetCleared event",
             out.ok() && r.target.is_none() && r.target_set_by == Pubkey::default() && ev == Some(id.registry(&wiki)), &format!("{} {:?}", out.brief(), ev));

    // ------------------------------------------------------------------ Primary IDs
    let out = e.exec(&[id.set_primary(&ap, &ap, &a_id, Target::Address(ap), None, None)], &alice, &[]);
    let pd = id.primary_data(&Target::Address(ap));
    e.record("P01", "wallet sets its Primary ID", out.ok() && pd == Some((Target::Address(ap), id.registry(&a_id), ap)), &format!("{} {:?}", out.brief(), pd));
    let r = id.reg(&a_id).unwrap();
    e.record("P02", "verified: forward and reverse match, both set by the current controllers",
             r.target == Some(Target::Address(ap)) && r.target_set_by == ap && pd.is_some_and(|p| p.1 == id.registry(&a_id) && p.2 == ap), "");
    let bump = Pubkey::find_program_address(&[b"primary_id", &[0], ap.as_ref()], &id.prog).1;
    let ev = event(&out, "PrimarySet", |r| Some((r.target()?, r.pk()?, r.pk()?)));
    e.record("P13", "Primary ID: 106 bytes, canonical bump; PrimarySet event (target, Registry, set_by)",
             e.data(&id.primary(&Target::Address(ap))).is_some_and(|d| d.len() == 106 && d[105] == bump)
                 && ev == Some((Target::Address(ap), id.registry(&a_id), ap)),
             &format!("{:?}", ev));
    rej0("P03", "set another wallet's Primary ID", id.set_primary(&bp, &bp, &a_id, Target::Address(ap), None, None), &bob, &[], NOT_CONTROLLER);
    // a Primary ID names a Registry: an account memo-id owns (SubId Index entry 0) is not one
    let mut not_registry = id.set_primary(&ap, &ap, &a_id, Target::Address(ap), None, None);
    not_registry.accounts[2].pubkey = id.subid_index(&parent_reg, 0);
    rej0("P15", "set a Primary ID naming a SubId Index entry as the MEMO ID", not_registry, &alice, &[], ANCHOR_DISCRIMINATOR_MISMATCH);
    let out = e.exec(&[id.set_primary(&ap, &ap, &wiki, Target::Address(ap), None, None)], &alice, &[]);
    e.record("P04", "set again: the Primary ID is replaced", out.ok() && id.primary_data(&Target::Address(ap)).map(|p| p.1) == Some(id.registry(&wiki)), &out.brief());
    let out = e.exec(&[id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), Some(&shop_asset), None)], &bob, &[]);
    e.record("P05", "an Asset's owner sets its Primary ID", out.ok() && id.primary_data(&Target::Asset(shop_asset)) == Some((Target::Asset(shop_asset), id.registry(&shop), bp)), &out.brief());
    rej0("P06", "set an Asset's Primary ID as a non-owner",
         id.set_primary(&ap, &ap, &a_id, Target::Asset(shop_asset), Some(&shop_asset), None), &alice, &[], NOT_CONTROLLER);
    rej0("P07", "Asset target without its account", id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), None, None), &bob, &[], WRONG_ASSET);
    rej0("P08", "Asset target with another account", id.set_primary(&bp, &bp, &shop, Target::Asset(shop_asset), Some(&a_asset), None), &bob, &[], WRONG_ASSET);
    rej0("P11", "clear an Asset target's Primary ID without its account", id.clear_primary(&bp, &Target::Asset(shop_asset), None, None), &bob, &[], WRONG_ASSET);
    rej0("P12", "clear an Asset target's Primary ID with another account",
         id.clear_primary(&bp, &Target::Asset(shop_asset), Some(&a_asset), None), &bob, &[], WRONG_ASSET);
    rej0("P09", "clear another's Primary ID", id.clear_primary(&bp, &Target::Address(ap), None, None), &bob, &[], NOT_CONTROLLER);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.primary(&Target::Address(ap))).unwrap();
    let out = e.exec(&[id.clear_primary(&ap, &Target::Address(ap), None, None)], &alice, &[]);
    let after = e.c.get_balance(&ap).unwrap();
    e.record("P10", "clear_primary closes it, rent to the signer", out.ok() && id.primary_data(&Target::Address(ap)).is_none() && after + 5000 == before + rent,
             &format!("{} rent {}", out.brief(), rent));

    // a keypair-addressed Core Asset X: the wallet keeps key X and sells X to bob. Address(X) and
    // Asset(X) have separate Primary IDs, so neither party can replace or close the other's
    let x = Keypair::new();
    let xp = x.pubkey();
    e.must("create a keypair Core Asset for the wallet", &[core_create_plain(&xp, &wp, &wp)], w, &[&x]);
    e.must("sell it to bob", &[core_transfer(&xp, &CORE, &wp, &bp)], w, &[]);
    let out = e.exec(&[id.set_primary(&bp, &bp, &shop, Target::Asset(xp), Some(&xp), None)], &bob, &[]);
    let bob_record = Some((Target::Asset(xp), id.registry(&shop), bp));
    e.record("K01", "the buyer sets the Primary ID of a keypair Core Asset",
             out.ok() && id.primary_data(&Target::Asset(xp)) == bob_record, &out.brief());
    let out = e.exec(&[id.set_primary(&wp, &xp, &a_id, Target::Address(xp), None, None)], w, &[&x]);
    e.record("K02", "the key holder sets Address(X) at its own address; the buyer's Asset(X) record is unchanged",
             out.ok() && id.primary_data(&Target::Address(xp)) == Some((Target::Address(xp), id.registry(&a_id), xp))
                 && id.primary_data(&Target::Asset(xp)) == bob_record,
             &out.brief());
    let mut onto = id.set_primary(&wp, &xp, &a_id, Target::Address(xp), None, None);
    onto.accounts[3].pubkey = id.primary(&Target::Asset(xp));
    e.reject_untouched("K03", "the key holder writes Address(X) into the Asset(X) Primary ID", vec![onto], w, &[&x],
                       id.err(0, ANCHOR_CONSTRAINT_SEEDS), &[], &[]);
    rej0("K04", "the key holder clears the Asset(X) Primary ID", id.clear_primary(&xp, &Target::Asset(xp), Some(&xp), None), w, &[&x], NOT_CONTROLLER);
    e.record("K05", "the buyer's record survives", id.primary_data(&Target::Asset(xp)) == bob_record, "");

    // ------------------------------------------------------------------ signatures
    // every controller / signer account named with the right key but not signing (bob or the
    // wallet pays the fee): Anchor refuses it before the handler
    let unsigned = |mut ix: Instruction, k: usize| {
        ix.accounts[k].is_signer = false;
        ix
    };
    rej0("G01", "create_subid: the controller does not sign", unsigned(id.create_subid(&bp, &ap, &a_id, &a_asset, None, "g", None), 1),
         &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    rej0("G02", "delete_subid: the controller does not sign", unsigned(id.delete_subid(&ap, &a_id, &a_asset, None, &blog), 0),
         &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    let n = id.reg(&a_id).unwrap().nft_subids_issued;
    e.reject_untouched("G03", "issue_subid: the controller does not sign (bob pays)",
                       id.tx(price5, &Id::payload("issue_subid", &bp, &blog), unsigned(id.issue_subid(&bp, &ap, &a_id, &a_asset, None, &blog, n, &bp, URI), 1)),
                       &bob, &[], id.err(IX, ANCHOR_ACCOUNT_NOT_SIGNER), &[e.ata(&bp)], &[bp]);
    rej0("G04", "set_target: the controller does not sign",
         unsigned(id.set_target(&ap, &a_id, None, &a_asset, None, Some(Target::Address(bp))), 0), &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    rej0("G05", "clear_target: the controller does not sign", unsigned(id.set_target(&ap, &a_id, None, &a_asset, None, None), 0),
         &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    rej0("G06", "set_primary: the signer does not sign", unsigned(id.set_primary(&bp, &ap, &a_id, Target::Address(ap), None, None), 1),
         &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    rej0("G07", "clear_primary: the signer does not sign", unsigned(id.clear_primary(&bp, &Target::Asset(xp), Some(&xp), None), 0),
         w, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    rej0("G08", "update_uri: the controller does not sign", unsigned(id.update_uri(&bp, &ap, &a_id, &a_asset, None, URI), 1),
         &bob, &[], ANCHOR_ACCOUNT_NOT_SIGNER);
    let before = e.c.get_balance(&bp).unwrap();
    let rent = e.c.get_balance(&id.primary(&Target::Asset(xp))).unwrap();
    let out = e.exec(&[id.clear_primary(&bp, &Target::Asset(xp), Some(&xp), None)], &bob, &[]);
    let ev = event(&out, "PrimaryCleared", |r| r.target());
    e.record("P14", "the buyer clears the Asset(X) Primary ID without a holder: closed, rent to the signer; PrimaryCleared event",
             out.ok() && id.primary_data(&Target::Asset(xp)).is_none() && e.c.get_balance(&bp).unwrap() + 5000 == before + rent
                 && ev == Some(Target::Asset(xp)),
             &format!("{} {:?}", out.brief(), ev));

    // ------------------------------------------------------------------ transfer and the Vault rule
    // bob points his NFT SubId at himself (his Primary ID for it is P05), then it moves into the
    // Vault of alice's MEMO ID Asset: alice controls it through the holder
    let a_vault = id.vault(&a_asset);
    e.must("bob sets shop's target", &[id.set_target(&bp, &shop, None, &shop_asset, None, Some(Target::Address(bp)))], &bob, &[]);
    e.must("move shop into alice's Vault", &[core_transfer(&shop_asset, &id.collection, &bp, &a_vault)], &bob, &[]);
    let r = id.reg(&shop).unwrap();
    let pd = id.primary_data(&Target::Asset(shop_asset));
    e.record("V01", "after the transfer bob's records are unchanged but lapse: valid only for bob, who is no longer the controller (the Vault, or alice through it)",
             r.target == Some(Target::Address(bp)) && r.target_set_by == bp && forward(&r, &bp) == Some(Target::Address(bp))
                 && forward(&r, &a_vault).is_none() && forward(&r, &ap).is_none()
                 && id.asset_data(&shop_asset).is_some_and(|a| a.0 == a_vault)
                 && pd == Some((Target::Asset(shop_asset), id.registry(&shop), bp)),
             &format!("{:?} {:?}", r, pd));
    rej0("V02", "the old owner can no longer set the target", id.set_target(&bp, &shop, None, &shop_asset, None, Some(Target::Address(bp))), &bob, &[], NOT_CONTROLLER);
    rej0("V03", "without the holder, the controller is the Vault (nobody can sign)",
         id.set_target(&ap, &shop, None, &shop_asset, None, Some(Target::Address(ap))), &alice, &[], NOT_CONTROLLER);
    rej0("V04", "a holder whose Vault does not hold the Asset",
         id.set_target(&ap, &shop, None, &shop_asset, Some(&wiki_asset), Some(Target::Address(ap))), &alice, &[], NOT_IN_HOLDER_VAULT);
    let out = e.exec(&[id.set_target(&ap, &shop, None, &shop_asset, Some(&a_asset), Some(Target::Address(ap)))], &alice, &[]);
    e.record("V05", "the holder's owner controls the held MEMO ID", out.ok() && id.reg(&shop).is_some_and(|r| r.target_set_by == ap), &out.brief());
    let out = e.exec(&[id.create_subid(&ap, &ap, &shop, &shop_asset, Some(&a_asset), "gift", None)], &alice, &[]);
    e.record("V06", "and creates SubIds under it", out.ok() && id.reg(&format!("gift.{}", shop)).is_some(), &out.brief());
    let out = e.exec(&[id.set_primary(&ap, &ap, &a_id, Target::Asset(shop_asset), Some(&shop_asset), Some(&a_asset))], &alice, &[]);
    e.record("V07", "and sets the held Asset's Primary ID", out.ok() && id.primary_data(&Target::Asset(shop_asset)).is_some_and(|p| p.2 == ap), &out.brief());
    let gift = format!("gift.{}", shop);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.registry(&gift)).unwrap();
    let out = e.exec(&[id.delete_subid(&ap, &shop, &shop_asset, Some(&a_asset), &gift)], &alice, &[]);
    e.record("V08", "through the holder: delete a Record SubId, rent to the holder's owner",
             out.ok() && id.reg(&gift).is_none() && e.c.get_balance(&ap).unwrap() + 5000 == before + rent, &out.brief());
    let out = e.exec(&[cu(400_000), id.update_uri(&ap, &ap, &shop, &shop_asset, Some(&a_asset), "ar://held")], &alice, &[]);
    e.record("V09", "through the holder: update_uri", out.ok() && id.asset_data(&shop_asset).is_some_and(|a| a.2 == shop && a.3 == "ar://held"), &out.brief());
    let held_sub = format!("held.{}", shop);
    e.must("create a Record SubId through the holder", &[id.create_subid(&ap, &ap, &shop, &shop_asset, Some(&a_asset), "held", None)], &alice, &[]);
    let n = id.reg(&shop).unwrap().nft_subids_issued;
    let pl_held = Id::payload("issue_subid", &ap, &held_sub);
    let ix = id.issue_subid(&ap, &ap, &shop, &shop_asset, Some(&a_asset), &held_sub, n, &ap, URI);
    e.burn_ok("V10", "through the holder: issue an NFT SubId (burns 69 mints)", id.tx(price5, &pl_held, ix), &alice, &[], &ap, price5, 1);
    let before = e.c.get_balance(&ap).unwrap();
    let rent = e.c.get_balance(&id.primary(&Target::Asset(shop_asset))).unwrap();
    let out = e.exec(&[id.clear_primary(&ap, &Target::Asset(shop_asset), Some(&shop_asset), Some(&a_asset))], &alice, &[]);
    e.record("V11", "through the holder: clear the held Asset's Primary ID, rent to the signer",
             out.ok() && id.primary_data(&Target::Asset(shop_asset)).is_none() && e.c.get_balance(&ap).unwrap() + 5000 == before + rent,
             &out.brief());
    let out = e.exec(&[id.set_target(&ap, &shop, None, &shop_asset, Some(&a_asset), None)], &alice, &[]);
    e.record("V15", "through the holder: clear_target", out.ok() && id.reg(&shop).is_some_and(|r| r.target.is_none() && r.target_set_by == Pubkey::default()),
             &out.brief());
    let rec = format!("rec.{}", shop);
    e.must("create a Record SubId through the holder", &[id.create_subid(&ap, &ap, &shop, &shop_asset, Some(&a_asset), "rec", None)], &alice, &[]);
    let out = e.exec(&[id.set_target(&ap, &rec, Some(&shop), &shop_asset, Some(&a_asset), Some(Target::Asset(a_asset)))], &alice, &[]);
    e.record("V16", "through the holder: set a Record SubId's target, stamped by the holder's owner",
             out.ok() && id.reg(&rec).is_some_and(|r| r.target == Some(Target::Asset(a_asset)) && r.target_set_by == ap), &out.brief());

    // a sale of alice's ID: everything alice set lapses for bob
    e.must("transfer alice's ID to bob", &[core_transfer(&a_asset, &id.collection, &ap, &bp)], &alice, &[]);
    let r = id.reg(&a_id).unwrap();
    e.record("V12", "after a sale the forward record (alice's, T01) is kept but not valid for the buyer",
             r.target == Some(Target::Address(ap)) && r.target_set_by == ap && forward(&r, &bp).is_none()
                 && id.asset_data(&a_asset).is_some_and(|a| a.0 == bp),
             &format!("{:?}", r));
    let out = e.exec(&[id.set_target(&bp, &blog, Some(&a_id), &a_asset, None, Some(Target::Address(bp)))], &bob, &[]);
    e.record("V13", "the buyer controls the Record SubIds", out.ok() && id.reg(&blog).is_some_and(|r| r.target_set_by == bp), &out.brief());
    rej0("V14", "the seller no longer does", id.set_target(&ap, &blog, Some(&a_id), &a_asset, None, None), &alice, &[], NOT_CONTROLLER);

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
    // memo-home would refuse another Collection too, with its own number (6004)
    let mut other_col = id.update_uri(&bp, &bp, &a_id, &a_asset, None, URI);
    other_col.accounts[7].pubkey = wrong_col;
    rej0("U08", "update with another Collection account", other_col, &bob, &[], WRONG_COLLECTION);
    rej0("U09", "update to a URI with https:// after another scheme", id.update_uri(&bp, &bp, &a_id, &a_asset, None, "javascript:alert(1)//https://x"),
         &bob, &[], INVALID_URI);
    // memo-id's Authority signs update_asset: with another program in its place, that program would get the signature
    let mut fake_home = id.update_uri(&bp, &bp, &a_id, &a_asset, None, URI);
    fake_home.accounts[10].pubkey = spl_memo::ID;
    rej0("U10", "update with SPL Memo in place of memo-home", fake_home, &bob, &[], ANCHOR_INVALID_PROGRAM_ID);

    e.finish();
}

fn ata_of_mint(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address_with_program_id(owner, mint, &t22())
}
