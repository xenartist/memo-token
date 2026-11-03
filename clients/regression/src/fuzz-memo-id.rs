//! Stateful model-based fuzzer for memo-id (local validator only).
//!
//! Sends random sequences of memo-id instructions, Core transfers and memo-home withdraw_asset
//! from four funded wallets (the fourth holds MEMO for two burns only) over a small pool of
//! labels, so names collide, Record SubIds are deleted and re-created, and IDs move between
//! wallets and into and out of the Vaults of MEMO ID Assets and of a plain Core Asset X (whose
//! key xk also signs; a Core-owned account cannot pay fees, so an actor always pays). Every
//! operation has valid and deliberately invalid variants: wrong or stale signer, missing or wrong
//! holder or holder Collection Config, wrong parent, Asset or index, an unsigned controller, a bad
//! label, URI or BurnMemo, a payer short of MEMO. Lamports are also sent to the PDAs of accounts
//! before they exist (Registry, SubId Index entry, Primary ID, Asset, Vault State), Assets are
//! given to themselves or to their own Vault, and owners give a TransferDelegate to someone else
//! and revoke it (a listed Asset: issue_subid through it, and withdrawals from its Vault, fail).
//!
//! A model of the expected state predicts each step: success, or the exact error (instruction
//! index, code, the program of the innermost failing frame and the Anchor name), in the order the
//! checks run (account deserialization, init, constraints, handler, CPIs). After every step the
//! model is compared with every account the run has named: each Registry byte for byte (its
//! fixed-size encoding, holder included), Primary ID, SubId
//! Index entry and Vault State, the Core owner, Collection, name and URI of each Asset, the
//! lamports of every memo-id account (its rent, or more when pre-funded), the MEMO balances,
//! supply and burn statistics, the lamports paid back by closes, and the events. Every accepted
//! step must be signed by the controller the spec gives (the one-level Vault rule, the holder
//! found by scanning the known Assets rather than taken from the client). Invariants are also
//! computed from the chain alone (counters, SubId Index completeness, the name / Registry / Asset
//! binding both ways), and the verified pairs computed from the chain (forward record valid for
//! the current controller, Primary ID valid for the Target's current controller) are compared
//! with those implied by the model's history of who signed what; a pair verified from accounts
//! read by address only (the controller re-derived through the recorded holder) must be among
//! them. A mismatch prints the earlier
//! steps that touched the accounts involved; the model then adopts the chain's state and the run
//! continues.
//!
//! Actors, labels and choices follow from --seed (and --tag, which separates runs on one ledger):
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin fuzz-memo-id -- --seed 1 --steps 500 [--tag b] [--edge] [--stop-on-mismatch]
//! --edge biases the choices towards the 253-byte limit (63-byte labels on the deepest IDs) and
//! towards SubIds issued to their own Asset or Vault (refused by memo-home), and one step in ten
//! is a step down that path (create a 63-byte label under the deepest ID an actor controls, or
//! issue it).
//! Exits with status 1 if any step mismatched.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::{engine::general_purpose, Engine as _};
use harness::*;
use memo_token_client::{get_program_id, memo_id::MemoIdAddresses};
use rand::{rngs::StdRng, Rng, SeedableRng};
use sha2::{Digest, Sha256};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::instruction as alt_ix;
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    signer::keypair::keypair_from_seed,
    transaction::VersionedTransaction,
};
use solana_client::rpc_config::RpcSendTransactionConfig;
use spl_associated_token_account::instruction::create_associated_token_account;

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const NFT: u8 = 0;
const RECORD: u8 = 1;
const URI: &str = "https://example.com/memo-id.json";
const FEE_PER_SIGNATURE: u64 = 5_000;

// memo-id error codes
const INVALID_LABEL: u32 = 6000;
const MEMO_ID_TOO_LONG: u32 = 6001;
const INVALID_URI: u32 = 6003;
const URI_TOO_LONG: u32 = 6004;
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
const USER_PUBKEY_MISMATCH: u32 = 6023;
const MEMO_ID_MISMATCH: u32 = 6024;
const HOLDER_NOT_HOME_ASSET: u32 = 6026;
const LISTED: u32 = 6027;
const ERROR_NAMES: [&str; 29] = [
    "InvalidLabel", "MemoIdTooLong", "HashMismatch", "InvalidUri", "UriTooLong", "WrongCollection", "InvalidMint",
    "WrongAsset", "NotController", "NotInHolderVault", "ParentNotNft", "NotNft", "NotRecord", "WrongParent",
    "WrongSubidIndex", "MemoRequired", "MemoTooShort", "InvalidMemoFormat", "UnsupportedMemoVersion",
    "BurnAmountMismatch", "UnsupportedPayloadVersion", "InvalidCategory", "InvalidOperation", "UserPubkeyMismatch",
    "MemoIdMismatch", "ArithmeticOverflow", "HolderNotHomeAsset", "Listed", "InvalidTargetKind",
];
// memo-home error codes (raised in memo-home's frame, or by its helpers inside memo-id's)
const HOME_WRONG_COLLECTION: u32 = 6004;
const HOME_INVALID_CORE_ACCOUNT: u32 = 6009;
const HOME_NOT_OWNER: u32 = 6010;
const HOME_LISTED: u32 = 6011;
const HOME_NOT_IN_VAULT: u32 = 6012;
const HOME_INVALID_OWNER: u32 = 6020;
const HOME_INVALID_DESTINATION: u32 = 6024;
// Anchor account errors
const A_CONSTRAINT_SEEDS: u32 = 2006;
const A_DISCRIMINATOR_MISMATCH: u32 = 3002;
const A_OWNED_BY_WRONG_PROGRAM: u32 = 3007;
const A_NOT_SIGNER: u32 = 3010;
const A_NOT_INITIALIZED: u32 = 3012;
// Metaplex Core: no plugin or owner approved the transfer
const CORE_NO_APPROVALS: u32 = 26;
/// Registry size without the memo_id bytes, and the Primary ID size
const REG_BASE: usize = 192;
const PRIM_LEN: usize = 138;

/// memo-id's events and memo-home's AssetWithdrawn, by name (for the "Program data:" logs)
const ID_EVENTS: [&str; 10] = [
    "Initialized", "Registered", "SubidCreated", "SubidDeleted", "SubidIssued", "TargetSet", "TargetCleared",
    "PrimarySet", "PrimaryCleared", "UriUpdated",
];

/// Second-level label suffixes (after a per-run prefix, so every label has 5+ bytes: 69 mints)
const POOL: [&str; 8] = ["alpha", "bravo", "cat", "dog", "eel", "fox", "gnu", "hen"];
/// SubId labels (a small pool, so SubIds are deleted and re-created); a 63-byte one is added rarely
const SUB_LABELS: [&str; 7] = ["www", "a", "blog", "x-1", "pay", "z9", "a-xn--b"];

fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}
fn disc8(kind: &str, name: &str) -> [u8; 8] { Sha256::digest(format!("{}:{}", kind, name).as_bytes())[..8].try_into().unwrap() }

/// Independent reference of the label rule (MEMO-ID-DESIGN.md §2): 1-63 bytes of a-z, 0-9, '-',
/// no '-' at either end, no "xn--" prefix, not the suffix itself
fn valid_label(l: &str) -> bool {
    !l.is_empty() && l != "memo" && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && !l.starts_with("xn--")
        && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn valid_memo_id(id: &str) -> bool {
    match id.strip_suffix(".memo") {
        Some(labels) => id.len() <= 253 && labels.split('.').all(valid_label),
        None => false,
    }
}
/// Price of a second-level ID by label length (design §4), in centi-mints
fn price_centi(len: usize) -> u64 {
    match len {
        1 => 42_069_000,
        2 => 4_206_900,
        3 => 420_690,
        4 => 42_069,
        _ => 6_900,
    }
}
fn bad_label(i: usize) -> String {
    ["Bad", "-ab", "ab-", "xn--ab", "memo", "a_b", "", "www.blog"].get(i).map(|s| s.to_string()).unwrap_or_else(|| "a".repeat(64))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Target {
    Address(Pubkey),
    Asset(Pubkey),
}
impl Target {
    fn key(&self) -> Pubkey { match self { Target::Address(k) | Target::Asset(k) => *k } }
    fn kind(&self) -> u8 { match self { Target::Address(_) => 0, Target::Asset(_) => 1 } }
    fn encode(&self, v: &mut Vec<u8>) {
        v.push(self.kind());
        v.extend(self.key().as_ref());
    }
    fn flip(&self) -> Target { match self { Target::Address(k) => Target::Asset(*k), Target::Asset(k) => Target::Address(*k) } }
}

/// Borsh reader
struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> { Some(self.take(1)?[0]) }
    fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    fn i64(&mut self) -> Option<i64> { Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
    fn pk(&mut self) -> Option<Pubkey> { Some(Pubkey::new_from_array(self.take(32)?.try_into().ok()?)) }
    fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn target(&mut self) -> Option<Target> {
        match self.u8()? {
            0 => Some(Target::Address(self.pk()?)),
            1 => Some(Target::Asset(self.pk()?)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------------------------

/// A Registry as the program stores it
#[derive(Clone, Debug, PartialEq)]
struct MReg {
    kind: u8,
    asset: Pubkey,
    parent: Pubkey,
    created_at: i64,
    nft: u32,
    rec: u32,
    bump: u8,
    set_by: Pubkey,
    /// target_holder
    holder: Pubkey,
    target: Option<Target>,
    memo_id: String,
}

impl MReg {
    /// Anchor's serialization (discriminator and Borsh fields), which fills the account exactly:
    /// without a target, set_by, holder and the target are all zero
    fn bytes(&self) -> Vec<u8> {
        let mut v = disc8("account", "Registry").to_vec();
        v.push(self.kind);
        v.extend(self.asset.as_ref());
        v.extend(self.parent.as_ref());
        v.extend(self.created_at.to_le_bytes());
        v.extend(self.nft.to_le_bytes());
        v.extend(self.rec.to_le_bytes());
        v.push(self.bump);
        v.extend(self.set_by.as_ref());
        v.extend(self.holder.as_ref());
        match self.target {
            None => v.extend([0u8; 34]),
            Some(t) => {
                v.push(1);
                t.encode(&mut v);
            }
        }
        bstr(&mut v, &self.memo_id);
        v
    }
    fn set_forward(&mut self, target: Option<Target>, ctrl: Pubkey, holder: Option<Pubkey>) {
        self.target = target;
        self.set_by = if target.is_some() { ctrl } else { Pubkey::default() };
        self.holder = if target.is_some() { holder.unwrap_or_default() } else { Pubkey::default() };
    }
    fn parse(d: &[u8]) -> Option<MReg> {
        if d.get(..8)? != disc8("account", "Registry") {
            return None;
        }
        let mut r = Rd { d, p: 8 };
        Some(MReg {
            kind: r.u8()?,
            asset: r.pk()?,
            parent: r.pk()?,
            created_at: r.i64()?,
            nft: r.u32()?,
            rec: r.u32()?,
            bump: r.u8()?,
            set_by: r.pk()?,
            holder: r.pk()?,
            target: match (r.u8()?, r.target()?) {
                (0, _) => None,
                (1, t) => Some(t),
                _ => return None,
            },
            memo_id: r.string()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct MAsset {
    owner: Pubkey,
    col: Option<Pubkey>,
    name: String,
    uri: String,
}

/// Core AssetV1: owner, Collection, name, URI
fn parse_core_asset(d: &[u8]) -> Option<MAsset> {
    if *d.first()? != 1 {
        return None;
    }
    let mut r = Rd { d, p: 1 };
    let owner = r.pk()?;
    let col = match r.u8()? {
        0 => None,
        1 => {
            r.pk()?;
            None
        }
        2 => Some(r.pk()?),
        _ => return None,
    };
    Some(MAsset { owner, col, name: r.string()?, uri: r.string()? })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MPrim {
    target: Target,
    registry: Pubkey,
    set_by: Pubkey,
    set_holder: Pubkey,
    bump: u8,
}
impl MPrim {
    fn bytes(&self) -> Vec<u8> {
        let mut v = disc8("account", "PrimaryId").to_vec();
        self.target.encode(&mut v);
        v.extend(self.registry.as_ref());
        v.extend(self.set_by.as_ref());
        v.extend(self.set_holder.as_ref());
        v.push(self.bump);
        v
    }
    fn parse(d: &[u8]) -> Option<MPrim> {
        if d.get(..8)? != disc8("account", "PrimaryId") {
            return None;
        }
        let mut r = Rd { d, p: 8 };
        Some(MPrim { target: r.target()?, registry: r.pk()?, set_by: r.pk()?, set_holder: r.pk()?, bump: r.u8()? })
    }
}

fn idx_bytes(registry: &Pubkey, bump: u8) -> Vec<u8> {
    let mut v = disc8("account", "SubidIndex").to_vec();
    v.extend(registry.as_ref());
    v.push(bump);
    v
}
fn vstate_bytes(nonce: u64, bump: u8) -> Vec<u8> {
    let mut v = disc8("account", "VaultState").to_vec();
    v.extend(nonce.to_le_bytes());
    v.push(bump);
    v
}

#[derive(Default)]
struct Model {
    /// existing Registries: the record and the full account data
    regs: BTreeMap<Pubkey, (MReg, Vec<u8>)>,
    /// every Registry address the run has named, with its name (checked to exist only if in regs)
    names: BTreeMap<Pubkey, String>,
    /// existing Primary IDs by address
    prims: BTreeMap<Pubkey, MPrim>,
    /// every Target the run has named
    targets: BTreeSet<Target>,
    /// SubId Index entries by address: (parent, index, registry, bump)
    idx: BTreeMap<Pubkey, (Pubkey, u32, Pubkey, u8)>,
    /// Core Assets (MEMO ID Assets and the plain Asset X)
    assets: BTreeMap<Pubkey, MAsset>,
    /// memo-home Vault States: (nonce, bump)
    vstate: BTreeMap<Pubkey, (u64, u8)>,
    /// lamports at an address other than the rent the model gives it: pre-funded addresses before
    /// their account exists, and memo-id accounts created over more than their rent
    funded: BTreeMap<Pubkey, u64>,
    /// the owner before the last transfer or withdrawal of an Asset (the stale controller)
    prev_owner: BTreeMap<Pubkey, Pubkey>,
    bal: BTreeMap<Pubkey, u64>,
    stats: BTreeMap<Pubkey, UserStats>,
    supply: u64,
    global: GlobalStats,
    /// history: who signed the forward record now stored (Registry -> (target, signer))
    fwd_hist: BTreeMap<Pubkey, (Target, Pubkey)>,
    /// history: who signed the Primary ID now stored (Target -> (registry, signer))
    prim_hist: BTreeMap<Target, (Pubkey, Pubkey)>,
    /// TransferDelegate plugins: Some(d) held by d (the Asset is listed), None held by the owner
    del: BTreeMap<Pubkey, Option<Pubkey>>,
}

// ---------------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------------

/// The BurnMemo at index 0: as required, or one field wrong, or placed after the instruction
#[derive(Clone, Debug)]
enum MemoV {
    Ok,
    Amount(i64),
    User(Pubkey),
    Id(String),
    Late,
}

#[derive(Clone, Debug)]
enum RegRef {
    Name(String),
    Acct(Pubkey),
}

/// A TransferDelegate change by an Asset's owner: add it held by someone else, give it to
/// someone else (approve), or take it back (revoke)
#[derive(Clone, Copy, Debug)]
enum DelOp {
    Add(Pubkey),
    Approve(Pubkey),
    Revoke,
}

/// Every action that takes a holder also takes `hcfg`, the holder_config account (the holder's
/// Collection Config when right; missing, or another account, when not)
#[derive(Clone, Debug)]
enum Act {
    Register { payer: Pubkey, label: String, uri: String, memo: MemoV, cu_last: bool },
    CreateSubid { payer: Pubkey, ctrl: Pubkey, signs: bool, parent: String, parent_asset: Pubkey, holder: Option<Pubkey>, hcfg: Option<Pubkey>,
                  label: String, target: Option<Target> },
    DeleteSubid { fee: Pubkey, ctrl: Pubkey, signs: bool, parent: String, parent_asset: Pubkey, holder: Option<Pubkey>, hcfg: Option<Pubkey>,
                  name: String },
    IssueSubid { payer: Pubkey, ctrl: Pubkey, signs: bool, parent: String, parent_asset: Pubkey, holder: Option<Pubkey>, hcfg: Option<Pubkey>,
                 name: String, index: u32, new_owner: Pubkey, uri: String, memo: MemoV, cu_last: bool },
    /// set_target, or clear_target when `target` is None
    SetTarget { fee: Pubkey, ctrl: Pubkey, signs: bool, name: String, parent: Option<String>, asset: Pubkey, holder: Option<Pubkey>,
                hcfg: Option<Pubkey>, target: Option<Target> },
    SetPrimary { payer: Pubkey, signer: Pubkey, signs: bool, reg: RegRef, target: Target, account: Pubkey, target_asset: Option<Pubkey>,
                 holder: Option<Pubkey>, hcfg: Option<Pubkey> },
    ClearPrimary { fee: Pubkey, signer: Pubkey, signs: bool, target: Target, target_asset: Option<Pubkey>, holder: Option<Pubkey>,
                   hcfg: Option<Pubkey> },
    UpdateUri { payer: Pubkey, ctrl: Pubkey, signs: bool, name: String, asset: Pubkey, holder: Option<Pubkey>, hcfg: Option<Pubkey>, uri: String },
    /// Core AddPluginV1 / ApprovePluginAuthorityV1 / RevokePluginAuthorityV1 of a TransferDelegate, by the owner
    Delegate { owner: Pubkey, asset: Pubkey, op: DelOp },
    /// Core TransferV1 authorized by `signer` (the owner, or someone who is not); `payer` pays
    Transfer { payer: Pubkey, signer: Pubkey, asset: Pubkey, new_owner: Pubkey },
    /// memo-home withdraw_asset of `inner` from the Vault of `holder`, signed by `owner`
    Withdraw { payer: Pubkey, owner: Pubkey, holder: Pubkey, inner: Pubkey, inner_col: Pubkey, new_owner: Pubkey },
    /// System transfer of lamports to an address (the PDA of an account that does not exist yet)
    Prefund { payer: Pubkey, addr: Pubkey, lamports: u64 },
}

impl Act {
    fn op(&self) -> &'static str {
        match self {
            Act::Register { .. } => "register",
            Act::CreateSubid { .. } => "create_subid",
            Act::DeleteSubid { .. } => "delete_subid",
            Act::IssueSubid { .. } => "issue_subid",
            Act::SetTarget { target: Some(_), .. } => "set_target",
            Act::SetTarget { .. } => "clear_target",
            Act::SetPrimary { .. } => "set_primary",
            Act::ClearPrimary { .. } => "clear_primary",
            Act::UpdateUri { .. } => "update_uri",
            Act::Transfer { .. } => "core_transfer",
            Act::Withdraw { .. } => "withdraw_asset",
            Act::Prefund { .. } => "prefund",
            Act::Delegate { op: DelOp::Add(_), .. } => "add_delegate",
            Act::Delegate { op: DelOp::Approve(_), .. } => "approve_delegate",
            Act::Delegate { op: DelOp::Revoke, .. } => "revoke_delegate",
        }
    }
    /// Fee payer and the other signers
    fn signers(&self) -> (Pubkey, Vec<Pubkey>) {
        let cond = |signs: bool, k: &Pubkey| if signs { vec![*k] } else { vec![] };
        match self {
            Act::Register { payer, .. } => (*payer, vec![]),
            Act::CreateSubid { payer, ctrl, signs, .. } | Act::IssueSubid { payer, ctrl, signs, .. } | Act::UpdateUri { payer, ctrl, signs, .. } => {
                (*payer, cond(*signs, ctrl))
            }
            Act::DeleteSubid { fee, ctrl, signs, .. } => (*fee, cond(*signs, ctrl)),
            Act::SetTarget { fee, ctrl, signs, .. } => (*fee, cond(*signs, ctrl)),
            Act::SetPrimary { payer, signer, signs, .. } => (*payer, cond(*signs, signer)),
            Act::ClearPrimary { fee, signer, signs, .. } => (*fee, cond(*signs, signer)),
            Act::Transfer { payer, signer, .. } => (*payer, vec![*signer]),
            Act::Withdraw { payer, owner, .. } => (*payer, vec![*owner]),
            Act::Prefund { payer, .. } => (*payer, vec![]),
            Act::Delegate { owner, .. } => (*owner, vec![]),
        }
    }
}

/// An expected failure: the code, the program of the innermost failing frame, and the Anchor
/// error name (None for System, Token-2022 and Core, which log none)
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fail {
    code: u32,
    prog: Pubkey,
    name: Option<&'static str>,
}

/// What a step that succeeds does, as predicted
#[derive(Clone, Copy, Debug, Default)]
struct Pred {
    price: u64,
    ctrl: Pubkey,
}

type Event = (&'static str, Vec<u8>);

// ---------------------------------------------------------------------------------------------
// Fuzzer
// ---------------------------------------------------------------------------------------------

struct Fz<'a> {
    e: &'a Env,
    id: Pubkey,
    home: Pubkey,
    authority: Pubkey,
    home_authority: Pubkey,
    burn_guard: Pubkey,
    collection: Pubkey,
    config: Pubkey,
    rng: StdRng,
    edge: bool,
    actors: Vec<Keypair>,
    xk: Keypair,
    pool: Vec<String>,
    alt: Option<AddressLookupTableAccount>,
    m: Model,
    pda_cache: RefCell<HashMap<Vec<u8>, (Pubkey, u8)>>,
    rent_cache: RefCell<HashMap<usize, u64>>,
    /// Vault address -> its Asset, for every known Core Asset
    vault_rev: HashMap<Pubkey, Pubkey>,
    trace: Vec<(usize, String, BTreeSet<Pubkey>)>,
    step: usize,
    hist: BTreeMap<String, usize>,
    mismatches: usize,
    skipped: usize,
    verified_seen: usize,
    /// pairs verified from accounts read by address only (through the recorded holders)
    addr_seen: usize,
    verified_max: usize,
}

impl<'a> Fz<'a> {
    fn new(e: &'a Env, seed: u64, tag: &str, edge: bool) -> Fz<'a> {
        let id = get_program_id("memo_id").expect("memo_id in Anchor.toml");
        let home = get_program_id("memo_home").expect("memo_home in Anchor.toml");
        let authority = Pubkey::find_program_address(&[b"authority"], &id).0;
        let collection = Pubkey::find_program_address(&[b"collection", authority.as_ref(), &sha(b"memo")], &home).0;
        let kp = |n: &str| keypair_from_seed(&sha(format!("fuzz-memo-id/{}/{}/{}", seed, tag, n).as_bytes())).unwrap();
        let prefix = format!("f{}{}", base36(seed), tag);
        Fz {
            e,
            id,
            home,
            authority,
            home_authority: Pubkey::find_program_address(&[b"authority"], &home).0,
            burn_guard: Pubkey::find_program_address(&[b"burn_guard"], &home).0,
            collection,
            config: Pubkey::find_program_address(&[b"collection_config", collection.as_ref()], &home).0,
            rng: StdRng::seed_from_u64(seed),
            edge,
            actors: (0..4).map(|i| kp(&format!("a{}", i))).collect(),
            xk: kp("xk"),
            pool: POOL.iter().map(|s| format!("{}{}", prefix, s))
                // a 4-byte label (420.69 mints, rounded up where the reward is a few units)
                .chain((prefix.len() <= 4).then(|| format!("{:q<4}", prefix))).collect(),
            alt: None,
            m: Model::default(),
            pda_cache: RefCell::new(HashMap::new()),
            rent_cache: RefCell::new(HashMap::new()),
            vault_rev: HashMap::new(),
            trace: vec![],
            step: 0,
            hist: BTreeMap::new(),
            mismatches: 0,
            skipped: 0,
            verified_seen: 0,
            addr_seen: 0,
            verified_max: 0,
        }
    }

    // ---------------- addresses ----------------
    fn pda(&self, prog: &Pubkey, seeds: &[&[u8]]) -> (Pubkey, u8) {
        let mut key = prog.to_bytes().to_vec();
        for s in seeds {
            key.extend((s.len() as u32).to_le_bytes());
            key.extend(*s);
        }
        if let Some(v) = self.pda_cache.borrow().get(&key) {
            return *v;
        }
        let v = Pubkey::find_program_address(seeds, prog);
        self.pda_cache.borrow_mut().insert(key, v);
        v
    }
    fn reg_addr(&self, name: &str) -> Pubkey { self.pda(&self.id, &[b"memo_id", &sha(name.as_bytes())]).0 }
    fn reg_bump(&self, name: &str) -> u8 { self.pda(&self.id, &[b"memo_id", &sha(name.as_bytes())]).1 }
    fn asset_addr(&self, name: &str) -> Pubkey { self.pda(&self.home, &[b"asset", self.collection.as_ref(), &sha(name.as_bytes())]).0 }
    fn vault(&self, asset: &Pubkey) -> Pubkey { self.pda(&self.home, &[b"vault", asset.as_ref()]).0 }
    fn vstate_addr(&self, asset: &Pubkey) -> (Pubkey, u8) { self.pda(&self.home, &[b"vault_state", asset.as_ref()]) }
    fn idx_addr(&self, parent: &Pubkey, i: u32) -> (Pubkey, u8) { self.pda(&self.id, &[b"subid", parent.as_ref(), &i.to_le_bytes()]) }
    fn prim_addr(&self, t: &Target) -> (Pubkey, u8) { self.pda(&self.id, &[b"primary_id", &[t.kind()], t.key().as_ref()]) }
    fn stats_pda(&self, k: &Pubkey) -> Pubkey { self.pda(&self.e.burn_prog, &[b"user_global_burn_stats", k.as_ref()]).0 }
    fn rent(&self, len: usize) -> u64 {
        if let Some(v) = self.rent_cache.borrow().get(&len) {
            return *v;
        }
        let v = self.e.c.get_minimum_balance_for_rent_exemption(len).expect("rent");
        self.rent_cache.borrow_mut().insert(len, v);
        v
    }
    fn x(&self) -> Pubkey { self.xk.pubkey() }

    // ---------------- failures ----------------
    fn idf(&self, code: u32) -> Fail { Fail { code, prog: self.id, name: Some(ERROR_NAMES[(code - 6000) as usize]) } }
    fn anchor(&self, prog: Pubkey, code: u32) -> Fail {
        let name = match code {
            A_CONSTRAINT_SEEDS => "ConstraintSeeds",
            A_DISCRIMINATOR_MISMATCH => "AccountDiscriminatorMismatch",
            A_OWNED_BY_WRONG_PROGRAM => "AccountOwnedByWrongProgram",
            A_NOT_SIGNER => "AccountNotSigner",
            A_NOT_INITIALIZED => "AccountNotInitialized",
            _ => "?",
        };
        Fail { code, prog, name: Some(name) }
    }
    fn homef(&self, prog: Pubkey, code: u32) -> Fail {
        let name = match code {
            HOME_WRONG_COLLECTION => "WrongCollection",
            HOME_INVALID_CORE_ACCOUNT => "InvalidCoreAccount",
            HOME_NOT_OWNER => "NotOwner",
            HOME_NOT_IN_VAULT => "NotInVault",
            HOME_INVALID_OWNER => "InvalidOwner",
            HOME_LISTED => "Listed",
            HOME_INVALID_DESTINATION => "InvalidDestination",
            _ => "?",
        };
        Fail { code, prog, name: Some(name) }
    }
    fn sys_in_use(&self) -> Fail { Fail { code: 0, prog: system_program(), name: None } }
    fn token_insufficient(&self) -> Fail { Fail { code: 1, prog: t22(), name: None } }

    fn describe_fail(&self, f: &Fail) -> String {
        let prog = if f.prog == self.id { "memo-id".into() } else if f.prog == self.home { "memo-home".into() }
                   else if f.prog == CORE { "Core".into() } else if f.prog == system_program() { "System".into() }
                   else if f.prog == t22() { "Token-2022".into() } else { f.prog.to_string() };
        format!("{}({}) by {}", f.name.unwrap_or("-"), f.code, prog)
    }

    // ---------------- model queries ----------------
    fn signable(&self, k: &Pubkey) -> bool { *k == self.x() || self.actors.iter().any(|a| a.pubkey() == *k) }
    fn kp(&self, k: &Pubkey) -> &Keypair {
        if *k == self.x() {
            return &self.xk;
        }
        self.actors.iter().find(|a| a.pubkey() == *k).unwrap_or_else(|| panic!("no keypair for {}", k))
    }
    fn reg(&self, name: &str) -> Option<&MReg> { self.m.regs.get(&self.reg_addr(name)).map(|e| &e.0) }
    fn names_of(&self, kind: Option<u8>) -> Vec<String> {
        self.m.regs.values().filter(|(r, _)| kind.is_none_or(|k| r.kind == k)).map(|(r, _)| r.memo_id.clone()).collect()
    }
    fn all_names(&self) -> Vec<String> { self.m.names.values().cloned().collect() }
    fn core_assets(&self) -> Vec<Pubkey> { self.m.assets.keys().copied().collect() }
    fn price_now(&self, centi: u64) -> u64 {
        curve_reference(self.m.supply).map(|r| (r as u128 * centi as u128).div_ceil(100) as u64).unwrap_or(0)
    }
    /// Owner of a Core Asset (memo-home's owner_of, raised inside memo-id's frame otherwise)
    fn core_owner(&self, k: &Pubkey) -> Result<Pubkey, Fail> {
        self.m.assets.get(k).map(|a| a.owner).ok_or(self.homef(self.id, HOME_INVALID_CORE_ACCOUNT))
    }
    /// The Collection Config clients derive for a holder: of its Collection (none: of the
    /// default address, which does not exist)
    fn cfg_for(&self, h: &Pubkey) -> Pubkey {
        let col = self.m.assets.get(h).and_then(|a| a.col).unwrap_or_default();
        if col == self.collection { self.config } else { self.pda(&self.home, &[b"collection_config", col.as_ref()]).0 }
    }
    /// Anchor `Option<Account<CollectionConfig>>` of holder_config (the MEMO ID Collection's is the only one)
    fn cfg_deser(&self, hcfg: &Option<Pubkey>) -> Result<(), Fail> {
        match hcfg {
            None => Ok(()),
            Some(k) if *k == self.config => Ok(()),
            Some(k) if self.m.regs.contains_key(k) || self.m.prims.contains_key(k) || self.m.idx.contains_key(k) => {
                Err(self.anchor(self.id, A_OWNED_BY_WRONG_PROGRAM))
            }
            Some(k) => Err(self.missing(self.id, k)),
        }
    }
    /// holder_of: holder and holder_config come together
    fn pair(&self, holder: &Option<Pubkey>, hcfg: &Option<Pubkey>) -> Result<(), Fail> {
        if holder.is_some() != hcfg.is_some() {
            return Err(self.idf(HOLDER_NOT_HOME_ASSET));
        }
        Ok(())
    }
    /// memo_home::is_listed: a TransferDelegate held by someone else
    fn listed(&self, a: &Pubkey) -> bool { matches!(self.m.del.get(a), Some(Some(_))) }
    /// memo-id's controller_of: the owner, or with a holder (a memo-home Asset, checked once the
    /// Config has deserialized), the holder's owner when its Vault owns the Asset
    fn ctrl_of(&self, asset: &Pubkey, holder: Option<&Pubkey>) -> Result<Pubkey, Fail> {
        let owner = self.core_owner(asset)?;
        match holder {
            None => Ok(owner),
            Some(h) => {
                if self.m.assets.get(h).and_then(|a| a.col) != Some(self.collection) {
                    return Err(self.idf(HOLDER_NOT_HOME_ASSET));
                }
                if owner != self.vault(h) {
                    return Err(self.idf(NOT_IN_HOLDER_VAULT));
                }
                self.core_owner(h)
            }
        }
    }
    fn nft_ctrl(&self, r: &MReg, asset: &Pubkey, holder: Option<&Pubkey>, signer: &Pubkey) -> Result<Pubkey, Fail> {
        if r.kind != NFT {
            return Err(self.idf(NOT_NFT));
        }
        if *asset != r.asset {
            return Err(self.idf(WRONG_ASSET));
        }
        let c = self.ctrl_of(asset, holder)?;
        if c != *signer {
            return Err(self.idf(NOT_CONTROLLER));
        }
        Ok(c)
    }
    fn target_ctrl(&self, t: &Target, target_asset: Option<&Pubkey>, holder: Option<&Pubkey>, signer: &Pubkey) -> Result<(), Fail> {
        match t {
            Target::Address(a) => {
                if a != signer {
                    return Err(self.idf(NOT_CONTROLLER));
                }
            }
            Target::Asset(a) => {
                let ta = target_asset.ok_or(self.idf(WRONG_ASSET))?;
                if ta != a {
                    return Err(self.idf(WRONG_ASSET));
                }
                if self.ctrl_of(ta, holder)? != *signer {
                    return Err(self.idf(NOT_CONTROLLER));
                }
            }
        }
        Ok(())
    }
    /// Anchor `Account<Registry>` deserialization of `key`
    fn deser_reg(&self, key: &Pubkey) -> Result<MReg, Fail> {
        if let Some((r, _)) = self.m.regs.get(key) {
            return Ok(r.clone());
        }
        if self.m.prims.contains_key(key) || self.m.idx.contains_key(key) {
            return Err(self.anchor(self.id, A_DISCRIMINATOR_MISMATCH));
        }
        Err(self.missing(self.id, key))
    }
    /// Anchor `Account<T>` of an address without that account: AccountNotInitialized only when it
    /// is System-owned with no lamports; a wallet, a Core Asset or a pre-funded PDA is owned by
    /// another program as far as Anchor can tell
    fn missing(&self, prog: Pubkey, key: &Pubkey) -> Fail {
        if self.m.assets.contains_key(key) || self.signable(key) || self.m.funded.get(key).is_some_and(|l| *l > 0) {
            return self.anchor(prog, A_OWNED_BY_WRONG_PROGRAM);
        }
        self.anchor(prog, A_NOT_INITIALIZED)
    }
    /// Holder (the one-level Vault rule, found by scanning the known Assets) and controller
    fn discover(&self, asset: &Pubkey) -> (Option<Pubkey>, Pubkey) {
        let Some(a) = self.m.assets.get(asset) else { return (None, Pubkey::default()) };
        match self.vault_rev.get(&a.owner) {
            Some(h) => (Some(*h), self.m.assets[h].owner),
            None => (None, a.owner),
        }
    }
    /// Controller of a Registry by the same rule (None if it, or its parent, does not exist)
    fn reg_ctrl(&self, regs: &BTreeMap<Pubkey, MReg>, k: &Pubkey, ctrl: &dyn Fn(&Pubkey) -> Option<Pubkey>) -> Option<Pubkey> {
        let r = regs.get(k)?;
        match r.kind {
            NFT => ctrl(&r.asset),
            _ => ctrl(&regs.get(&r.parent)?.asset),
        }
    }
    fn short(&self, k: &Pubkey) -> String {
        if let Some(i) = self.actors.iter().position(|a| a.pubkey() == *k) {
            return format!("a{}", i);
        }
        if *k == self.x() {
            return "X".into();
        }
        if *k == CORE {
            return "CORE".into();
        }
        if *k == self.id {
            return "-".into();
        }
        if let Some(a) = self.m.assets.get(k) {
            return format!("@{}", a.name);
        }
        if let Some(h) = self.vault_rev.get(k) {
            return format!("V({})", self.short(h));
        }
        if let Some(n) = self.m.names.get(k) {
            return format!("R({})", n);
        }
        if let Some(p) = self.m.idx.get(k) {
            return format!("idx({},{})", self.short(&p.0), p.1);
        }
        if let Some(p) = self.m.targets.iter().find(|t| self.prim_addr(t).0 == *k) {
            return format!("P({:?})", self.short_t(p));
        }
        let s = k.to_string();
        s[..6].to_string()
    }
    fn short_t(&self, t: &Target) -> String {
        match t {
            Target::Address(k) => format!("Addr({})", self.short(k)),
            Target::Asset(k) => format!("Asset({})", self.short(k)),
        }
    }
    fn short_o(&self, k: &Option<Pubkey>) -> String { k.map(|k| self.short(&k)).unwrap_or_else(|| "-".into()) }
    fn describe(&self, act: &Act) -> String {
        let s = |k: &Pubkey| self.short(k);
        let so = |k: &Option<Pubkey>| self.short_o(k);
        let st = |t: &Option<Target>| t.map(|t| self.short_t(&t)).unwrap_or_else(|| "-".into());
        let sign = |b: &bool| if *b { "" } else { " UNSIGNED" };
        // the holder, and its Config when it is not the one clients derive
        let so = |h: &Option<Pubkey>, c: &Option<Pubkey>| -> String {
            let derived = h.map(|h| self.cfg_for(&h));
            if *c == derived { so(h) } else { format!("{} cfg={}", so(h), so(c)) }
        };
        match act {
            Act::Register { payer, label, uri, memo, cu_last } => format!("register {:?} payer={} uri={:.40} memo={:?}{}", label, s(payer), uri,
                                                                          memo, if *cu_last { " cu-last" } else { "" }),
            Act::CreateSubid { payer, ctrl, signs, parent, parent_asset, holder, hcfg, label, target } =>
                format!("create_subid {:?} under {} payer={} ctrl={}{} asset={} holder={} target={}", label, parent, s(payer), s(ctrl), sign(signs),
                        s(parent_asset), so(holder, hcfg), st(target)),
            Act::DeleteSubid { fee, ctrl, signs, parent, parent_asset, holder, hcfg, name } =>
                format!("delete_subid {} parent={} fee={} ctrl={}{} asset={} holder={}", name, parent, s(fee), s(ctrl), sign(signs), s(parent_asset),
                        so(holder, hcfg)),
            Act::IssueSubid { payer, ctrl, signs, parent, parent_asset, holder, hcfg, name, index, new_owner, uri, memo, .. } =>
                format!("issue_subid {} parent={} index={} payer={} ctrl={}{} asset={} holder={} to={} uri={:.30} memo={:?}", name, parent, index,
                        s(payer), s(ctrl), sign(signs), s(parent_asset), so(holder, hcfg), s(new_owner), uri, memo),
            Act::SetTarget { fee, ctrl, signs, name, parent, asset, holder, hcfg, target } =>
                format!("{} {} parent={} fee={} ctrl={}{} asset={} holder={} target={}", act.op(), name, parent.as_deref().unwrap_or("-"), s(fee),
                        s(ctrl), sign(signs), s(asset), so(holder, hcfg), st(target)),
            Act::SetPrimary { payer, signer, signs, reg, target, account, target_asset, holder, hcfg } =>
                format!("set_primary {} reg={} payer={} signer={}{} account={} target_asset={} holder={}", self.short_t(target),
                        match reg { RegRef::Name(n) => n.clone(), RegRef::Acct(k) => s(k) }, s(payer), s(signer), sign(signs), s(account),
                        self.short_o(target_asset), so(holder, hcfg)),
            Act::ClearPrimary { fee, signer, signs, target, target_asset, holder, hcfg } =>
                format!("clear_primary {} fee={} signer={}{} target_asset={} holder={}", self.short_t(target), s(fee), s(signer), sign(signs),
                        self.short_o(target_asset), so(holder, hcfg)),
            Act::UpdateUri { payer, ctrl, signs, name, asset, holder, hcfg, uri } =>
                format!("update_uri {} payer={} ctrl={}{} asset={} holder={} uri={:.40}({})", name, s(payer), s(ctrl), sign(signs), s(asset),
                        so(holder, hcfg), uri, uri.len()),
            Act::Delegate { owner, asset, op } => format!("{} {} owner={}{}", act.op(), s(asset), s(owner), match op {
                DelOp::Add(d) | DelOp::Approve(d) => format!(" to={}", s(d)),
                DelOp::Revoke => String::new(),
            }),
            Act::Transfer { payer, signer, asset, new_owner } => format!("core_transfer {} payer={} signer={} to={}", s(asset), s(payer), s(signer),
                                                                        s(new_owner)),
            Act::Withdraw { payer, owner, holder, inner, inner_col, new_owner } =>
                format!("withdraw_asset {} from V({}) payer={} owner={} inner_col={} to={}", s(inner), s(holder), s(payer), s(owner), s(inner_col),
                        s(new_owner)),
            Act::Prefund { payer, addr, lamports } => format!("prefund {} with {} lamports payer={}", s(addr), lamports, s(payer)),
        }
    }

    // ---------------- instructions ----------------
    fn opt(&self, k: Option<&Pubkey>) -> AccountMeta { AccountMeta::new_readonly(k.copied().unwrap_or(self.id), false) }
    fn ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.id, &d, accounts)
    }
    fn home_accounts(&self, name: &str) -> Vec<AccountMeta> {
        let asset = self.asset_addr(name);
        vec![
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.collection, false),
            AccountMeta::new(asset, false),
            AccountMeta::new(self.vstate_addr(&asset).0, false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
        ]
    }
    fn burn_accounts(&self, payer: &Pubkey) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.e.mint, false),
            AccountMeta::new(self.e.ata(payer), false),
            AccountMeta::new(self.stats_pda(payer), false),
            AccountMeta::new(self.e.gbs, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(self.e.burn_prog, false),
        ]
    }
    fn burn_memo(&self, memo: &MemoV, op: &str, payer: &Pubkey, name: &str, price: u64) -> Instruction {
        let (amount, user, id) = match memo {
            MemoV::Amount(d) => ((price as i64 + d) as u64, *payer, name.to_string()),
            MemoV::User(u) => (price, *u, name.to_string()),
            MemoV::Id(s) => (price, *payer, s.clone()),
            _ => (price, *payer, name.to_string()),
        };
        let mut p = vec![1u8];
        bstr(&mut p, "memo_id");
        bstr(&mut p, op);
        bstr(&mut p, &user.to_string());
        bstr(&mut p, &id);
        app_burn_memo(amount, &p)
    }
    /// [BurnMemo, compute budget, ix], [BurnMemo, ix, compute budget] or (Late) [ix, BurnMemo, compute budget]
    fn with_memo(memo_ix: Instruction, ix: Instruction, memo: &MemoV, cu_last: bool, units: u32) -> (Vec<Instruction>, u64) {
        match memo {
            MemoV::Late => (vec![ix, memo_ix, cu(units)], 0),
            _ if cu_last => (vec![memo_ix, ix, cu(units)], 1),
            _ => (vec![memo_ix, cu(units), ix], 2),
        }
    }
    /// The instructions, the index of the instruction under test, and whether it goes as v0 with the lookup table
    fn build(&self, act: &Act) -> (Vec<Instruction>, u64, bool) {
        // a compute limit unique to the step keeps two identical steps from being one transaction
        let units = 1_000_000 + (self.step % 200_000) as u32;
        let plain = |ix: Instruction| (vec![cu(units), ix], 1u64, false);
        match act {
            Act::Register { payer, label, uri, memo, cu_last } => {
                let name = format!("{}.memo", label);
                let mut a = sha(name.as_bytes()).to_vec();
                bstr(&mut a, label);
                bstr(&mut a, uri);
                let mut accounts = vec![AccountMeta::new(*payer, true), AccountMeta::new(self.reg_addr(&name), false)];
                accounts.extend(self.home_accounts(&name));
                accounts.extend(self.burn_accounts(payer));
                accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
                accounts.push(AccountMeta::new_readonly(system_program(), false));
                let ix = self.ix("register", a, accounts);
                let m = self.burn_memo(memo, "register", payer, &name, self.price_now(price_centi(label.len())));
                let (ixs, i) = Self::with_memo(m, ix, memo, *cu_last, units);
                (ixs, i, true)
            }
            Act::CreateSubid { payer, ctrl, signs, parent, parent_asset, holder, hcfg, label, target } => {
                let name = format!("{}.{}", label, parent);
                let mut a = sha(name.as_bytes()).to_vec();
                bstr(&mut a, label);
                match target {
                    Some(t) => {
                        a.push(1);
                        t.encode(&mut a);
                    }
                    None => a.push(0),
                }
                plain(self.ix("create_subid", a, vec![
                    AccountMeta::new(*payer, true),
                    AccountMeta::new_readonly(*ctrl, *signs),
                    AccountMeta::new(self.reg_addr(parent), false),
                    AccountMeta::new_readonly(*parent_asset, false),
                    self.opt(holder.as_ref()),
                    self.opt(hcfg.as_ref()),
                    AccountMeta::new(self.reg_addr(&name), false),
                    AccountMeta::new_readonly(system_program(), false),
                ]))
            }
            Act::DeleteSubid { ctrl, signs, parent, parent_asset, holder, hcfg, name, .. } => plain(self.ix("delete_subid", vec![], vec![
                AccountMeta { pubkey: *ctrl, is_signer: *signs, is_writable: true },
                AccountMeta::new(self.reg_addr(parent), false),
                AccountMeta::new_readonly(*parent_asset, false),
                self.opt(holder.as_ref()),
                self.opt(hcfg.as_ref()),
                AccountMeta::new(self.reg_addr(name), false),
            ])),
            Act::IssueSubid { payer, ctrl, signs, parent, parent_asset, holder, hcfg, name, index, new_owner, uri, memo, cu_last } => {
                let p = self.reg_addr(parent);
                let mut a = index.to_le_bytes().to_vec();
                bstr(&mut a, uri);
                let mut accounts = vec![
                    AccountMeta::new(*payer, true),
                    AccountMeta::new_readonly(*ctrl, *signs),
                    AccountMeta::new(p, false),
                    AccountMeta::new_readonly(*parent_asset, false),
                    self.opt(holder.as_ref()),
                    self.opt(hcfg.as_ref()),
                    AccountMeta::new(self.reg_addr(name), false),
                    AccountMeta::new(self.idx_addr(&p, *index).0, false),
                    AccountMeta::new_readonly(*new_owner, false),
                ];
                accounts.extend(self.home_accounts(name));
                accounts.extend(self.burn_accounts(payer));
                accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
                accounts.push(AccountMeta::new_readonly(system_program(), false));
                let ix = self.ix("issue_subid", a, accounts);
                let m = self.burn_memo(memo, "issue_subid", payer, name, self.price_now(6_900));
                let (ixs, i) = Self::with_memo(m, ix, memo, *cu_last, units);
                (ixs, i, true)
            }
            Act::SetTarget { ctrl, signs, name, parent, asset, holder, hcfg, target, .. } => {
                let accounts = vec![
                    AccountMeta::new_readonly(*ctrl, *signs),
                    AccountMeta::new(self.reg_addr(name), false),
                    self.opt(parent.as_ref().map(|p| self.reg_addr(p)).as_ref()),
                    AccountMeta::new_readonly(*asset, false),
                    self.opt(holder.as_ref()),
                    self.opt(hcfg.as_ref()),
                ];
                plain(match target {
                    Some(t) => {
                        let mut a = vec![];
                        t.encode(&mut a);
                        self.ix("set_target", a, accounts)
                    }
                    None => self.ix("clear_target", vec![], accounts),
                })
            }
            Act::SetPrimary { payer, signer, signs, reg, target, account, target_asset, holder, hcfg } => {
                // (target_kind, target_key): the same bytes as the Borsh Target
                let mut a = vec![];
                target.encode(&mut a);
                let reg_key = match reg { RegRef::Name(n) => self.reg_addr(n), RegRef::Acct(k) => *k };
                plain(self.ix("set_primary", a, vec![
                    AccountMeta::new(*payer, true),
                    AccountMeta::new_readonly(*signer, *signs),
                    AccountMeta::new_readonly(reg_key, false),
                    AccountMeta::new(*account, false),
                    self.opt(target_asset.as_ref()),
                    self.opt(holder.as_ref()),
                    self.opt(hcfg.as_ref()),
                    AccountMeta::new_readonly(system_program(), false),
                ]))
            }
            Act::ClearPrimary { signer, signs, target, target_asset, holder, hcfg, .. } => plain(self.ix("clear_primary", vec![], vec![
                AccountMeta { pubkey: *signer, is_signer: *signs, is_writable: true },
                AccountMeta::new(self.prim_addr(target).0, false),
                self.opt(target_asset.as_ref()),
                self.opt(holder.as_ref()),
                self.opt(hcfg.as_ref()),
            ])),
            Act::UpdateUri { payer, ctrl, signs, name, asset, holder, hcfg, uri } => {
                let mut a = vec![];
                bstr(&mut a, uri);
                plain(self.ix("update_uri", a, vec![
                    AccountMeta::new(*payer, true),
                    AccountMeta::new_readonly(*ctrl, *signs),
                    AccountMeta::new_readonly(self.reg_addr(name), false),
                    AccountMeta::new(*asset, false),
                    self.opt(holder.as_ref()),
                    self.opt(hcfg.as_ref()),
                    AccountMeta::new_readonly(self.authority, false),
                    AccountMeta::new_readonly(self.config, false),
                    AccountMeta::new_readonly(self.collection, false),
                    AccountMeta::new_readonly(self.home_authority, false),
                    AccountMeta::new_readonly(CORE, false),
                    AccountMeta::new_readonly(self.home, false),
                    AccountMeta::new_readonly(system_program(), false),
                ]))
            }
            Act::Transfer { payer, signer, asset, new_owner } => {
                let col = self.m.assets.get(asset).and_then(|a| a.col).unwrap_or(CORE);
                plain(Instruction {
                    program_id: CORE,
                    accounts: vec![
                        AccountMeta::new(*asset, false),
                        AccountMeta::new_readonly(col, false),
                        AccountMeta::new(*payer, true),
                        // authority: none (the payer) or the signer
                        if payer == signer { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new_readonly(*signer, true) },
                        AccountMeta::new_readonly(*new_owner, false),
                        AccountMeta::new_readonly(system_program(), false),
                        AccountMeta::new_readonly(CORE, false),
                    ],
                    data: vec![14, 0],
                })
            }
            Act::Withdraw { payer, owner, holder, inner, inner_col, new_owner } => plain(Instruction::new_with_bytes(self.home, &disc("withdraw_asset"), vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new(*payer, true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new_readonly(*holder, false),
                AccountMeta::new_readonly(self.vault(holder), false),
                AccountMeta::new(self.vstate_addr(holder).0, false),
                AccountMeta::new(*inner, false),
                AccountMeta::new_readonly(*inner_col, false),
                AccountMeta::new_readonly(*new_owner, false),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new_readonly(system_program(), false),
            ])),
            Act::Prefund { payer, addr, lamports } => plain(solana_system_interface::instruction::transfer(payer, addr, *lamports)),
            Act::Delegate { owner, asset, op } => {
                // TransferDelegate = plugin type 3; Authority::Address = 3
                let data = match op {
                    DelOp::Add(d) => [vec![2u8, 3, 1, 3], d.as_ref().to_vec()].concat(),
                    DelOp::Approve(d) => [vec![8u8, 3, 3], d.as_ref().to_vec()].concat(),
                    DelOp::Revoke => vec![10u8, 3],
                };
                let col = self.m.assets.get(asset).and_then(|a| a.col).unwrap_or(CORE);
                plain(Instruction {
                    program_id: CORE,
                    accounts: vec![
                        AccountMeta::new(*asset, false),
                        if col == CORE { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new(col, false) },
                        AccountMeta::new(*owner, true),
                        AccountMeta::new_readonly(CORE, false),
                        AccountMeta::new_readonly(system_program(), false),
                        AccountMeta::new_readonly(CORE, false),
                    ],
                    data,
                })
            }
        }
    }

    // ---------------- prediction ----------------
    fn uri_check(&self, uri: &str) -> Result<(), Fail> {
        if uri.len() > 256 {
            return Err(self.idf(URI_TOO_LONG));
        }
        if !["https://", "ar://", "ipfs://"].iter().any(|s| uri.starts_with(s) && uri.len() > s.len()) {
            return Err(self.idf(INVALID_URI));
        }
        Ok(())
    }
    /// memo-id's BurnMemo checks, then memo-burn's burn of the payer's MEMO
    fn burn_check(&self, memo: &MemoV, price: u64, payer: &Pubkey) -> Result<(), Fail> {
        match memo {
            MemoV::Ok => {}
            MemoV::Late => return Err(self.idf(MEMO_REQUIRED)),
            MemoV::Amount(_) => return Err(self.idf(BURN_AMOUNT_MISMATCH)),
            MemoV::User(_) => return Err(self.idf(USER_PUBKEY_MISMATCH)),
            MemoV::Id(_) => return Err(self.idf(MEMO_ID_MISMATCH)),
        }
        if self.m.bal.get(payer).copied().unwrap_or(0) < price {
            return Err(self.token_insufficient());
        }
        Ok(())
    }

    /// Success with what it does, or the exact failure, in the order the checks run: Anchor
    /// deserializes every account in declaration order (nested structs with their own
    /// constraints), then creates the init accounts, then checks the other constraints; then the
    /// handler and its CPIs
    fn predict(&self, act: &Act) -> Result<Pred, Fail> {
        match act {
            Act::Register { payer, label, uri, memo, .. } => {
                let name = format!("{}.memo", label);
                if self.m.regs.contains_key(&self.reg_addr(&name)) {
                    return Err(self.sys_in_use());
                }
                if !valid_label(label) {
                    return Err(self.idf(INVALID_LABEL));
                }
                if name.len() > 253 {
                    return Err(self.idf(MEMO_ID_TOO_LONG));
                }
                self.uri_check(uri)?;
                let price = self.price_now(price_centi(label.len()));
                self.burn_check(memo, price, payer)?;
                Ok(Pred { price, ctrl: *payer })
            }
            Act::CreateSubid { ctrl, signs, parent, parent_asset, holder, hcfg, label, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let p = self.deser_reg(&self.reg_addr(parent))?;
                self.cfg_deser(hcfg)?;
                let name = format!("{}.{}", label, parent);
                if self.m.regs.contains_key(&self.reg_addr(&name)) {
                    return Err(self.sys_in_use());
                }
                if !valid_label(label) {
                    return Err(self.idf(INVALID_LABEL));
                }
                if p.kind != NFT {
                    return Err(self.idf(PARENT_NOT_NFT));
                }
                self.pair(holder, hcfg)?;
                let c = self.nft_ctrl(&p, parent_asset, holder.as_ref(), ctrl)?;
                if name.len() > 253 {
                    return Err(self.idf(MEMO_ID_TOO_LONG));
                }
                Ok(Pred { price: 0, ctrl: c })
            }
            Act::DeleteSubid { ctrl, signs, parent, parent_asset, holder, hcfg, name, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let pk = self.reg_addr(parent);
                let p = self.deser_reg(&pk)?;
                self.cfg_deser(hcfg)?;
                let r = self.deser_reg(&self.reg_addr(name))?;
                if r.kind != RECORD {
                    return Err(self.idf(NOT_RECORD));
                }
                if r.parent != pk {
                    return Err(self.idf(WRONG_PARENT));
                }
                self.pair(holder, hcfg)?;
                let c = self.nft_ctrl(&p, parent_asset, holder.as_ref(), ctrl)?;
                Ok(Pred { price: 0, ctrl: c })
            }
            Act::IssueSubid { payer, ctrl, signs, parent, parent_asset, holder, hcfg, name, index, new_owner, uri, memo, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let pk = self.reg_addr(parent);
                let p = self.deser_reg(&pk)?;
                self.cfg_deser(hcfg)?;
                let r = self.deser_reg(&self.reg_addr(name))?;
                if self.m.idx.contains_key(&self.idx_addr(&pk, *index).0) {
                    return Err(self.sys_in_use());
                }
                if r.kind != RECORD {
                    return Err(self.idf(NOT_RECORD));
                }
                if r.parent != pk {
                    return Err(self.idf(WRONG_PARENT));
                }
                self.uri_check(uri)?;
                if *index != p.nft {
                    return Err(self.idf(WRONG_SUBID_INDEX));
                }
                self.pair(holder, hcfg)?;
                let c = self.nft_ctrl(&p, parent_asset, holder.as_ref(), ctrl)?;
                if self.listed(parent_asset) || holder.is_some_and(|h| self.listed(&h)) {
                    return Err(self.idf(LISTED));
                }
                let price = self.price_now(6_900);
                self.burn_check(memo, price, payer)?;
                // memo-home create_asset: not Core, the new Asset itself or its own Vault (locked for good)
                let new_asset = self.asset_addr(name);
                if [CORE, new_asset, self.vault(&new_asset)].contains(new_owner) {
                    return Err(self.homef(self.home, HOME_INVALID_OWNER));
                }
                Ok(Pred { price, ctrl: c })
            }
            Act::SetTarget { ctrl, signs, name, parent, asset, holder, hcfg, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let r = self.deser_reg(&self.reg_addr(name))?;
                let p = match parent {
                    Some(n) => Some((self.reg_addr(n), self.deser_reg(&self.reg_addr(n))?)),
                    None => None,
                };
                self.cfg_deser(hcfg)?;
                self.pair(holder, hcfg)?;
                let c = match r.kind {
                    NFT => self.nft_ctrl(&r, asset, holder.as_ref(), ctrl)?,
                    _ => {
                        let (pk, p) = p.ok_or(self.idf(WRONG_PARENT))?;
                        if pk != r.parent {
                            return Err(self.idf(WRONG_PARENT));
                        }
                        self.nft_ctrl(&p, asset, holder.as_ref(), ctrl)?
                    }
                };
                Ok(Pred { price: 0, ctrl: c })
            }
            Act::SetPrimary { signer, signs, reg, target, account, target_asset, holder, hcfg, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                self.deser_reg(&match reg { RegRef::Name(n) => self.reg_addr(n), RegRef::Acct(k) => *k })?;
                // every account deserializes before the init_if_needed seeds are checked
                self.cfg_deser(hcfg)?;
                if *account != self.prim_addr(target).0 {
                    return Err(self.anchor(self.id, A_CONSTRAINT_SEEDS));
                }
                self.pair(holder, hcfg)?;
                self.target_ctrl(target, target_asset.as_ref(), holder.as_ref(), signer)?;
                Ok(Pred { price: 0, ctrl: *signer })
            }
            Act::ClearPrimary { signer, signs, target, target_asset, holder, hcfg, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let pa = self.prim_addr(target).0;
                if !self.m.prims.contains_key(&pa) {
                    return Err(self.missing(self.id, &pa));
                }
                self.cfg_deser(hcfg)?;
                self.pair(holder, hcfg)?;
                self.target_ctrl(target, target_asset.as_ref(), holder.as_ref(), signer)?;
                Ok(Pred { price: 0, ctrl: *signer })
            }
            Act::UpdateUri { ctrl, signs, name, asset, holder, hcfg, uri, .. } => {
                if !signs {
                    return Err(self.anchor(self.id, A_NOT_SIGNER));
                }
                let r = self.deser_reg(&self.reg_addr(name))?;
                self.cfg_deser(hcfg)?;
                self.uri_check(uri)?;
                if r.kind != NFT {
                    return Err(self.idf(NOT_NFT));
                }
                self.pair(holder, hcfg)?;
                let c = self.nft_ctrl(&r, asset, holder.as_ref(), ctrl)?;
                Ok(Pred { price: 0, ctrl: c })
            }
            Act::Transfer { signer, asset, .. } => {
                // the owner, or the holder of the TransferDelegate
                if self.m.assets[asset].owner != *signer && self.m.del.get(asset) != Some(&Some(*signer)) {
                    return Err(Fail { code: CORE_NO_APPROVALS, prog: CORE, name: None });
                }
                Ok(Pred::default())
            }
            Act::Withdraw { owner, holder, inner, inner_col, new_owner, .. } => {
                // the holder's Vault State (Account<VaultState>) exists only for memo-home Assets
                if !self.m.vstate.contains_key(holder) {
                    return Err(self.missing(self.home, &self.vstate_addr(holder).0));
                }
                let h = &self.m.assets[holder];
                if h.col != Some(self.collection) {
                    return Err(self.homef(self.home, HOME_WRONG_COLLECTION));
                }
                if h.owner != *owner {
                    return Err(self.homef(self.home, HOME_NOT_OWNER));
                }
                if self.listed(holder) {
                    return Err(self.homef(self.home, HOME_LISTED));
                }
                let i = &self.m.assets[inner];
                if i.owner != self.vault(holder) {
                    return Err(self.homef(self.home, HOME_NOT_IN_VAULT));
                }
                if *inner_col != i.col.unwrap_or(CORE) {
                    return Err(self.homef(self.home, HOME_WRONG_COLLECTION));
                }
                if [self.vault(holder), *inner, self.vault(inner), *holder, CORE].contains(new_owner) {
                    return Err(self.homef(self.home, HOME_INVALID_DESTINATION));
                }
                Ok(Pred::default())
            }
            Act::Prefund { .. } | Act::Delegate { .. } => Ok(Pred::default()),
        }
    }

    // ---------------- applying a success ----------------
    /// Lamports of a memo-id account the model knows (or of a pre-funded address)
    fn lamports_of(&self, k: &Pubkey, len: usize) -> u64 { self.m.funded.get(k).copied().unwrap_or_else(|| self.rent(len)) }
    /// Anchor init over an address: it tops the lamports up to the rent, and keeps any excess
    fn created(&mut self, k: &Pubkey, len: usize) {
        if self.m.funded.get(k).is_some_and(|l| *l <= self.rent(len)) {
            self.m.funded.remove(k);
        }
    }
    fn write_reg(&mut self, k: &Pubkey) {
        let (r, raw) = self.m.regs.get_mut(k).unwrap();
        let b = r.bytes();
        raw[..b.len()].copy_from_slice(&b);
    }
    fn new_reg(&mut self, r: MReg) -> Pubkey {
        let k = self.reg_addr(&r.memo_id);
        let raw = vec![0u8; REG_BASE + r.memo_id.len()];
        self.created(&k, raw.len());
        self.m.names.insert(k, r.memo_id.clone());
        self.m.regs.insert(k, (r, raw));
        self.write_reg(&k);
        k
    }
    fn add_asset(&mut self, asset: Pubkey, a: MAsset, vault_state: bool) {
        self.vault_rev.insert(self.vault(&asset), asset);
        self.m.funded.remove(&asset);
        if vault_state {
            let (vk, bump) = self.vstate_addr(&asset);
            self.created(&vk, 17);
            self.m.vstate.insert(asset, (0, bump));
        }
        self.m.assets.insert(asset, a);
        self.m.targets.insert(Target::Asset(asset));
    }
    fn burn(&mut self, payer: &Pubkey, price: u64, time: i64) {
        *self.m.bal.get_mut(payer).unwrap() -= price;
        self.m.supply -= price;
        let s = self.m.stats.get_mut(payer).unwrap();
        s.user = *payer;
        s.total = s.total.saturating_add(price).min(MAX_USER_GLOBAL_BURN);
        s.count += 1;
        s.last_time = time;
        self.m.global.total += price as u128;
        self.m.global.count += 1;
        self.m.global.last_time = time;
    }
    /// A transfer (Core's, or memo-home's withdrawal) also gives an owner-managed TransferDelegate
    /// back to the (new) owner
    fn set_owner(&mut self, asset: &Pubkey, new_owner: Pubkey) {
        if let Some(d) = self.m.del.get_mut(asset) {
            *d = None;
        }
        let a = self.m.assets.get_mut(asset).unwrap();
        self.m.prev_owner.insert(*asset, a.owner);
        a.owner = new_owner;
    }

    /// Applies a predicted success; returns memo-id's expected events (memo-home's AssetWithdrawn
    /// for withdraw_asset) and the account credited with the rent of a closed account
    fn apply(&mut self, act: &Act, pred: Pred, time: i64) -> (Vec<Event>, Option<(Pubkey, u64)>) {
        let mut ev: Vec<Event> = vec![];
        let mut credit = None;
        let mut w = vec![];
        macro_rules! event {
            ($name:expr) => {{
                ev.push(($name, std::mem::take(&mut w)));
            }};
        }
        match act.clone() {
            Act::Register { payer, label, uri, .. } => {
                let name = format!("{}.memo", label);
                let asset = self.asset_addr(&name);
                let k = self.new_reg(MReg { kind: NFT, asset, parent: Pubkey::default(), created_at: time, nft: 0, rec: 0,
                                            bump: self.reg_bump(&name), set_by: Pubkey::default(), holder: Pubkey::default(), target: None,
                                            memo_id: name.clone() });
                self.add_asset(asset, MAsset { owner: payer, col: Some(self.collection), name: name.clone(), uri }, true);
                self.burn(&payer, pred.price, time);
                bstr(&mut w, &name);
                w.extend(k.as_ref());
                w.extend(asset.as_ref());
                w.extend(payer.as_ref());
                w.extend(pred.price.to_le_bytes());
                event!("Registered");
            }
            Act::CreateSubid { parent, label, target, holder, .. } => {
                let pk = self.reg_addr(&parent);
                let name = format!("{}.{}", label, parent);
                let mut r = MReg { kind: RECORD, asset: Pubkey::default(), parent: pk, created_at: time, nft: 0, rec: 0,
                                   bump: self.reg_bump(&name), set_by: Pubkey::default(), holder: Pubkey::default(), target: None,
                                   memo_id: name.clone() };
                r.set_forward(target, pred.ctrl, holder);
                let k = self.new_reg(r);
                self.m.regs.get_mut(&pk).unwrap().0.rec += 1;
                self.write_reg(&pk);
                bstr(&mut w, &name);
                w.extend(k.as_ref());
                w.extend(pk.as_ref());
                event!("SubidCreated");
                if let Some(t) = target {
                    self.m.fwd_hist.insert(k, (t, pred.ctrl));
                    w.extend(k.as_ref());
                    t.encode(&mut w);
                    w.extend(pred.ctrl.as_ref());
                    event!("TargetSet");
                }
            }
            Act::DeleteSubid { parent, name, .. } => {
                let (pk, k) = (self.reg_addr(&parent), self.reg_addr(&name));
                let (_, raw) = self.m.regs.remove(&k).unwrap();
                credit = Some((pred.ctrl, self.lamports_of(&k, raw.len())));
                self.m.funded.remove(&k);
                self.m.fwd_hist.remove(&k);
                let p = &mut self.m.regs.get_mut(&pk).unwrap().0;
                p.rec = p.rec.saturating_sub(1);
                self.write_reg(&pk);
                bstr(&mut w, &name);
                w.extend(k.as_ref());
                event!("SubidDeleted");
            }
            Act::IssueSubid { payer, parent, name, index, new_owner, uri, .. } => {
                let (pk, k, asset) = (self.reg_addr(&parent), self.reg_addr(&name), self.asset_addr(&name));
                let r = &mut self.m.regs.get_mut(&k).unwrap().0;
                r.kind = NFT;
                r.asset = asset;
                self.write_reg(&k);
                let p = &mut self.m.regs.get_mut(&pk).unwrap().0;
                p.nft += 1;
                p.rec = p.rec.saturating_sub(1);
                self.write_reg(&pk);
                let (ia, ib) = self.idx_addr(&pk, index);
                self.created(&ia, 41);
                self.m.idx.insert(ia, (pk, index, k, ib));
                self.add_asset(asset, MAsset { owner: new_owner, col: Some(self.collection), name: name.clone(), uri }, true);
                self.burn(&payer, pred.price, time);
                bstr(&mut w, &name);
                w.extend(k.as_ref());
                w.extend(pk.as_ref());
                w.extend(index.to_le_bytes());
                w.extend(asset.as_ref());
                w.extend(new_owner.as_ref());
                w.extend(pred.price.to_le_bytes());
                event!("SubidIssued");
            }
            Act::SetTarget { name, target, holder, .. } => {
                let k = self.reg_addr(&name);
                self.m.regs.get_mut(&k).unwrap().0.set_forward(target, pred.ctrl, holder);
                self.write_reg(&k);
                w.extend(k.as_ref());
                match target {
                    Some(t) => {
                        self.m.fwd_hist.insert(k, (t, pred.ctrl));
                        t.encode(&mut w);
                        w.extend(pred.ctrl.as_ref());
                        event!("TargetSet");
                    }
                    None => {
                        self.m.fwd_hist.remove(&k);
                        event!("TargetCleared");
                    }
                }
            }
            Act::SetPrimary { signer, reg, target, account, holder, .. } => {
                let RegRef::Name(n) = reg else { unreachable!() };
                let rk = self.reg_addr(&n);
                if !self.m.prims.contains_key(&account) {
                    self.created(&account, PRIM_LEN);
                }
                // a holder decides only an Asset target's controller
                let set_holder = match target { Target::Asset(_) => holder.unwrap_or_default(), Target::Address(_) => Pubkey::default() };
                self.m.prims.insert(account, MPrim { target, registry: rk, set_by: signer, set_holder, bump: self.prim_addr(&target).1 });
                self.m.prim_hist.insert(target, (rk, signer));
                target.encode(&mut w);
                w.extend(rk.as_ref());
                w.extend(signer.as_ref());
                event!("PrimarySet");
            }
            Act::ClearPrimary { signer, target, .. } => {
                let pa = self.prim_addr(&target).0;
                self.m.prims.remove(&pa);
                self.m.prim_hist.remove(&target);
                credit = Some((signer, self.lamports_of(&pa, PRIM_LEN)));
                self.m.funded.remove(&pa);
                target.encode(&mut w);
                event!("PrimaryCleared");
            }
            Act::UpdateUri { name, asset, uri, .. } => {
                self.m.assets.get_mut(&asset).unwrap().uri = uri;
                w.extend(self.reg_addr(&name).as_ref());
                w.extend(asset.as_ref());
                event!("UriUpdated");
            }
            Act::Transfer { asset, new_owner, .. } => self.set_owner(&asset, new_owner),
            Act::Delegate { asset, op, .. } => {
                self.m.del.insert(asset, match op { DelOp::Add(d) | DelOp::Approve(d) => Some(d), DelOp::Revoke => None });
            }
            Act::Withdraw { holder, inner, new_owner, .. } => {
                self.set_owner(&inner, new_owner);
                let n = &mut self.m.vstate.get_mut(&holder).unwrap().0;
                *n += 1;
                let nonce = *n;
                w.extend(holder.as_ref());
                w.extend(inner.as_ref());
                w.extend(new_owner.as_ref());
                w.extend(nonce.to_le_bytes());
                event!("AssetWithdrawn");
            }
            Act::Prefund { addr, lamports, .. } => {
                // an existing memo-id account holds its rent; an address without an account, nothing
                let len = self.m.regs.get(&addr).map(|e| e.1.len())
                    .or(self.m.idx.contains_key(&addr).then_some(41))
                    .or(self.m.prims.contains_key(&addr).then_some(PRIM_LEN));
                let now = match len {
                    Some(l) => self.lamports_of(&addr, l),
                    None => self.m.funded.get(&addr).copied().unwrap_or(0),
                };
                self.m.funded.insert(addr, now + lamports);
            }
        }
        (ev, credit)
    }

    /// The memo-id events (and memo-home's AssetWithdrawn) a transaction emitted, in order
    fn events_of(&self, out: &Out) -> Vec<Event> {
        let names: Vec<&'static str> = ID_EVENTS.iter().copied().chain(["AssetWithdrawn"]).collect();
        out.logs.iter().filter_map(|l| l.strip_prefix("Program data: "))
            .filter_map(|b| general_purpose::STANDARD.decode(b).ok())
            .filter_map(|v| names.iter().find(|n| v.len() >= 8 && v[..8] == disc8("event", n)).map(|n| (*n, v[8..].to_vec())))
            .collect()
    }

    // ---------------- generation ----------------
    fn chance(&mut self, p: f64) -> bool { self.rng.gen_bool(p) }
    fn pick<T: Clone>(&mut self, v: &[T]) -> T { v[self.rng.gen_range(0..v.len())].clone() }
    fn rand_actor(&mut self) -> Pubkey {
        let i = self.rng.gen_range(0..self.actors.len());
        self.actors[i].pubkey()
    }
    fn rand_signable(&mut self) -> Pubkey { if self.chance(0.1) { self.x() } else { self.rand_actor() } }
    fn other_signable(&mut self, not: &Pubkey) -> Pubkey {
        loop {
            let k = self.rand_signable();
            if k != *not {
                return k;
            }
        }
    }
    fn other_actor(&mut self, not: &Pubkey) -> Pubkey {
        loop {
            let k = self.rand_actor();
            if k != *not {
                return k;
            }
        }
    }
    fn is_actor(&self, k: &Pubkey) -> bool { self.actors.iter().any(|a| a.pubkey() == *k) }
    /// The fee payer: the signer when it is an actor (with probability `same`), else another actor
    fn gen_payer(&mut self, signer: &Pubkey, same: f64) -> Pubkey {
        if self.is_actor(signer) && self.chance(same) { *signer } else { self.other_actor(signer) }
    }
    fn rand_asset(&mut self) -> Pubkey {
        if self.chance(0.08) {
            return self.x();
        }
        let v = self.core_assets();
        self.pick(&v)
    }
    fn other_asset(&mut self, not: &Pubkey) -> Pubkey {
        for _ in 0..20 {
            let k = self.rand_asset();
            if k != *not {
                return k;
            }
        }
        self.x()
    }
    fn rand_target(&mut self) -> Target {
        let r = self.rng.gen::<f64>();
        if r < 0.35 {
            Target::Address(self.rand_signable())
        } else if r < 0.97 {
            Target::Asset(self.rand_asset())
        } else {
            // a wallet as an Asset target (memo-home's InvalidCoreAccount inside memo-id)
            Target::Asset(self.rand_actor())
        }
    }
    fn gen_uri(&mut self, bad: f64, long: bool) -> String {
        if self.chance(bad) {
            if long && self.chance(0.3) {
                return format!("https://{}", "a".repeat(249));
            }
            let v = ["http://example.com/x.json", "https://", "javascript:alert(1)//https://x", "ar:/x", "", "IPFS://x"];
            return self.pick(&v).to_string();
        }
        if long && self.chance(0.05) {
            return format!("https://{}", "a".repeat(248));
        }
        let n = self.rng.gen_range(0..1000);
        match self.rng.gen_range(0..3) {
            0 => format!("https://example.com/{}.json", n),
            1 => format!("ar://tx{}", n),
            _ => format!("ipfs://bafy{}", n),
        }
    }
    fn gen_memo(&mut self, payer: &Pubkey, name: &str) -> MemoV {
        let r = self.rng.gen::<f64>();
        if r < 0.86 {
            MemoV::Ok
        } else if r < 0.9 {
            MemoV::Amount(if self.chance(0.5) { 1 } else { -1 })
        } else if r < 0.94 {
            MemoV::User(self.other_actor(payer))
        } else if r < 0.97 {
            MemoV::Id(format!("x{}", name))
        } else {
            MemoV::Late
        }
    }
    /// Holder: the one the Vault rule finds (usually), none, or a wrong one
    fn gen_holder(&mut self, h: Option<Pubkey>) -> Option<Pubkey> {
        let r = self.rng.gen::<f64>();
        if r < 0.86 {
            h
        } else if r < 0.93 {
            match h {
                Some(_) => None,
                None => Some(self.rand_asset()),
            }
        } else if self.chance(0.8) {
            Some(self.rand_asset())
        } else {
            Some(self.rand_actor())
        }
    }
    /// The signer: the controller when it can sign (usually), the previous owner (stale), or anyone
    fn gen_signer(&mut self, ctrl: Pubkey, asset: &Pubkey) -> Pubkey {
        let r = self.rng.gen::<f64>();
        if r < 0.85 && self.signable(&ctrl) {
            return ctrl;
        }
        if r < 0.92 {
            if let Some(p) = self.m.prev_owner.get(asset).copied().filter(|p| self.signable(p)) {
                return p;
            }
        }
        self.other_signable(&ctrl)
    }
    /// holder_config: the Config clients derive for the holder (usually), none, the MEMO ID
    /// Collection's Config whatever the holder, or a memo-id account
    fn gen_hcfg(&mut self, holder: Option<Pubkey>) -> Option<Pubkey> {
        let r = self.rng.gen::<f64>();
        match holder {
            Some(h) if r < 0.9 => Some(self.cfg_for(&h)),
            Some(_) if r < 0.95 => None,
            None if r < 0.97 => None,
            _ if r < 0.99 || self.m.regs.is_empty() => Some(self.config),
            _ => {
                let v: Vec<Pubkey> = self.m.regs.keys().copied().collect();
                Some(self.pick(&v))
            }
        }
    }
    /// (Asset passed, holder, holder_config, signer) for an action controlled through `deciding`
    /// (the Asset of the ID, or of the parent for a Record SubId)
    fn gen_control(&mut self, deciding: Pubkey) -> (Pubkey, Option<Pubkey>, Option<Pubkey>, Pubkey) {
        let (h, c) = self.discover(&deciding);
        let asset = if self.chance(0.92) { deciding } else { self.other_asset(&deciding) };
        let holder = self.gen_holder(h);
        let hcfg = self.gen_hcfg(holder);
        let signer = self.gen_signer(c, &deciding);
        (asset, holder, hcfg, signer)
    }
    fn deciding_asset(&mut self, name: &str) -> Pubkey {
        match self.reg(name).cloned() {
            Some(r) if r.kind == NFT => r.asset,
            Some(r) => match self.m.regs.get(&r.parent) {
                Some((p, _)) => p.asset,
                None => self.rand_asset(),
            },
            None => self.rand_asset(),
        }
    }
    fn parent_name(&self, name: &str) -> Option<String> { self.reg(name).and_then(|r| self.m.names.get(&r.parent).cloned()) }
    /// A random name: existing of `kind` (probability p), else any name the run has named
    fn gen_name(&mut self, kind: u8, p: f64) -> Option<String> {
        let v = self.names_of(Some(kind));
        if !v.is_empty() && self.chance(p) {
            return Some(self.pick(&v));
        }
        let all = self.all_names();
        if all.is_empty() { None } else { Some(self.pick(&all)) }
    }

    fn gen_register(&mut self) -> Act {
        let payer = self.rand_actor();
        let label = if self.chance(0.12) {
            let i = self.rng.gen_range(0..9);
            bad_label(i)
        } else {
            let p = self.pool.clone();
            self.pick(&p)
        };
        let uri = self.gen_uri(0.06, true);
        let memo = self.gen_memo(&payer, &format!("{}.memo", label));
        Act::Register { payer, label, uri, memo, cu_last: self.chance(0.5) }
    }
    fn gen_create(&mut self) -> Option<Act> {
        let nfts = self.names_of(Some(NFT));
        if nfts.is_empty() {
            return None;
        }
        let label = if self.chance(0.08) {
            let i = self.rng.gen_range(0..9);
            bad_label(i)
        } else if self.chance(if self.edge { 0.25 } else { 0.08 }) {
            "l".repeat(63)
        } else {
            self.pick(&SUB_LABELS).to_string()
        };
        let r = self.rng.gen::<f64>();
        let parent = if label.len() == 63 && self.chance(if self.edge { 0.8 } else { 0.5 }) {
            // grow the deepest NFT ID towards the 253-byte limit
            nfts.iter().max_by_key(|n| n.len()).unwrap().clone()
        } else if r < 0.9 {
            self.pick(&nfts)
        } else {
            self.gen_name(RECORD, 0.6)?
        };
        let deciding = self.deciding_asset(&parent);
        let (asset, holder, hcfg, ctrl) = self.gen_control(deciding);
        let target = if self.chance(0.4) { None } else { Some(self.rand_target()) };
        let payer = self.gen_payer(&ctrl, 0.7);
        let signs = payer == ctrl || !self.chance(0.05);
        Some(Act::CreateSubid { payer, ctrl, signs, parent, parent_asset: asset, holder, hcfg, label, target })
    }
    fn gen_delete(&mut self) -> Option<Act> {
        let r = self.rng.gen::<f64>();
        let name = if r < 0.9 { self.gen_name(RECORD, 0.92)? } else { self.gen_name(NFT, 0.7)? };
        let nfts = self.names_of(Some(NFT));
        let parent = match self.parent_name(&name) {
            Some(p) if self.chance(0.88) => p,
            _ if !nfts.is_empty() => self.pick(&nfts),
            _ => return None,
        };
        let deciding = self.deciding_asset(&parent);
        let (asset, holder, hcfg, ctrl) = self.gen_control(deciding);
        let fee = self.gen_payer(&ctrl, 0.85);
        let signs = fee == ctrl || !self.chance(0.05);
        Some(Act::DeleteSubid { fee, ctrl, signs, parent, parent_asset: asset, holder, hcfg, name })
    }
    fn gen_issue(&mut self) -> Option<Act> {
        let r = self.rng.gen::<f64>();
        let recs = self.names_of(Some(RECORD));
        let name = if r < (if self.edge { 0.5 } else { 0.25 }) && !recs.is_empty() {
            // the longest Record, towards the 253-byte limit
            recs.iter().max_by_key(|n| n.len()).unwrap().clone()
        } else if r < 0.9 {
            self.gen_name(RECORD, 0.92)?
        } else {
            self.gen_name(NFT, 0.7)?
        };
        let nfts = self.names_of(Some(NFT));
        let parent = match self.parent_name(&name) {
            Some(p) if self.chance(0.9) => p,
            _ if !nfts.is_empty() => self.pick(&nfts),
            _ => return None,
        };
        let n = self.reg(&parent).map(|p| p.nft).unwrap_or(0);
        let r = self.rng.gen::<f64>();
        let index = if r < 0.85 { n } else if r < 0.93 || n == 0 { n + 1 } else { n - 1 };
        let deciding = self.deciding_asset(&parent);
        let (asset, holder, hcfg, ctrl) = self.gen_control(deciding);
        let r = self.rng.gen::<f64>();
        let (vault_hi, x_hi, own_hi) = if self.edge { (0.7, 0.74, 0.94) } else { (0.88, 0.92, 0.94) };
        let new_owner = if r < 0.35 {
            ctrl
        } else if r < 0.65 {
            self.other_actor(&ctrl)
        } else if r < vault_hi {
            let a = self.rand_asset();
            self.vault(&a)
        } else if r < x_hi {
            self.x()
        } else if r < own_hi {
            // the new Asset itself, or its own Vault (memo-home refuses both: InvalidOwner)
            let a = self.asset_addr(&name);
            if self.chance(0.5) { a } else { self.vault(&a) }
        } else {
            CORE
        };
        let uri = self.gen_uri(0.05, false);
        let payer = self.gen_payer(&ctrl, 0.7);
        let signs = payer == ctrl || !self.chance(0.05);
        let memo = self.gen_memo(&payer, &name);
        Some(Act::IssueSubid { payer, ctrl, signs, parent, parent_asset: asset, holder, hcfg, name, index, new_owner, uri, memo,
                               cu_last: self.chance(0.5) })
    }
    fn gen_set_target(&mut self, clear: bool) -> Option<Act> {
        let r = self.rng.gen::<f64>();
        let name = if r < 0.5 { self.gen_name(NFT, 0.96)? } else { self.gen_name(RECORD, 0.96)? };
        let nfts = self.names_of(Some(NFT));
        let any = self.all_names();
        let r = self.rng.gen::<f64>();
        let parent = match self.reg(&name).cloned() {
            Some(reg) if reg.kind == RECORD => {
                if r < 0.85 { self.parent_name(&name) } else if r < 0.92 || nfts.is_empty() { None } else { Some(self.pick(&nfts)) }
            }
            _ => if r < 0.9 { None } else { Some(self.pick(&any)) },
        };
        let deciding = self.deciding_asset(&name);
        let (asset, holder, hcfg, ctrl) = self.gen_control(deciding);
        // often the Target whose Primary ID already names this ID, so that verified pairs form
        let rk = self.reg_addr(&name);
        let named_by: Vec<Target> = self.m.prims.values().filter(|p| p.registry == rk).map(|p| p.target).collect();
        let target = if clear {
            None
        } else if !named_by.is_empty() && self.chance(0.5) {
            Some(self.pick(&named_by))
        } else {
            Some(self.rand_target())
        };
        let fee = self.gen_payer(&ctrl, 0.8);
        let signs = fee == ctrl || !self.chance(0.05);
        Some(Act::SetTarget { fee, ctrl, signs, name, parent, asset, holder, hcfg, target })
    }
    /// (target_asset, holder, holder_config, signer) for a Target
    fn gen_target_control(&mut self, target: &Target) -> (Option<Pubkey>, Option<Pubkey>, Option<Pubkey>, Pubkey) {
        match *target {
            Target::Address(a) => {
                let ta = if self.chance(0.85) { None } else { Some(self.rand_asset()) };
                let holder = if self.chance(0.9) { None } else { Some(self.rand_asset()) };
                let hcfg = self.gen_hcfg(holder);
                let signer = if self.chance(0.85) && self.signable(&a) { a } else { self.other_signable(&a) };
                (ta, holder, hcfg, signer)
            }
            Target::Asset(a) => {
                let (h, c) = self.discover(&a);
                let r = self.rng.gen::<f64>();
                let ta = if r < 0.9 { Some(a) } else if r < 0.95 { None } else { Some(self.other_asset(&a)) };
                let holder = self.gen_holder(h);
                let hcfg = self.gen_hcfg(holder);
                let signer = self.gen_signer(c, &a);
                (ta, holder, hcfg, signer)
            }
        }
    }
    fn gen_set_primary(&mut self) -> Option<Act> {
        // often the reverse of an existing forward record, so that verified pairs form
        let forwards: Vec<(String, Target)> = self.m.regs.values().filter_map(|(r, _)| r.target.map(|t| (r.memo_id.clone(), t))).collect();
        let paired = !forwards.is_empty() && self.chance(0.45);
        let (n, t) = if paired { self.pick(&forwards) } else { (String::new(), self.rand_target()) };
        let target = t;
        let r = self.rng.gen::<f64>();
        let existing = self.names_of(None);
        let reg = if paired {
            RegRef::Name(n)
        } else if r < 0.85 && !existing.is_empty() {
            RegRef::Name(self.pick(&existing))
        } else if r < 0.93 {
            RegRef::Name(self.gen_name(NFT, 0.0)?)
        } else if r < 0.97 && !(self.m.idx.is_empty() && self.m.prims.is_empty()) {
            let v: Vec<Pubkey> = self.m.idx.keys().chain(self.m.prims.keys()).copied().collect();
            RegRef::Acct(self.pick(&v))
        } else {
            RegRef::Acct(self.rand_actor())
        };
        let (target_asset, holder, hcfg, signer) = self.gen_target_control(&target);
        let account = if self.chance(0.03) { self.prim_addr(&target.flip()).0 } else { self.prim_addr(&target).0 };
        let payer = self.gen_payer(&signer, 0.7);
        let signs = payer == signer || !self.chance(0.05);
        Some(Act::SetPrimary { payer, signer, signs, reg, target, account, target_asset, holder, hcfg })
    }
    fn gen_clear_primary(&mut self) -> Option<Act> {
        let set: Vec<Target> = self.m.prims.values().map(|p| p.target).collect();
        let target = if !set.is_empty() && self.chance(0.85) { self.pick(&set) } else { self.rand_target() };
        let (target_asset, holder, hcfg, signer) = self.gen_target_control(&target);
        let fee = self.gen_payer(&signer, 0.8);
        let signs = fee == signer || !self.chance(0.05);
        Some(Act::ClearPrimary { fee, signer, signs, target, target_asset, holder, hcfg })
    }
    fn gen_update_uri(&mut self) -> Option<Act> {
        let r = self.rng.gen::<f64>();
        let name = if r < 0.88 { self.gen_name(NFT, 0.96)? } else { self.gen_name(RECORD, 0.8)? };
        let deciding = self.deciding_asset(&name);
        let (asset, holder, hcfg, ctrl) = self.gen_control(deciding);
        let r = self.rng.gen::<f64>();
        let uri = if r < 0.9 { self.gen_uri(0.0, true) } else if r < 0.96 { self.gen_uri(1.0, false) } else { format!("https://{}", "a".repeat(249)) };
        let payer = self.gen_payer(&ctrl, 0.7);
        let signs = payer == ctrl || !self.chance(0.05);
        Some(Act::UpdateUri { payer, ctrl, signs, name, asset, holder, hcfg, uri })
    }
    fn gen_transfer(&mut self) -> Option<Act> {
        let asset = self.rand_asset();
        let owner = self.m.assets[&asset].owner;
        let signer = if self.chance(0.85) && self.signable(&owner) { owner } else { self.other_signable(&owner) };
        let r = self.rng.gen::<f64>();
        let new_owner = if r < 0.55 {
            self.other_actor(&owner)
        } else if r < 0.95 {
            // into a Vault: of another Asset, rarely of its own (locked for good)
            let h = if self.chance(0.03) { asset } else { self.other_asset(&asset) };
            self.vault(&h)
        } else if r < 0.98 {
            self.x()
        } else {
            // to its own address (locked for good)
            asset
        };
        let payer = self.gen_payer(&signer, 0.85);
        Some(Act::Transfer { payer, signer, asset, new_owner })
    }
    fn gen_withdraw(&mut self) -> Option<Act> {
        let held: Vec<Pubkey> = self.m.assets.iter().filter(|(_, a)| self.vault_rev.contains_key(&a.owner)).map(|(k, _)| *k).collect();
        let inner = if !held.is_empty() && self.chance(0.88) { self.pick(&held) } else { self.rand_asset() };
        let holder = match self.vault_rev.get(&self.m.assets[&inner].owner).copied() {
            Some(h) if self.chance(0.92) => h,
            _ => self.other_asset(&inner),
        };
        let owner = self.m.assets[&holder].owner;
        let signer = if self.chance(0.88) && self.signable(&owner) { owner } else { self.other_signable(&owner) };
        let r = self.rng.gen::<f64>();
        let new_owner = if r < 0.75 {
            self.rand_actor()
        } else if r < 0.95 {
            let a = self.other_asset(&inner);
            self.vault(&a)
        } else {
            self.x()
        };
        let right = self.m.assets[&inner].col.unwrap_or(CORE);
        let inner_col = if self.chance(0.95) { right } else if right == CORE { self.collection } else { CORE };
        let payer = self.gen_payer(&signer, 0.85);
        Some(Act::Withdraw { payer, owner: signer, holder, inner, inner_col, new_owner })
    }
    /// Lamports to the PDA of an account that does not exist yet (or, rarely, one that does)
    fn gen_prefund(&mut self) -> Option<Act> {
        let payer = self.rand_actor();
        let nfts = self.names_of(Some(NFT));
        let r = self.rng.gen::<f64>();
        let addr = if r < 0.3 || nfts.is_empty() {
            // a second-level name or a SubId name: its Registry, Asset or Vault State
            let name = if nfts.is_empty() || self.chance(0.4) {
                let p = self.pool.clone();
                format!("{}.memo", self.pick(&p))
            } else {
                let parent = self.pick(&nfts);
                format!("{}.{}", self.pick(&SUB_LABELS), parent)
            };
            let k = self.reg_addr(&name);
            self.m.names.insert(k, name.clone());
            match self.rng.gen_range(0..4) {
                0 => self.asset_addr(&name),
                1 => self.vstate_addr(&self.asset_addr(&name)).0,
                _ => k,
            }
        } else if r < 0.6 {
            // the next SubId Index entry of an NFT ID
            let p = self.pick(&nfts);
            let pr = self.reg(&p).unwrap().clone();
            self.idx_addr(&self.reg_addr(&p), pr.nft).0
        } else {
            let t = self.rand_target();
            self.m.targets.insert(t);
            self.prim_addr(&t).0
        };
        let rent0 = self.rent(0);
        let v = [rent0, rent0 + 1, 3_000_000, 10_000_000];
        let lamports = self.pick(&v);
        Some(Act::Prefund { payer, addr, lamports })
    }
    /// A TransferDelegate change by an Asset's owner (an actor, who pays): add one held by another
    /// actor, take it back, or give it again
    fn gen_delegate(&mut self) -> Option<Act> {
        let owned: Vec<Pubkey> = self.m.assets.iter().filter(|(_, a)| self.is_actor(&a.owner)).map(|(k, _)| *k).collect();
        if owned.is_empty() {
            return None;
        }
        let asset = self.pick(&owned);
        let owner = self.m.assets[&asset].owner;
        let d = self.other_actor(&owner);
        let op = match self.m.del.get(&asset) {
            None => DelOp::Add(d),
            Some(Some(_)) => DelOp::Revoke,
            Some(None) => DelOp::Approve(d),
        };
        Some(Act::Delegate { owner, asset, op })
    }
    /// --edge: a valid step down a chain of 63-byte labels under the deepest NFT ID whose
    /// controller can sign (creating the next one fails with MemoIdTooLong past 253 bytes), the
    /// SubIds issued to their controller, or to their own Asset or Vault (refused by memo-home)
    fn gen_directed(&mut self) -> Option<Act> {
        let deepest = self.names_of(Some(NFT)).into_iter()
            .filter(|n| self.reg(n).is_some_and(|r| self.signable(&self.discover(&r.asset).1)))
            .max_by_key(|n| n.len())?;
        let parent_asset = self.reg(&deepest)?.asset;
        let (holder, ctrl) = self.discover(&parent_asset);
        let payer = *self.m.bal.iter().max_by_key(|(_, b)| **b)?.0;
        // the first 63-byte label not yet issued under it (an issued one may be locked)
        let label = ["l", "m", "n", "k"].iter().map(|c| c.repeat(63))
            .find(|l| self.reg(&format!("{}.{}", l, deepest)).is_none_or(|r| r.kind == RECORD))?;
        let name = format!("{}.{}", label, deepest);
        let hcfg = holder.map(|h| self.cfg_for(&h));
        if self.reg(&name).is_none() {
            return Some(Act::CreateSubid { payer, ctrl, signs: true, parent: deepest, parent_asset, holder, hcfg, label, target: None });
        }
        let index = self.reg(&deepest)?.nft;
        let asset = self.asset_addr(&name);
        let r = self.rng.gen::<f64>();
        let new_owner = if r < 0.6 { ctrl } else if r < 0.8 { asset } else { self.vault(&asset) };
        Some(Act::IssueSubid { payer, ctrl, signs: true, parent: deepest, parent_asset, holder, hcfg, name, index, new_owner, uri: URI.into(),
                               memo: MemoV::Ok, cu_last: false })
    }
    fn gen(&mut self) -> Act {
        const W: [u32; 13] = [6, 14, 7, 9, 10, 4, 10, 5, 5, 10, 7, 3, 4];
        if self.edge && self.chance(0.1) {
            if let Some(a) = self.gen_directed() {
                return a;
            }
        }
        loop {
            let mut x = self.rng.gen_range(0..W.iter().sum::<u32>());
            let op = W.iter().position(|w| if x < *w { true } else { x -= *w; false }).unwrap();
            let act = match op {
                0 => Some(self.gen_register()),
                1 => self.gen_create(),
                2 => self.gen_delete(),
                3 => self.gen_issue(),
                4 => self.gen_set_target(false),
                5 => self.gen_set_target(true),
                6 => self.gen_set_primary(),
                7 => self.gen_clear_primary(),
                8 => self.gen_update_uri(),
                9 => self.gen_transfer(),
                10 => self.gen_withdraw(),
                11 => self.gen_prefund(),
                _ => self.gen_delegate(),
            };
            if let Some(a) = act {
                return a;
            }
        }
    }
    /// Remember every Registry name an action refers to (each is checked after every step)
    fn note_names(&mut self, act: &Act) {
        let mut v: Vec<String> = vec![];
        match act {
            Act::Register { label, .. } => v.push(format!("{}.memo", label)),
            Act::CreateSubid { parent, label, .. } => {
                v.push(parent.clone());
                v.push(format!("{}.{}", label, parent));
            }
            Act::DeleteSubid { parent, name, .. } | Act::IssueSubid { parent, name, .. } => {
                v.push(parent.clone());
                v.push(name.clone());
            }
            Act::SetTarget { name, parent, .. } => {
                v.push(name.clone());
                v.extend(parent.clone());
            }
            Act::SetPrimary { reg: RegRef::Name(n), .. } => v.push(n.clone()),
            Act::UpdateUri { name, .. } => v.push(name.clone()),
            _ => {}
        }
        for n in v {
            let k = self.reg_addr(&n);
            self.m.names.insert(k, n);
        }
        match act {
            Act::SetTarget { target: Some(t), .. } | Act::CreateSubid { target: Some(t), .. } => {
                self.m.targets.insert(*t);
            }
            Act::SetPrimary { target, .. } | Act::ClearPrimary { target, .. } => {
                self.m.targets.insert(*target);
            }
            _ => {}
        }
    }

    // ---------------- running a step ----------------
    fn run_step(&mut self, forced: Option<Act>) {
        let act = forced.unwrap_or_else(|| self.gen());
        self.note_names(&act);
        let (ixs, ix_index, v0) = self.build(&act);
        let want = self.predict(&act);
        let (fee, others) = act.signers();
        let mut signer_keys: Vec<Pubkey> = vec![];
        for k in &others {
            if *k != fee && !signer_keys.contains(k) {
                signer_keys.push(*k);
            }
        }
        let sigs = 1 + signer_keys.len() as u64;
        let line = self.describe(&act);
        let touched: BTreeSet<Pubkey> = ixs.iter().flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey))
            .filter(|k| !self.common(k)).collect();

        // send (the size check keeps the run honest: a transaction too large for one packet is not a step)
        let payer_kp = self.kp(&fee);
        let signer_kps: Vec<&Keypair> = signer_keys.iter().map(|k| self.kp(k)).collect();
        let credit_hint = match (&want, &act) {
            (Ok(p), Act::DeleteSubid { .. }) => Some(p.ctrl),
            (Ok(_), Act::ClearPrimary { signer, .. }) => Some(*signer),
            _ => None,
        };
        let bal_before = credit_hint.map(|k| self.e.c.get_balance_with_commitment(&k, CommitmentConfig::confirmed()).unwrap().value);
        let out = if v0 {
            let tx = v0_tx(self.e, &ixs, payer_kp, &signer_kps, self.alt.as_ref().unwrap());
            if bincode::serialize(&tx).unwrap().len() > 1232 {
                self.skipped += 1;
                println!("#{:04} SKIP (transaction over 1232 bytes) {}", self.step, line);
                return;
            }
            exec_v0(self.e, &tx)
        } else {
            let bh = self.e.c.get_latest_blockhash().expect("blockhash");
            let tx = self.e.build(&ixs, payer_kp, &signer_kps, bh);
            if bincode::serialize(&tx).unwrap().len() > 1232 {
                self.skipped += 1;
                println!("#{:04} SKIP (transaction over 1232 bytes) {}", self.step, line);
                return;
            }
            self.e.send_tx(&tx)
        };

        let mut problems: Vec<(String, Vec<Pubkey>)> = vec![];
        let want_s = match &want {
            Ok(_) => "ok".to_string(),
            Err(f) => format!("{}@{}", self.describe_fail(f), ix_index),
        };
        let got_s = if !out.landed {
            format!("NOT LANDED {}", out.err_text())
        } else if out.ok() {
            "ok".into()
        } else {
            let f = Fail { code: out.custom().map(|c| c.1).unwrap_or(u32::MAX), prog: out.failed_program().unwrap_or_default(), name: None };
            format!("{}({}) by {}@{} [{}]", out.error_name().unwrap_or_else(|| "-".into()), f.code,
                    self.describe_fail(&f).rsplit(" by ").next().unwrap_or(""), out.custom().map(|c| c.0 as i64).unwrap_or(-1), out.err_text())
        };
        let outcome_ok = match &want {
            Ok(_) => out.ok(),
            Err(f) => out.landed && out.custom() == Some((ix_index, f.code)) && out.failed_program() == Some(f.prog)
                && out.error_name().as_deref() == f.name,
        };
        let key = format!("{} {}", act.op(), match &want { Ok(_) => "ok".into(), Err(f) => self.describe_fail(f) });
        *self.hist.entry(key).or_default() += 1;
        if !outcome_ok {
            problems.push((format!("outcome: want {} got {}", want_s, got_s), touched.iter().copied().collect()));
        }
        let emitted = self.events_of(&out);
        let mut adopt = !outcome_ok;
        if out.ok() {
            if let Some((signer, spec)) = self.spec_controller(&act) {
                if Some(signer) != spec {
                    problems.push((format!("spec: accepted signer {} but the controller by the one-level Vault rule is {}", self.short(&signer),
                                           self.short_o(&spec)), touched.iter().copied().collect()));
                }
            }
        }
        match (&want, out.ok()) {
            (Ok(p), true) => {
                let (ev, credit) = self.apply(&act, *p, out.block_time.unwrap_or_default());
                if emitted != ev {
                    problems.push((format!("events: want {:?} got {:?}", ev.iter().map(|e| e.0).collect::<Vec<_>>(),
                                           emitted.iter().map(|e| e.0).collect::<Vec<_>>()), touched.iter().copied().collect()));
                    for (a, b) in ev.iter().zip(emitted.iter()) {
                        if a != b {
                            problems.push((format!("event {} fields differ: want {:?} got {:?}", a.0, a.1, b.1), vec![]));
                        }
                    }
                }
                if let (Some((k, rent)), Some(b0)) = (credit, bal_before) {
                    let b1 = self.e.c.get_balance_with_commitment(&k, CommitmentConfig::confirmed()).unwrap().value;
                    let expected = b0 as i128 + rent as i128 - if k == fee { (FEE_PER_SIGNATURE * sigs) as i128 } else { 0 };
                    if b1 as i128 != expected {
                        problems.push((format!("close: {} expected {} lamports (rent {}), has {}", self.short(&k), expected, rent, b1), vec![k]));
                    }
                }
            }
            (_, false) if !emitted.is_empty() => {
                problems.push((format!("failed transaction emitted events {:?}", emitted.iter().map(|e| e.0).collect::<Vec<_>>()), vec![]));
            }
            (Err(_), true) => adopt = true,
            _ => {}
        }
        problems.extend(self.check_state(false));
        let verdict = if problems.is_empty() { "OK" } else { "MISMATCH" };
        println!("#{:04} {} | want {} | got {} | {}", self.step, line, want_s, got_s, verdict);
        self.trace.push((self.step, format!("{} | want {} | got {}", line, want_s, got_s), touched));
        if !problems.is_empty() {
            self.mismatches += 1;
            let mut keys: BTreeSet<Pubkey> = BTreeSet::new();
            for (p, ks) in &problems {
                println!("  MISMATCH: {}", p);
                keys.extend(ks.iter().copied());
            }
            println!("  trace (earlier steps that touched the accounts involved):");
            let lines: Vec<&(usize, String, BTreeSet<Pubkey>)> = self.trace.iter().filter(|(_, _, t)| !t.is_disjoint(&keys)).collect();
            for (n, l, _) in lines.iter().rev().take(40).rev() {
                println!("    #{:04} {}", n, l);
            }
            adopt = true;
        }
        if adopt {
            self.check_state(true);
        }
        self.step += 1;
    }
    /// For an accepted step, (the signer the program accepted, the controller the spec gives): the
    /// Asset's owner, or the owner of the Asset whose Vault owns it, found by scanning the known
    /// Assets rather than from the holder the client passed (Core and memo-home: the owner)
    fn spec_controller(&self, act: &Act) -> Option<(Pubkey, Option<Pubkey>)> {
        let ctrl = |a: &Pubkey| self.m.assets.contains_key(a).then(|| self.discover(a).1);
        let of_name = |n: &str| -> Option<Pubkey> {
            let r = self.reg(n)?;
            match r.kind {
                NFT => ctrl(&r.asset),
                _ => ctrl(&self.m.regs.get(&r.parent)?.0.asset),
            }
        };
        Some(match act {
            Act::CreateSubid { ctrl: c, parent, .. } | Act::DeleteSubid { ctrl: c, parent, .. } | Act::IssueSubid { ctrl: c, parent, .. } => {
                (*c, of_name(parent))
            }
            Act::SetTarget { ctrl: c, name, .. } | Act::UpdateUri { ctrl: c, name, .. } => (*c, of_name(name)),
            Act::SetPrimary { signer, target, .. } | Act::ClearPrimary { signer, target, .. } => (*signer, match target {
                Target::Address(a) => Some(*a),
                Target::Asset(a) => ctrl(a),
            }),
            // the owner, or the holder of the TransferDelegate (the model is still the state before the step)
            Act::Transfer { signer, asset, .. } => (*signer, match self.m.del.get(asset) {
                Some(Some(d)) if d == signer => Some(*d),
                _ => self.m.assets.get(asset).map(|a| a.owner),
            }),
            Act::Withdraw { owner, holder, .. } => (*owner, self.m.assets.get(holder).map(|a| a.owner)),
            Act::Delegate { owner, asset, .. } => (*owner, self.m.assets.get(asset).map(|a| a.owner)),
            Act::Register { .. } | Act::Prefund { .. } => return None,
        })
    }
    /// Accounts too common to tell steps apart in a trace
    fn common(&self, k: &Pubkey) -> bool {
        [CORE, system_program(), self.id, self.home, self.e.mint, self.e.gbs, self.e.burn_prog, t22(), IXS_SYSVAR, self.authority,
         self.home_authority, self.collection, self.config, spl_memo::ID].contains(k)
    }

    // ---------------- comparing with the chain ----------------
    fn fetch(&self, keys: &[Pubkey]) -> HashMap<Pubkey, Account> {
        let mut m = HashMap::new();
        for chunk in keys.chunks(100) {
            let v = self.e.c.get_multiple_accounts_with_commitment(chunk, CommitmentConfig::confirmed()).expect("getMultipleAccounts").value;
            for (k, a) in chunk.iter().zip(v) {
                if let Some(a) = a.filter(|a| a.lamports > 0) {
                    m.insert(*k, a);
                }
            }
        }
        m
    }

    /// Compares the model with the chain (and checks the chain-only invariants and the verified
    /// pairs); with `adopt`, the model takes the chain's state instead
    fn check_state(&mut self, adopt: bool) -> Vec<(String, Vec<Pubkey>)> {
        let mut keys: BTreeSet<Pubkey> = BTreeSet::new();
        let names: Vec<(Pubkey, String)> = self.m.names.iter().map(|(k, n)| (*k, n.clone())).collect();
        let mut assets: BTreeSet<Pubkey> = self.m.assets.keys().copied().collect();
        for (k, n) in &names {
            keys.insert(*k);
            assets.insert(self.asset_addr(n));
        }
        for a in &assets {
            keys.insert(*a);
            keys.insert(self.vstate_addr(a).0);
        }
        let mut idx_keys: BTreeSet<Pubkey> = self.m.idx.keys().copied().collect();
        for (k, (r, _)) in &self.m.regs {
            if r.kind == NFT {
                for i in 0..=r.nft + 1 {
                    idx_keys.insert(self.idx_addr(k, i).0);
                }
            }
        }
        keys.extend(idx_keys.iter().copied());
        let targets: Vec<Target> = self.m.targets.iter().copied().collect();
        for t in &targets {
            keys.insert(self.prim_addr(t).0);
        }
        let actors: Vec<Pubkey> = self.actors.iter().map(|a| a.pubkey()).collect();
        for a in &actors {
            keys.insert(self.e.ata(a));
            keys.insert(self.stats_pda(a));
        }
        keys.insert(self.e.mint);
        keys.insert(self.e.gbs);
        let keys: Vec<Pubkey> = keys.into_iter().collect();
        let ch = self.fetch(&keys);
        let mut p: Vec<(String, Vec<Pubkey>)> = vec![];

        // Registries, byte for byte
        for (k, n) in &names {
            let chain = ch.get(k);
            let model = self.m.regs.get(k).cloned();
            let same = match (&model, chain) {
                (Some((_, raw)), Some(a)) => a.owner == self.id && a.lamports == self.lamports_of(k, raw.len()) && a.data == *raw,
                (None, None) => !self.m.funded.contains_key(k),
                (None, Some(a)) => self.m.funded.get(k) == Some(&a.lamports) && a.owner == system_program() && a.data.is_empty(),
                _ => false,
            };
            if !same {
                let first_diff = match (&model, chain) {
                    (Some((_, raw)), Some(a)) => raw.iter().zip(a.data.iter()).position(|(x, y)| x != y),
                    _ => None,
                };
                let related = model.as_ref().map(|(r, _)| vec![*k, r.parent, r.asset]).unwrap_or_else(|| vec![*k]);
                p.push((format!("Registry {}: model {:?} chain {:?} (lamports {:?}, len {:?}, first differing byte {:?})", n,
                                model.as_ref().map(|m| &m.0), chain.and_then(|a| MReg::parse(&a.data)), chain.map(|a| a.lamports),
                                chain.map(|a| a.data.len()), first_diff), related));
                if adopt {
                    self.adopt_lamports(k, chain, REG_BASE + n.len());
                    match chain.and_then(|a| MReg::parse(&a.data).map(|r| (r, a.data.clone()))) {
                        Some(e) => self.m.regs.insert(*k, e),
                        None => self.m.regs.remove(k),
                    };
                }
            }
        }
        // Core Assets and Vault States
        for a in &assets {
            let chain = ch.get(a).filter(|x| x.owner == CORE).and_then(|x| parse_core_asset(&x.data));
            let model = self.m.assets.get(a).cloned();
            if chain != model {
                p.push((format!("Asset {}: model {:?} chain {:?}", self.short(a), model, chain), vec![*a]));
                if adopt {
                    match chain {
                        Some(c) => {
                            self.vault_rev.insert(self.vault(a), *a);
                            self.m.targets.insert(Target::Asset(*a));
                            self.m.assets.insert(*a, c)
                        }
                        None => self.m.assets.remove(a),
                    };
                }
            }
            let (vk, _) = self.vstate_addr(a);
            let chain = ch.get(&vk).filter(|x| x.owner != system_program()).map(|x| (x.owner, x.data.clone()));
            let model = self.m.vstate.get(a).map(|(n, b)| (self.home, vstate_bytes(*n, *b)));
            if chain != model {
                p.push((format!("Vault State of {}: model {:?} chain {:?}", self.short(a), model, chain), vec![*a, vk]));
                if adopt {
                    match chain.filter(|c| c.1.len() == 17) {
                        Some((_, d)) => self.m.vstate.insert(*a, (u64::from_le_bytes(d[8..16].try_into().unwrap()), d[16])),
                        None => self.m.vstate.remove(a),
                    };
                }
            }
        }
        // SubId Index entries
        for k in &idx_keys {
            let chain = ch.get(k).map(|x| (x.owner, x.lamports, x.data.clone()));
            let model = match self.m.idx.get(k) {
                Some((_, _, r, b)) => Some((self.id, self.lamports_of(k, 41), idx_bytes(r, *b))),
                None => self.m.funded.get(k).map(|l| (system_program(), *l, vec![])),
            };
            if chain != model {
                p.push((format!("SubId Index {}: model {:?} chain {:?}", self.short(k), self.m.idx.get(k), chain.as_ref().map(|c| c.2.clone())),
                        vec![*k, self.m.idx.get(k).map(|e| e.0).unwrap_or_default()]));
                if adopt {
                    self.adopt_lamports(k, ch.get(k), 41);
                    self.m.idx.remove(k);
                    // entries are only ever created at the parent's counter: find which one this is
                    if let Some((_, _, d)) = chain.filter(|c| c.2.len() == 41) {
                        let found = self.m.regs.iter().flat_map(|(pk, (r, _))| (0..=r.nft + 1).map(move |i| (*pk, i)))
                            .find(|(pk, i)| self.idx_addr(pk, *i).0 == *k);
                        if let Some((pk, i)) = found {
                            self.m.idx.insert(*k, (pk, i, Pubkey::new_from_array(d[8..40].try_into().unwrap()), d[40]));
                        }
                    }
                }
            }
        }
        // Primary IDs
        for t in &targets {
            let (pa, _) = self.prim_addr(t);
            let chain = ch.get(&pa).map(|x| (x.owner, x.lamports, x.data.clone()));
            let model = match self.m.prims.get(&pa) {
                Some(m) => Some((self.id, self.lamports_of(&pa, PRIM_LEN), m.bytes())),
                None => self.m.funded.get(&pa).map(|l| (system_program(), *l, vec![])),
            };
            if chain != model {
                p.push((format!("Primary ID of {}: model {:?} chain {:?}", self.short_t(t), self.m.prims.get(&pa),
                                chain.as_ref().and_then(|c| MPrim::parse(&c.2))), vec![pa, t.key()]));
                if adopt {
                    self.adopt_lamports(&pa, ch.get(&pa), PRIM_LEN);
                    match chain.and_then(|c| MPrim::parse(&c.2)) {
                        Some(m) => self.m.prims.insert(pa, m),
                        None => self.m.prims.remove(&pa),
                    };
                }
            }
        }
        // MEMO balances, supply, burn statistics
        for a in &actors {
            let ata = self.e.ata(a);
            let bal = ch.get(&ata).filter(|x| x.data.len() >= 72).map(|x| u64::from_le_bytes(x.data[64..72].try_into().unwrap()));
            if bal != self.m.bal.get(a).copied() {
                p.push((format!("MEMO balance of {}: model {:?} chain {:?}", self.short(a), self.m.bal.get(a), bal), vec![*a, ata]));
                if adopt {
                    self.m.bal.insert(*a, bal.unwrap_or(0));
                }
            }
            let st = ch.get(&self.stats_pda(a)).filter(|x| x.data.len() >= 64).map(|x| {
                let d = &x.data;
                UserStats {
                    user: Pubkey::new_from_array(d[8..40].try_into().unwrap()),
                    total: u64::from_le_bytes(d[40..48].try_into().unwrap()),
                    count: u64::from_le_bytes(d[48..56].try_into().unwrap()),
                    last_time: i64::from_le_bytes(d[56..64].try_into().unwrap()),
                }
            });
            if st != self.m.stats.get(a).copied() {
                p.push((format!("burn stats of {}: model {:?} chain {:?}", self.short(a), self.m.stats.get(a), st), vec![*a, ata]));
                if adopt {
                    self.m.stats.insert(*a, st.unwrap_or_default());
                }
            }
        }
        let supply = ch.get(&self.e.mint).map(|x| u64::from_le_bytes(x.data[36..44].try_into().unwrap())).unwrap_or(0);
        if supply != self.m.supply {
            p.push((format!("supply: model {} chain {}", self.m.supply, supply), vec![]));
            self.m.supply = if adopt { supply } else { self.m.supply };
        }
        let global = ch.get(&self.e.gbs).map(|x| {
            let d = &x.data;
            GlobalStats {
                total: u128::from_le_bytes(d[8..24].try_into().unwrap()),
                count: u64::from_le_bytes(d[24..32].try_into().unwrap()),
                last_time: i64::from_le_bytes(d[32..40].try_into().unwrap()),
            }
        }).unwrap_or_default();
        if global != self.m.global {
            p.push((format!("global burn stats: model {:?} chain {:?}", self.m.global, global), vec![]));
            if adopt {
                self.m.global = global;
            }
        }
        if adopt {
            // the history follows the adopted records
            self.m.fwd_hist = self.m.regs.iter().filter_map(|(k, (r, _))| r.target.map(|t| (*k, (t, r.set_by)))).collect();
            self.m.prim_hist = self.m.prims.values().map(|m| (m.target, (m.registry, m.set_by))).collect();
        }
        p.extend(self.invariants(&names, &ch));
        p.extend(self.verified(&names, &ch));
        p
    }

    fn adopt_lamports(&mut self, k: &Pubkey, chain: Option<&Account>, len: usize) {
        match chain {
            Some(a) if a.lamports != self.rent(len) || a.owner == system_program() => self.m.funded.insert(*k, a.lamports),
            _ => self.m.funded.remove(k),
        };
    }

    /// Invariants computed from the chain alone (no model state)
    fn invariants(&self, names: &[(Pubkey, String)], ch: &HashMap<Pubkey, Account>) -> Vec<(String, Vec<Pubkey>)> {
        let mut p = vec![];
        let regs: BTreeMap<Pubkey, MReg> = names.iter().filter_map(|(k, _)| ch.get(k).and_then(|a| MReg::parse(&a.data)).map(|r| (*k, r))).collect();
        for (k, n) in names {
            let Some(r) = regs.get(k) else { continue };
            let mut bad: Vec<String> = vec![];
            if r.memo_id != *n || !valid_memo_id(&r.memo_id) || r.bump != self.reg_bump(n) {
                bad.push(format!("holds {:?} (bump {}) at the address of {:?}", r.memo_id, r.bump, n));
            }
            if r.kind == NFT {
                let core = ch.get(&r.asset).filter(|a| a.owner == CORE).and_then(|a| parse_core_asset(&a.data));
                if r.asset != self.asset_addr(&r.memo_id) || core.as_ref().is_none_or(|c| c.name != r.memo_id || c.col != Some(self.collection)) {
                    bad.push(format!("NFT whose Asset {} is {:?}", self.short(&r.asset), core));
                }
            } else if r.kind != RECORD || r.asset != Pubkey::default() || r.nft != 0 || r.parent == Pubkey::default() {
                bad.push("malformed Record".into());
            }
            if r.target.is_none() != (r.set_by == Pubkey::default()) {
                bad.push(format!("target {:?} with target_set_by {}", r.target, r.set_by));
            }
            // the fixed-size encoding fills the account; without a target bytes 90..188 are zero
            let d = &ch[k].data;
            if d.len() != REG_BASE + r.memo_id.len() || (r.target.is_none() && d[90..188].iter().any(|b| *b != 0)) {
                bad.push(format!("not its canonical encoding (len {})", d.len()));
            }
            match (r.parent == Pubkey::default(), regs.get(&r.parent)) {
                (true, _) => {
                    if r.memo_id.matches('.').count() != 1 {
                        bad.push("no parent but not a second-level ID".into());
                    }
                }
                (false, Some(pr)) => {
                    if pr.kind != NFT || !r.memo_id.strip_suffix(&pr.memo_id).is_some_and(|l| l.ends_with('.') && valid_label(&l[..l.len() - 1])) {
                        bad.push(format!("parent {:?} (kind {}) does not name it", pr.memo_id, pr.kind));
                    }
                }
                (false, None) => bad.push("parent Registry missing".into()),
            }
            if r.kind == NFT {
                let recs = regs.values().filter(|c| c.parent == *k && c.kind == RECORD).count() as u32;
                let nfts: Vec<Pubkey> = regs.iter().filter(|(_, c)| c.parent == *k && c.kind == NFT).map(|(ck, _)| *ck).collect();
                if r.rec != recs {
                    bad.push(format!("record_subids {} but {} Record SubIds exist", r.rec, recs));
                }
                if r.nft as usize != nfts.len() {
                    bad.push(format!("nft_subids_issued {} but {} NFT SubIds exist", r.nft, nfts.len()));
                }
                let entries: Vec<Option<Pubkey>> = (0..=r.nft).map(|i| {
                    ch.get(&self.idx_addr(k, i).0).filter(|a| a.data.len() == 41).map(|a| Pubkey::new_from_array(a.data[8..40].try_into().unwrap()))
                }).collect();
                let listed: BTreeSet<Pubkey> = entries[..r.nft as usize].iter().flatten().copied().collect();
                if entries[r.nft as usize].is_some() || listed.len() != r.nft as usize || listed != nfts.iter().copied().collect() {
                    bad.push(format!("SubId Index {:?} does not list the {} issued SubIds exactly", entries, nfts.len()));
                }
            }
            if !bad.is_empty() {
                p.push((format!("invariant: Registry {}: {}", n, bad.join("; ")), vec![*k, r.parent, r.asset]));
            }
        }
        // every Asset in the MEMO ID Collection is the Asset of the NFT Registry of its name
        for (k, a) in ch.iter().filter(|(_, a)| a.owner == CORE).filter_map(|(k, a)| parse_core_asset(&a.data).map(|c| (k, c))) {
            if a.col == Some(self.collection) {
                let r = ch.get(&self.reg_addr(&a.name)).and_then(|x| MReg::parse(&x.data));
                if !r.as_ref().is_some_and(|r| r.kind == NFT && r.asset == *k && r.memo_id == a.name) || *k != self.asset_addr(&a.name) {
                    p.push((format!("invariant: Asset {} named {:?} is not the Asset of that ID's NFT Registry ({:?})", k, a.name, r), vec![*k]));
                }
            }
        }
        for t in &self.m.targets {
            let (pa, bump) = self.prim_addr(t);
            if let Some(m) = ch.get(&pa).and_then(|a| MPrim::parse(&a.data)) {
                if m.target != *t || m.bump != bump {
                    p.push((format!("invariant: the Primary ID of {} holds {:?} (bump {})", self.short_t(t), m.target, m.bump), vec![pa]));
                }
            }
        }
        p
    }

    /// Verified pairs (Registry, Target): from the chain with the documented rule (holders found
    /// by scanning the known Assets), and from the model's history of signers with the model's
    /// owners; both must agree
    fn verified(&mut self, names: &[(Pubkey, String)], ch: &HashMap<Pubkey, Account>) -> Vec<(String, Vec<Pubkey>)> {
        // from the chain
        let assets: BTreeMap<Pubkey, Pubkey> = self.m.assets.keys().chain(self.vault_rev.values())
            .filter_map(|k| ch.get(k).filter(|a| a.owner == CORE).and_then(|a| parse_core_asset(&a.data)).map(|a| (*k, a.owner))).collect();
        let vaults: HashMap<Pubkey, Pubkey> = assets.keys().map(|k| (self.vault(k), *k)).collect();
        let ctrl = |a: &Pubkey| -> Option<Pubkey> {
            let o = *assets.get(a)?;
            Some(match vaults.get(&o) { Some(h) => *assets.get(h)?, None => o })
        };
        let regs: BTreeMap<Pubkey, MReg> = names.iter().filter_map(|(k, _)| ch.get(k).and_then(|a| MReg::parse(&a.data)).map(|r| (*k, r))).collect();
        let mut chain_set: BTreeSet<(Pubkey, Target)> = BTreeSet::new();
        for t in &self.m.targets {
            let Some(pr) = ch.get(&self.prim_addr(t).0).and_then(|a| MPrim::parse(&a.data)) else { continue };
            let t_ctrl = match t { Target::Address(a) => Some(*a), Target::Asset(a) => ctrl(a) };
            let Some(r) = regs.get(&pr.registry) else { continue };
            let fwd = r.target.filter(|_| self.reg_ctrl(&regs, &pr.registry, &ctrl) == Some(r.set_by));
            if fwd == Some(*t) && t_ctrl == Some(pr.set_by) {
                chain_set.insert((pr.registry, *t));
            }
        }
        // from the history
        let mregs: BTreeMap<Pubkey, MReg> = self.m.regs.iter().map(|(k, (r, _))| (*k, r.clone())).collect();
        let mctrl = |a: &Pubkey| -> Option<Pubkey> {
            if !self.m.assets.contains_key(a) {
                return None;
            }
            Some(self.discover(a).1)
        };
        let mut hist_set: BTreeSet<(Pubkey, Target)> = BTreeSet::new();
        for (t, (rk, w2)) in &self.m.prim_hist {
            let Some((ft, w)) = self.m.fwd_hist.get(rk) else { continue };
            let t_ctrl = match t { Target::Address(a) => Some(*a), Target::Asset(a) => mctrl(a) };
            if ft == t && self.reg_ctrl(&mregs, rk, &mctrl) == Some(*w) && t_ctrl == Some(*w2) {
                hist_set.insert((*rk, *t));
            }
        }
        // from accounts read by address only (F32): each controller re-derived through the holder
        // the record names; never a pair the rule does not verify
        let by_addr = |a: &Pubkey, h: &Pubkey| -> Option<Pubkey> {
            let o = *assets.get(a)?;
            if *h != Pubkey::default() && o == self.vault(h) { assets.get(h).copied() } else { Some(o) }
        };
        let mut addr_set: BTreeSet<(Pubkey, Target)> = BTreeSet::new();
        for t in &self.m.targets {
            let Some(pr) = ch.get(&self.prim_addr(t).0).and_then(|a| MPrim::parse(&a.data)) else { continue };
            let Some(r) = regs.get(&pr.registry) else { continue };
            let deciding = match r.kind {
                NFT => r.asset,
                _ => match regs.get(&r.parent) { Some(p) => p.asset, None => continue },
            };
            let t_ctrl = match t { Target::Address(a) => Some(*a), Target::Asset(a) => by_addr(a, &pr.set_holder) };
            if r.target == Some(*t) && by_addr(&deciding, &r.holder) == Some(r.set_by) && t_ctrl == Some(pr.set_by) {
                addr_set.insert((pr.registry, *t));
            }
        }
        self.verified_seen += chain_set.len();
        self.addr_seen += addr_set.len();
        self.verified_max = self.verified_max.max(chain_set.len());
        let show = |s: &BTreeSet<(Pubkey, Target)>| s.iter().map(|(r, t)| format!("{}<->{}", self.short(r), self.short_t(t))).collect::<Vec<_>>();
        let mut p = vec![];
        if !addr_set.is_subset(&chain_set) {
            let keys: Vec<Pubkey> = addr_set.difference(&chain_set).flat_map(|(r, t)| [*r, t.key()]).collect();
            p.push((format!("verified by address only but not by the rule: {:?}", show(&addr_set.difference(&chain_set).copied().collect())), keys));
        }
        if chain_set != hist_set {
            let keys: Vec<Pubkey> = chain_set.symmetric_difference(&hist_set).flat_map(|(r, t)| [*r, t.key()]).collect();
            p.push((format!("verified pairs: chain {:?} history {:?}", show(&chain_set), show(&hist_set)), keys));
        }
        p
    }

    // ---------------- setup ----------------
    fn fund(&self, kp: &Keypair, memo_units: u64) {
        let e = self.e;
        let k = kp.pubkey();
        e.airdrop(&k, 50);
        let mut ixs = vec![create_associated_token_account(&e.wallet.pubkey(), &k, &e.mint, &t22())];
        ixs.push(spl_token_2022::instruction::transfer_checked(&t22(), &e.ata(&e.wallet.pubkey()), &e.mint, &e.ata(&k), &e.wallet.pubkey(), &[],
                                                               memo_units, 6).unwrap());
        e.must("fund an actor with MEMO", &ixs, &e.wallet, &[]);
        e.must("init the actor's burn stats", &[e.init_stats_ix(&k)], kp, &[]);
    }
    fn setup(&mut self) {
        let e = self.e;
        let w = &e.wallet;
        if e.data(&self.burn_guard).is_none() {
            e.must("init memo-home Burn Guard", &[Instruction::new_with_bytes(self.home, &disc("init_burn_guard"), vec![
                AccountMeta::new(w.pubkey(), true),
                AccountMeta::new(self.burn_guard, false),
                AccountMeta::new_readonly(system_program(), false),
            ])], w, &[]);
        }
        if e.data(&self.collection).is_none() {
            e.must("init the MEMO ID Collection", &[self.ix("init", vec![], vec![
                AccountMeta::new(w.pubkey(), true),
                AccountMeta::new_readonly(self.authority, false),
                AccountMeta::new(self.collection, false),
                AccountMeta::new_readonly(self.home_authority, false),
                AccountMeta::new_readonly(self.burn_guard, false),
                AccountMeta::new(self.config, false),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new_readonly(self.home, false),
                AccountMeta::new_readonly(system_program(), false),
            ])], w, &[]);
        }
        self.m.supply = e.supply();
        let price = self.price_now(6_900);
        for (i, a) in self.actors.iter().enumerate() {
            // the last actor can pay for two burns only
            self.fund(a, if i == 3 { 2 * price + 1 } else { 6_000 * UNIT });
        }
        // the plain Core Asset X (no Collection) at the key xk, owned by a0
        let mut d = vec![0u8, 0];
        bstr(&mut d, "plain core asset");
        bstr(&mut d, "https://example.com/plain.json");
        d.push(0);
        let a0 = self.actors[0].pubkey();
        e.must("create the plain Core Asset X", &[Instruction {
            program_id: CORE,
            accounts: vec![
                AccountMeta::new(self.x(), true),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new(w.pubkey(), true),
                AccountMeta::new_readonly(a0, false),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new_readonly(system_program(), false),
                AccountMeta::new_readonly(CORE, false),
            ],
            data: d,
        }], w, &[&self.xk]);
        let shared = MemoIdAddresses::new(self.id, self.home);
        self.alt = Some(create_frozen_alt(e, shared.lookup_table_addresses(e.mint, e.burn_prog, e.gbs)));

        self.m.supply = e.supply();
        self.m.global = e.global_stats().unwrap_or_default();
        for a in self.actors.iter().map(|a| a.pubkey()).collect::<Vec<_>>() {
            self.m.bal.insert(a, e.balance(&e.ata(&a)).unwrap_or(0));
            self.m.stats.insert(a, e.user_stats(&a).unwrap_or_default());
            self.m.targets.insert(Target::Address(a));
        }
        let x = self.x();
        self.add_asset(x, MAsset { owner: a0, col: None, name: "plain core asset".into(), uri: "https://example.com/plain.json".into() }, false);
        self.m.targets.insert(Target::Address(x));
        self.m.targets.insert(Target::Asset(a0));
        println!("memo-id {}  memo-home {}  Collection {}  supply {} units, 69 mints = {} units", self.id, self.home, self.collection,
                 self.m.supply, price);
        for (i, a) in self.actors.iter().enumerate() {
            println!("actor a{} {}  MEMO {}", i, a.pubkey(), self.m.bal[&a.pubkey()]);
        }
        println!("X / xk {}  labels {:?}", x, self.pool);
        let problems = self.check_state(false);
        if !problems.is_empty() {
            for (p, _) in problems {
                println!("SETUP MISMATCH: {}", p);
            }
            std::process::exit(2);
        }
    }

    fn summary(&self) {
        println!("\n== fuzz-memo-id: {} steps, {} mismatched, {} skipped ==", self.step, self.mismatches, self.skipped);
        println!("verified pairs: {} observed over all steps, at most {} at once; {} of them verified from accounts read by address only",
                 self.verified_seen, self.verified_max, self.addr_seen);
        println!("final state: {} Registries ({} NFT, {} Record), {} Assets, {} Primary IDs, {} SubId Index entries, {} names checked",
                 self.m.regs.len(), self.names_of(Some(NFT)).len(), self.names_of(Some(RECORD)).len(), self.m.assets.len(), self.m.prims.len(),
                 self.m.idx.len(), self.m.names.len());
        println!("predicted outcomes:");
        for (k, n) in &self.hist {
            println!("  {:5}  {}", n, k);
        }
    }
}

fn base36(mut n: u64) -> String {
    const CH: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = vec![];
    loop {
        s.push(CH[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Create the lookup table with the fixed accounts and freeze it
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

fn main() {
    let (mut seed, mut steps, mut tag, mut stop, mut edge) = (1u64, 200usize, String::new(), false, false);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = || args.get(i + 1).cloned().unwrap_or_else(|| { eprintln!("{} needs a value", args[i]); std::process::exit(2) });
        match args[i].as_str() {
            "--seed" => seed = val().parse().expect("--seed <u64>"),
            "--steps" => steps = val().parse().expect("--steps <n>"),
            "--tag" => tag = val(),
            "--edge" => edge = true,
            "--stop-on-mismatch" => stop = true,
            other => {
                eprintln!("unknown argument {} (--seed <u64> --steps <n> [--tag <a-z0-9>] [--edge] [--stop-on-mismatch])", other);
                std::process::exit(2);
            }
        }
        i += if matches!(args[i].as_str(), "--edge" | "--stop-on-mismatch") { 1 } else { 2 };
    }
    assert!(tag.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "--tag: a-z and 0-9 only (it goes into labels)");
    let started = std::time::Instant::now();
    let e = Env::new("fuzz-memo-id");
    let mut f = Fz::new(&e, seed, &tag, edge);
    println!("seed {} steps {} tag {:?}{}", seed, steps, tag, if edge { " edge" } else { "" });
    f.setup();
    // every actor starts with one ID
    for i in 0..4 {
        let act = Act::Register { payer: f.actors[i].pubkey(), label: f.pool[i].clone(), uri: URI.into(), memo: MemoV::Ok, cu_last: i % 2 == 1 };
        f.run_step(Some(act));
    }
    while f.step < steps {
        f.run_step(None);
        if stop && f.mismatches > 0 {
            break;
        }
    }
    f.summary();
    println!("elapsed {:.0} s", started.elapsed().as_secs_f64());
    std::process::exit(if f.mismatches == 0 { 0 } else { 1 });
}
