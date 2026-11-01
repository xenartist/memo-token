//! memo-mint's own compute units, per build, for the Revision 5 audit's options stage.
//!
//! The audit proposes program changes (constant PDAs, a cheaper success log, a faster character
//! count, a clearer MemoTooShort log) and measures them as separate builds, each loaded in place of
//! memo-mint on a fresh local validator (scripts/local-validator.sh --so-dir). This client sends the
//! same transactions to every build and prints memo-mint's CU, so the builds compare directly:
//!   CU   [memo, process_mint] and [memo, process_mint_to(recipient = the wallet itself)], no compute
//!        budget instruction (as in the MEMO-MINT-AUDIT.md Revision 5 table), with the memos 'a' x 69,
//!        'é' x 69, 'é' x 137 and 'a' x 700, each sent --reps times (default 2): the CU must repeat
//!   LOG  every mint must log its success line in the format the build uses (--log), and the
//!        "Instruction: <name>" line unless the build drops it (--ix-name off)
//!   SHORT rejected memos of 68 bytes, 69 bytes / 35 characters and 68 'é' (process_mint): exact
//!        error 6001 raised by memo-mint, no state change, and the build's MemoTooShort log (--short-log)
//!   COUNT (--count-check <n>) n random memos of 60-75 characters mixing 1- to 4-byte characters,
//!        simulated: memo-mint must accept exactly those of 69 or more characters (a check of a
//!        changed character count on-chain, e.g. the 8-bytes-at-a-time count of option O3)
//! Every mint is checked as in the regression suites (exact amount against the curve reference,
//! supply delta, MintState = (fingerprint of the transaction, its slot)) and must land in a slot
//! without an earlier mint (a mint after another one in the same slot spends 27 more CU).
//!
//! Run (one build per validator start):
//!   ./scripts/local-validator.sh start --so-dir <dir with memo_mint.so> --supply 5000000 --balance 1000000
//!   (cd clients && MEMO_PROGRAM_ENV=testnet MEMO_RPC_URL=http://127.0.0.1:8899 \
//!     MEMO_WALLET=../target/local-validator/wallet.json cargo run --bin options-memo-mint-cu -- \
//!     --variant O1a --so <dir>/memo_mint.so [--log full|no-owner|event] [--ix-name on|off] \
//!     [--short-log rev4|rev5|o4|o4-no-count] [--reps 2] [--count-check <n>] [--json <path>])
//!   ./scripts/local-validator.sh stop
//! Defaults describe Revision 5 (--log full --ix-name on --short-log rev5).

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use std::{cell::RefCell, fs::OpenOptions, io::Write, thread::sleep, time::Duration};

use base64::{engine::general_purpose, Engine as _};
use harness::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_client::{rpc_config::RpcSimulateTransactionConfig, rpc_request::RpcRequest};
use solana_sdk::{commitment_config::CommitmentConfig, instruction::Instruction, pubkey::Pubkey, signature::Signer};

const MEMO_TOO_SHORT: u32 = 6001;
/// sha256 of X1's SPL Memo program (MemoSq4g..., the same on testnet and mainnet)
const X1_MEMO_SHA256: &str = "f520eaf096361abbb9639ea4dc3e5388a87b9330e121f476607b87c46ef67954";
const LOADER_V3: Pubkey = solana_sdk::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");

#[derive(Clone, Copy, PartialEq)]
enum SuccessLog {
    /// Revision 5: "Minted {amount} units to {owner}, supply before mint: {supply} units"
    Full,
    /// "Minted {amount} units, supply before mint: {supply} units"
    NoOwner,
    /// Anchor event `Minted { amount: u64, owner: Pubkey, supply_before: u64 }` ("Program data: ...")
    Event,
}

#[derive(Clone, Copy, PartialEq)]
enum ShortLog {
    /// Revision 4: "Memo too short: {bytes} bytes (minimum: 69)" (a byte minimum)
    Rev4,
    /// Revision 5: "Memo too short: {bytes} bytes (minimum: 69 characters)"
    Rev5,
    /// "Memo too short: {bytes} bytes" below 69 bytes, else "Memo too short: {chars} characters (minimum: 69)"
    O4,
    /// "Memo too short: {bytes} bytes" below 69 bytes, else "Memo too short: fewer than 69 characters ({bytes} bytes)"
    O4NoCount,
}

fn consumed(logs: &[String], prog: &Pubkey) -> Option<u64> {
    let p = format!("Program {} consumed ", prog);
    logs.iter().filter_map(|l| l.strip_prefix(p.as_str())).filter_map(|r| r.split(' ').next()?.parse().ok()).last()
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

struct Run<'a> {
    e: &'a Env,
    variant: String,
    log: SuccessLog,
    ix_name: bool,
    short: ShortLog,
    json: Option<RefCell<std::fs::File>>,
    /// (case, memo-mint CU of each repetition, Memo CU, total CU)
    rows: RefCell<Vec<(String, Vec<Option<u64>>, Option<u64>, Option<u64>)>>,
}

impl Run<'_> {
    fn total_cu(&self, sig: &str) -> Option<u64> {
        self.e.c.send::<Value>(RpcRequest::GetTransaction,
            json!([sig, {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}]))
            .ok().and_then(|v| v["meta"]["computeUnitsConsumed"].as_u64())
    }
    /// Waits until the chain has left the slot of the last mint, so that the next mint lands in a
    /// slot without an earlier mint (MintState copied from testnet holds a slot this validator never
    /// reaches, hence `==`)
    fn after_last_mint(&self) {
        let last = self.e.mint_state_value().map(|v| v.1).unwrap_or(0);
        while self.e.c.get_slot_with_commitment(CommitmentConfig::processed()).unwrap_or(0) == last {
            sleep(Duration::from_millis(50));
        }
    }
    fn success_line(&self, amount: u64, owner: &Pubkey, supply_before: u64) -> String {
        match self.log {
            SuccessLog::Full => format!("Program log: Minted {} units to {}, supply before mint: {} units", amount, owner, supply_before),
            SuccessLog::NoOwner => format!("Program log: Minted {} units, supply before mint: {} units", amount, supply_before),
            SuccessLog::Event => {
                let mut d = Sha256::digest(b"event:Minted")[..8].to_vec();
                d.extend(amount.to_le_bytes());
                d.extend(owner.as_ref());
                d.extend(supply_before.to_le_bytes());
                format!("Program data: {}", general_purpose::STANDARD.encode(d))
            }
        }
    }
    fn short_line(&self, memo: &str) -> Option<String> {
        let (bytes, chars) = (memo.len(), memo.chars().count());
        match self.short {
            ShortLog::Rev4 if bytes < 69 => Some(format!("Program log: Memo too short: {} bytes (minimum: 69)", bytes)),
            ShortLog::Rev4 => None, // Revision 4 counts bytes: a 69-byte memo mints
            ShortLog::Rev5 => Some(format!("Program log: Memo too short: {} bytes (minimum: 69 characters)", bytes)),
            ShortLog::O4 if bytes < 69 => Some(format!("Program log: Memo too short: {} bytes", bytes)),
            ShortLog::O4 => Some(format!("Program log: Memo too short: {} characters (minimum: 69)", chars)),
            ShortLog::O4NoCount if bytes < 69 => Some(format!("Program log: Memo too short: {} bytes", bytes)),
            ShortLog::O4NoCount => Some(format!("Program log: Memo too short: fewer than 69 characters ({} bytes)", bytes)),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn json_row(&self, id: &str, memo: &str, ix: &str, rep: usize, out: &Out, mint_cu: Option<u64>, memo_cu: Option<u64>, total: Option<u64>) {
        if let Some(f) = &self.json {
            let line = json!({"variant": self.variant, "supply": self.e.supply(), "id": id, "ix": ix, "bytes": memo.len(),
                "chars": memo.chars().count(), "rep": rep, "ok": out.ok(), "err": out.err, "mint_cu": mint_cu, "memo_cu": memo_cu,
                "total_cu": total, "slot": out.slot, "sig": out.sig});
            writeln!(f.borrow_mut(), "{}", line).expect("write --json");
        }
    }

    /// The mint must succeed with the exact curve amount to the wallet's token account, MintState =
    /// (fingerprint, slot), in a slot without an earlier mint, with exactly the build's success line
    /// and instruction-name line. Returns (memo-mint CU, Memo CU, total CU).
    fn mint(&self, id: &str, memo: &str, ix: Instruction, ix_name: &str, rep: usize) -> (Option<u64>, Option<u64>, Option<u64>) {
        let e = self.e;
        let wallet = e.wallet.pubkey();
        let ta = e.ata(&wallet);
        self.after_last_mint();
        let before = e.snap(&[ta], &[]);
        let expected = curve_reference(before.supply).map(|x| x as i128).unwrap_or(-1);
        let out = e.exec(&[memo_ix(memo.as_bytes()), ix], &e.wallet, &[]);
        let after = e.snap(&[ta], &[]);
        let minted = after.bal(&ta) as i128 - before.bal(&ta) as i128;
        let supply_delta = after.supply as i128 - before.supply as i128;
        let state_ok = after.mint_state == Some((out.fp, out.slot));
        let fresh_slot = before.mint_state.map(|m| m.1) != Some(out.slot);
        let line = self.success_line(expected.max(0) as u64, &wallet, before.supply);
        let success_lines: Vec<&String> = out.logs.iter()
            .filter(|l| l.starts_with("Program log: Minted ") || l.starts_with("Program data: ")).collect();
        let log_ok = success_lines.len() == 1 && *success_lines[0] == line;
        let name_line = format!("Program log: Instruction: {}", ix_name);
        let name_present = out.logs.iter().any(|l| *l == name_line);
        let name_ok = name_present == self.ix_name;
        let pass = out.ok() && minted == expected && supply_delta == expected && state_ok && fresh_slot && log_ok && name_ok;
        let (mint_cu, memo_cu) = (consumed(&out.logs, &e.mint_prog), consumed(&out.logs, &spl_memo::ID));
        let total = if out.landed { self.total_cu(&out.sig) } else { None };
        e.record(&format!("{}#{}", id, rep), &format!("{} ({} bytes, {} chars) mints; memo-mint {:?} CU", ix_name, memo.len(), memo.chars().count(), mint_cu),
            pass, &format!("{}; expected {} minted {} supply delta {} mint_state matches tx {} slot without an earlier mint {}; success lines {:?} (want {:?}); '{}' present {} (want {})",
                out.brief(), expected, minted, supply_delta, state_ok, fresh_slot, success_lines, line, name_line, name_present, self.ix_name));
        if !pass {
            println!("     logs:\n       {}", out.logs.join("\n       "));
        }
        self.json_row(id, memo, ix_name, rep, &out, mint_cu, memo_cu, total);
        (mint_cu, memo_cu, total)
    }

    /// The transaction must fail with Custom(1, 6001) raised by memo-mint, change no watched state,
    /// and log exactly the build's MemoTooShort line
    fn short(&self, id: &str, memo: &str, line: &str) {
        let e = self.e;
        let wallet = e.wallet.pubkey();
        let ta = e.ata(&wallet);
        let before = e.snap(&[ta], &[]);
        let out = e.exec(&[memo_ix(memo.as_bytes()), e.mint_ix(&wallet)], &e.wallet, &[]);
        let after = e.snap(&[ta], &[]);
        // the program's own line (the AnchorError line that follows quotes the error message)
        let too_short: Vec<&String> = out.logs.iter().filter(|l| l.starts_with("Program log: Memo too short:")).collect();
        let pass = Expect::Raised(1, MEMO_TOO_SHORT, e.mint_prog, "MemoTooShort").matches(&out) && before == after
            && too_short.len() == 1 && too_short[0] == line;
        let mint_cu = consumed(&out.logs, &e.mint_prog);
        e.record(id, &format!("{} bytes / {} characters rejected (6001) with '{}'", memo.len(), memo.chars().count(), line), pass,
            &format!("{} ({:?} in {:?}); {}; MemoTooShort lines {:?}", out.brief(), out.error_name(), out.failed_program(), before.diff(&after), too_short));
        println!("     {}: memo-mint {:?} CU (failing path)", id, mint_cu);
        self.json_row(id, memo, "ProcessMint", 0, &out, mint_cu, consumed(&out.logs, &spl_memo::ID), None);
    }
}

/// xorshift64: deterministic, no extra dependency
fn next(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

/// COUNT: random memos of 60 to 75 characters mixing 1- to 4-byte characters (most take the
/// counting path: at least 69 bytes, at most 275), simulated as [memo, process_mint,
/// SetComputeUnitLimit(1.4M)] (nothing lands). memo-mint must accept exactly the memos of 69 or more
/// characters (str::chars), and reject the others with 6001 and the build's MemoTooShort line, which
/// with --short-log o4 carries the character count memo-mint computed on-chain.
fn count_check(r: &Run, n: usize) {
    let e = r.e;
    let wallet = e.wallet.pubkey();
    // (character, weight): ASCII, 2-, 3- and 4-byte characters including the first and last of each width
    let pool: &[(char, u64)] = &[('a', 30), ('z', 15), ('\u{7F}', 5), ('\u{80}', 4), ('\u{E9}', 12), ('\u{436}', 6), ('\u{7FF}', 4),
        ('\u{800}', 3), ('\u{4E2D}', 4), ('\u{FFFF}', 2), ('\u{10000}', 2), ('\u{1F30D}', 2), ('\u{10FFFF}', 1)];
    let total: u64 = pool.iter().map(|p| p.1).sum();
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let (mut accepted, mut rejected, mut counted) = (0, 0, 0);
    let mut bad: Vec<String> = vec![];
    for _ in 0..n {
        let chars = 60 + (next(&mut x) % 16) as usize;
        let s: String = (0..chars).map(|_| {
            let mut k = next(&mut x) % total;
            pool.iter().find(|(_, w)| if k < *w { true } else { k -= w; false }).unwrap().0
        }).collect();
        if (69..276).contains(&s.len()) && !s.is_ascii() {
            counted += 1;
        }
        let ixs = vec![memo_ix(s.as_bytes()), e.mint_ix(&wallet), cu(1_400_000)];
        let tx = e.build(&ixs, &e.wallet, &[], e.c.get_latest_blockhash().expect("blockhash"));
        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false, replace_recent_blockhash: true, commitment: Some(CommitmentConfig::processed()), ..Default::default()
        };
        let v = e.c.simulate_transaction_with_config(&tx, cfg).expect("simulate").value;
        let logs = v.logs.unwrap_or_default();
        let want_ok = s.chars().count() >= 69;
        let ok = match &v.err {
            None => {
                accepted += 1;
                want_ok
            }
            Some(err) => {
                rejected += 1;
                let lines: Vec<&String> = logs.iter().filter(|l| l.starts_with("Program log: Memo too short:")).collect();
                !want_ok && serde_json::to_value(err).ok() == Some(json!({"InstructionError": [1, {"Custom": MEMO_TOO_SHORT}]}))
                    && r.short_line(&s).is_some_and(|want| lines.len() == 1 && *lines[0] == want)
            }
        };
        if !ok {
            bad.push(format!("{} bytes {} chars {:?}: err {:?} logs {:?}", s.len(), s.chars().count(), s, v.err,
                logs.iter().filter(|l| l.contains("too short")).collect::<Vec<_>>()));
        }
    }
    println!("     {} memos: {} accepted, {} rejected, {} took the counting path (69-275 bytes, not ASCII)", n, accepted, rejected, counted);
    for b in bad.iter().take(5) {
        println!("     mismatch: {}", b);
    }
    e.record("COUNT", &format!("{} random 60-75-character memos: memo-mint accepts exactly those of 69+ characters and logs its MemoTooShort line for the rest", n),
        bad.is_empty(), &format!("{} mismatches", bad.len()));
}

fn usage() -> ! {
    eprintln!("usage: options-memo-mint-cu --variant <name> [--so <memo_mint.so>] [--log full|no-owner|event] [--ix-name on|off] \
               [--short-log rev4|rev5|o4|o4-no-count] [--reps <n>] [--count-check <n>] [--json <path>]");
    std::process::exit(2);
}

fn main() {
    let (mut variant, mut so, mut json_path, mut reps, mut count) = (None, None, None, 2usize, 0usize);
    let (mut log, mut ix_name, mut short) = (SuccessLog::Full, true, ShortLog::Rev5);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let v = args.next().unwrap_or_else(|| usage());
        match (a.as_str(), v.as_str()) {
            ("--variant", x) => variant = Some(x.to_string()),
            ("--so", x) => so = Some(x.to_string()),
            ("--json", x) => json_path = Some(x.to_string()),
            ("--reps", n) => reps = n.parse().unwrap_or_else(|_| usage()),
            ("--count-check", n) => count = n.parse().unwrap_or_else(|_| usage()),
            ("--log", "full") => log = SuccessLog::Full,
            ("--log", "no-owner") => log = SuccessLog::NoOwner,
            ("--log", "event") => log = SuccessLog::Event,
            ("--ix-name", "on") => ix_name = true,
            ("--ix-name", "off") => ix_name = false,
            ("--short-log", "rev4") => short = ShortLog::Rev4,
            ("--short-log", "rev5") => short = ShortLog::Rev5,
            ("--short-log", "o4") => short = ShortLog::O4,
            ("--short-log", "o4-no-count") => short = ShortLog::O4NoCount,
            _ => usage(),
        }
    }
    let variant = variant.unwrap_or_else(|| usage());
    let e = Env::new(&format!("options-memo-mint-cu ({})", variant));
    let json = json_path.map(|p| RefCell::new(OpenOptions::new().create(true).append(true).open(p).expect("--json file")));
    let r = Run { e: &e, variant: variant.clone(), log, ix_name, short, json, rows: RefCell::new(vec![]) };
    let supply = e.supply();
    println!("supply {} units ({} MEMO), reward {:?} units", supply, supply / UNIT, curve_reference(supply));

    // The loaded programs: X1's Memo ELF (so Memo CU here equal X1's) and the given memo-mint build
    let memo = elf_sha256(&e, &spl_memo::ID);
    e.record("ELF.memo", "the SPL Memo program is X1's build (sha256 f520eaf0...)", memo.as_deref() == Some(X1_MEMO_SHA256), &format!("sha256 {:?}", memo));
    let mint = elf_sha256(&e, &e.mint_prog);
    println!("     memo-mint {} ELF sha256 {:?}", e.mint_prog, mint);
    if let Some(path) = &so {
        let file = std::fs::read(path).map(|b| hex::encode(Sha256::digest(&b))).ok();
        e.record("ELF.mint", &format!("the loaded memo-mint ELF is {}", path), mint.is_some() && mint == file, &format!("loaded {:?}, file {:?}", mint, file));
    }

    println!("\n-- CU: [memo, process_mint] and [memo, process_mint_to(self)], no compute budget instruction");
    let wallet = e.wallet.pubkey();
    let memos: Vec<(&str, String)> = vec![
        ("a69", "a".repeat(69)),
        ("e69", "é".repeat(69)),
        ("e137", "é".repeat(137)),
        ("a700", "a".repeat(700)),
    ];
    for (key, memo) in &memos {
        for (ix_key, ix_name) in [("mint", "ProcessMint"), ("mint_to", "ProcessMintTo")] {
            let id = format!("CU.{}.{}", ix_key, key);
            let mut cus = vec![];
            let (mut memo_cu, mut total) = (None, None);
            for rep in 1..=reps {
                let ix = if ix_key == "mint" { e.mint_ix(&wallet) } else { e.mint_to_ix(&wallet, &wallet, &e.ata(&wallet)) };
                let (c, m, t) = r.mint(&id, memo, ix, ix_name, rep);
                cus.push(c);
                memo_cu = m;
                total = t;
            }
            e.record(&format!("{}.repeat", id), "memo-mint CU is the same in every repetition",
                cus.iter().all(|c| c.is_some() && *c == cus[0]), &format!("{:?}", cus));
            r.rows.borrow_mut().push((id, cus, memo_cu, total));
        }
    }

    println!("\n-- SHORT: MemoTooShort log lines ([memo, process_mint])");
    for (key, memo) in [("x68", "x".repeat(68)), ("e34+R", format!("{}R", "é".repeat(34))), ("e68", "é".repeat(68))] {
        match r.short_line(&memo) {
            Some(line) => r.short(&format!("SHORT.{}", key), &memo, &line),
            None => println!("     SHORT.{}: this build counts bytes and mints a {}-byte memo; not sent", key, memo.len()),
        }
    }

    if count > 0 {
        println!("\n-- COUNT: random 60-75-character memos, simulated ([memo, process_mint, SetComputeUnitLimit(1.4M)])");
        count_check(&r, count);
    }

    println!("\n   {:<18} {:>9} {:>9} {:>9}  ({})", "case", "memo-mint", "Memo", "total", variant);
    for (id, cus, memo_cu, total) in r.rows.borrow().iter() {
        let c = cus.first().copied().flatten();
        println!("   {:<18} {:>9} {:>9} {:>9}", id, c.map(|v| v.to_string()).unwrap_or("-".into()),
                 memo_cu.map(|v| v.to_string()).unwrap_or("-".into()), total.map(|v| v.to_string()).unwrap_or("-".into()));
    }
    e.finish();
}
