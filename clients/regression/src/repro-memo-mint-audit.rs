//! Reproductions of the memo-mint Revision 5 audit findings on a local validator.
//!
//! Each case asserts the CURRENT behaviour of the loaded memo-mint build (Revision 4, the build
//! deployed on testnet and mainnet, or Revision 5 as audited: testnet build sha256 8fc80212...,
//! before decisions D3-D5 changed its logs, accounts and CU) together with the SPL Memo
//! program the test validator bundles (checked at start to be byte-identical to X1's, so Memo
//! CU here equals Memo CU on X1), and prints the compute units (Memo program, memo-mint, whole
//! transaction) and transaction sizes the audit cites. This is not a regression suite: it
//! documents issues, and several cases pass only while the issue exists. Case ids name the
//! finding (S00.zh@400k is the 69-character '中' memo of finding S00 sent with a 400,000 CU limit).
//!   S00 / S23 / S27 / S32 / S56 / S57  69-character memos by script: Memo-program CU, the
//!        400,000 CU limit memo-sdk and memo-app simulate with, the 1.4M CU and 1,232-byte limits
//!        (sections s00, fit, mixed)
//!   S01  Revision 4 accepts 69-byte memos of fewer than 69 characters, Revision 5 rejects them;
//!        memo-mint CU next to the MEMO-MINT-AUDIT.md table (sections s01, cmp)
//!   S43  the slowest Revision 5 path (69-275 bytes, non-ASCII) (section s43)
//!   S44 / S61  the MemoTooShort log reports bytes against a character minimum (section s44)
//!   S04  invalid UTF-8 at index 0 is rejected by the Memo program (section s04)
//!   SLOT a mint that lands after another mint in the same slot spends 27 more CU (section slot)
//!   S68  MINT-01: identical instructions signed twice collide in one slot (section mint01)
//!   S16 / S25 / S30 / S69  the mainnet bot's transaction shape, and the memo-miner shape (live
//!        traffic since 2026-10-31 09:18Z) across the cut-over (section cutover)
//! memo-mint's own CU here is that of the testnet-ID build; the mainnet build spends more (its
//! mint_authority bump is 254). Run Revision 4 at the mainnet supply to measure the offset
//! (printed by the cutover section) and pass it to other runs with --mainnet-offset.
//! Simulations (used only to search for boundaries) run in a slot without a mint, and every
//! boundary is then confirmed with sent transactions.
//!
//! Run (one build per validator start; --rev names the loaded build and is checked). target/deploy
//! now holds the final Revision 5, so load the audited build (sha256 8fc80212...) with --so-dir:
//!   ./scripts/local-validator.sh start --so-dir <dir with the audited memo_mint.so> --supply 5000000 --balance 1000000
//!   (cd clients && MEMO_PROGRAM_ENV=testnet MEMO_RPC_URL=http://127.0.0.1:8899 \
//!     MEMO_WALLET=../target/local-validator/wallet.json cargo run --bin repro-memo-mint-audit -- --rev 5 --so <dir>/memo_mint.so)
//!   mkdir -p <dir> && cp <Revision 4 memo_mint.so, e.g. a dump of testnet A31a17...> <dir>/
//!   ./scripts/local-validator.sh start --so-dir <dir> --supply 5000000 --balance 1000000
//!   (... cargo run --bin repro-memo-mint-audit -- --rev 4 --so <dir>/memo_mint.so)
//!   ./scripts/local-validator.sh stop
//! The fit, mixed and cmp sections pin values measured at a supply below 10M MEMO; the cutover
//! section is meant for the mainnet supply (--supply 11963090.9).
//! Options: --so <path> checks that the loaded memo-mint ELF is that file; --json <path> appends
//! every sent transaction as a JSON line; --mainnet-offset <cu> converts local CU to mainnet;
//! --rev4-miner-cu <cu> (Revision 5 runs) is the memo-miner shape's simulated CU that a Revision 4
//! run at the same supply printed; trailing section names run only those sections
//! (s00 fit mixed s01 cmp s43 s44 s04 slot mint01 cutover).

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use std::{
    cell::{Cell, RefCell},
    fs::{File, OpenOptions},
    io::Write,
    thread::sleep,
    time::Duration,
};

use harness::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_client::{rpc_config::RpcSimulateTransactionConfig, rpc_request::RpcRequest};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};
use spl_associated_token_account::instruction::create_associated_token_account_idempotent;

const MEMO_TOO_SHORT: u32 = 6001;
/// The success log line of the audited builds (Revision 4 and the audited Revision 5), which still
/// named the recipient; decision D4 (option O2b) dropped it
fn audited_minted_log(amount: u64, owner: &Pubkey, supply_before: u64) -> String {
    format!("Program log: Minted {} units to {}, supply before mint: {} units", amount, owner, supply_before)
}
/// The CU limit memo-sdk (SIMULATION_CU_LIMIT) and memo-app simulate every mint with
const CLIENT_SIM_CU: u32 = 400_000;
const MAX_CU: u32 = 1_400_000;
const MAX_TX_BYTES: usize = 1232;
/// sha256 of X1's SPL Memo program (MemoSq4g..., BPFLoader2, the same on testnet and mainnet)
const X1_MEMO_SHA256: &str = "f520eaf096361abbb9639ea4dc3e5388a87b9330e121f476607b87c46ef67954";
const COMPUTE_BUDGET: Pubkey = solana_sdk::pubkey!("ComputeBudget111111111111111111111111111111");
const LOADER_V3: Pubkey = solana_sdk::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
/// The dominant mainnet minter (9CRGtz..., S16 / S25 / S30): [Memo(two 2-byte Latin-1 characters,
/// e.g. "æÀ", + "é" x 32 + "a": 69 bytes, 35 characters), process_mint, SetComputeUnitLimit(37,200)],
/// 37,063 CU on mainnet (Memo 19,871, memo-mint 17,042)
const BOT_CU_LIMIT: u32 = 37_200;
const MAINNET_BOT_MINT_CU: u64 = 17_042;
/// A second mainnet minter (CWJCzR..., S30): "é" x 34 + "W" with a 37,108 limit
const BOT2_CU_LIMIT: u32 = 37_108;
/// memo-mint's extra CU when MintState was already written in the same slot (record_mint then
/// compares the 32-byte fingerprint)
const SAME_SLOT_CU: u64 = 27;
const ONE_MINT_PER_TRANSACTION: u32 = 6008;
/// memo-miner (the live mainnet traffic since 2026-10-31 09:18Z, S68 / S69) adds this margin to
/// the simulated CU
const MINER_CU_MARGIN: u64 = 50;
/// Realistic Chinese text for the CJK sentence memo (cut or cycled to the wanted length)
const ZH_SENTENCE: &str = "今天我们测试新的备忘录规则：每次铸币都需要至少六十九个字符的备忘录，而不是六十九个字节。\
这样写中文的用户需要写更长的句子，交易消耗的计算单元也会大幅增加，所以客户端必须提高模拟时的计算单元上限。";

fn exceeded_at(ix: u64) -> Value { json!({"InstructionError": [ix, "ProgramFailedToComplete"]}) }
fn budget_exceeded_at(ix: u64) -> Value { json!({"InstructionError": [ix, "ComputationalBudgetExceeded"]}) }
fn custom_at(ix: u64, code: u32) -> Value { json!({"InstructionError": [ix, {"Custom": code}]}) }
fn invalid_data_at(ix: u64) -> Value { json!({"InstructionError": [ix, "InvalidInstructionData"]}) }

fn n(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
fn on(v: Option<u64>) -> String { v.map(n).unwrap_or_else(|| "-".into()) }
fn chars_from(first: u32, step: u32, count: usize) -> String {
    (0..count as u32).map(|i| char::from_u32(first + i * step).unwrap()).collect()
}
fn cycle(s: &str, count: usize) -> String { s.chars().cycle().take(count).collect() }
fn consumed(logs: &[String], prog: &Pubkey) -> Option<u64> {
    let p = format!("Program {} consumed ", prog);
    logs.iter().filter_map(|l| l.strip_prefix(p.as_str())).filter_map(|r| r.split(' ').next()?.parse().ok()).last()
}
fn has_log(out: &Out, s: &str) -> bool { out.logs.iter().any(|l| l.contains(s)) }
fn has_line(out: &Out, s: &str) -> bool { out.logs.iter().any(|l| l == s) }
fn tx_size(tx: &Transaction) -> usize { bincode::serialize(tx).expect("serialize").len() }
fn memo_too_short_line(bytes: usize) -> String { format!("Program log: Memo too short: {} bytes (minimum: 69 characters)", bytes) }
/// memo-miner's memo (69 ASCII bytes; `t` is a 10-digit Unix time in seconds)
fn miner_memo(t: u64) -> String { format!(r#"{{"action":"mint","platform":"memo-miner","chain":"X1","t":{}}}"#, t) }
/// Memo instruction with `signer` as memo signer, as the released memo-miner sends it (memo-sdk v0.1.0)
fn signed_memo_ix(memo: &str, signer: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(spl_memo::ID, memo.as_bytes(), vec![AccountMeta::new_readonly(*signer, true)])
}

/// A script: `memo(c)` is a c-character memo written in it
struct Script {
    key: &'static str,
    label: &'static str,
    memo: fn(usize) -> String,
    /// Memo-program CU of the 69-character memo (X1's Memo ELF, so the same on X1)
    memo_cu: u64,
    /// the static phase measured the same value on X1 mainnet
    on_mainnet: bool,
    /// the 69-character mint fits the clients' 400,000 CU simulation limit
    fits_400k: bool,
}

#[rustfmt::skip]
fn scripts() -> Vec<Script> {
    vec![
        Script { key: "x", label: "'x' (printable ASCII)", memo: |c| "x".repeat(c), memo_cu: 26_037, on_mainnet: true, fits_400k: true },
        Script { key: "tab", label: "TAB", memo: |c| "\t".repeat(c), memo_cu: 24_035, on_mainnet: true, fits_400k: true },
        Script { key: "quote", label: "'\"'", memo: |c| "\"".repeat(c), memo_cu: 24_104, on_mainnet: true, fits_400k: true },
        Script { key: "bslash", label: "backslash", memo: |c| "\\".repeat(c), memo_cu: 24_242, on_mainnet: true, fits_400k: true },
        Script { key: "lf", label: "LF", memo: |c| "\n".repeat(c), memo_cu: 24_242, on_mainnet: true, fits_400k: true },
        Script { key: "p2", label: "U+20000 (plane 2)", memo: |c| "\u{20000}".repeat(c), memo_cu: 20_097, on_mainnet: true, fits_400k: true },
        Script { key: "p2mix", label: "U+20000.. 69 distinct Ext-B", memo: |c| chars_from(0x20000, 1, c), memo_cu: 20_097, on_mainnet: true, fits_400k: true },
        Script { key: "p2spread", label: "Ext-B U+20000+0x200*i", memo: |c| chars_from(0x20000, 0x200, c), memo_cu: 20_097, on_mainnet: false, fits_400k: true },
        Script { key: "p2f800", label: "U+2F800 (compat. suppl.)", memo: |c| "\u{2F800}".repeat(c), memo_cu: 20_097, on_mainnet: true, fits_400k: true },
        Script { key: "e", label: "'é' (Latin-1)", memo: |c| "é".repeat(c), memo_cu: 37_702, on_mainnet: true, fits_400k: true },
        Script { key: "greek", label: "Greek 'α'", memo: |c| "α".repeat(c), memo_cu: 49_225, on_mainnet: false, fits_400k: true },
        Script { key: "cyr", label: "Cyrillic 'ж'", memo: |c| "ж".repeat(c), memo_cu: 45_430, on_mainnet: true, fits_400k: true },
        Script { key: "zh", label: "CJK '中'", memo: |c| "中".repeat(c), memo_cu: 767_024, on_mainnet: true, fits_400k: false },
        Script { key: "zhs", label: "Chinese sentence", memo: |c| cycle(ZH_SENTENCE, c), memo_cu: 777_333, on_mainnet: false, fits_400k: false },
        Script { key: "kana", label: "hiragana 'あ'", memo: |c| "あ".repeat(c), memo_cu: 735_422, on_mainnet: true, fits_400k: false },
        Script { key: "hangul", label: "Hangul '한'", memo: |c| "한".repeat(c), memo_cu: 914_270, on_mainnet: true, fits_400k: false },
        Script { key: "globe", label: "emoji '🌍'", memo: |c| "🌍".repeat(c), memo_cu: 1_194_339, on_mainnet: true, fits_400k: false },
        Script { key: "emix", label: "emoji U+1F600.. mix", memo: |c| (0..c).map(|i| char::from_u32(0x1F600 + (i % 80) as u32).unwrap()).collect(),
                 memo_cu: 1_194_339, on_mainnet: false, fits_400k: false },
        Script { key: "teapot", label: "U+1FAD6 '🫖'", memo: |c| "\u{1FAD6}".repeat(c), memo_cu: 1_368_243, on_mainnet: true, fits_400k: false },
        Script { key: "u1faf8", label: "U+1FAF8", memo: |c| "\u{1FAF8}".repeat(c), memo_cu: 1_368_243, on_mainnet: true, fits_400k: false },
    ]
}
fn script(key: &str) -> Script { scripts().into_iter().find(|s| s.key == key).unwrap() }

/// One sent transaction
struct Sent {
    out: Out,
    tx_bytes: usize,
    total_cu: Option<u64>,
}

struct Row {
    id: String,
    label: String,
    chars: Option<usize>,
    bytes: usize,
    tx_bytes: usize,
    limit: Option<u32>,
    result: String,
    memo_cu: Option<u64>,
    mint_cu: Option<u64>,
    total_cu: Option<u64>,
}

struct R<'a> {
    e: &'a Env,
    rev: u8,
    supply: u64,
    last_bh: Cell<Hash>,
    json: Option<RefCell<File>>,
    rows: RefCell<Vec<Row>>,
}

impl R<'_> {
    fn wallet(&self) -> Pubkey { self.e.wallet.pubkey() }
    fn ta(&self) -> Pubkey { self.e.ata(&self.wallet()) }
    /// [memo, process_mint] plus SetComputeUnitLimit(limit) when given (the memo-sdk / bot shape)
    fn mint_ixs(&self, memo: &[u8], limit: Option<u32>) -> Vec<Instruction> {
        let mut v = vec![memo_ix(memo), self.e.mint_ix(&self.wallet())];
        if let Some(l) = limit {
            v.push(cu(l));
        }
        v
    }
    /// [memo, ATA create_idempotent, process_mint, SetComputeUnitLimit(limit)] (the memo-app shape)
    fn app_ixs(&self, memo: &[u8], limit: u32) -> Vec<Instruction> {
        let w = self.wallet();
        vec![memo_ix(memo), create_associated_token_account_idempotent(&w, &w, &self.e.mint, &t22()), self.e.mint_ix(&w), cu(limit)]
    }
    /// A blockhash no earlier transaction of this run used, so that repeating a memo never
    /// repeats a signature
    fn blockhash(&self) -> Hash {
        loop {
            let bh = self.e.c.get_latest_blockhash().expect("blockhash");
            if bh != self.last_bh.get() {
                self.last_bh.set(bh);
                return bh;
            }
            sleep(Duration::from_millis(100));
        }
    }
    fn size_of(&self, ixs: &[Instruction]) -> usize { tx_size(&self.e.build(ixs, &self.e.wallet, &[], Hash::default())) }
    fn total_cu(&self, sig: &str) -> Option<u64> {
        self.e.c.send::<Value>(RpcRequest::GetTransaction,
            json!([sig, {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}]))
            .ok().and_then(|v| v["meta"]["computeUnitsConsumed"].as_u64())
    }
    fn send(&self, ixs: &[Instruction]) -> Sent {
        let tx = self.e.build(ixs, &self.e.wallet, &[], self.blockhash());
        let tx_bytes = tx_size(&tx);
        let out = self.e.send_tx(&tx);
        let total_cu = if out.landed { self.total_cu(&out.sig) } else { None };
        Sent { out, tx_bytes, total_cu }
    }
    /// Simulation (nothing lands): (succeeded, units consumed, error, logs). It runs in a slot
    /// other than MintState's, as a sent transaction does, so that its CU equal the sent one's
    /// (SLOT). MintState copied from testnet holds a slot this validator never reaches.
    fn simulate(&self, ixs: &[Instruction]) -> (bool, u64, Value, Vec<String>) {
        let last = self.e.mint_state_value().map(|v| v.1).unwrap_or(0);
        while self.e.c.get_slot_with_commitment(CommitmentConfig::processed()).unwrap_or(0) == last {
            sleep(Duration::from_millis(50));
        }
        let tx = self.e.build(ixs, &self.e.wallet, &[], self.e.c.get_latest_blockhash().expect("blockhash"));
        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false, replace_recent_blockhash: true, commitment: Some(CommitmentConfig::processed()), ..Default::default()
        };
        let v = self.e.c.simulate_transaction_with_config(&tx, cfg).expect("simulate").value;
        let err = v.err.as_ref().map(|x| serde_json::to_value(x).unwrap()).unwrap_or(Value::Null);
        (v.err.is_none(), v.units_consumed.unwrap_or(0), err, v.logs.unwrap_or_default())
    }

    // ---------------- recording ----------------
    fn row(&self, id: &str, label: &str, memo: &[u8], limit: Option<u32>, s: &Sent) {
        let result = if s.out.ok() {
            "ok".to_string()
        } else if s.out.landed {
            s.out.err_text()
        } else {
            s.out.err_text().chars().take(110).collect()
        };
        let r = Row {
            id: id.into(), label: label.into(), chars: std::str::from_utf8(memo).ok().map(|t| t.chars().count()), bytes: memo.len(),
            tx_bytes: s.tx_bytes, limit, result, memo_cu: consumed(&s.out.logs, &spl_memo::ID),
            mint_cu: consumed(&s.out.logs, &self.e.mint_prog), total_cu: s.total_cu,
        };
        println!("     {} {}: chars {} bytes {} tx {} B, CU limit {}, Memo {} memo-mint {} total {}: {}", r.id, r.label,
                 r.chars.map(|c| c.to_string()).unwrap_or("-".into()), r.bytes, r.tx_bytes, limit.map(|l| n(l as u64)).unwrap_or("default".into()),
                 on(r.memo_cu), on(r.mint_cu), on(r.total_cu), r.result);
        if let Some(f) = &self.json {
            let line = json!({"rev": self.rev, "supply": self.supply, "id": r.id, "label": r.label, "chars": r.chars, "bytes": r.bytes,
                "tx_bytes": r.tx_bytes, "limit": r.limit, "ok": s.out.ok(), "landed": s.out.landed, "err": s.out.err, "memo_cu": r.memo_cu,
                "mint_cu": r.mint_cu, "total_cu": r.total_cu, "slot": s.out.slot, "sig": s.out.sig});
            writeln!(f.borrow_mut(), "{}", line).expect("write --json");
        }
        self.rows.borrow_mut().push(r);
    }
    fn mark(&self) -> usize { self.rows.borrow().len() }
    fn table(&self, title: &str, from: usize) {
        println!("\n   {}", title);
        println!("   {:<20} {:<30} {:>5} {:>5} {:>5} {:>9} {:>9} {:>9} {:>9}  result", "case", "memo", "chars", "bytes", "tx B",
                 "CU limit", "Memo CU", "mint CU", "total");
        for r in &self.rows.borrow()[from..] {
            println!("   {:<20} {:<30} {:>5} {:>5} {:>5} {:>9} {:>9} {:>9} {:>9}  {}", r.id, r.label,
                     r.chars.map(|c| c.to_string()).unwrap_or("-".into()), r.bytes, r.tx_bytes,
                     r.limit.map(|l| n(l as u64)).unwrap_or("default".into()), on(r.memo_cu), on(r.mint_cu), on(r.total_cu), r.result);
        }
    }
    fn dump(&self, out: &Out) {
        if !out.logs.is_empty() {
            println!("     logs:\n       {}", out.logs.join("\n       "));
        }
    }

    // ---------------- checks ----------------
    /// The transaction must succeed and mint exactly curve_reference(supply before) to the
    /// wallet, with one "Minted" log, exactly audited_minted_log, and MintState = (fingerprint of
    /// this transaction, its slot)
    fn mint_ok(&self, id: &str, desc: &str, label: &str, memo: &[u8], ixs: &[Instruction], limit: Option<u32>) -> Sent {
        let ta = self.ta();
        let before = self.e.snap(&[ta], &[]);
        let expected = curve_reference(before.supply).map(|x| x as i128).unwrap_or(-1);
        let s = self.send(ixs);
        let after = self.e.snap(&[ta], &[]);
        let minted = after.bal(&ta) as i128 - before.bal(&ta) as i128;
        let supply_delta = after.supply as i128 - before.supply as i128;
        let state_ok = after.mint_state == Some((s.out.fp, s.out.slot));
        let log_ok = curve_reference(before.supply).is_some_and(|x| s.out.logs.contains(&audited_minted_log(x, &self.wallet(), before.supply)));
        let pass = s.out.ok() && minted == expected && supply_delta == expected && state_ok && s.out.minted_log_count() == 1 && log_ok;
        self.e.record(id, desc, pass, &format!("{}; expected {} minted {} supply delta {} mint_state matches tx {} mint logs {} mint log as expected {}",
            s.out.brief(), expected, minted, supply_delta, state_ok, s.out.minted_log_count(), log_ok));
        if !pass {
            self.dump(&s.out);
        }
        self.row(id, label, memo, limit, &s);
        s
    }
    /// The transaction must land and fail with exactly `err`, with `failed` as the innermost
    /// failing program and every line of `lines` in its logs, and change neither the supply,
    /// the wallet's balance nor MintState
    #[allow(clippy::too_many_arguments)]
    fn reject(&self, id: &str, desc: &str, label: &str, memo: &[u8], ixs: &[Instruction], limit: Option<u32>, err: &Value,
              failed: &Pubkey, lines: &[&str]) -> Sent {
        let ta = self.ta();
        let before = self.e.snap(&[ta], &[]);
        let s = self.send(ixs);
        let after = self.e.snap(&[ta], &[]);
        let missing: Vec<&&str> = lines.iter().filter(|t| !has_line(&s.out, t)).collect();
        let pass = s.out.landed && s.out.err.as_ref() == Some(err) && s.out.failed_program() == Some(*failed) && missing.is_empty() && before == after;
        self.e.record(id, desc, pass, &format!("expected {} raised by {} with logs {:?} and no state change; got {} raised by {:?}; missing {:?}; {}",
            err, failed, lines, s.out.brief(), s.out.failed_program(), missing, before.diff(&after)));
        if !pass {
            self.dump(&s.out);
        }
        self.row(id, label, memo, limit, &s);
        s
    }
    /// The transaction must land and run out of compute units in instruction `ix` (the whole
    /// limit consumed), changing neither the supply, the wallet's balance nor MintState. Whether
    /// the meter runs out inside a BPF instruction (ProgramFailedToComplete) or in a syscall or
    /// CPI charge (ComputationalBudgetExceeded) depends on the build, so either is accepted and
    /// the one seen is printed.
    fn reject_cu(&self, id: &str, desc: &str, label: &str, memo: &[u8], ixs: &[Instruction], limit: u32, ix: u64) -> Sent {
        let ta = self.ta();
        let before = self.e.snap(&[ta], &[]);
        let s = self.send(ixs);
        let after = self.e.snap(&[ta], &[]);
        let out_of_cu = s.out.err.as_ref().is_some_and(|x| *x == exceeded_at(ix) || *x == budget_exceeded_at(ix));
        let pass = s.out.landed && out_of_cu && s.total_cu == Some(limit as u64) && before == after;
        self.e.record(id, desc, pass, &format!("expected instruction {} out of compute units with all {} CU consumed and no state change; got {} raised by {:?}, {:?} CU; {}",
            ix, limit, s.out.brief(), s.out.failed_program(), s.total_cu, before.diff(&after)));
        if !pass {
            self.dump(&s.out);
        }
        self.row(id, label, memo, Some(limit), &s);
        s
    }
    /// The RPC must refuse the transaction as larger than 1,232 bytes (nothing lands or changes)
    fn reject_size(&self, id: &str, desc: &str, label: &str, memo: &[u8], ixs: &[Instruction], limit: Option<u32>) -> Sent {
        let ta = self.ta();
        let before = self.e.snap(&[ta], &[]);
        let s = self.send(ixs);
        let after = self.e.snap(&[ta], &[]);
        let pass = !s.out.landed && s.out.err_text().contains("too large") && s.tx_bytes > MAX_TX_BYTES && before == after;
        self.e.record(id, desc, pass, &format!("expected an RPC refusal (too large) for {} bytes and no state change; got {}; {}",
            s.tx_bytes, s.out.err_text(), before.diff(&after)));
        self.row(id, label, memo, limit, &s);
        s
    }
}

/// sha256 of a program's ELF (the account data for BPFLoader2, the programdata after its 45-byte
/// header for the upgradeable loader)
fn elf_sha256(e: &Env, program: &Pubkey) -> Option<String> {
    let acc = e.c.get_account(program).ok()?;
    let elf = if acc.owner == LOADER_V3 {
        let pd = Pubkey::new_from_array(acc.data.get(4..36)?.try_into().ok()?);
        e.c.get_account(&pd).ok()?.data.get(45..)?.to_vec()
    } else {
        acc.data
    };
    Some(hex::encode(Sha256::digest(&elf)))
}

/// Largest count whose transaction fits in 1,232 bytes
fn max_by_size(r: &R, ixs_for: &dyn Fn(usize) -> Vec<Instruction>) -> usize {
    let (mut lo, mut hi) = (0usize, MAX_TX_BYTES + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if r.size_of(&ixs_for(mid)) <= MAX_TX_BYTES { lo = mid } else { hi = mid }
    }
    lo
}

/// Largest count in [lo, hi) whose transaction succeeds in simulation, given that `lo` succeeds
/// and `hi` fails
fn max_by_sim(r: &R, ixs_for: &dyn Fn(usize) -> Vec<Instruction>, mut lo: usize, mut hi: usize) -> usize {
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if r.simulate(&ixs_for(mid)).0 { lo = mid } else { hi = mid }
    }
    lo
}

fn main() {
    let mut rev = None;
    let (mut so, mut json_path, mut mainnet_offset, mut rev4_miner_cu) = (None, None, None, None);
    let mut sections: Vec<String> = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--rev" => rev = args.next().and_then(|v| v.parse::<u8>().ok()),
            "--so" => so = args.next(),
            "--json" => json_path = args.next(),
            "--mainnet-offset" => mainnet_offset = args.next().and_then(|v| v.parse::<i64>().ok()),
            "--rev4-miner-cu" => rev4_miner_cu = args.next().and_then(|v| v.parse::<u64>().ok()),
            s if !s.starts_with("--") => sections.push(s.to_string()),
            s => {
                eprintln!("unknown option {}", s);
                std::process::exit(2);
            }
        }
    }
    let rev = match rev {
        Some(v @ (4 | 5)) => v,
        _ => {
            eprintln!("usage: repro-memo-mint-audit --rev 4|5 [--so <memo_mint.so>] [--json <path>] [--mainnet-offset <cu>] [--rev4-miner-cu <cu>] [sections]");
            std::process::exit(2);
        }
    };
    let e = Env::new(&format!("repro-memo-mint-audit (Revision {})", rev));
    let json = json_path.map(|p| RefCell::new(OpenOptions::new().create(true).append(true).open(p).expect("--json file")));
    let r = R { e: &e, rev, supply: e.supply(), last_bh: Cell::new(Hash::default()), json, rows: RefCell::new(vec![]) };
    println!("supply {} units ({} MEMO), reward {:?} units", r.supply, r.supply / UNIT, curve_reference(r.supply));
    let wants = |s: &str| sections.is_empty() || sections.iter().any(|x| x == s);

    build(&r, so.as_deref());
    if wants("s00") {
        s00(&r);
    }
    if wants("fit") {
        fit(&r);
    }
    if wants("mixed") {
        mixed(&r, mainnet_offset);
    }
    if wants("s01") {
        s01(&r);
    }
    if wants("cmp") {
        cmp(&r);
    }
    if wants("s43") {
        s43(&r);
    }
    if wants("s44") {
        s44(&r);
    }
    if wants("s04") {
        s04(&r);
    }
    if wants("slot") {
        slot(&r);
    }
    if wants("mint01") {
        mint01(&r);
    }
    if wants("cutover") {
        cutover(&r, mainnet_offset, rev4_miner_cu);
    }
    e.finish();
}

/// The loaded programs: the Memo ELF is X1's, the memo-mint ELF is the given file, and the
/// memo-mint revision named by --rev (its 68-byte rejection log differs between revisions)
fn build(r: &R, so: Option<&str>) {
    println!("\n-- loaded programs");
    let e = r.e;
    let memo = elf_sha256(e, &spl_memo::ID);
    e.record("ELF.memo", "the SPL Memo program is X1's build (sha256 f520eaf0...), so its CU here equal X1's",
        memo.as_deref() == Some(X1_MEMO_SHA256), &format!("sha256 {:?}", memo));
    let mint = elf_sha256(e, &e.mint_prog);
    println!("     memo-mint {} ELF sha256 {:?}", e.mint_prog, mint);
    if let Some(path) = so {
        let file = std::fs::read(path).map(|b| hex::encode(Sha256::digest(&b))).ok();
        e.record("ELF.mint", &format!("the loaded memo-mint ELF is {}", path), mint.is_some() && mint == file,
            &format!("loaded {:?}, file {:?}", mint, file));
    }
    let memo68 = "x".repeat(68);
    let s = r.send(&r.mint_ixs(memo68.as_bytes(), None));
    let rev4 = has_line(&s.out, "Program log: Memo too short: 68 bytes (minimum: 69)") && has_log(&s.out, "Must be at least 69 bytes.");
    let rev5 = has_line(&s.out, &memo_too_short_line(68)) && has_log(&s.out, "Must be at least 69 characters.");
    let loaded = if rev4 { 4 } else if rev5 { 5 } else { 0 };
    let pass = s.out.custom() == Some((1, MEMO_TOO_SHORT)) && loaded == r.rev;
    e.record("BUILD", &format!("the loaded memo-mint is Revision {} (its MemoTooShort log for a 68-byte memo)", r.rev), pass,
        &format!("{}; detected revision {}", s.out.brief(), loaded));
    for l in s.out.logs.iter().filter(|l| l.contains("too short") || l.contains("Error Message")) {
        println!("     log: {}", l);
    }
    if !pass {
        r.dump(&s.out);
        eprintln!("the loaded memo-mint is not Revision {}; stopping", r.rev);
        std::process::exit(2);
    }
}

/// S00 (with S23 / S27 / S32 / S56 / S57): one 69-character memo per script, no memo signer
/// accounts, [memo, process_mint, SetComputeUnitLimit], sent with the clients' 400,000 CU and
/// then with 1,400,000. Plane-2 ideographs are the cheapest memo that meets the minimum, so the
/// Revision 5 write-up's original claim that ASCII is again the cheapest way to meet the minimum
/// (corrected in MEMO-MINT-AUDIT.md, MINT-07) does not hold.
fn s00(r: &R) {
    println!("\n-- S00 / S23 / S27 / S32 / S56 / S57: 69-character memos by script");
    let e = r.e;
    let from = r.mark();
    // key, label, Memo CU, total, tx bytes; only for scripts whose 1,400,000 CU mint succeeded
    let mut measured: Vec<(&'static str, &'static str, u64, u64, usize)> = vec![];
    for sc in scripts() {
        let memo = (sc.memo)(69);
        assert_eq!(memo.chars().count(), 69, "{}", sc.key);
        let m = memo.as_bytes();
        let id = format!("S00.{}@400k", sc.key);
        if sc.fits_400k {
            r.mint_ok(&id, &format!("{} x 69 mints at the clients' 400,000 CU limit", sc.label), sc.label, m, &r.mint_ixs(m, Some(CLIENT_SIM_CU)), Some(CLIENT_SIM_CU));
        } else {
            let line = format!("Program {} consumed 400000 of 400000 compute units", spl_memo::ID);
            let fail = format!("Program {} failed: exceeded CUs meter at BPF instruction", spl_memo::ID);
            r.reject(&id, &format!("{} x 69 at the clients' 400,000 CU limit: the Memo program runs out at index 0", sc.label), sc.label, m,
                &r.mint_ixs(m, Some(CLIENT_SIM_CU)), Some(CLIENT_SIM_CU), &exceeded_at(0), &spl_memo::ID, &[&line, &fail]);
        }
        let s = r.mint_ok(&format!("S00.{}@1.4M", sc.key), &format!("{} x 69 mints at 1,400,000 CU", sc.label), sc.label, m,
            &r.mint_ixs(m, Some(MAX_CU)), Some(MAX_CU));
        let memo_cu = consumed(&s.out.logs, &spl_memo::ID);
        e.record(&format!("S00.{}.memo-cu", sc.key), &format!("Memo program CU of {} x 69 is {}{}", sc.label, n(sc.memo_cu),
            if sc.on_mainnet { " (= X1 mainnet's, static phase)" } else { " (first measured here)" }), memo_cu == Some(sc.memo_cu), &format!("local {:?}", memo_cu));
        if s.out.ok() {
            if let (Some(m_cu), Some(t)) = (memo_cu, s.total_cu) {
                measured.push((sc.key, sc.label, m_cu, t, s.tx_bytes));
            }
        }
    }
    r.table("69-character memos, [memo, process_mint, SetComputeUnitLimit]", from);

    let get = |k: &str| measured.iter().find(|m| m.0 == k).map(|m| m.2);
    let ascii_min = ["x", "tab", "quote", "bslash", "lf"].iter().filter_map(|k| get(k)).min();
    let plane2: Vec<u64> = ["p2", "p2mix", "p2spread", "p2f800"].iter().filter_map(|k| get(k)).collect();
    let cheapest = measured.iter().min_by_key(|m| m.2);
    let dearest = measured.iter().max_by_key(|m| m.2);
    println!("\n   cheapest 69-character memo (label, Memo CU, total CU): {:?}", cheapest.map(|m| (m.1, m.2, m.3)));
    println!("   most expensive 69-character memo (label, Memo CU, total CU): {:?}", dearest.map(|m| (m.1, m.2, m.3)));
    e.record("S00.plane2-below-ascii",
        "every plane-2 memo costs less in the Memo program than the cheapest 69-character ASCII memo (the original Revision 5 claim 'ASCII is again the cheapest', since corrected)",
        plane2.len() == 4 && ascii_min.is_some_and(|a| plane2.iter().all(|p| *p < a)), &format!("plane 2 {:?}, cheapest ASCII {:?}", plane2, ascii_min));
    e.record("S00.plane2-flat", "the Memo program charges the same for any plane-2 ideograph (the memo can vary at no cost)",
        plane2.len() == 4 && plane2.windows(2).all(|w| w[0] == w[1]), &format!("{:?}", plane2));
    e.record("S00.cheapest", "the cheapest 69-character memo measured is a plane-2 memo",
        cheapest.is_some_and(|m| m.0.starts_with("p2")), &format!("{:?}", cheapest.map(|m| m.0)));
    let order = [get("p2"), get("tab"), get("x")];
    e.record("S00.order", "Memo CU: plane 2 < TAB < printable ASCII", order.iter().all(|v| v.is_some()) && order[0] < order[1] && order[1] < order[2],
        &format!("{:?}", order));
    // Every transaction above was sent with a 1,400,000 CU limit, and one over 1,232 bytes never
    // lands, so the limits themselves are not asserted; the headroom is printed instead
    if let Some(m) = dearest {
        println!("   headroom of the most expensive ({}): {} CU, {} bytes", m.0, n((MAX_CU as u64).saturating_sub(m.3)), MAX_TX_BYTES.saturating_sub(m.4));
    }
    e.record("S00.all-fit", "every 69-character memo, the most expensive included, mints at 1,400,000 CU",
        measured.len() == scripts().len(),
        &format!("{} of {} minted; most expensive {:?}", measured.len(), scripts().len(), dearest.map(|m| (m.0, m.3))));
}

/// S23 / S57: how many characters of each script fit in one mint at 1,400,000 CU and 1,232
/// bytes, and which limit binds (the docs say the transaction size bounds the memo). The
/// maxima are pinned for the testnet-ID build below 10M MEMO; mainnet's memo-mint spends about
/// 1.4k CU more, which can lower a CU-bound maximum by one.
fn fit(r: &R) {
    println!("\n-- S23 / S57: longest memo per script at 1,400,000 CU and 1,232 bytes");
    let e = r.e;
    let from = r.mark();
    // (script, pinned maximum, bound by the transaction size rather than by CU)
    let pinned: [(&str, usize, bool); 14] = [("x", 780, true), ("tab", 780, true), ("e", 390, true), ("greek", 390, true), ("cyr", 390, true),
        ("p2", 195, true), ("zh", 124, false), ("zhs", 123, false), ("kana", 130, false), ("hangul", 104, false), ("globe", 79, false),
        ("emix", 79, false), ("teapot", 69, false), ("u1faf8", 69, false)];
    let mut summary = vec![];
    for (key, want, size_bound) in pinned {
        let sc = script(key);
        let ixs_for = |c: usize| r.mint_ixs((sc.memo)(c).as_bytes(), Some(MAX_CU));
        let by_size = max_by_size(r, &ixs_for);
        let (max, by_tx_size) = if r.simulate(&ixs_for(by_size)).0 {
            (by_size, true)
        } else if r.simulate(&ixs_for(69)).0 {
            (max_by_sim(r, &ixs_for, 69, by_size), false)
        } else {
            (0, false)
        };
        let bound = if by_tx_size { "1,232 bytes" } else { "1,400,000 CU" };
        e.record(&format!("FIT.{}", key), &format!("the longest {} memo is {} characters, bound by {}", sc.label, want,
            if size_bound { "the 1,232-byte transaction" } else { "1,400,000 CU" }), max == want && by_tx_size == size_bound,
            &format!("found {} characters, bound by {}", max, bound));
        if max < 69 {
            continue;
        }
        let memo = (sc.memo)(max);
        let s = r.mint_ok(&format!("FIT.{}.max", key), &format!("{} x {} mints", sc.label, max), sc.label, memo.as_bytes(), &ixs_for(max), Some(MAX_CU));
        let next = (sc.memo)(max + 1);
        let s2 = if by_tx_size {
            r.reject_size(&format!("FIT.{}.max+1", key), &format!("{} x {} exceeds 1,232 bytes", sc.label, max + 1), sc.label,
                next.as_bytes(), &ixs_for(max + 1), Some(MAX_CU))
        } else {
            r.reject_cu(&format!("FIT.{}.max+1", key), &format!("{} x {} exceeds 1,400,000 CU (memo-mint runs out)", sc.label, max + 1), sc.label,
                next.as_bytes(), &ixs_for(max + 1), MAX_CU, 1)
        };
        summary.push((sc.label, max, memo.len(), s.tx_bytes, s.total_cu, bound, s2.out.err_text()));
    }
    r.table("longest memo per script (max and max+1 characters)", from);
    println!("\n   {:<30} {:>6} {:>6} {:>6} {:>10}  {:<14} first failure", "script", "chars", "bytes", "tx B", "total CU", "bound");
    for (label, max, bytes, tx, total, bound, err) in summary {
        println!("   {:<30} {:>6} {:>6} {:>6} {:>10}  {:<14} {}", label, max, bytes, tx, on(total), bound, err.chars().take(90).collect::<String>());
    }
}

/// S23 (c)/(d), S56: the most CJK (or emoji) characters a 69-character memo padded with ASCII
/// can hold at the clients' 400,000 CU limit, in the memo-sdk shape and the memo-app shape; and
/// the CU of '中' x 32..34 + ASCII at 1.4M (Revision 5 minus Revision 4 gives the counting cost)
fn mixed(r: &R, mainnet_offset: Option<i64>) {
    println!("\n-- S23 / S56: CJK or emoji + ASCII padding at the clients' 400,000 CU limit");
    let from = r.mark();
    // (case, unit, memo-app shape, pinned largest count of `unit` that fits; the instruction
    // that runs out one count above: memo-mint is index 1 in the memo-sdk shape, 2 in memo-app's)
    for (key, unit, app, want, ix) in [("zh+a", "中", false, 33usize, 1u64), ("zh+a.app", "中", true, 32, 2), ("globe+a", "🌍", false, 21, 1)] {
        let memo_for = |k: usize| format!("{}{}", unit.repeat(k), "a".repeat(69 - k));
        let ixs_for = |k: usize| {
            let m = memo_for(k);
            if app { r.app_ixs(m.as_bytes(), CLIENT_SIM_CU) } else { r.mint_ixs(m.as_bytes(), Some(CLIENT_SIM_CU)) }
        };
        let k = max_by_sim(r, &ixs_for, 0, 69);
        let shape = if app { "memo-app shape (ATA create_idempotent)" } else { "memo-sdk shape" };
        r.e.record(&format!("MIX.{}", key), &format!("at most {} '{}' fit a 69-character memo at 400,000 CU, {}", want, unit, shape), k == want,
            &format!("found {}", k));
        let ok = memo_for(k);
        let s = r.mint_ok(&format!("MIX.{}.{}", key, k), &format!("'{}' x {} + 'a' x {} mints at 400,000 CU", unit, k, 69 - k),
            &format!("'{}'x{}+'a'x{}", unit, k, 69 - k), ok.as_bytes(), &ixs_for(k), Some(CLIENT_SIM_CU));
        if let (Some(o), Some(t)) = (mainnet_offset, s.total_cu) {
            println!("     mainnet estimate {} CU (local + offset {}), {} the 400,000 limit", n((t as i64 + o) as u64), o,
                     if t as i64 + o <= CLIENT_SIM_CU as i64 { "within" } else { "OVER" });
        }
        let bad = memo_for(k + 1);
        r.reject_cu(&format!("MIX.{}.{}", key, k + 1), &format!("'{}' x {} + 'a' x {} runs out of 400,000 CU", unit, k + 1, 68 - k),
            &format!("'{}'x{}+'a'x{}", unit, k + 1, 68 - k), bad.as_bytes(), &ixs_for(k + 1), CLIENT_SIM_CU, ix);
    }
    for k in [32usize, 33, 34] {
        let m = format!("{}{}", "中".repeat(k), "a".repeat(69 - k));
        r.mint_ok(&format!("MIX.zh{}@1.4M", k), &format!("'中' x {} + 'a' x {} mints at 1,400,000 CU (CU reference)", k, 69 - k),
            &format!("'中'x{}+'a'x{}", k, 69 - k), m.as_bytes(), &r.mint_ixs(m.as_bytes(), Some(MAX_CU)), Some(MAX_CU));
    }
    r.table("largest CJK / emoji share of a 69-character memo at 400,000 CU", from);
}

/// S01: Revision 4 accepts 69-byte memos of 18 to 35 characters (and 'é' x 68); Revision 5
/// rejects every one with MemoTooShort (6001) and logs the byte count
fn s01(r: &R) {
    println!("\n-- S01: 69-byte memos with fewer than 69 characters (Revision 4 floor per script)");
    let e = r.e;
    let from = r.mark();
    // (case, memo, Memo-program CU measured on X1 mainnet in the static phase)
    let cases: Vec<(&str, String, Option<u64>)> = vec![
        ("p2x17+x", format!("{}x", "\u{20000}".repeat(17)), Some(6_679)),
        ("e34+R", format!("{}R", "é".repeat(34)), Some(19_871)),
        ("bot", format!("æÀ{}a", "é".repeat(32)), Some(19_871)),
        ("zh23", "中".repeat(23), Some(256_886)),
        ("smile17+a", format!("{}a", "😀".repeat(17)), Some(295_985)),
        ("e68", "é".repeat(68), None),
    ];
    for (key, memo, mainnet_memo_cu) in cases {
        let m = memo.as_bytes();
        let chars = memo.chars().count();
        let label = format!("{} ({} B, {} chars)", key, m.len(), chars);
        let ixs = r.mint_ixs(m, Some(CLIENT_SIM_CU));
        let id = format!("S01.{}", key);
        let s = if r.rev == 4 {
            r.mint_ok(&id, &format!("Revision 4 mints with a {}-byte memo of {} characters", m.len(), chars), &label, m, &ixs, Some(CLIENT_SIM_CU))
        } else {
            r.reject(&id, &format!("Revision 5 rejects a {}-byte memo of {} characters with MemoTooShort", m.len(), chars), &label, m, &ixs,
                Some(CLIENT_SIM_CU), &custom_at(1, MEMO_TOO_SHORT), &e.mint_prog, &[&memo_too_short_line(m.len())])
        };
        if let Some(want) = mainnet_memo_cu {
            let got = consumed(&s.out.logs, &spl_memo::ID);
            e.record(&format!("S01.{}.memo-cu", key), &format!("Memo program CU equals X1 mainnet's {}", n(want)), got == Some(want), &format!("local {:?}", got));
        }
    }
    r.table("69-byte memos of fewer than 69 characters, at 400,000 CU", from);
}

/// S01 (CU): memo-mint's own CU for the memos of the MEMO-MINT-AUDIT.md Revision 5 table,
/// measured as there ([memo, process_mint], no compute budget instruction)
fn cmp(r: &R) {
    println!("\n-- S01 (CU): memo-mint CU of the MEMO-MINT-AUDIT.md Revision 5 table");
    let e = r.e;
    let from = r.mark();
    // (case, memo, documented Revision 4, documented Revision 5)
    let cases: Vec<(&str, String, Option<u64>, Option<u64>)> = vec![
        ("a69", "a".repeat(69), Some(15_571), Some(15_638)),
        ("e69", "é".repeat(69), Some(15_605), Some(17_751)),
        ("e137", "é".repeat(137), Some(15_673), Some(19_927)),
        ("x271+globe", format!("{}🌍", "x".repeat(271)), Some(15_674), Some(20_056)),
        ("a700", "a".repeat(700), Some(15_886), Some(15_886)),
    ];
    // The table was measured at a supply below 10M MEMO (1 MEMO per mint)
    let comparable = r.supply <= 10_000_000 * UNIT;
    for (key, memo, doc4, doc5) in cases {
        let m = memo.as_bytes();
        let s = r.mint_ok(&format!("CMP.{}", key), &format!("{} ({} bytes) mints", key, m.len()), key, m, &r.mint_ixs(m, None), None);
        let got = consumed(&s.out.logs, &e.mint_prog);
        let doc = if r.rev == 4 { doc4 } else { doc5 };
        match (doc, comparable) {
            (Some(want), true) => e.record(&format!("CMP.{}.doc", key), &format!("memo-mint CU equals the documented Revision {} value {}", r.rev, n(want)),
                got == Some(want), &format!("local {:?}", got)),
            (Some(want), false) => println!("     CMP.{}: documented {} at a supply below 10M; not compared at this supply", key, n(want)),
            _ => {}
        }
    }
    r.table("memo-mint CU, [memo, process_mint]", from);
}

/// S43: the slowest Revision 5 path is a 275-byte memo whose non-ASCII bytes come last, not
/// 'é' x 137, which the Revision 5 table called the slowest path before this re-audit
fn s43(r: &R) {
    println!("\n-- S43: memo-mint CU over 69-275-byte non-ASCII memos ([memo, process_mint])");
    let e = r.e;
    let mut cands: Vec<(String, String)> = vec![
        ("'é'x137 (274 B)".into(), "é".repeat(137)),
        ("'é'x137+'x'".into(), format!("{}x", "é".repeat(137))),
        ("'a'x275 (ASCII)".into(), "a".repeat(275)),
        ("'a'x276 (4x69 fast path)".into(), "a".repeat(276)),
        ("'é'x138 (276 B)".into(), "é".repeat(138)),
    ];
    for t in ["é", "中", "🌍", "éé", "é中", "中é", "🌍é", "é🌍", "中中", "🌍中", "中🌍", "🌍🌍", "éééé", "😀😀", "ééé"] {
        cands.push((format!("'x'x{}+{}", 275 - t.len(), t), format!("{}{}", "x".repeat(275 - t.len()), t)));
    }
    for k in 1..=9 {
        cands.push((format!("'x'x{}+'é'+'x'x{}", 273 - k, k), format!("{}é{}", "x".repeat(273 - k), "x".repeat(k))));
    }
    let mut sims: Vec<(String, String, u64)> = vec![];
    for (label, memo) in cands {
        let (ok, _, err, logs) = r.simulate(&r.mint_ixs(memo.as_bytes(), None));
        if !ok {
            println!("     {} failed in simulation: {}", label, err);
            continue;
        }
        sims.push((label, memo, consumed(&logs, &e.mint_prog).unwrap_or(0)));
    }
    sims.sort_by(|a, b| b.2.cmp(&a.2));
    println!("\n   {:<28} {:>6} {:>6} {:>9}", "memo (simulated)", "bytes", "chars", "mint CU");
    for (label, memo, c) in &sims {
        println!("   {:<28} {:>6} {:>6} {:>9}", label, memo.len(), memo.chars().count(), n(*c));
    }
    // Send 'é' x 137 and the three highest, and check that each sent CU equals its simulation
    let from = r.mark();
    let e137 = "é".repeat(137);
    let e137_sim = sims.iter().find(|s| s.1 == e137).map(|s| s.2);
    let base = r.mint_ok("S43.e137", "'é' x 137 (274 bytes) mints", "'é'x137 (274 B)", e137.as_bytes(), &r.mint_ixs(e137.as_bytes(), None), None);
    let b = consumed(&base.out.logs, &e.mint_prog);
    let mut sent = vec![(e137_sim, b)];
    let mut top = None;
    for (i, (label, memo, sim)) in sims.iter().take(3).enumerate() {
        let s = r.mint_ok(&format!("S43.top{}", i + 1), &format!("{} mints", label), label, memo.as_bytes(), &r.mint_ixs(memo.as_bytes(), None), None);
        let c = consumed(&s.out.logs, &e.mint_prog);
        sent.push((Some(*sim), c));
        if i == 0 {
            top = Some((label.clone(), memo.len(), c));
        }
    }
    r.table("'é' x 137 (the former 'slowest path') vs the measured maximum", from);
    e.record("S43.sim", "every sent memo-mint CU equals its simulation", sent.iter().all(|(s, c)| s.is_some() && s == c), &format!("{:?}", sent));
    let (top_label, top_len, t) = top.unwrap();
    if r.rev == 5 {
        e.record("S43.slowest", &format!("a 275-byte memo with its non-ASCII bytes last ({}) costs more memo-mint CU than 'é' x 137", top_label),
            top_len == 275 && t.is_some() && b.is_some() && t > b,
            &format!("'é' x 137 {:?}, maximum {:?} (+{:?})", b, t, t.zip(b).map(|(t, b)| t as i64 - b as i64)));
    } else {
        println!("     Revision 4 (no counting path): 'é' x 137 {:?}, maximum {:?}", b, t);
    }
}

/// S44 / S61: Revision 5's MemoTooShort log gives the memo's byte count against a minimum in
/// characters, so a rejected memo can log a number at or above 69
fn s44(r: &R) {
    println!("\n-- S44 / S61: MemoTooShort log lines");
    if r.rev != 5 {
        println!("     Revision 4 logs bytes against a byte minimum (see BUILD); nothing to reproduce");
        return;
    }
    let e = r.e;
    let from = r.mark();
    for (key, memo, limit) in [
        ("e34+R", format!("{}R", "é".repeat(34)), CLIENT_SIM_CU),
        ("e68", "é".repeat(68), CLIENT_SIM_CU),
        ("zh68", "中".repeat(68), MAX_CU),
        ("globe68", "🌍".repeat(68), MAX_CU),
    ] {
        let m = memo.as_bytes();
        let chars = memo.chars().count();
        let line = memo_too_short_line(m.len());
        let label = format!("{} ({} B, {} chars)", key, m.len(), chars);
        let s = r.reject(&format!("S44.{}", key), &format!("{} characters rejected; the log says '{} bytes (minimum: 69 characters)'", chars, m.len()),
            &label, m, &r.mint_ixs(m, Some(limit)), Some(limit), &custom_at(1, MEMO_TOO_SHORT), &e.mint_prog, &[&line]);
        let anchor = has_log(&s.out, "Error Code: MemoTooShort. Error Number: 6001. Error Message: Memo too short. Must be at least 69 characters.");
        let count_logged = s.out.logs.iter().any(|l| l.contains(&format!(" {} characters", chars)));
        e.record(&format!("S44.{}.log", key), "the error message names characters, and no log line gives the character count",
            anchor && !count_logged, &format!("anchor message {} character count logged {}", anchor, count_logged));
        for l in s.out.logs.iter().filter(|l| l.contains("too short") || l.contains("Error Message")) {
            println!("     log: {}", l);
        }
    }
    r.table("MemoTooShort rejections", from);
}

/// S04: invalid UTF-8 at index 0 never reaches memo-mint: the Memo program fails the
/// transaction at instruction 0 with InvalidInstructionData (Revision 5 relies on this)
fn s04(r: &R) {
    println!("\n-- S04: invalid UTF-8 memos at index 0");
    let from = r.mark();
    let cases: Vec<(&str, &str, Vec<u8>, usize)> = vec![
        ("ff69", "0xFF x 69 (memo-mint alone would count 69)", vec![0xFF; 69], 0),
        ("x68+c3", "'x' x 68 + lone 0xC3", [vec![b'x'; 68], vec![0xC3]].concat(), 68),
        ("80x276", "0x80 x 276 (memo-mint alone would accept: 4x69 fast path)", vec![0x80; 276], 0),
        ("c3x69", "0xC3 x 69 (lone lead bytes)", vec![0xC3; 69], 0),
    ];
    for (key, desc, memo, at) in cases {
        let line = format!("Program log: Invalid UTF-8, from byte {}", at);
        r.reject(&format!("S04.{}", key), &format!("{}: the Memo program rejects it at index 0", desc), key, &memo, &r.mint_ixs(&memo, None), None,
            &invalid_data_at(0), &spl_memo::ID, &[&line]);
    }
    r.table("invalid UTF-8 at index 0", from);
}

/// SLOT (new): memo-mint's CU depends on the slot. When MintState was already written in the
/// slot, record_mint also compares the 32-byte fingerprint, 27 CU. A mint whose CU limit is
/// exactly its simulated CU (simulated after a mint-free slot) fails when another mint lands
/// before it in the same slot. Two transactions are sent back to back until they share a slot.
fn slot(r: &R) {
    println!("\n-- SLOT: memo-mint CU depends on whether another mint landed earlier in the same slot");
    let e = r.e;
    let ta = r.ta();
    let (ma, mb) = ("x".repeat(69), "y".repeat(69)); // the same Memo-program and memo-mint cost
    // (a) both sized at 1.4M: the later one spends SAME_SLOT_CU more in memo-mint
    let mut observed = false;
    for attempt in 1..=20 {
        let bh = r.blockhash();
        let txa = e.build(&r.mint_ixs(ma.as_bytes(), Some(MAX_CU)), &e.wallet, &[], bh);
        let txb = e.build(&r.mint_ixs(mb.as_bytes(), Some(MAX_CU)), &e.wallet, &[], bh);
        let before = e.snap(&[ta], &[]);
        let (sa, sb) = (e.send(&txa), e.send(&txb));
        let (oa, ob) = match (sa, sb) {
            (Ok(a), Ok(b)) => (e.wait(&a), e.wait(&b)),
            other => {
                println!("     attempt {}: send failed {:?}", attempt, other);
                continue;
            }
        };
        if oa.slot != ob.slot {
            continue;
        }
        let after = e.snap(&[ta], &[]);
        let reward = curve_reference(before.supply).unwrap_or(0);
        // MintState holds the fingerprint of the later of the two
        let a_last = after.mint_state.map(|m| m.0) == Some(fingerprint(&txa.message));
        let (first, second) = if a_last { (&ob, &oa) } else { (&oa, &ob) };
        let (c1, c2) = (consumed(&first.logs, &e.mint_prog), consumed(&second.logs, &e.mint_prog));
        let pass = oa.ok() && ob.ok() && after.supply == before.supply + 2 * reward && c1.is_some() && c2 == c1.map(|c| c + SAME_SLOT_CU);
        e.record("SLOT.pair", &format!("two mints in slot {}: the later one spends {} CU more in memo-mint", oa.slot, SAME_SLOT_CU), pass,
            &format!("{} / {}; memo-mint first {:?} later {:?}", oa.brief(), ob.brief(), c1, c2));
        println!("     attempt {}: slot {}, memo-mint first {} later {}", attempt, oa.slot, on(c1), on(c2));
        observed = true;
        break;
    }
    if !observed {
        e.record("SLOT.pair", "two mints landed in the same slot", false, "they never shared a slot in 20 attempts");
    }
    // (b) Both sized at exactly their simulated CU (simulated in a slot without a mint): when
    // they land in the same slot, the later one's memo-mint needs 27 CU of the compute budget
    // instruction's 150, which then runs out (index 2); the earlier one mints
    let (ok_a, units_a, err_a, _) = r.simulate(&r.mint_ixs(ma.as_bytes(), Some(MAX_CU)));
    let (ok_b, units_b, err_b, _) = r.simulate(&r.mint_ixs(mb.as_bytes(), Some(MAX_CU)));
    assert!(ok_a && ok_b, "simulation: {} {}", err_a, err_b);
    for attempt in 1..=40 {
        let bh = r.blockhash();
        let txa = e.build(&r.mint_ixs(ma.as_bytes(), Some(units_a as u32)), &e.wallet, &[], bh);
        let txb = e.build(&r.mint_ixs(mb.as_bytes(), Some(units_b as u32)), &e.wallet, &[], bh);
        let before = e.snap(&[ta], &[]);
        let (sa, sb) = (e.send(&txa), e.send(&txb));
        let (oa, ob) = match (sa, sb) {
            (Ok(a), Ok(b)) => (e.wait(&a), e.wait(&b)),
            _ => continue,
        };
        if oa.slot != ob.slot {
            println!("     attempt {}: slots {} / {}", attempt, oa.slot, ob.slot);
            continue;
        }
        let after = e.snap(&[ta], &[]);
        let reward = curve_reference(before.supply).unwrap_or(0);
        let (won, lost, won_tx, lost_units) = if oa.ok() { (&oa, &ob, &txa, units_b) } else { (&ob, &oa, &txb, units_a) };
        let pass = won.ok() && lost.err == Some(budget_exceeded_at(2)) && r.total_cu(&lost.sig) == Some(lost_units)
            && consumed(&lost.logs, &e.mint_prog) == consumed(&won.logs, &e.mint_prog).map(|c| c + SAME_SLOT_CU)
            && after.supply == before.supply + reward && after.mint_state == Some((fingerprint(&won_tx.message), won.slot));
        e.record("SLOT.exact-limit", &format!("of two mints each sized at exactly its simulated CU ({} / {}) in slot {}, the later runs out of CU",
            n(units_a), n(units_b), oa.slot), pass, &format!("{} / {}; {}", oa.brief(), ob.brief(), before.diff(&after)));
        println!("     attempt {}: slot {}: earlier {}, later {}", attempt, oa.slot, won.brief(), lost.brief());
        for l in lost.logs.iter().filter(|l| l.contains("consumed") || l.contains("failed")) {
            println!("     log: {}", l);
        }
        return;
    }
    e.record("SLOT.exact-limit", "two exactly-sized mints landed in the same slot", false, "they never shared a slot in 40 attempts");
}

/// MINT-01 / S68: byte-identical instructions (memo-miner's memo with an unchanged `t`, user as
/// memo signer) signed with two blockhashes: when both land in one slot the second fails with
/// OneMintPerTransaction (6008) and pays its fee, as on mainnet
fn mint01(r: &R) {
    println!("\n-- S68 / MINT-01: the same instructions signed twice, in one slot");
    let e = r.e;
    let ta = r.ta();
    let ap = r.wallet();
    for attempt in 1..=20u64 {
        let m = miner_memo(1_793_500_000 + e.nonce() % 100_000);
        let ixs = vec![signed_memo_ix(&m, &ap), e.mint_ix(&ap), cu(MAX_CU)];
        let (bh1, bh2) = (r.blockhash(), r.blockhash());
        let (t1, t2) = (e.build(&ixs, &e.wallet, &[], bh1), e.build(&ixs, &e.wallet, &[], bh2));
        let before = e.snap(&[ta], &[]);
        let lamports = e.c.get_balance(&ap).unwrap_or(0);
        let (s1, s2) = (e.send(&t1), e.send(&t2));
        let (o1, o2) = match (s1, s2) {
            (Ok(a), Ok(b)) => (e.wait(&a), e.wait(&b)),
            _ => continue,
        };
        if o1.slot != o2.slot {
            println!("     attempt {}: slots {} / {}", attempt, o1.slot, o2.slot);
            continue;
        }
        let after = e.snap(&[ta], &[]);
        let paid = lamports.saturating_sub(e.c.get_balance(&ap).unwrap_or(0));
        let reward = curve_reference(before.supply).unwrap_or(0);
        let fp = fingerprint(&t1.message);
        let (won, lost) = if o1.ok() { (&o1, &o2) } else { (&o2, &o1) };
        let pass = fp == fingerprint(&t2.message) && won.ok() && lost.err == Some(custom_at(1, ONE_MINT_PER_TRANSACTION))
            && lost.error_name().as_deref() == Some("OneMintPerTransaction") && after.supply == before.supply + reward
            && after.bal(&ta) == before.bal(&ta) + reward && after.mint_state == Some((fp, won.slot)) && paid == 2 * 5_000;
        e.record("S68.same-slot", &format!("identical instructions signed with two blockhashes, both in slot {}: one mints, the other fails with 6008 and pays its fee", o1.slot),
            pass, &format!("{} / {}; fees paid {} lamports; {}", o1.brief(), o2.brief(), paid, before.diff(&after)));
        println!("     attempt {}: slot {}: {} / {}; fees paid {} lamports (local: 5,000 per signature)", attempt, o1.slot, o1.brief(), o2.brief(), paid);
        return;
    }
    e.record("S68.same-slot", "the two transactions landed in the same slot", false, "they never shared a slot in 20 attempts");
}

/// S16 / S25 / S30: the mainnet bot's transaction shape. Revision 5 rejects its memo whatever the
/// CU limit; a compliant replacement needs a higher limit than the bot's fixed 37,200 CU
fn cutover(r: &R, mainnet_offset: Option<i64>, rev4_miner_cu: Option<u64>) {
    println!("\n-- S16 / S25 / S30: the mainnet bot's transaction shape");
    let e = r.e;
    let from = r.mark();
    let bots: Vec<(&str, String, u32)> = vec![
        ("bot", format!("æÀ{}a", "é".repeat(32)), BOT_CU_LIMIT),
        ("e34+R", format!("{}R", "é".repeat(34)), BOT_CU_LIMIT),
        ("e34+W", format!("{}W", "é".repeat(34)), BOT2_CU_LIMIT),
    ];
    let mut offset = mainnet_offset;
    for (key, memo, limit) in &bots {
        let m = memo.as_bytes();
        assert!(m.len() == 69 && memo.chars().count() == 35, "{}", key);
        let label = format!("{} (69 B, 35 chars)", key);
        let id = format!("CUT.{}@{}", key, limit);
        if r.rev == 4 {
            let s = r.mint_ok(&id, &format!("Revision 4 mints the bot shape at its {} CU limit", n(*limit as u64)), &label, m, &r.mint_ixs(m, Some(*limit)), Some(*limit));
            println!("     margin under the limit: {:?} CU", s.total_cu.map(|t| *limit as i64 - t as i64));
            if *key == "bot" {
                if let Some(c) = consumed(&s.out.logs, &e.mint_prog) {
                    let o = MAINNET_BOT_MINT_CU as i64 - c as i64;
                    println!("     mainnet offset for this build (mainnet memo-mint {} - local {}): {} CU", n(MAINNET_BOT_MINT_CU), n(c), o);
                    offset = offset.or(Some(o));
                }
            }
        } else {
            r.reject(&id, &format!("Revision 5 rejects the bot shape (memo of 35 characters) at its {} CU limit", n(*limit as u64)), &label, m,
                &r.mint_ixs(m, Some(*limit)), Some(*limit), &custom_at(1, MEMO_TOO_SHORT), &e.mint_prog, &[&memo_too_short_line(69)]);
        }
    }
    // Compliant 69-character replacements: outcome at the bot's 37,200, then the exact minimum limit
    println!("\n   replacements (69 characters): at the bot's 37,200, at the exact minimum limit, and one below");
    let mut minima = vec![];
    for key in ["p2", "p2mix", "tab", "quote", "x"] {
        let sc = script(key);
        let memo = (sc.memo)(69);
        let m = memo.as_bytes();
        let (ok, units, err, _) = r.simulate(&r.mint_ixs(m, Some(MAX_CU)));
        assert!(ok, "{} at 1.4M: {}", key, err);
        let id = format!("CUT.{}@{}", key, BOT_CU_LIMIT);
        if units <= BOT_CU_LIMIT as u64 {
            r.mint_ok(&id, &format!("{} x 69 fits the bot's 37,200 here ({} CU)", sc.label, n(units)), sc.label, m, &r.mint_ixs(m, Some(BOT_CU_LIMIT)), Some(BOT_CU_LIMIT));
        } else {
            // the meter runs out in memo-mint (index 1) when more than the compute budget
            // instruction's 150 CU are missing, else in that instruction (index 2)
            let ix = if units - 150 > BOT_CU_LIMIT as u64 { 1 } else { 2 };
            r.reject_cu(&id, &format!("{} x 69 needs {} CU, more than the bot's 37,200", sc.label, n(units)), sc.label, m,
                &r.mint_ixs(m, Some(BOT_CU_LIMIT)), BOT_CU_LIMIT, ix);
        }
        let lim = units as u32;
        r.mint_ok(&format!("CUT.{}@min", key), &format!("{} x 69 mints at exactly its {} CU", sc.label, n(units)), sc.label, m,
            &r.mint_ixs(m, Some(lim)), Some(lim));
        r.reject(&format!("CUT.{}@min-1", key), &format!("{} x 69 fails at {} CU (in the compute budget instruction)", sc.label, n(units - 1)), sc.label, m,
            &r.mint_ixs(m, Some(lim - 1)), Some(lim - 1), &budget_exceeded_at(2), &COMPUTE_BUDGET, &[]);
        minima.push((key, sc.label, units));
    }
    // The bot's 37,200 on mainnet corresponds to 37,200 - offset here
    if let Some(o) = offset {
        let eq = (BOT_CU_LIMIT as i64 - o) as u32;
        println!("\n   the bot's 37,200 on mainnet = {} here (offset {})", n(eq as u64), o);
        if r.rev == 4 {
            let (_, memo, _) = &bots[0];
            let s = r.mint_ok("CUT.bot@eq", &format!("Revision 4 mints the bot shape at the local equivalent of 37,200 ({})", n(eq as u64)), "bot (69 B, 35 chars)",
                memo.as_bytes(), &r.mint_ixs(memo.as_bytes(), Some(eq)), Some(eq));
            println!("     margin under the equivalent limit: {:?} CU (137 on mainnet)", s.total_cu.map(|t| eq as i64 - t as i64));
        }
        let (_, label, units) = minima[0];
        let m = (script("p2").memo)(69);
        if units > eq as u64 {
            let ix = if units - 150 > eq as u64 { 1 } else { 2 };
            r.reject_cu("CUT.p2@eq", &format!("{} x 69, the cheapest compliant memo, runs out at the local equivalent of the bot's 37,200", label), label,
                m.as_bytes(), &r.mint_ixs(m.as_bytes(), Some(eq)), eq, ix);
        } else {
            r.mint_ok("CUT.p2@eq", &format!("{} x 69 fits the local equivalent of the bot's 37,200", label), label, m.as_bytes(),
                &r.mint_ixs(m.as_bytes(), Some(eq)), Some(eq));
        }
    }
    // The live mainnet traffic since 09:18Z (S69): memo-miner's memo, signed, with CU limit =
    // simulated + 50. Revision 5 accepts the memo (69 ASCII characters) and adds CU to
    // memo-mint for this shape (76 CU, measured), more than the margin: a transaction simulated on
    // Revision 4 and executed on Revision 5 runs out (only around the upgrade); one simulated in a
    // slot that already had a mint carries 27 CU more (SLOT) and still fits
    println!("\n   memo-miner shape: signed 69-byte ASCII JSON memo, CU limit = simulated + {}", MINER_CU_MARGIN);
    let t = 1_793_400_000 + e.nonce() % 100_000;
    let ap = r.wallet();
    let miner = |t: u64, limit: u32| vec![signed_memo_ix(&miner_memo(t), &ap), e.mint_ix(&ap), cu(limit)];
    let (ok, units, err, _) = r.simulate(&miner(t, MAX_CU));
    assert!(ok, "memo-miner shape at 1.4M: {}", err);
    let m0 = miner_memo(t);
    assert_eq!(m0.len(), 69);
    let s = r.mint_ok("CUT.miner@sim+50", &format!("Revision {} mints the memo-miner shape at its simulated {} + 50 CU", r.rev, n(units)), "memo-miner (signed)",
        m0.as_bytes(), &miner(t, (units + MINER_CU_MARGIN) as u32), Some((units + MINER_CU_MARGIN) as u32));
    println!("     memo-miner shape: simulated {} CU (pass it to a Revision 5 run as --rev4-miner-cu when this is Revision 4); margin {:?}",
             units, s.total_cu.map(|c| units + MINER_CU_MARGIN - c));
    if let (5, Some(x4)) = (r.rev, rev4_miner_cu) {
        let lim = (x4 + MINER_CU_MARGIN) as u32;
        let m1 = miner_memo(t + 1);
        if units > lim as u64 {
            let ix = if units - 150 > lim as u64 { 1 } else { 2 };
            r.reject_cu("CUT.miner@rev4+50", &format!("simulated on Revision 4 ({}) + 50, the memo-miner shape runs out of CU on Revision 5 ({})", n(x4), n(units)),
                "memo-miner (signed)", m1.as_bytes(), &miner(t + 1, lim), lim, ix);
        } else {
            e.record("CUT.miner@rev4+50", "Revision 5 spends more than the Revision 4 simulation + 50", false, &format!("Revision 4 {} Revision 5 {}", x4, units));
        }
        let lim77 = (x4 + MINER_CU_MARGIN + SAME_SLOT_CU) as u32;
        let m2 = miner_memo(t + 2);
        r.mint_ok("CUT.miner@rev4+77", &format!("Revision 4 simulation + 77 (a simulation that took the 27-CU same-slot path) still fits Revision 5 ({})", n(units)),
            "memo-miner (signed)", m2.as_bytes(), &miner(t + 2, lim77), Some(lim77));
    }
    r.table("bot shape and compliant replacements", from);
    println!("\n   {:<30} {:>10} {:>14} {:>18}", "replacement memo (69 chars)", "local min", "mainnet min", "mainnet fee");
    for (_, label, units) in minima {
        let main = offset.map(|o| units as i64 + o);
        println!("   {:<30} {:>10} {:>14} {:>18}", label, n(units), main.map(|v| n(v as u64)).unwrap_or("-".into()),
                 main.map(|v| format!("{} lamports", n(1_500 + 10 * v as u64))).unwrap_or("-".into()));
    }
    println!("   (mainnet = local + offset {:?}; X1 fee = 1,500 + 10 x CU limit; the bot pays {} at 37,200)", offset, n(1_500 + 10 * BOT_CU_LIMIT as u64));
}
