//! Shared harness for the on-chain regression suites (clients/regression/src/regression-*.rs).
//!
//! The suites only run against a local solana-test-validator (scripts/local-validator.sh,
//! scripts/run-regression-tests.sh). Every transaction is sent with skip_preflight, so failing
//! transactions land on-chain, and each case asserts the exact outcome:
//!   - rejections: the exact `InstructionError [index, Custom(code)]` (or other exact error) and no
//!     change to any watched state (balances, supply, MintState, user and global burn stats)
//!   - successes: exact token deltas, the minted amount against an independent curve reference,
//!     MintState = (fingerprint of this transaction, its slot), exact burn statistics deltas
//! A suite exits with status 1 if any case fails.

use std::{cell::Cell, thread::sleep, time::{Duration, Instant}};

use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig, rpc_request::RpcRequest};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    transaction::Transaction,
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
pub const ANCHOR_CONSTRAINT_MUT: u32 = 2000;
pub const ANCHOR_CONSTRAINT_SEEDS: u32 = 2006;
pub const ANCHOR_CONSTRAINT_ADDRESS: u32 = 2012;
pub const ANCHOR_DISCRIMINATOR_MISMATCH: u32 = 3002;
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

pub fn memo_ix(data: &[u8]) -> Instruction { Instruction::new_with_bytes(spl_memo::ID, data, vec![]) }
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

/// SHA-256 of the instructions sysvar content (without the trailing 2-byte current index), built
/// the way the runtime serializes it; this is what memo-mint stores in MintState.
pub fn fingerprint(msg: &Message) -> [u8; 32] {
    let ixs = &msg.instructions;
    let mut data: Vec<u8> = vec![];
    data.extend((ixs.len() as u16).to_le_bytes());
    for _ in ixs {
        data.extend([0u8, 0u8]);
    }
    for (i, ix) in ixs.iter().enumerate() {
        let off = data.len() as u16;
        data[2 + 2 * i..4 + 2 * i].copy_from_slice(&off.to_le_bytes());
        data.extend((ix.accounts.len() as u16).to_le_bytes());
        for &ai in &ix.accounts {
            let ai = ai as usize;
            let mut flags = 0u8;
            if msg.is_signer(ai) {
                flags |= 1;
            }
            if msg.is_maybe_writable(ai, None) {
                flags |= 2;
            }
            data.push(flags);
            data.extend(msg.account_keys[ai].as_ref());
        }
        data.extend(msg.account_keys[ix.program_id_index as usize].as_ref());
        data.extend((ix.data.len() as u16).to_le_bytes());
        data.extend(&ix.data);
    }
    Sha256::digest(&data).into()
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
}

/// Expected failure of a transaction
#[derive(Clone, Copy, Debug)]
pub enum Expect<'a> {
    /// InstructionError at this instruction index with this custom code
    Custom(u64, u32),
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
            Expect::ErrContains(s) => out.err_text().contains(s),
        }
    }
    pub fn describe(&self) -> String {
        match self {
            Expect::Custom(ix, code) => format!("InstructionError [{}, Custom({})]", ix, code),
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
            nonce: Cell::new(seed), pass: Cell::new(0), fail: Cell::new(0), suite: suite.into(),
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
    pub fn send(&self, tx: &Transaction) -> Result<Signature, String> {
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
    /// Prints the summary and exits with status 1 if any case failed
    pub fn finish(&self) -> ! {
        let (p, f) = (self.pass.get(), self.fail.get());
        println!("\n== {}: {} passed, {} failed ==", self.suite, p, f);
        std::process::exit(if f == 0 && p > 0 { 0 } else { 1 });
    }

    /// The transaction must fail with `expect`, and no watched state may change
    pub fn reject(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                  expect: Expect, token_accounts: &[Pubkey], users: &[Pubkey]) -> Out {
        let before = self.snap(token_accounts, users);
        let out = self.exec(&ixs, payer, signers);
        let after = self.snap(token_accounts, users);
        let matched = expect.matches(&out);
        let unchanged = before == after;
        let detail = format!("expected {} and no state change; got {}; {}", expect.describe(), out.brief(), before.diff(&after));
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
    /// `recipient_ta` (supply up by the same amount, `others` unchanged), with exactly one mint
    /// and MintState = (fingerprint of this transaction, its slot). Returns the minted amount.
    pub fn mint_ok(&self, id: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair],
                   recipient_ta: &Pubkey, others: &[Pubkey]) -> u64 {
        let mut accounts = vec![*recipient_ta];
        accounts.extend_from_slice(others);
        let before = self.snap(&accounts, &[]);
        let expected = curve_reference(before.supply);
        let out = self.exec(&ixs, payer, signers);
        let after = self.snap(&accounts, &[]);
        let minted = after.bal(recipient_ta) as i128 - before.bal(recipient_ta) as i128;
        let supply_delta = after.supply as i128 - before.supply as i128;
        let others_same = others.iter().all(|o| before.bal(o) == after.bal(o));
        let state_ok = after.mint_state == Some((out.fp, out.slot));
        let exp = expected.map(|x| x as i128).unwrap_or(-1);
        let pass = out.ok() && minted == exp && supply_delta == exp && others_same && state_ok && out.minted_log_count() == 1;
        let detail = format!("{}; supply before {} expected {:?} minted {} supply delta {} others unchanged {} mint_state matches tx {} mint logs {}",
            out.brief(), before.supply, expected, minted, supply_delta, others_same, state_ok, out.minted_log_count());
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
