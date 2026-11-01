//! Shared harness for the on-chain regression suites (clients/regression/src/regression-*.rs).
//!
//! The suites only run against a local solana-test-validator (scripts/local-validator.sh,
//! scripts/run-regression-tests.sh). Every transaction is sent with skip_preflight, so failing
//! transactions land on-chain, and each case asserts the exact outcome:
//!   - rejections: the exact `InstructionError [index, Custom(code)]` (or other exact error; with
//!     `Expect::Raised` also the Anchor error name and the failing program, with `Expect::RaisedBy`
//!     and `Expect::BuiltinBy` the failing program) and no change to any watched state (balances,
//!     supply, MintState, user and global burn stats, and with `reject_untouched` every account the
//!     transaction references except the fee payer)
//!   - successes: exact token deltas, the minted amount against an independent curve reference,
//!     memo-mint's success log line, MintState = (fingerprint of this transaction, its slot),
//!     exact burn statistics deltas
//! Transactions are legacy, or v0 over an address lookup table (`mint_ok_v0`, `reject_v0`).
//! A suite exits with status 1 if any case fails.

use std::{cell::Cell, collections::HashSet, thread::sleep, time::{Duration, Instant}};

use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_client::{
    rpc_client::{RpcClient, SerializableTransaction},
    rpc_config::RpcSendTransactionConfig,
    rpc_request::RpcRequest,
};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::instruction as alt_ix;
#[allow(deprecated)]
use solana_sdk::{reserved_account_keys::ReservedAccountKeys, sysvar::instructions::construct_instructions_data};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{
        v0::{self, LoadedAddresses},
        AddressLookupTableAccount, Message, SanitizedMessage, SanitizedVersionedMessage, SimpleAddressLoader, VersionedMessage,
    },
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    transaction::{Transaction, VersionedTransaction},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id as ata_of, instruction::create_associated_token_account,
};

use memo_token_client::{get_program_id, get_rpc_url, get_token_mint, get_wallet_path, global_burn_stats_pda, mint_state_pda};

pub const UNIT: u64 = 1_000_000;
pub const ONE_T: u64 = 1_000_000_000_000 * UNIT;
pub const MAX_SUPPLY: u64 = 10_000_000_000_000 * UNIT;
pub const MAX_USER_GLOBAL_BURN: u64 = 18_000_000_000_000 * UNIT;
pub const IXS_SYSVAR: Pubkey = solana_sdk::sysvar::instructions::ID;
pub const LEGACY_MEMO_V1: Pubkey = solana_sdk::pubkey!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
pub const SPL_TOKEN_LEGACY: Pubkey = solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

// Anchor framework error codes
pub const ANCHOR_INSTRUCTION_FALLBACK_NOT_FOUND: u32 = 101;
pub const ANCHOR_INSTRUCTION_DID_NOT_DESERIALIZE: u32 = 102;
pub const ANCHOR_CONSTRAINT_MUT: u32 = 2000;
pub const ANCHOR_CONSTRAINT_SEEDS: u32 = 2006;
pub const ANCHOR_CONSTRAINT_ADDRESS: u32 = 2012;
pub const ANCHOR_DISCRIMINATOR_NOT_FOUND: u32 = 3001;
pub const ANCHOR_DISCRIMINATOR_MISMATCH: u32 = 3002;
pub const ANCHOR_ACCOUNT_NOT_ENOUGH_KEYS: u32 = 3005;
pub const ANCHOR_OWNED_BY_WRONG_PROGRAM: u32 = 3007;
pub const ANCHOR_INVALID_PROGRAM_ID: u32 = 3008;
pub const ANCHOR_ACCOUNT_NOT_SIGNER: u32 = 3010;
pub const ANCHOR_ACCOUNT_NOT_INITIALIZED: u32 = 3012;
// System program "account already in use" and Token-2022 errors
pub const SYSTEM_ACCOUNT_IN_USE: u32 = 0;
pub const TOKEN_INSUFFICIENT_FUNDS: u32 = 1;
pub const TOKEN_CPI_GUARD_BURN_BLOCKED: u32 = 43;

pub fn t22() -> Pubkey { spl_token_2022::id() }
pub fn system_program() -> Pubkey { solana_system_interface::program::ID }

/// Anchor instruction discriminator
pub fn disc(name: &str) -> Vec<u8> {
    Sha256::digest(format!("global:{}", name).as_bytes())[..8].to_vec()
}

/// Anchor account discriminator
pub fn account_disc(name: &str) -> Vec<u8> {
    Sha256::digest(format!("account:{}", name).as_bytes())[..8].to_vec()
}

/// Seed of the shadow MintState address. clients/smoke-test/src/prepare-local-validator.rs plants
/// a MintState-shaped account owned by memo-mint at sha256(this seed), which is not memo-mint's
/// MintState PDA (and has no known key), to test that memo-mint binds MintState to the PDA.
pub const SHADOW_MINT_STATE_SEED: &[u8] = b"memo-token regression: shadow MintState";
pub fn shadow_mint_state() -> Pubkey { Pubkey::new_from_array(Sha256::digest(SHADOW_MINT_STATE_SEED).into()) }

pub fn memo_ix(data: &[u8]) -> Instruction { Instruction::new_with_bytes(spl_memo::ID, data, vec![]) }
/// memo-mint's success log line for a mint of `amount` units at supply `supply_before` (units).
/// It does not name the recipient (D4 / O2b): the balance checks do
pub fn minted_log(amount: u64, supply_before: u64) -> String {
    format!("Program log: Minted {} units, supply before mint: {} units", amount, supply_before)
}
pub fn cu(units: u32) -> Instruction { ComputeBudgetInstruction::set_compute_unit_limit(units) }
pub fn b64(raw: &[u8]) -> Vec<u8> { general_purpose::STANDARD.encode(raw).into_bytes() }

/// Independent reference for the emission curve, written from the documented definition using the
/// decimal representation of the supply (no code shared with the program or with
/// clients/src/mint_curve.rs). Supply and result in units; None at or above the 10T cap or when
/// the reward would overshoot it.
///   <= 10M tokens: 1 token. Above: decade n (10M*10^n, 10M*10^(n+1)] pays (1 token / 10^n),
///   reduced by a tenth for every leading digit step: supply in (d*10^k, (d+1)*10^k] pays
///   base * (10 - d) / 10.
pub fn curve_reference(supply: u64) -> Option<u64> {
    if supply >= MAX_SUPPLY {
        return None;
    }
    if supply <= 10_000_000 * UNIT {
        return Some(UNIT);
    }
    let below = (supply - 1).to_string(); // 14..=19 digits here
    let decade = below.len() as u32 - 14;
    let digit = (below.as_bytes()[0] - b'0') as u64;
    let reward = 10u64.pow(6 - decade) * (10 - digit) / 10;
    if supply + reward > MAX_SUPPLY { None } else { Some(reward) }
}

/// SHA-256 of the instructions sysvar content (without the trailing 2-byte current index) of a
/// legacy message; this is what memo-mint stores in MintState. See `fingerprint_versioned`.
pub fn fingerprint(msg: &Message) -> [u8; 32] {
    fingerprint_versioned(&VersionedMessage::Legacy(msg.clone()), &LoadedAddresses::default())
}

/// `fingerprint` of a legacy or v0 message, built with the runtime's own code rather than a
/// re-implementation: the message is sanitized with every reserved account key active (as on the
/// local validator; a write lock requested on a reserved key, e.g. a sysvar or a builtin program,
/// is demoted to read-only, and so is a program id the message invokes), with `loaded` as the
/// addresses a v0 message loads from its lookup tables (writable ones, then read-only ones, in
/// lookup order). Its instructions are decompiled with the resulting signer / writable flags and
/// serialized by `construct_instructions_data`, as the runtime builds the instructions sysvar.
/// A message the runtime cannot sanitize never executes; its fingerprint is all zero.
pub fn fingerprint_versioned(msg: &VersionedMessage, loaded: &LoadedAddresses) -> [u8; 32] {
    #[allow(deprecated)]
    let reserved = ReservedAccountKeys::new_all_activated().active;
    fingerprint_with_reserved(msg, loaded, &reserved)
}

/// The fingerprint with `reserved` as the reserved account keys. With an empty set no write lock
/// on a reserved key is demoted: that is what this harness computed before (MEMO-MINT Revision 5
/// re-audit S07), which a suite can use to show that a case depends on the demotion.
pub fn fingerprint_with_reserved(msg: &VersionedMessage, loaded: &LoadedAddresses, reserved: &HashSet<Pubkey>) -> [u8; 32] {
    let sanitized = SanitizedVersionedMessage::try_new(msg.clone()).ok()
        .and_then(|m| SanitizedMessage::try_new(m, SimpleAddressLoader::Enabled(loaded.clone()), reserved).ok());
    let Some(m) = sanitized else { return [0; 32] };
    let data = construct_instructions_data(&m.decompile_instructions());
    Sha256::digest(&data[..data.len() - 2]).into()
}

/// Every account the instructions reference, once each, except the fee payer
fn referenced(ixs: &[Instruction], payer: &Keypair) -> Vec<Pubkey> {
    let mut keys: Vec<Pubkey> = vec![];
    for k in ixs.iter().flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey)) {
        if k != payer.pubkey() && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// Raw Borsh BurnMemo bytes with an explicit (possibly forged) payload length prefix
pub fn burn_memo_raw(version: u8, amount: u64, declared_len: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![version];
    v.extend(amount.to_le_bytes());
    v.extend(declared_len.to_le_bytes());
    v.extend(payload);
    v
}

/// Base64(Borsh(BurnMemo { version: 1, burn_amount, payload })) memo instruction for an app
pub fn app_burn_memo(amount: u64, payload: &[u8]) -> Instruction {
    memo_ix(&b64(&burn_memo_raw(1, amount, payload.len() as u32, payload)))
}

// ---------------------------------------------------------------------------------------------
// Transaction outcome
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Out {
    pub landed: bool,
    pub sig: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub err: Option<Value>,
    pub logs: Vec<String>,
    pub fp: [u8; 32],
}

impl Out {
    pub fn ok(&self) -> bool { self.landed && self.err.is_none() }
    pub fn err_text(&self) -> String { self.err.as_ref().map(|e| e.to_string()).unwrap_or_else(|| "none".into()) }
    /// (instruction index, custom code) of an `InstructionError [index, {"Custom": code}]`
    pub fn custom(&self) -> Option<(u64, u32)> {
        let ie = self.err.as_ref()?.get("InstructionError")?;
        let idx = ie.get(0)?.as_u64()?;
        let code = ie.get(1)?.get("Custom")?.as_u64()? as u32;
        Some((idx, code))
    }
    pub fn brief(&self) -> String {
        format!("{} slot={} err={}", if self.landed { "landed" } else { "NOT landed" }, self.slot, self.err_text())
    }
    pub fn minted_log_count(&self) -> usize { self.logs.iter().filter(|l| l.contains("Minted ")).count() }
    /// Compute units the last "Program <program> consumed N of M compute units" line reports (a
    /// program's top-level invocation comes after its CPIs)
    pub fn consumed(&self, program: &Pubkey) -> Option<u64> {
        let p = format!("Program {} consumed ", program);
        self.logs.iter().filter_map(|l| l.strip_prefix(&p)?.split(' ').next()?.parse().ok()).last()
    }
    /// The Anchor error named by the first "Error Code: <Name>." log line (the innermost one)
    pub fn error_name(&self) -> Option<String> {
        self.logs.iter().find_map(|l| l.split("Error Code: ").nth(1).map(|s| s.split('.').next().unwrap_or("").to_string()))
    }
    /// The program of the innermost failing frame: the first "Program <id> failed: ..." line
    pub fn failed_program(&self) -> Option<Pubkey> {
        self.logs.iter().find_map(|l| {
            let (id, tail) = l.strip_prefix("Program ")?.split_once(' ')?;
            if tail.starts_with("failed:") { id.parse().ok() } else { None }
        })
    }
}

/// Expected failure of a transaction
#[derive(Clone, Copy, Debug)]
pub enum Expect<'a> {
    /// InstructionError at this instruction index with this custom code
    Custom(u64, u32),
    /// Custom(index, code), raised as the Anchor error `name` with `program` as the innermost
    /// failing frame. Error numbers alone do not say which program failed: every Anchor program
    /// numbers its errors from 6000, and a CPI error surfaces under the caller's index.
    Raised(u64, u32, Pubkey, &'a str),
    /// Custom(index, code) with `program` as the innermost failing frame, for programs that log no
    /// Anchor error name (System, Token-2022): their numbers overlap, e.g. Custom(0) is System's
    /// AccountAlreadyInUse and Token-2022's NotRentExempt
    RaisedBy(u64, u32, Pubkey),
    /// InstructionError at this instruction index with this non-custom error, exactly, e.g.
    /// (0, "InvalidInstructionData"), with `program` as the innermost failing frame. The error
    /// alone does not say which program failed: a program and the programs it calls share the
    /// index, and they can raise the same ProgramError.
    BuiltinBy(u64, &'a str, Pubkey),
    /// any error whose JSON contains this text (non-custom errors, e.g. InvalidInstructionData)
    ErrContains(&'a str),
}

impl Expect<'_> {
    pub fn matches(&self, out: &Out) -> bool {
        if !out.landed || out.err.is_none() {
            return false;
        }
        match self {
            Expect::Custom(ix, code) => out.custom() == Some((*ix, *code)),
            Expect::Raised(ix, code, program, name) => out.custom() == Some((*ix, *code))
                && out.failed_program() == Some(*program) && out.error_name().as_deref() == Some(*name),
            Expect::RaisedBy(ix, code, program) => out.custom() == Some((*ix, *code)) && out.failed_program() == Some(*program),
            Expect::BuiltinBy(ix, name, program) => out.err == Some(json!({"InstructionError": [ix, name]}))
                && out.failed_program() == Some(*program),
            Expect::ErrContains(s) => out.err_text().contains(s),
        }
    }
    pub fn describe(&self) -> String {
        match self {
            Expect::Custom(ix, code) => format!("InstructionError [{}, Custom({})]", ix, code),
            Expect::Raised(ix, code, program, name) => format!("InstructionError [{}, Custom({})] = {} raised by {}", ix, code, name, program),
            Expect::RaisedBy(ix, code, program) => format!("InstructionError [{}, Custom({})] raised by {}", ix, code, program),
            Expect::BuiltinBy(ix, name, program) => format!("InstructionError [{}, {}] raised by {}", ix, name, program),
            Expect::ErrContains(s) => format!("error containing {:?}", s),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// State snapshots
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UserStats {
    pub user: Pubkey,
    pub total: u64,
    pub count: u64,
    pub last_time: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GlobalStats {
    pub total: u128,
    pub count: u64,
    pub last_time: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub supply: u64,
    pub mint_state: Option<([u8; 32], u64)>,
    pub global: Option<GlobalStats>,
    pub balances: Vec<(Pubkey, Option<u64>)>,
    pub users: Vec<(Pubkey, Option<UserStats>)>,
}

impl Snap {
    pub fn bal(&self, ta: &Pubkey) -> u64 {
        self.balances.iter().find(|(k, _)| k == ta).and_then(|(_, b)| *b).unwrap_or(0)
    }
    pub fn user(&self, u: &Pubkey) -> UserStats {
        self.users.iter().find(|(k, _)| k == u).and_then(|(_, s)| *s).unwrap_or_default()
    }
    pub fn global(&self) -> GlobalStats { self.global.unwrap_or_default() }
    pub fn diff(&self, o: &Snap) -> String {
        let mut v = vec![];
        if self.supply != o.supply {
            v.push(format!("supply {} -> {}", self.supply, o.supply));
        }
        if self.mint_state != o.mint_state {
            v.push(format!("mint_state slot {:?} -> {:?}", self.mint_state.map(|m| m.1), o.mint_state.map(|m| m.1)));
        }
        if self.global != o.global {
            v.push(format!("global {:?} -> {:?}", self.global, o.global));
        }
        for ((k, a), (_, b)) in self.balances.iter().zip(o.balances.iter()) {
            if a != b {
                v.push(format!("balance {} {:?} -> {:?}", k, a, b));
            }
        }
        for ((k, a), (_, b)) in self.users.iter().zip(o.users.iter()) {
            if a != b {
                v.push(format!("user stats {} {:?} -> {:?}", k, a, b));
            }
        }
        if v.is_empty() { "no state change".into() } else { v.join("; ") }
    }
}

// ---------------------------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------------------------

pub struct Env {
    pub c: RpcClient,
    pub wallet: Keypair,
    pub mint_prog: Pubkey,
    pub burn_prog: Pubkey,
    pub blog_prog: Pubkey,
    pub forum_prog: Pubkey,
    pub mint: Pubkey,
    pub mint_auth: Pubkey,
    pub mint_state: Pubkey,
    pub gbs: Pubkey,
    nonce: Cell<u64>,
    pass: Cell<u32>,
    fail: Cell<u32>,
    skip: Cell<u32>,
    suite: String,
}

impl Env {
    /// Connects to the configured RPC; refuses to run against anything but a local validator
    pub fn new(suite: &str) -> Env {
        let url = get_rpc_url();
        if !(url.contains("127.0.0.1") || url.contains("localhost")) {
            eprintln!("{} only runs against a local validator (MEMO_RPC_URL=http://127.0.0.1:8899), got {}", suite, url);
            std::process::exit(2);
        }
        let c = RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed());
        let wallet = read_keypair_file(get_wallet_path()).unwrap_or_else(|e| {
            eprintln!("cannot read wallet {}: {}", get_wallet_path(), e);
            std::process::exit(2);
        });
        let mint_prog = get_program_id("memo_mint").unwrap();
        let burn_prog = get_program_id("memo_burn").unwrap();
        let blog_prog = get_program_id("memo_blog").unwrap();
        let forum_prog = get_program_id("memo_forum").unwrap();
        let mint = get_token_mint("memo_token").unwrap();
        let mint_auth = Pubkey::find_program_address(&[b"mint_authority"], &mint_prog).0;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 % 1_000_000;
        let e = Env {
            c, wallet, mint_prog, burn_prog, blog_prog, forum_prog, mint, mint_auth,
            mint_state: mint_state_pda(), gbs: global_burn_stats_pda(),
            nonce: Cell::new(seed), pass: Cell::new(0), fail: Cell::new(0), skip: Cell::new(0), suite: suite.into(),
        };
        println!("== {} ==", suite);
        println!("RPC {}  wallet {}  memo-mint {}  memo-burn {}  mint {}", url, e.wallet.pubkey(), mint_prog, burn_prog, mint);
        e
    }

    pub fn nonce(&self) -> u64 {
        let v = self.nonce.get() + 1;
        self.nonce.set(v);
        v
    }
    pub fn ata(&self, owner: &Pubkey) -> Pubkey { ata_of(owner, &self.mint, &t22()) }
    pub fn stats_pda(&self, user: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"user_global_burn_stats", user.as_ref()], &self.burn_prog).0
    }

    // ---------------- reading state ----------------
    pub fn data(&self, k: &Pubkey) -> Option<Vec<u8>> {
        self.c.get_account_with_commitment(k, CommitmentConfig::confirmed()).ok()?.value.map(|a| a.data)
    }
    pub fn supply(&self) -> u64 {
        let d = self.data(&self.mint).expect("MEMO mint account");
        u64::from_le_bytes(d[36..44].try_into().unwrap())
    }
    pub fn balance(&self, ta: &Pubkey) -> Option<u64> {
        self.data(ta).filter(|d| d.len() >= 72).map(|d| u64::from_le_bytes(d[64..72].try_into().unwrap()))
    }
    pub fn user_stats(&self, user: &Pubkey) -> Option<UserStats> {
        self.data(&self.stats_pda(user)).filter(|d| d.len() >= 64).map(|d| UserStats {
            user: Pubkey::new_from_array(d[8..40].try_into().unwrap()),
            total: u64::from_le_bytes(d[40..48].try_into().unwrap()),
            count: u64::from_le_bytes(d[48..56].try_into().unwrap()),
            last_time: i64::from_le_bytes(d[56..64].try_into().unwrap()),
        })
    }
    pub fn global_stats(&self) -> Option<GlobalStats> {
        self.data(&self.gbs).filter(|d| d.len() >= 40).map(|d| GlobalStats {
            total: u128::from_le_bytes(d[8..24].try_into().unwrap()),
            count: u64::from_le_bytes(d[24..32].try_into().unwrap()),
            last_time: i64::from_le_bytes(d[32..40].try_into().unwrap()),
        })
    }
    pub fn mint_state_value(&self) -> Option<([u8; 32], u64)> {
        self.data(&self.mint_state).filter(|d| d.len() >= 48)
            .map(|d| (d[8..40].try_into().unwrap(), u64::from_le_bytes(d[40..48].try_into().unwrap())))
    }
    pub fn snap(&self, token_accounts: &[Pubkey], users: &[Pubkey]) -> Snap {
        Snap {
            supply: self.supply(),
            mint_state: self.mint_state_value(),
            global: self.global_stats(),
            balances: token_accounts.iter().map(|t| (*t, self.balance(t))).collect(),
            users: users.iter().map(|u| (*u, self.user_stats(u))).collect(),
        }
    }
    /// Owner, lamports and data of each account (None while it does not exist)
    pub fn raw(&self, keys: &[Pubkey]) -> Vec<Option<(Pubkey, u64, Vec<u8>)>> {
        keys.iter().map(|k| {
            self.c.get_account_with_commitment(k, CommitmentConfig::confirmed()).ok()
                .and_then(|r| r.value).map(|a| (a.owner, a.lamports, a.data))
        }).collect()
    }

    // ---------------- sending ----------------
    pub fn build(&self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair], blockhash: Hash) -> Transaction {
        let mut tx = Transaction::new_with_payer(ixs, Some(&payer.pubkey()));
        let n = tx.message.header.num_required_signatures as usize;
        let required: Vec<Pubkey> = tx.message.account_keys[..n].to_vec();
        let keys: Vec<&Keypair> = required.iter().map(|k| {
            std::iter::once(payer).chain(signers.iter().copied()).find(|kp| kp.pubkey() == *k)
                .unwrap_or_else(|| panic!("missing signer {}", k))
        }).collect();
        tx.try_sign(&keys, blockhash).expect("sign");
        tx
    }
    pub fn send(&self, tx: &impl SerializableTransaction) -> Result<Signature, String> {
        self.c.send_transaction_with_config(tx, RpcSendTransactionConfig { skip_preflight: true, max_retries: Some(10), ..Default::default() })
            .map_err(|e| e.to_string())
    }
    pub fn wait(&self, sig: &Signature) -> Out {
        let start = Instant::now();
        loop {
            let r: Result<Value, _> = self.c.send(RpcRequest::GetTransaction,
                json!([sig.to_string(), {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}]));
            if let Ok(v) = r {
                if !v.is_null() {
                    let meta = &v["meta"];
                    let err = if meta["err"].is_null() { None } else { Some(meta["err"].clone()) };
                    let logs = meta["logMessages"].as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                    return Out { landed: true, sig: sig.to_string(), slot: v["slot"].as_u64().unwrap_or(0),
                                 block_time: v["blockTime"].as_i64(), err, logs, fp: [0; 32] };
                }
            }
            if start.elapsed() > Duration::from_secs(60) {
                return Out { sig: sig.to_string(), err: Some(json!("not landed within 60s")), ..Default::default() };
            }
            sleep(Duration::from_millis(200));
        }
    }
    pub fn send_tx(&self, tx: &Transaction) -> Out {
        let fp = fingerprint(&tx.message);
        let mut out = match self.send(tx) {
            Ok(sig) => self.wait(&sig),
            Err(e) => Out { err: Some(json!(format!("RPC rejected: {}", e))), ..Default::default() },
        };
        out.fp = fp;
        out
    }
    pub fn exec(&self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Out {
        let bh = self.c.get_latest_blockhash().expect("blockhash");
        self.send_tx(&self.build(ixs, payer, signers, bh))
    }

    // ---------------- v0 transactions over an address lookup table ----------------
    /// Creates and extends an address lookup table (authority and payer: the wallet) and waits
    /// until its addresses can be loaded (from the slot after the one that extended it)
    pub fn create_alt(&self, addresses: &[Pubkey]) -> AddressLookupTableAccount {
        let wp = self.wallet.pubkey();
        let recent = self.c.get_slot_with_commitment(CommitmentConfig::finalized()).expect("slot");
        #[allow(deprecated)]
        let (create, key) = alt_ix::create_lookup_table(wp, wp, recent);
        #[allow(deprecated)]
        let extend = alt_ix::extend_lookup_table(key, wp, Some(wp), addresses.to_vec());
        let out = self.must("create and extend an address lookup table", &[create, extend], &self.wallet, &[]);
        let start = Instant::now();
        while self.c.get_slot_with_commitment(CommitmentConfig::confirmed()).unwrap_or(0) <= out.slot + 1 {
            if start.elapsed() > Duration::from_secs(60) {
                eprintln!("SETUP FAILED: the validator did not pass slot {} within 60s (address lookup table activation)", out.slot + 1);
                std::process::exit(2);
            }
            sleep(Duration::from_millis(100));
        }
        AddressLookupTableAccount { key, addresses: addresses.to_vec() }
    }
    /// v0 transaction over `alt` (every non-signer, non-program account found in it is loaded
    /// from it), and the addresses it loads: writable ones first, then read-only ones
    pub fn build_v0(&self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair], alt: &AddressLookupTableAccount,
                    blockhash: Hash) -> (VersionedTransaction, LoadedAddresses) {
        let msg = v0::Message::try_compile(&payer.pubkey(), ixs, std::slice::from_ref(alt), blockhash).expect("compile v0 message");
        let mut loaded = LoadedAddresses::default();
        for l in &msg.address_table_lookups {
            loaded.writable.extend(l.writable_indexes.iter().map(|&i| alt.addresses[i as usize]));
            loaded.readonly.extend(l.readonly_indexes.iter().map(|&i| alt.addresses[i as usize]));
        }
        let n = msg.header.num_required_signatures as usize;
        let keys: Vec<&Keypair> = msg.account_keys[..n].iter().map(|k| {
            std::iter::once(payer).chain(signers.iter().copied()).find(|kp| kp.pubkey() == *k)
                .unwrap_or_else(|| panic!("missing signer {}", k))
        }).collect();
        (VersionedTransaction::try_new(VersionedMessage::V0(msg), &keys).expect("sign v0"), loaded)
    }
    pub fn send_v0(&self, tx: &VersionedTransaction, loaded: &LoadedAddresses) -> Out {
        let fp = fingerprint_versioned(&tx.message, loaded);
        let mut out = match self.send(tx) {
            Ok(sig) => self.wait(&sig),
            Err(e) => Out { err: Some(json!(format!("RPC rejected: {}", e))), ..Default::default() },
        };
        out.fp = fp;
        out
    }
    /// `ixs` as a legacy transaction, or as a v0 transaction over `alt`
    pub fn exec_over(&self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair], alt: Option<&AddressLookupTableAccount>) -> Out {
        match alt {
            None => self.exec(ixs, payer, signers),
            Some(alt) => {
                let bh = self.c.get_latest_blockhash().expect("blockhash");
                let (tx, loaded) = self.build_v0(ixs, payer, signers, alt, bh);
                self.send_v0(&tx, &loaded)
            }
        }
    }

    // ---------------- simulation ----------------
    /// simulateTransaction with signature verification off and the blockhash replaced: every
    /// signature is left empty, so a transaction can be signed by any address (e.g. the admin)
    /// without its key. Nothing lands. Returns the outcome (`landed` = executed, `slot` = the
    /// simulated bank's slot, `fp` = the fingerprint) and the post-simulation owner, lamports and
    /// data of `addresses` (None for an account that does not exist afterwards). When the
    /// simulation fails the RPC returns no account (every entry is None, even the fee payer's), so
    /// a failed simulation says nothing about account state.
    pub fn simulate_unsigned(&self, ixs: &[Instruction], payer: &Pubkey, addresses: &[Pubkey]) -> (Out, Vec<Option<(Pubkey, u64, Vec<u8>)>>) {
        let tx = Transaction::new_unsigned(Message::new(ixs, Some(payer)));
        let wire = general_purpose::STANDARD.encode(bincode::serialize(&tx).expect("serialize"));
        let r: Value = self.c.send(RpcRequest::SimulateTransaction, json!([wire, {
            "encoding": "base64", "sigVerify": false, "replaceRecentBlockhash": true, "commitment": "confirmed",
            "accounts": {"encoding": "base64", "addresses": addresses.iter().map(|k| k.to_string()).collect::<Vec<_>>()}
        }])).expect("simulateTransaction");
        let v = &r["value"];
        let out = Out {
            landed: true,
            slot: r["context"]["slot"].as_u64().unwrap_or(0),
            err: if v["err"].is_null() { None } else { Some(v["err"].clone()) },
            logs: v["logs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            fp: fingerprint(&tx.message),
            ..Default::default()
        };
        let accounts = (0..addresses.len()).map(|i| {
            let a = &v["accounts"][i];
            if a.is_null() {
                return None;
            }
            let data = general_purpose::STANDARD.decode(a["data"][0].as_str().unwrap_or("")).unwrap_or_default();
            Some((a["owner"].as_str().and_then(|o| o.parse().ok()).unwrap_or_default(), a["lamports"].as_u64().unwrap_or(0), data))
        }).collect();
        (out, accounts)
    }

    /// Signatures of the transactions in `slot`'s block, in execution order
    pub fn block_signatures(&self, slot: u64) -> Vec<String> {
        let r: Result<Value, _> = self.c.send(RpcRequest::GetBlock, json!([slot, {
            "encoding": "json", "transactionDetails": "signatures", "rewards": false, "commitment": "confirmed",
            "maxSupportedTransactionVersion": 0
        }]));
        r.ok().and_then(|v| v["signatures"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()))
            .unwrap_or_default()
    }
    /// Setup transaction that must succeed (aborts the suite otherwise)
    pub fn must(&self, what: &str, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Out {
        let out = self.exec(ixs, payer, signers);
        if !out.ok() {
            eprintln!("SETUP FAILED: {}: {}\n  {}", what, out.brief(), out.logs.join("\n  "));
            std::process::exit(2);
        }
        out
    }
    pub fn airdrop(&self, to: &Pubkey, sol: u64) {
        let sig = self.c.request_airdrop(to, sol * 1_000_000_000).expect("airdrop");
        let out = self.wait(&sig);
        if !out.ok() {
            eprintln!("SETUP FAILED: airdrop to {}: {}", to, out.brief());
            std::process::exit(2);
        }
    }

    // ---------------- results ----------------
    pub fn record(&self, id: &str, desc: &str, pass: bool, detail: &str) {
        if pass {
            self.pass.set(self.pass.get() + 1);
            println!("PASS [{}] {}", id, desc);
        } else {
            self.fail.set(self.fail.get() + 1);
            println!("FAIL [{}] {}\n     {}", id, desc, detail);
        }
    }
    fn record_out(&self, id: &str, desc: &str, pass: bool, detail: &str, out: &Out) {
        self.record(id, desc, pass, detail);
        if !pass && !out.logs.is_empty() {
            println!("     logs:\n       {}", out.logs.join("\n       "));
        }
    }
    /// A case that could not run here (e.g. it needs a fresh validator, or a timing condition such
    /// as M26's same-slot pair never occurred). Skips are counted and make the suite fail unless
    /// MEMO_REGRESSION_ALLOW_SKIP=1, so a run never reports less coverage than it claims.
    pub fn skip(&self, id: &str, reason: &str) {
        self.skip.set(self.skip.get() + 1);
        println!("SKIP [{}] {}", id, reason);
    }
    /// Prints the summary and exits with status 1 if any case failed, or was skipped without
    /// MEMO_REGRESSION_ALLOW_SKIP=1
    pub fn finish(&self) -> ! {
        let (p, f, s) = (self.pass.get(), self.fail.get(), self.skip.get());
        let allow_skip = std::env::var("MEMO_REGRESSION_ALLOW_SKIP").is_ok_and(|v| v == "1");
        if s == 0 {
            println!("\n== {}: {} passed, {} failed ==", self.suite, p, f);
        } else {
            println!("\n== {}: {} passed, {} failed, {} skipped{} ==", self.suite, p, f, s,
                     if allow_skip { " (allowed by MEMO_REGRESSION_ALLOW_SKIP=1)" } else { " (see the SKIP lines above; set MEMO_REGRESSION_ALLOW_SKIP=1 to accept them)" });
        }
        std::process::exit(if f == 0 && p > 0 && (s == 0 || allow_skip) { 0 } else { 1 });
    }

    /// The transaction must fail with `expect`, and no watched state may change
    pub fn reject(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                  expect: Expect, token_accounts: &[Pubkey], users: &[Pubkey]) -> Out {
        self.reject_watching(id, desc, ixs, payer, signers, expect, token_accounts, users, &[])
    }

    /// As `reject`, and no account the instructions reference may change either (owner, lamports
    /// and data), except the fee payer, which pays the fee
    #[allow(clippy::too_many_arguments)]
    pub fn reject_untouched(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                            expect: Expect, token_accounts: &[Pubkey], users: &[Pubkey]) -> Out {
        let keys = referenced(&ixs, payer);
        self.reject_watching(id, desc, ixs, payer, signers, expect, token_accounts, users, &keys)
    }

    /// As `reject_untouched`, sent as a v0 transaction over `alt`
    #[allow(clippy::too_many_arguments)]
    pub fn reject_v0(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                     expect: Expect, token_accounts: &[Pubkey], users: &[Pubkey], alt: &AddressLookupTableAccount) -> Out {
        let keys = referenced(&ixs, payer);
        self.reject_over(id, desc, ixs, payer, signers, expect, token_accounts, users, &keys, Some(alt))
    }

    #[allow(clippy::too_many_arguments)]
    fn reject_watching(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                       expect: Expect, token_accounts: &[Pubkey], users: &[Pubkey], accounts: &[Pubkey]) -> Out {
        self.reject_over(id, desc, ixs, payer, signers, expect, token_accounts, users, accounts, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn reject_over(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair], expect: Expect,
                   token_accounts: &[Pubkey], users: &[Pubkey], accounts: &[Pubkey], alt: Option<&AddressLookupTableAccount>) -> Out {
        let (before, raw_before) = (self.snap(token_accounts, users), self.raw(accounts));
        let out = self.exec_over(&ixs, payer, signers, alt);
        let (after, raw_after) = (self.snap(token_accounts, users), self.raw(accounts));
        let matched = expect.matches(&out);
        let changed: Vec<String> = accounts.iter().zip(raw_before.iter().zip(raw_after.iter()))
            .filter(|(_, (a, b))| a != b).map(|(k, _)| k.to_string()).collect();
        let unchanged = before == after && changed.is_empty();
        let detail = format!("expected {} and no state change; got {} ({:?} in {:?}); {}{}", expect.describe(), out.brief(),
                             out.error_name(), out.failed_program(), before.diff(&after),
                             if changed.is_empty() { String::new() } else { format!("; accounts changed: {}", changed.join(", ")) });
        self.record_out(id, desc, matched && unchanged, &detail, &out);
        out
    }

    // ---------------- memo-mint instructions ----------------
    pub fn text_memo(&self, tag: &str, len: usize) -> Instruction {
        let mut s = format!("regression:{}:{}:", self.nonce(), tag);
        while s.len() < len {
            s.push('x');
        }
        s.truncate(len);
        memo_ix(s.as_bytes())
    }
    pub fn mint_accounts(&self, user: &Pubkey, token_account: &Pubkey) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(self.mint, false),
            AccountMeta::new_readonly(self.mint_auth, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(IXS_SYSVAR, false),
            AccountMeta::new(self.mint_state, false),
        ]
    }
    pub fn mint_ix_with(&self, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction::new_with_bytes(self.mint_prog, &disc("process_mint"), accounts)
    }
    pub fn mint_ix(&self, user: &Pubkey) -> Instruction { self.mint_ix_with(self.mint_accounts(user, &self.ata(user))) }
    pub fn mint_to_ix_with(&self, recipient: &Pubkey, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc("process_mint_to");
        d.extend(recipient.as_ref());
        Instruction::new_with_bytes(self.mint_prog, &d, accounts)
    }
    pub fn mint_to_ix(&self, caller: &Pubkey, recipient: &Pubkey, recipient_ta: &Pubkey) -> Instruction {
        self.mint_to_ix_with(recipient, self.mint_accounts(caller, recipient_ta))
    }

    // ---------------- memo-burn instructions ----------------
    pub fn burn_accounts(&self, user: &Pubkey, token_account: &Pubkey, stats: &Pubkey) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(self.mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*stats, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(IXS_SYSVAR, false),
            AccountMeta::new(self.gbs, false),
        ]
    }
    pub fn burn_ix_with(&self, amount: u64, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc("process_burn");
        d.extend(amount.to_le_bytes());
        Instruction::new_with_bytes(self.burn_prog, &d, accounts)
    }
    pub fn burn_ix(&self, user: &Pubkey, amount: u64) -> Instruction {
        self.burn_ix_with(amount, self.burn_accounts(user, &self.ata(user), &self.stats_pda(user)))
    }
    /// Payload of exactly `len` bytes, unique per call
    pub fn payload(&self, tag: &str, len: usize) -> Vec<u8> {
        let mut s = format!("regression-burn:{}:{}:", self.nonce(), tag).into_bytes();
        while s.len() < len {
            s.push(b'p');
        }
        s.truncate(len);
        s
    }
    /// Base64 BurnMemo (60-byte payload, 100-byte memo) for `amount`
    pub fn burn_memo(&self, version: u8, amount: u64, tag: &str) -> Vec<u8> {
        let p = self.payload(tag, 60);
        b64(&burn_memo_raw(version, amount, p.len() as u32, &p))
    }
    pub fn init_stats_ix(&self, user: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(self.burn_prog, &disc("initialize_user_global_burn_stats"), vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(self.stats_pda(user), false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }

    // ---------------- setup helpers ----------------
    /// New funded keypair with a MEMO ATA holding `memo_units` (taken from the wallet) and,
    /// optionally, initialized burn stats
    pub fn new_user(&self, label: &str, memo_units: u64, with_stats: bool) -> Keypair {
        let k = Keypair::new();
        self.airdrop(&k.pubkey(), 10);
        let mut ixs = vec![create_associated_token_account(&self.wallet.pubkey(), &k.pubkey(), &self.mint, &t22())];
        if memo_units > 0 {
            ixs.push(spl_token_2022::instruction::transfer_checked(&t22(), &self.ata(&self.wallet.pubkey()), &self.mint,
                &self.ata(&k.pubkey()), &self.wallet.pubkey(), &[], memo_units, 6).unwrap());
        }
        self.must(&format!("create user {} with {} units", label, memo_units), &ixs, &self.wallet, &[]);
        if with_stats {
            self.must(&format!("init burn stats for {}", label), &[self.init_stats_ix(&k.pubkey())], &k, &[]);
        }
        k
    }
    /// A fresh Token-2022 mint (no extensions) with the given mint authority
    pub fn new_mint(&self, authority: &Pubkey) -> Keypair {
        let m = Keypair::new();
        let rent = self.c.get_minimum_balance_for_rent_exemption(82).unwrap();
        self.must("create a second Token-2022 mint", &[
            sys_ix::create_account(&self.wallet.pubkey(), &m.pubkey(), rent, 82, &t22()),
            spl_token_2022::instruction::initialize_mint2(&t22(), &m.pubkey(), authority, None, 6).unwrap(),
        ], &self.wallet, &[&m]);
        m
    }
    pub fn create_ata_ix(&self, owner: &Pubkey, mint: &Pubkey) -> Instruction {
        create_associated_token_account(&self.wallet.pubkey(), owner, mint, &t22())
    }

    // ---------------- success checks ----------------
    /// The transaction must succeed and mint exactly `curve_reference(supply before)` to
    /// `recipient_ta` (supply up by the same amount, `others` unchanged), with exactly one mint,
    /// whose log line is exactly `minted_log` (amount, supply before),
    /// and MintState = (fingerprint of this transaction, its slot). Returns the minted amount.
    pub fn mint_ok(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                   recipient_ta: &Pubkey, others: &[Pubkey]) -> u64 {
        self.mint_ok_over(id, desc, ixs, payer, signers, recipient_ta, others, None)
    }

    /// As `mint_ok`, sent as a v0 transaction over `alt` (MintState holds the fingerprint of the
    /// instructions as the runtime resolves them through the table)
    #[allow(clippy::too_many_arguments)]
    pub fn mint_ok_v0(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                      recipient_ta: &Pubkey, others: &[Pubkey], alt: &AddressLookupTableAccount) -> u64 {
        self.mint_ok_over(id, desc, ixs, payer, signers, recipient_ta, others, Some(alt))
    }

    #[allow(clippy::too_many_arguments)]
    fn mint_ok_over(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                    recipient_ta: &Pubkey, others: &[Pubkey], alt: Option<&AddressLookupTableAccount>) -> u64 {
        let mut accounts = vec![*recipient_ta];
        accounts.extend_from_slice(others);
        let before = self.snap(&accounts, &[]);
        let expected = curve_reference(before.supply);
        let out = self.exec_over(&ixs, payer, signers, alt);
        let after = self.snap(&accounts, &[]);
        let minted = after.bal(recipient_ta) as i128 - before.bal(recipient_ta) as i128;
        let supply_delta = after.supply as i128 - before.supply as i128;
        let others_same = others.iter().all(|o| before.bal(o) == after.bal(o));
        let state_ok = after.mint_state == Some((out.fp, out.slot));
        let exp = expected.map(|x| x as i128).unwrap_or(-1);
        let log_ok = expected.is_some_and(|x| out.logs.contains(&minted_log(x, before.supply)));
        let pass = out.ok() && minted == exp && supply_delta == exp && others_same && state_ok && out.minted_log_count() == 1 && log_ok;
        let detail = format!("{}; supply before {} expected {:?} minted {} supply delta {} others unchanged {} mint_state matches tx {} mint logs {} mint log as expected {}",
            out.brief(), before.supply, expected, minted, supply_delta, others_same, state_ok, out.minted_log_count(), log_ok);
        self.record_out(id, desc, pass, &detail, &out);
        minted.max(0) as u64
    }

    /// The transaction must succeed and burn exactly `total` from `user`'s ATA in `count`
    /// process_burn instructions: balance and supply -total, user stats total +total (capped at
    /// 18T) and count +count, global stats +total / +count, and both last_burn_time fields equal
    /// to the block time of the transaction.
    pub fn burn_ok(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                   user: &Pubkey, total: u64, count: u64) -> Out {
        let ta = self.ata(user);
        let before = self.snap(&[ta], &[*user]);
        let out = self.exec(&ixs, payer, signers);
        let after = self.snap(&[ta], &[*user]);
        let pass = out.ok() && self.burn_deltas_ok(&before, &after, &out, user, &ta, total, count);
        let detail = format!("{}; expected -{} units in {} burns; {}", out.brief(), total, count, before.diff(&after));
        self.record_out(id, desc, pass, &detail, &out);
        out
    }

    pub fn burn_deltas_ok(&self, before: &Snap, after: &Snap, out: &Out, user: &Pubkey, ta: &Pubkey, total: u64, count: u64) -> bool {
        let (u0, u1) = (before.user(user), after.user(user));
        let (g0, g1) = (before.global(), after.global());
        let expected_user_total = u0.total.saturating_add(total).min(MAX_USER_GLOBAL_BURN);
        before.bal(ta).checked_sub(after.bal(ta)) == Some(total)
            && before.supply.checked_sub(after.supply) == Some(total)
            && u1.user == *user
            && u1.total == expected_user_total
            && u1.count == u0.count + count
            && g1.total == g0.total + total as u128
            && g1.count == g0.count + count
            && Some(u1.last_time) == out.block_time
            && Some(g1.last_time) == out.block_time
    }
}
