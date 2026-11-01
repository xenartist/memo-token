//! Stateful model-based fuzzer for memo-mint (local validator only).
//!
//! Sends random transactions that mint MEMO, or try to, from four funded wallets (A is the
//! regression wallet; C also holds a second MEMO token account that is not an ATA) through every
//! path into memo-mint: process_mint and process_mint_to at the top level, memo-blog mint_for_blog
//! and memo-forum mint_for_post (process_mint by CPI), and memo-home mine (process_mint_to by CPI
//! into an Asset's Vault, with the share to a chosen token account). A transaction has its SPL Memo
//! at index 0, elsewhere, twice, or not at all, or another program at index 0 (ComputeBudget,
//! System, the legacy Memo v1). Memo texts come from generators (ASCII, TAB / quotes / backslash,
//! ASCII controls and NUL, Latin-1 and its controls, other 2-byte scripts, combining marks, CJK /
//! Hangul / kana, zero-width and other 3-byte characters, plane-2 CJK, emoji, other 4-byte
//! characters, mixtures, two segments of different styles (e.g. an ASCII run, then multi-byte
//! text), fixed edge patterns, invalid UTF-8) with lengths clustered at the boundaries (66-72
//! characters, 66-72 bytes, 268-280 bytes) and up to the transaction size limit (and rarely past
//! it). Memo-mint instructions sit at random indices, sometimes duplicated, with occasional account
//! and data substitutions: another mint, a System account or a missing account as the mint, another
//! owner's or another mint's token account, the MEMO mint as token account, the memo-burn PDA as
//! mint authority, another sysvar, the legacy token program, MintState impostors (missing, zeroed,
//! 4 bytes, memo-burn's, the shadow MintState), read-only or unsigned accounts, too few or extra
//! accounts, bad discriminators and arguments, a recipient that does not own the token account,
//! minting into a memo-home Vault; and rarely initialize_mint_state by a non-admin while MintState
//! exists. Messages are legacy or v0 over an address lookup table. The compute unit limit is the
//! default, ample, set at index 0, above the 1.4M cap, tiny, or too low for a chosen instruction.
//! Some steps resend the same instructions in a later slot (two or more slots later with a new
//! blockhash, or at once from another fee payer, usually the next slot), resend the very same
//! signed transaction, or send two transactions with one blockhash into one slot (same or different
//! fingerprints).
//!
//! An independent model predicts each transaction: success, or the exact error (instruction index
//! and the Anchor code, name and failing program, or the exact runtime error), in the order the
//! checks run: the Memo program (signers, then UTF-8); memo-mint's dispatch, argument, account
//! deserialization field by field, constraints field by field (mut before address), then its handler
//! (memo position, characters counted with str::chars after std UTF-8 validation, one mint per
//! transaction against the stored (fingerprint, slot), supply cap, reward from an independent curve
//! reference); memo-home's share cap (its ShareTooHigh and memo-mint's OneMintPerTransaction are
//! both Custom(6008) at a mine instruction: only the failing program and the Anchor name tell them
//! apart). Account flags are resolved at the message level (a key is a signer or writable in every
//! instruction if it is in one; write locks on reserved keys and invoked program ids are demoted),
//! checked against the runtime's SanitizedMessage. On success it predicts the exact MEMO deltas
//! (reward, memo-home share), the supply, MintState = (fingerprint per the harness oracle, slot)
//! and memo-mint's log line. Compute: the Memo program's cost comes from a simulation of the memo
//! instruction alone (an oracle for a program that is not under test); every other program has
//! [low, high] bounds; a limit is only chosen where the outcome is certain, and the meter
//! ("consumed N of M"), the default limit and the bounds are checked against the logs. After every
//! step the chain is compared with the model: the supply, every MEMO token account in play,
//! MintState, and accounts that must never change (the second mint and its token accounts, the
//! MintState impostors, memo-burn's global stats, the unfunded PDAs). A mismatch is printed in full
//! (every instruction with its accounts and data, the logs) and the model adopts the chain's state.
//!
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin fuzz-memo-mint -- --seed 1 --steps 500 [--subs <p>] [--stop-on-mismatch] [--verbose]
//! --subs sets the probability that a memo-mint instruction carries substitutions (default 0.22).
//! The validator must carry the shadow MintState that prepare-local-validator plants
//! (scripts/local-validator.sh). Exits with status 1 if any step mismatched.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use borsh::BorshSerialize;
use harness::*;
use memo_token_client::get_program_id;
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use serde_json::json;
#[allow(deprecated)]
use solana_sdk::reserved_account_keys::ReservedAccountKeys;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{
        v0::LoadedAddresses, AddressLookupTableAccount, SanitizedMessage, SanitizedVersionedMessage, SimpleAddressLoader,
        VersionedMessage,
    },
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, VersionedTransaction},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id as ata_of, instruction::create_associated_token_account_idempotent,
};

const MEMO_V2: Pubkey = spl_memo::ID;
const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const ATA_PROGRAM: Pubkey = spl_associated_token_account::ID;
const MAX_TX_BYTES: usize = 1232;
const MEMO_MIN_CHARS: usize = 69;
const MAX_SHARE_BPS: u16 = 3000;

// memo-mint error codes
const MEMO_REQUIRED: u32 = 6000;
const MEMO_TOO_SHORT: u32 = 6001;
const INVALID_TOKEN_ACCOUNT: u32 = 6002;
const UNAUTHORIZED_MINT: u32 = 6003;
const UNAUTHORIZED_TOKEN_ACCOUNT: u32 = 6004;
const SUPPLY_LIMIT_REACHED: u32 = 6005;
const ONE_MINT_PER_TRANSACTION: u32 = 6008;
// memo-home: share_bps above the Collection's max_share_bps (same number as memo-mint's 6008)
const HOME_SHARE_TOO_HIGH: u32 = 6008;

/// Anchor names of the codes memo-mint raises here (framework and its own)
fn mint_error_name(code: u32) -> &'static str {
    match code {
        101 => "InstructionFallbackNotFound",
        102 => "InstructionDidNotDeserialize",
        2000 => "ConstraintMut",
        2006 => "ConstraintSeeds",
        2012 => "ConstraintAddress",
        3001 => "AccountDiscriminatorNotFound",
        3002 => "AccountDiscriminatorMismatch",
        3005 => "AccountNotEnoughKeys",
        3007 => "AccountOwnedByWrongProgram",
        3008 => "InvalidProgramId",
        3010 => "AccountNotSigner",
        3012 => "AccountNotInitialized",
        MEMO_REQUIRED => "MemoRequired",
        MEMO_TOO_SHORT => "MemoTooShort",
        INVALID_TOKEN_ACCOUNT => "InvalidTokenAccount",
        UNAUTHORIZED_MINT => "UnauthorizedMint",
        UNAUTHORIZED_TOKEN_ACCOUNT => "UnauthorizedTokenAccount",
        SUPPLY_LIMIT_REACHED => "SupplyLimitReached",
        ONE_MINT_PER_TRANSACTION => "OneMintPerTransaction",
        _ => "?",
    }
}

// ---------------------------------------------------------------------------------------------
// Compute units
// ---------------------------------------------------------------------------------------------

/// Runtime rules (agave 2.2): an explicit SetComputeUnitLimit is capped at 1.4M; without one the
/// limit is 200,000 per instruction of an SBF program plus 3,000 per builtin instruction (compute
/// budget, System), capped at 1.4M. A builtin instruction consumes 150 CU.
const MAX_CU: u64 = 1_400_000;
const DEFAULT_SBF_CU: u64 = 200_000;
const DEFAULT_BUILTIN_CU: u64 = 3_000;
const BUILTIN_CU: u64 = 150;
/// Bounds (CU) for the programs other than the Memo program, from a calibration run (see the
/// stats at the end of each run); every executed instruction's consumption is checked against
/// them. memo-mint's high bound grows with the memo it counts and with the instructions sysvar.
/// The low bounds follow the final Revision 5 (constant PDAs, no owner in the log; calibration
/// run of November 1, 2026: memo-mint from 10,026, mint_for_blog from 38,194, mint_for_post from
/// 38,747, mine from 50,037; about 5,600 below the audited Revision 5 each).
const MM_LO: u64 = 9_000;
const MM_HI_BASE: u64 = 20_000;
const FAIL_LO: u64 = 1_000;
const APP_LO: u64 = 30_000;
const APP_HI: u64 = 70_000;
const MINE_LO: u64 = 32_000;
const MINE_HI: u64 = 90_000;
const INIT_HI: u64 = 60_000;
const INF: u64 = 1_000_000_000_000;

// ---------------------------------------------------------------------------------------------
// Independent references
// ---------------------------------------------------------------------------------------------

/// Emission curve, written from the documented step table (a scan over the steps, no code
/// shared with the program or with harness::curve_reference): up to 10M tokens every mint pays 1
/// token; supply in (d x 10^k, (d+1) x 10^k] of decade n (10^k = 10M x 10^n) pays
/// (1 token / 10^n) x (10 - d) / 10. None at or above the 10T cap or when the reward would
/// overshoot it. Units in and out.
fn reward(supply: u64) -> Option<u64> {
    const START: u64 = 10_000_000 * UNIT;
    if supply >= MAX_SUPPLY {
        return None;
    }
    if supply <= START {
        return Some(UNIT);
    }
    let mut p = START;
    let mut base = UNIT;
    for _ in 0..6 {
        for d in 1..=9u64 {
            if supply > d * p && supply <= (d + 1) * p {
                let r = base * (10 - d) / 10;
                return if supply + r > MAX_SUPPLY { None } else { Some(r) };
            }
        }
        p *= 10;
        base /= 10;
    }
    unreachable!("supply {} below the cap but above every step", supply)
}

/// The two references must agree (both are independent of the program); boundaries and random
/// supplies across every decade
fn check_curve(rng: &mut StdRng) {
    let mut probes: Vec<u64> = vec![0, 1, MAX_SUPPLY - 1, MAX_SUPPLY, MAX_SUPPLY + 1, u64::MAX];
    let mut p = 10_000_000 * UNIT;
    for _ in 0..7 {
        for d in 1..=10u64 {
            let b = d.saturating_mul(p).min(MAX_SUPPLY);
            probes.extend([b - 1, b, b + 1]);
        }
        p = p.saturating_mul(10).min(MAX_SUPPLY);
    }
    for _ in 0..200_000 {
        probes.push(rng.gen_range(0..=MAX_SUPPLY + 10));
    }
    for s in probes {
        assert_eq!(reward(s), curve_reference(s), "curve references disagree at supply {}", s);
    }
}

// ---------------------------------------------------------------------------------------------
// Memo text generators
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Pal { Ascii, Esc, Ctl, Latin1, Latin1Ctl, TwoByte, Combining, Cjk, ZeroWidth, ThreeOther, Plane2, Emoji, FourOther }

const ALL_PALS: [Pal; 13] = [
    Pal::Ascii, Pal::Esc, Pal::Ctl, Pal::Latin1, Pal::Latin1Ctl, Pal::TwoByte, Pal::Combining, Pal::Cjk, Pal::ZeroWidth,
    Pal::ThreeOther, Pal::Plane2, Pal::Emoji, Pal::FourOther,
];

/// Memo styles: (name, palettes a character is drawn from, weight)
const STYLES: [(&str, &[Pal], u32); 16] = [
    ("ascii", &[Pal::Ascii], 25),
    ("ascii+esc", &[Pal::Ascii, Pal::Esc], 8),
    ("ascii+ctl", &[Pal::Ascii, Pal::Ctl], 4),
    ("ascii+latin1", &[Pal::Ascii, Pal::Latin1], 10),
    ("latin1", &[Pal::Latin1], 6),
    ("2-byte", &[Pal::TwoByte, Pal::Latin1Ctl, Pal::Latin1], 5),
    ("combining", &[Pal::Combining], 6),
    ("cjk", &[Pal::Cjk], 4),
    ("cjk+ascii", &[Pal::Cjk, Pal::Ascii, Pal::Ascii], 5),
    ("zero-width+ascii", &[Pal::ZeroWidth, Pal::Ascii, Pal::Ascii], 3),
    ("plane2", &[Pal::Plane2], 6),
    ("plane2+ascii", &[Pal::Plane2, Pal::Ascii], 4),
    ("emoji", &[Pal::Emoji], 3),
    ("emoji+ascii", &[Pal::Emoji, Pal::Ascii, Pal::Ascii], 3),
    ("4-byte", &[Pal::FourOther], 3),
    ("mixed", &ALL_PALS, 5),
];

fn pal_char(rng: &mut StdRng, p: Pal) -> char {
    let cp: u32 = match p {
        Pal::Ascii => loop {
            let c = rng.gen_range(0x20..0x7F);
            if !matches!(c, 0x22 | 0x27 | 0x5C) {
                break c;
            }
        },
        Pal::Esc => *[0x09, 0x0A, 0x0D, 0x22, 0x27, 0x5C].choose(rng).unwrap(),
        Pal::Ctl => *[0x00, 0x01, 0x07, 0x08, 0x1B, 0x1F, 0x7F].choose(rng).unwrap(),
        Pal::Latin1 => loop {
            let c = rng.gen_range(0xA1..=0xFF);
            if c != 0xAD {
                break c;
            }
        },
        Pal::Latin1Ctl => *[0x80, 0x85, 0x9F, 0xA0, 0xAD].choose(rng).unwrap(),
        Pal::TwoByte => {
            let opts = [rng.gen_range(0x391..=0x3A9), rng.gen_range(0x410..=0x44F), rng.gen_range(0x5D0..=0x5EA),
                        rng.gen_range(0x627..=0x64A), rng.gen_range(0x100..=0x17F), 0x7FF];
            *opts.choose(rng).unwrap()
        }
        Pal::Combining => if rng.gen_bool(0.5) { rng.gen_range(0x61..=0x7A) } else { rng.gen_range(0x300..=0x36F) },
        Pal::Cjk => {
            let opts = [rng.gen_range(0x4E00..=0x9FFF), rng.gen_range(0xAC00..=0xD7A3), rng.gen_range(0x3041..=0x3096)];
            *opts.choose(rng).unwrap()
        }
        Pal::ZeroWidth => *[0x200B, 0x200C, 0x200D, 0x2060, 0xFEFF, 0x202E, 0x2028].choose(rng).unwrap(),
        Pal::ThreeOther => *[0x800, 0x20AC, 0x2603, 0xE000, 0xFFFD, 0xFFFE, 0xFFFF].choose(rng).unwrap(),
        Pal::Plane2 => rng.gen_range(0x20000..=0x2A6DF),
        Pal::Emoji => {
            let opts = [rng.gen_range(0x1F300..=0x1F5FF), rng.gen_range(0x1F600..=0x1F64F), rng.gen_range(0x1F900..=0x1F9FF)];
            *opts.choose(rng).unwrap()
        }
        Pal::FourOther => {
            let opts = [0x10000, 0x1D11E, 0xE0001, 0xE0100, 0xF0000, 0x10FFFD, rng.gen_range(0x10000..=0x1007F)];
            *opts.choose(rng).unwrap()
        }
    };
    char::from_u32(cp).unwrap()
}

/// Length target of a generated memo
#[derive(Clone, Copy, Debug)]
enum Target {
    Chars(usize),
    Bytes(usize),
}

/// Invalid UTF-8 sequences: a lone continuation byte, 0xFF, an overlong NUL, an encoded
/// surrogate, a truncated 4-byte sequence, a code point above U+10FFFF, a truncated 2-byte one
const INVALID: [&[u8]; 7] = [&[0x80], &[0xFF], &[0xC0, 0x80], &[0xED, 0xA0, 0x80], &[0xF0, 0x9F, 0x8C], &[0xF4, 0x90, 0x80, 0x80], &[0xC3]];

fn weighted<T: Copy>(rng: &mut StdRng, items: &[(T, u32)]) -> T {
    let total: u32 = items.iter().map(|x| x.1).sum();
    let mut r = rng.gen_range(0..total);
    for (v, w) in items {
        if r < *w {
            return *v;
        }
        r -= w;
    }
    items[0].0
}

/// Text of the style, starting with `prefix`, to exactly `target` characters or bytes (a byte
/// target is topped up with ASCII where the style's characters do not fit)
fn gen_text(rng: &mut StdRng, pals: &[Pal], target: Target, prefix: &str) -> String {
    let mut s = String::from(prefix);
    match target {
        Target::Chars(n) => {
            if s.chars().count() > n {
                s = s.chars().take(n).collect();
            }
            let mut count = s.chars().count();
            while count < n {
                let p = *pals.choose(rng).unwrap();
                s.push(pal_char(rng, p));
                count += 1;
            }
        }
        Target::Bytes(b) => {
            while s.len() > b {
                s.pop();
            }
            let mut misses = 0;
            while s.len() < b {
                let p = *pals.choose(rng).unwrap();
                let c = pal_char(rng, p);
                if s.len() + c.len_utf8() <= b {
                    s.push(c);
                } else {
                    misses += 1;
                    if misses > 6 {
                        s.push(char::from(rng.gen_range(b'a'..=b'z')));
                    }
                }
            }
        }
    }
    s
}

/// Fixed edge patterns: (description, bytes)
fn edge_memos(rng: &mut StdRng) -> Vec<(&'static str, Vec<u8>)> {
    let p2 = |rng: &mut StdRng| char::from_u32(rng.gen_range(0x20000..=0x2A6DF)).unwrap();
    let plane2 = |rng: &mut StdRng, n: usize| (0..n).map(|_| p2(rng)).collect::<String>();
    let ascii = |rng: &mut StdRng, n: usize| (0..n).map(|_| char::from(rng.gen_range(b'a'..=b'z'))).collect::<String>();
    let mut v = vec![
        ("'é' x 34 + 'R' (69 B, 35 ch)", format!("{}R", "é".repeat(34)).into_bytes()),
        ("68 plane-2 (272 B, 68 ch)", plane2(rng, 68).into_bytes()),
        ("68 plane-2 + 'x' (273 B, 69 ch)", format!("{}x", plane2(rng, 68)).into_bytes()),
        ("69 plane-2 (276 B)", plane2(rng, 69).into_bytes()),
        ("'e'+U+0301 x 34 (68 ch)", "e\u{301}".repeat(34).into_bytes()),
        ("'e'+U+0301 x 34 + 'e' (69 ch)", format!("{}e", "e\u{301}".repeat(34)).into_bytes()),
        ("69 NUL", vec![0u8; 69]),
        ("'é' + 68 NUL (70 B, 69 ch)", format!("é{}", "\0".repeat(68)).into_bytes()),
        ("46 ASCII + 12 'é' (70 B, 58 ch)", format!("{}{}", ascii(rng, 46), "é".repeat(12)).into_bytes()),
        ("12 'é' + 46 ASCII (70 B, 58 ch)", format!("{}{}", "é".repeat(12), ascii(rng, 46)).into_bytes()),
        ("69 ASCII", ascii(rng, 69).into_bytes()),
        ("68 ASCII", ascii(rng, 68).into_bytes()),
        ("23 '中' (69 B, 23 ch)", "中".repeat(23).into_bytes()),
        ("69 U+200B (207 B)", "\u{200B}".repeat(69).into_bytes()),
        ("275 ASCII + 'é' at the end (277 B)", format!("{}é", ascii(rng, 275)).into_bytes()),
        ("274 ASCII + 'é' (276 B)", format!("{}é", ascii(rng, 274)).into_bytes()),
        ("273 ASCII + 'é' (275 B)", format!("{}é", ascii(rng, 273)).into_bytes()),
        ("'é' x 137 (274 B, 137 ch)", "é".repeat(137).into_bytes()),
    ];
    let mut bad = vec![b'a'; 68];
    bad.push(0xC3);
    v.push(("68 'a' + 0xC3 (truncated, invalid)", bad));
    v.push(("276 x 0x80 (invalid)", vec![0x80u8; 276]));
    let mut bad2 = ascii(rng, 80).into_bytes();
    bad2.insert(40, 0xFF);
    v.push(("80 ASCII with 0xFF inside (invalid)", bad2));
    v
}

// ---------------------------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------------------------

/// What an account is, as far as memo-mint's checks are concerned
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// does not exist (System-owned, 0 lamports)
    Missing,
    /// System-owned, funded
    Wallet,
    MemoMint,
    /// a Token-2022 mint other than MEMO
    OtherMint,
    /// a Token-2022 token account
    Ta { mint: Pubkey, owner: Pubkey },
    /// memo-mint's MintState at its PDA
    MintStatePda,
    /// memo-mint-owned, MintState discriminator and size, not at the PDA
    Shadow,
    /// memo-mint-owned, 8+ bytes, another discriminator
    ZeroState,
    /// memo-mint-owned, shorter than a discriminator
    TinyState,
    /// any other existing account (programs, sysvars, other programs' state)
    Other,
}

#[derive(Clone, Debug, PartialEq)]
struct St {
    supply: u64,
    ms: ([u8; 32], u64),
    bal: BTreeMap<Pubkey, u64>,
}

/// Predicted (or observed) outcome of a transaction
#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Ok,
    /// InstructionError [ix, Custom(code)]: `name`, raised by `program` (innermost failing frame)
    Custom { ix: usize, code: u32, program: Pubkey, name: &'static str },
    /// InstructionError [ix, Custom(code)] raised by `program`, which logs no Anchor error name
    /// (System: Custom(0) is AccountAlreadyInUse)
    CustomBy { ix: usize, code: u32, program: Pubkey },
    /// InstructionError [ix, <err>], a non-custom error, raised by `program`
    Builtin { ix: usize, err: &'static str, program: Pubkey },
    /// the compute meter runs out in instruction `ix` (ProgramFailedToComplete in the SBF VM, or
    /// ComputationalBudgetExceeded in a syscall or a builtin)
    Budget { ix: usize },
    /// would depend on exactly where the meter runs out (never sent)
    Ambiguous { ix: usize },
    /// refused by the RPC (more than 1232 bytes)
    TooLarge,
}

impl Outcome {
    fn matches(&self, out: &Out) -> bool {
        match self {
            Outcome::Ok => out.ok(),
            Outcome::Custom { ix, code, program, name } => out.landed && out.custom() == Some((*ix as u64, *code))
                && out.failed_program() == Some(*program) && out.error_name().as_deref() == Some(*name),
            Outcome::CustomBy { ix, code, program } => out.landed && out.custom() == Some((*ix as u64, *code))
                && out.failed_program() == Some(*program),
            Outcome::Builtin { ix, err, program } => out.landed && out.err == Some(json!({"InstructionError": [ix, err]}))
                && out.failed_program() == Some(*program),
            Outcome::Budget { ix } => out.landed
                && (out.err == Some(json!({"InstructionError": [ix, "ProgramFailedToComplete"]}))
                    || out.err == Some(json!({"InstructionError": [ix, "ComputationalBudgetExceeded"]}))),
            Outcome::Ambiguous { .. } => false,
            Outcome::TooLarge => !out.landed && out.err_text().contains("too large"),
        }
    }
    fn label(&self, names: &HashMap<Pubkey, &'static str>) -> String {
        let pn = |p: &Pubkey| names.get(p).copied().unwrap_or("?");
        match self {
            Outcome::Ok => "ok".into(),
            Outcome::Custom { ix, code, program, name } => format!("[{}] {} {} ({})", ix, code, name, pn(program)),
            Outcome::CustomBy { ix, code, program } => format!("[{}] Custom({}) ({})", ix, code, pn(program)),
            Outcome::Builtin { ix, err, program } => format!("[{}] {} ({})", ix, err, pn(program)),
            Outcome::Budget { ix } => format!("[{}] compute budget exceeded", ix),
            Outcome::Ambiguous { ix } => format!("[{}] ambiguous", ix),
            Outcome::TooLarge => "too large (RPC)".into(),
        }
    }
    /// The class of the outcome, for the histogram (no instruction index)
    fn class(&self, names: &HashMap<Pubkey, &'static str>) -> String {
        let pn = |p: &Pubkey| names.get(p).copied().unwrap_or("?");
        match self {
            Outcome::Ok => "ok".into(),
            Outcome::Custom { code, program, name, .. } => format!("{} {} ({})", code, name, pn(program)),
            Outcome::CustomBy { code, program, .. } => format!("Custom({}) ({})", code, pn(program)),
            Outcome::Builtin { err, program, .. } => format!("{} ({})", err, pn(program)),
            Outcome::Budget { .. } => "compute budget exceeded".into(),
            Outcome::Ambiguous { .. } => "ambiguous".into(),
            Outcome::TooLarge => "too large (RPC)".into(),
        }
    }
}

fn observed(out: &Out) -> String {
    if !out.landed {
        return format!("not landed: {}", out.err_text());
    }
    if out.err.is_none() {
        return "ok".into();
    }
    format!("{} ({:?} by {:?})", out.err_text(), out.error_name(), out.failed_program().map(|p| p.to_string()))
}

/// What a top-level instruction is
#[derive(Clone, Copy, Debug, PartialEq)]
enum K {
    Memo,
    MemoV1,
    CuLimit,
    CuPrice,
    Transfer,
    /// memo-mint instruction (any data, any accounts)
    Mm,
    Blog,
    /// memo-forum mint_for_post by actor
    Forum(usize),
    /// memo-home mine: share_bps, share recipient token account
    Mine { share: u16, rta: Pubkey },
    /// memo-mint initialize_mint_state by a non-admin actor, while MintState exists
    Init,
}

#[derive(Clone)]
struct Gx {
    k: K,
    ix: Instruction,
    label: String,
}

#[derive(Clone)]
struct TxSpec {
    ixs: Vec<Gx>,
    payer: usize,
    v0: bool,
    cu: String,
}

/// Account flags at the message level
struct Flags {
    signer: HashSet<Pubkey>,
    writable: HashSet<Pubkey>,
}

/// A successful mint
#[derive(Clone, Debug)]
struct Minted {
    amount: u64,
    supply_before: u64,
}

struct Walk {
    out: Outcome,
    post: St,
    minted: Vec<Minted>,
    /// [low, high] CU of each instruction that runs (up to the failing one)
    costs: Vec<(u64, u64)>,
}

enum Wire {
    Legacy(Transaction),
    V0(VersionedTransaction),
}

impl Wire {
    fn size(&self) -> usize {
        match self {
            Wire::Legacy(t) => bincode::serialize(t).unwrap().len(),
            Wire::V0(t) => bincode::serialize(t).unwrap().len(),
        }
    }
}

/// The last single transaction sent (for resends)
struct Sent {
    spec: TxSpec,
    fp: [u8; 32],
    wire: Wire,
    slot: u64,
    sig: String,
    bh: Hash,
    landed: bool,
}

/// Top-level frame of the logs: program, "consumed N of M", success
struct Frame {
    program: Pubkey,
    consumed: Option<(u64, u64)>,
    success: Option<bool>,
}

fn top_frames(logs: &[String]) -> Vec<Frame> {
    let mut frames: Vec<Frame> = vec![];
    let mut depth = 0usize;
    for l in logs {
        let Some(rest) = l.strip_prefix("Program ") else { continue };
        let Some((id, tail)) = rest.split_once(' ') else { continue };
        if let Some(d) = tail.strip_prefix("invoke [") {
            depth = d.trim_end_matches(']').parse().unwrap_or(0);
            if depth == 1 {
                frames.push(Frame { program: id.parse().unwrap_or_default(), consumed: None, success: None });
            }
        } else if let Some(c) = tail.strip_prefix("consumed ") {
            if depth == 1 {
                let w: Vec<&str> = c.split(' ').collect();
                if let (Some(n), Some(m)) = (w.first().and_then(|x| x.parse().ok()), w.get(2).and_then(|x| x.parse().ok())) {
                    if let Some(f) = frames.last_mut() {
                        f.consumed = Some((n, m));
                    }
                }
            }
        } else if tail == "success" || tail.starts_with("failed") {
            if depth == 1 {
                if let Some(f) = frames.last_mut() {
                    f.success = Some(tail == "success");
                }
            }
            depth = depth.saturating_sub(1);
        }
    }
    frames
}

fn seed32(tag: &str) -> [u8; 32] {
    let mut s = [0u8; 32];
    let b = tag.as_bytes();
    s[..b.len().min(32)].copy_from_slice(&b[..b.len().min(32)]);
    s
}

fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}

fn short(k: &Pubkey) -> String {
    let s = k.to_string();
    s[..6].to_string()
}

fn esc(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(s) => format!("{:?}", s),
        Err(_) => format!("invalid UTF-8 hex {}", hex::encode(data)),
    }
}

#[derive(BorshSerialize)]
struct BlogCreationData { version: u8, category: String, operation: String, creator: String, name: String, description: String, image: String }
#[derive(BorshSerialize)]
struct BlogMintData { version: u8, category: String, operation: String, minter: String, message: String }
#[derive(BorshSerialize)]
struct PostCreationData { version: u8, category: String, operation: String, creator: String, post_id: u64, title: String, content: String, image: String }
#[derive(BorshSerialize)]
struct PostMintData { version: u8, category: String, operation: String, user: String, post_id: u64, message: String }

// ---------------------------------------------------------------------------------------------
// Fuzzer
// ---------------------------------------------------------------------------------------------

struct Fz<'a> {
    e: &'a Env,
    rng: StdRng,
    seed: u64,
    verbose: bool,
    sub_rate: f64,
    actors: Vec<Keypair>,
    app: Keypair,
    ata: Vec<Pubkey>,
    c_extra: Pubkey,
    fake_mint: Pubkey,
    fake_ta: Vec<Pubkey>,
    shadow: Pubkey,
    zero_state: Pubkey,
    tiny_state: Pubkey,
    other_pda: Pubkey,
    home: Pubkey,
    config: Pubkey,
    asset: Pubkey,
    vault: Pubkey,
    vault_ata: Pubkey,
    post_id: u64,
    alt: AddressLookupTableAccount,
    roles: HashMap<Pubkey, Role>,
    names: HashMap<Pubkey, &'static str>,
    reserved: HashSet<Pubkey>,
    st: St,
    frozen_keys: Vec<Pubkey>,
    frozen: Vec<Option<(Pubkey, u64, Vec<u8>)>>,
    memo_cost: HashMap<Vec<u8>, (u64, u64)>,
    last: Option<Sent>,
    step: usize,
    mismatches: usize,
    hist: BTreeMap<String, usize>,
    kinds: BTreeMap<String, usize>,
    cu_seen: BTreeMap<String, (u64, u64, usize)>,
    memo_classes: BTreeMap<String, usize>,
    subs: BTreeMap<String, usize>,
    log_truncated: usize,
    retries: usize,
}

impl<'a> Fz<'a> {
    fn setup(e: &'a Env, seed: u64, verbose: bool, sub_rate: f64) -> Fz<'a> {
        let mut rng = StdRng::seed_from_u64(seed);
        check_curve(&mut rng);
        println!("curve references agree (boundaries of every step and 200,000 random supplies)");
        let a = e.wallet.insecure_clone();
        let ap = a.pubkey();
        let b = e.new_user("B", 0, false);
        let c = e.new_user("C", 0, false);
        let d = e.new_user("D", 0, false);
        let app = e.new_user("memo-home app (Collection Authority)", 0, false);
        let actors = vec![a, b, c, d];
        let ata: Vec<Pubkey> = actors.iter().map(|k| e.ata(&k.pubkey())).collect();

        // C's second MEMO token account, not an ATA (same size as an ATA of the MEMO mint)
        let c_extra_kp = Keypair::new();
        let c_extra = c_extra_kp.pubkey();
        let ata_len = e.data(&ata[2]).expect("C's ATA").len();
        let rent = e.c.get_minimum_balance_for_rent_exemption(ata_len).unwrap();
        e.must("create C's second MEMO token account", &[
            sys_ix::create_account(&ap, &c_extra, rent, ata_len as u64, &t22()),
            spl_token_2022::instruction::initialize_account3(&t22(), &c_extra, &e.mint, &actors[2].pubkey()).unwrap(),
        ], &actors[0], &[&c_extra_kp]);

        // a second mint whose mint authority IS the memo-mint PDA, and A's and B's token accounts of it
        let fake = e.new_mint(&e.mint_auth).pubkey();
        let fake_ta: Vec<Pubkey> = [0usize, 1].iter().map(|&i| ata_of(&actors[i].pubkey(), &fake, &t22())).collect();
        e.must("create A's and B's token accounts of the second mint",
            &[e.create_ata_ix(&actors[0].pubkey(), &fake), e.create_ata_ix(&actors[1].pubkey(), &fake)], &actors[0], &[]);

        // memo-mint-owned impostors: zeroed with MintState's size, and 4 bytes
        let zero_kp = Keypair::new();
        let tiny_kp = Keypair::new();
        let r113 = e.c.get_minimum_balance_for_rent_exemption(113).unwrap();
        let r4 = e.c.get_minimum_balance_for_rent_exemption(4).unwrap();
        e.must("create memo-mint-owned impostor accounts", &[
            sys_ix::create_account(&ap, &zero_kp.pubkey(), r113, 113, &e.mint_prog),
            sys_ix::create_account(&ap, &tiny_kp.pubkey(), r4, 4, &e.mint_prog),
        ], &actors[0], &[&zero_kp, &tiny_kp]);

        let tag = format!("fz{}-{}", seed, rng.gen_range(0..1_000_000u32));
        #[allow(deprecated)]
        let reserved = ReservedAccountKeys::new_all_activated().active;
        let mut f = Fz {
            e, rng, seed, verbose, sub_rate,
            actors, app, ata, c_extra, fake_mint: fake, fake_ta,
            shadow: shadow_mint_state(), zero_state: zero_kp.pubkey(), tiny_state: tiny_kp.pubkey(),
            other_pda: Pubkey::find_program_address(&[b"mint_authority"], &e.burn_prog).0,
            home: get_program_id("memo_home").expect("memo_home program id"),
            config: Pubkey::default(), asset: Pubkey::default(), vault: Pubkey::default(), vault_ata: Pubkey::default(),
            post_id: 0,
            alt: AddressLookupTableAccount { key: Pubkey::default(), addresses: vec![] },
            roles: HashMap::new(), names: HashMap::new(),
            reserved,
            st: St { supply: 0, ms: ([0; 32], 0), bal: BTreeMap::new() },
            frozen_keys: vec![], frozen: vec![],
            memo_cost: HashMap::new(), last: None, step: 0, mismatches: 0,
            hist: BTreeMap::new(), kinds: BTreeMap::new(), cu_seen: BTreeMap::new(), memo_classes: BTreeMap::new(), subs: BTreeMap::new(),
            log_truncated: 0, retries: 0,
        };
        f.setup_apps(&tag);

        // address lookup table with every non-signer account the mint instructions use
        let mut alt_keys = vec![e.mint, e.mint_auth, t22(), IXS_SYSVAR, e.mint_state, solana_sdk::sysvar::clock::ID,
            f.fake_mint, f.c_extra, f.vault_ata, f.shadow, f.zero_state, f.tiny_state, e.gbs, f.other_pda, SPL_TOKEN_LEGACY];
        alt_keys.extend(f.ata.iter().copied());
        alt_keys.extend(f.fake_ta.iter().copied());
        f.alt = e.create_alt(&alt_keys);

        f.classify();
        f.names = HashMap::from([
            (MEMO_V2, "Memo"), (LEGACY_MEMO_V1, "Memo v1"), (e.mint_prog, "memo-mint"), (e.blog_prog, "memo-blog"),
            (e.forum_prog, "memo-forum"), (f.home, "memo-home"), (t22(), "Token-2022"),
            (solana_sdk::compute_budget::ID, "ComputeBudget"), (system_program(), "System"),
        ]);
        let (st, frozen) = f.read();
        f.st = st;
        f.frozen = frozen;
        println!("supply {} units, reward {:?}, MintState slot {}", f.st.supply, reward(f.st.supply), f.st.ms.1);
        println!("actors A {} B {} C {} D {}; C's second token account {}; app {}", f.actors[0].pubkey(), f.actors[1].pubkey(),
            f.actors[2].pubkey(), f.actors[3].pubkey(), f.c_extra, f.app.pubkey());
        println!("memo-home Asset {} Vault {} Vault ATA {}; forum post {}; lookup table {}", f.asset, f.vault, f.vault_ata, f.post_id, f.alt.key);
        f
    }

    /// memo-blog blog of A, memo-forum post by A, memo-home Collection (Authority = app, max share
    /// 30%) with an Asset owned by B and the Vault's MEMO token account
    fn setup_apps(&mut self, tag: &str) {
        let e = self.e;
        let a = &self.actors[0];
        let ap = a.pubkey();
        let blog = Pubkey::find_program_address(&[b"blog", ap.as_ref()], &e.blog_prog).0;
        if e.data(&blog).is_none() {
            let amount = 2 * curve_reference(e.supply()).unwrap_or(1);
            let p = BlogCreationData { version: 1, category: "blog".into(), operation: "create_blog".into(), creator: ap.to_string(),
                name: "fuzz blog".into(), description: "fuzz-memo-mint blog".into(), image: "".into() };
            let mut d = disc("create_blog");
            d.extend(amount.to_le_bytes());
            let ix = Instruction::new_with_bytes(e.blog_prog, &d, vec![
                AccountMeta::new(ap, true), AccountMeta::new(blog, false), AccountMeta::new(e.mint, false),
                AccountMeta::new(e.ata(&ap), false), AccountMeta::new(e.stats_pda(&ap), false), AccountMeta::new_readonly(t22(), false),
                AccountMeta::new_readonly(e.burn_prog, false), AccountMeta::new_readonly(system_program(), false),
                AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.gbs, false)]);
            e.must("create A's blog", &[app_burn_memo(amount, &p.try_to_vec().unwrap()), ix, cu(600_000)], a, &[]);
        }
        let counter = Pubkey::find_program_address(&[b"global_counter"], &e.forum_prog).0;
        let id = u64::from_le_bytes(e.data(&counter).expect("forum global counter")[8..16].try_into().unwrap());
        let amount = 2 * curve_reference(e.supply()).unwrap_or(1);
        let p = PostCreationData { version: 1, category: "forum".into(), operation: "create_post".into(), creator: ap.to_string(), post_id: id,
            title: "fuzz post".into(), content: "fuzz-memo-mint post".into(), image: "".into() };
        let mut d = disc("create_post");
        d.extend(id.to_le_bytes());
        d.extend(amount.to_le_bytes());
        let post = Pubkey::find_program_address(&[b"post", id.to_le_bytes().as_ref()], &e.forum_prog).0;
        let ix = Instruction::new_with_bytes(e.forum_prog, &d, vec![
            AccountMeta::new(ap, true), AccountMeta::new(counter, false), AccountMeta::new(post, false),
            AccountMeta::new(e.mint, false), AccountMeta::new(e.ata(&ap), false), AccountMeta::new(e.stats_pda(&ap), false),
            AccountMeta::new_readonly(t22(), false), AccountMeta::new_readonly(e.burn_prog, false), AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.gbs, false)]);
        e.must("create a forum post", &[app_burn_memo(amount, &p.try_to_vec().unwrap()), ix, cu(600_000)], a, &[]);
        self.post_id = id;

        // memo-home
        let h = self.home;
        let program_authority = Pubkey::find_program_address(&[b"authority"], &h).0;
        let burn_guard = Pubkey::find_program_address(&[b"burn_guard"], &h).0;
        if e.data(&burn_guard).is_none() {
            e.must("init memo-home's Burn Guard", &[Instruction::new_with_bytes(h, &disc("init_burn_guard"), vec![
                AccountMeta::new(ap, true), AccountMeta::new(burn_guard, false), AccountMeta::new_readonly(system_program(), false)])], a, &[]);
        }
        let app = self.app.pubkey();
        let cseed = seed32(&format!("{}-col", tag));
        let col = Pubkey::find_program_address(&[b"collection", app.as_ref(), &cseed], &h).0;
        let config = Pubkey::find_program_address(&[b"collection_config", col.as_ref()], &h).0;
        let mut args = cseed.to_vec();
        bstr(&mut args, "FUZZ Rooms");
        bstr(&mut args, "https://example.com/collection.json");
        args.extend(MAX_SHARE_BPS.to_le_bytes());
        args.push(0); // max_assets: None
        args.push(0); // royalty: None
        let mut d = disc("create_collection");
        d.extend(args);
        e.must("create a memo-home Collection (max share 30%)", &[Instruction::new_with_bytes(h, &d, vec![
            AccountMeta::new(ap, true), AccountMeta::new_readonly(app, true), AccountMeta::new(col, false),
            AccountMeta::new_readonly(program_authority, false), AccountMeta::new_readonly(burn_guard, false),
            AccountMeta::new(config, false), AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program(), false)]),
            cu(400_000)], a, &[&self.app]);
        let aseed = seed32(&format!("{}-asset", tag));
        let asset = Pubkey::find_program_address(&[b"asset", col.as_ref(), &aseed], &h).0;
        let vault_state = Pubkey::find_program_address(&[b"vault_state", asset.as_ref()], &h).0;
        let mut d = disc("create_asset");
        d.extend(aseed);
        bstr(&mut d, "Room #1");
        bstr(&mut d, "https://example.com/asset.json");
        e.must("create a memo-home Asset owned by B", &[Instruction::new_with_bytes(h, &d, vec![
            AccountMeta::new(ap, true), AccountMeta::new_readonly(app, true), AccountMeta::new_readonly(config, false),
            AccountMeta::new(col, false), AccountMeta::new(asset, false), AccountMeta::new(vault_state, false),
            AccountMeta::new_readonly(self.actors[1].pubkey(), false), AccountMeta::new_readonly(program_authority, false),
            AccountMeta::new_readonly(CORE, false), AccountMeta::new_readonly(system_program(), false)]), cu(400_000)], a, &[&self.app]);
        let vault = Pubkey::find_program_address(&[b"vault", asset.as_ref()], &h).0;
        let vault_ata = e.ata(&vault);
        e.must("create the Vault's MEMO token account", &[create_associated_token_account_idempotent(&ap, &vault, &e.mint, &t22())], a, &[]);
        self.config = config;
        self.asset = asset;
        self.vault = vault;
        self.vault_ata = vault_ata;
    }

    /// Roles of every fixed account, from the chain; the model's assumptions about them are
    /// checked here once (a wrong assumption aborts the run)
    fn classify(&mut self) {
        let e = self.e;
        let mut keys = vec![e.mint, self.fake_mint, e.mint_auth, self.other_pda, t22(), SPL_TOKEN_LEGACY, IXS_SYSVAR,
            solana_sdk::sysvar::clock::ID, e.mint_state, self.shadow, self.zero_state, self.tiny_state, e.gbs, self.c_extra,
            self.vault, self.vault_ata, e.mint_prog, self.app.pubkey()];
        keys.extend(self.ata.iter().copied());
        keys.extend(self.fake_ta.iter().copied());
        keys.extend(self.actors.iter().map(|k| k.pubkey()));
        let accs = e.c.get_multiple_accounts_with_commitment(&keys, CommitmentConfig::confirmed()).expect("accounts").value;
        for (k, a) in keys.iter().zip(accs) {
            let role = match a {
                None => Role::Missing,
                Some(a) if a.owner == system_program() && a.lamports == 0 => Role::Missing,
                Some(a) if a.owner == system_program() => Role::Wallet,
                Some(a) if a.owner == t22() && !a.executable => {
                    if *k == e.mint {
                        Role::MemoMint
                    } else if a.data.len() == 82 || (a.data.len() > 165 && a.data[165] == 1) {
                        Role::OtherMint
                    } else {
                        Role::Ta { mint: Pubkey::new_from_array(a.data[0..32].try_into().unwrap()),
                                   owner: Pubkey::new_from_array(a.data[32..64].try_into().unwrap()) }
                    }
                }
                Some(a) if a.owner == e.mint_prog => {
                    if *k == e.mint_state {
                        Role::MintStatePda
                    } else if a.data.len() < 8 {
                        Role::TinyState
                    } else if a.data[..8] == account_disc("MintState")[..] && a.data.len() == 113 {
                        Role::Shadow
                    } else {
                        Role::ZeroState
                    }
                }
                Some(_) => Role::Other,
            };
            self.roles.insert(*k, role);
        }
        // the instructions sysvar is not stored; the runtime builds it (owner Sysvar1111...) for
        // each transaction
        self.roles.insert(IXS_SYSVAR, Role::Other);
        let expect = |k: &Pubkey, r: Role, what: &str| {
            if self.roles.get(k) != Some(&r) {
                eprintln!("SETUP FAILED: {} {} is {:?}, expected {:?}", what, k, self.roles.get(k), r);
                std::process::exit(2);
            }
        };
        expect(&e.mint, Role::MemoMint, "MEMO mint");
        expect(&self.fake_mint, Role::OtherMint, "second mint");
        expect(&e.mint_auth, Role::Missing, "memo-mint mint authority PDA");
        expect(&self.other_pda, Role::Missing, "memo-burn's mint_authority PDA");
        expect(&self.vault, Role::Missing, "memo-home Vault PDA");
        expect(&e.mint_state, Role::MintStatePda, "MintState");
        expect(&self.shadow, Role::Shadow, "shadow MintState (prepare-local-validator plants it)");
        expect(&self.zero_state, Role::ZeroState, "zeroed memo-mint account");
        expect(&self.tiny_state, Role::TinyState, "4-byte memo-mint account");
        expect(&e.gbs, Role::Other, "memo-burn GlobalBurnStats");
        expect(&t22(), Role::Other, "Token-2022");
        expect(&IXS_SYSVAR, Role::Other, "instructions sysvar");
        for (i, k) in self.actors.iter().enumerate() {
            expect(&k.pubkey(), Role::Wallet, "actor");
            expect(&self.ata[i], Role::Ta { mint: e.mint, owner: k.pubkey() }, "actor ATA");
        }
        expect(&self.c_extra, Role::Ta { mint: e.mint, owner: self.actors[2].pubkey() }, "C's second token account");
        expect(&self.vault_ata, Role::Ta { mint: e.mint, owner: self.vault }, "Vault ATA");
        for (i, k) in self.fake_ta.iter().enumerate() {
            expect(k, Role::Ta { mint: self.fake_mint, owner: self.actors[i].pubkey() }, "second-mint token account");
        }
        self.frozen_keys = vec![self.fake_mint, self.fake_ta[0], self.fake_ta[1], self.shadow, self.zero_state, self.tiny_state, e.gbs,
                                e.mint_auth, self.other_pda, self.vault];
    }

    fn role(&self, k: &Pubkey) -> Role { self.roles.get(k).copied().unwrap_or(Role::Missing) }

    fn watched(&self) -> Vec<Pubkey> {
        let mut v = self.ata.clone();
        v.push(self.c_extra);
        v.push(self.vault_ata);
        v
    }

    /// The chain's state: supply, MintState, watched balances; and the frozen accounts
    fn read(&self) -> (St, Vec<Option<(Pubkey, u64, Vec<u8>)>>) {
        let e = self.e;
        let watched = self.watched();
        let mut keys = vec![e.mint, e.mint_state];
        keys.extend(watched.iter().copied());
        keys.extend(self.frozen_keys.iter().copied());
        let accs = loop {
            match e.c.get_multiple_accounts_with_commitment(&keys, CommitmentConfig::confirmed()) {
                Ok(r) => break r.value,
                Err(err) => {
                    eprintln!("getMultipleAccounts: {} (retrying)", err);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        };
        let m = accs[0].as_ref().expect("MEMO mint");
        let s = accs[1].as_ref().expect("MintState");
        let mut bal = BTreeMap::new();
        for (i, k) in watched.iter().enumerate() {
            let b = accs[2 + i].as_ref().filter(|a| a.data.len() >= 72).map(|a| u64::from_le_bytes(a.data[64..72].try_into().unwrap()));
            bal.insert(*k, b.unwrap_or(u64::MAX));
        }
        let st = St {
            supply: u64::from_le_bytes(m.data[36..44].try_into().unwrap()),
            ms: (s.data[8..40].try_into().unwrap(), u64::from_le_bytes(s.data[40..48].try_into().unwrap())),
            bal,
        };
        let frozen = accs[2 + watched.len()..].iter().map(|a| a.as_ref().map(|a| (a.owner, a.lamports, a.data.clone()))).collect();
        (st, frozen)
    }

    // ---------------- instruction builders ----------------

    fn mint_metas(&self, user: &Pubkey, ta: &Pubkey) -> Vec<AccountMeta> { self.e.mint_accounts(user, ta) }

    fn blog_ix(&self) -> Gx {
        let e = self.e;
        let m = self.actors[0].pubkey();
        let blog = Pubkey::find_program_address(&[b"blog", m.as_ref()], &e.blog_prog).0;
        let ix = Instruction::new_with_bytes(e.blog_prog, &disc("mint_for_blog"), vec![
            AccountMeta::new(m, true), AccountMeta::new(blog, false), AccountMeta::new(e.mint, false),
            AccountMeta::new_readonly(e.mint_auth, false), AccountMeta::new(self.ata[0], false), AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(e.mint_prog, false), AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.mint_state, false)]);
        Gx { k: K::Blog, ix, label: "blog(A)".into() }
    }

    fn blog_memo(&mut self) -> Gx {
        let m = self.actors[0].pubkey();
        let n = self.e.nonce();
        let p = BlogMintData { version: 1, category: "blog".into(), operation: "mint_for_blog".into(), minter: m.to_string(),
            message: format!("fuzz {} {}", self.seed, n) };
        Gx { k: K::Memo, ix: app_burn_memo(0, &p.try_to_vec().unwrap()), label: "memo(blog BurnMemo)".into() }
    }

    fn forum_ix(&self, u: usize) -> Gx {
        let e = self.e;
        let user = self.actors[u].pubkey();
        let post = Pubkey::find_program_address(&[b"post", self.post_id.to_le_bytes().as_ref()], &e.forum_prog).0;
        let mut d = disc("mint_for_post");
        d.extend(self.post_id.to_le_bytes());
        let ix = Instruction::new_with_bytes(e.forum_prog, &d, vec![
            AccountMeta::new(user, true), AccountMeta::new(post, false), AccountMeta::new(e.mint, false),
            AccountMeta::new_readonly(e.mint_auth, false), AccountMeta::new(self.ata[u], false), AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(e.mint_prog, false), AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.mint_state, false)]);
        Gx { k: K::Forum(u), ix, label: format!("forum({})", actor_name(u)) }
    }

    fn forum_memo(&mut self, u: usize) -> Gx {
        let user = self.actors[u].pubkey();
        let n = self.e.nonce();
        let p = PostMintData { version: 1, category: "forum".into(), operation: "mint_for_post".into(), user: user.to_string(),
            post_id: self.post_id, message: format!("fuzz {} {}", self.seed, n) };
        Gx { k: K::Memo, ix: app_burn_memo(0, &p.try_to_vec().unwrap()), label: "memo(forum BurnMemo)".into() }
    }

    fn gen_mine(&mut self) -> Gx {
        let e = self.e;
        let caller = self.rng.gen_range(0..4);
        let r = self.rng.gen_range(0..4);
        let rta = if r == 2 && self.rng.gen_bool(0.3) { self.c_extra } else { self.ata[r] };
        let share: u16 = weighted(&mut self.rng, &[(0u8, 70), (1, 10), (2, 10), (3, 10)]) as u16;
        let share = match share {
            0 => self.rng.gen_range(0..=MAX_SHARE_BPS),
            1 => 0,
            2 => MAX_SHARE_BPS,
            _ => self.rng.gen_range(MAX_SHARE_BPS + 1..=10_000),
        };
        let cp = self.actors[caller].pubkey();
        let mut d = disc("mine");
        d.extend(share.to_le_bytes());
        let ix = Instruction::new_with_bytes(self.home, &d, vec![
            AccountMeta::new(cp, true),
            AccountMeta::new_readonly(self.app.pubkey(), true),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(self.asset, false),
            AccountMeta::new_readonly(self.vault, false),
            AccountMeta::new(self.vault_ata, false),
            AccountMeta::new(rta, false),
            AccountMeta::new(e.mint, false),
            AccountMeta::new_readonly(e.mint_auth, false),
            AccountMeta::new(e.mint_state, false),
            AccountMeta::new_readonly(IXS_SYSVAR, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(ATA_PROGRAM, false),
            AccountMeta::new_readonly(e.mint_prog, false),
            AccountMeta::new_readonly(system_program(), false),
        ]);
        Gx { k: K::Mine { share, rta }, ix, label: format!("mine(caller {} share {} -> {})", actor_name(caller), share, self.ta_name(&rta)) }
    }

    fn ta_name(&self, k: &Pubkey) -> String {
        if let Some(i) = self.ata.iter().position(|x| x == k) {
            return format!("{}.ata", actor_name(i));
        }
        if *k == self.c_extra {
            return "C.ta2".into();
        }
        if *k == self.vault_ata {
            return "vault.ata".into();
        }
        if let Some(i) = self.fake_ta.iter().position(|x| x == k) {
            return format!("{}.fake_ta", actor_name(i));
        }
        short(k)
    }

    /// A random pubkey (nonexistent), from the seeded generator
    fn random_key(&mut self) -> Pubkey { Pubkey::new_from_array(self.rng.gen()) }

    /// memo-mint process_mint (`to` false) or process_mint_to, sometimes with substitutions
    fn gen_mm(&mut self, to: bool) -> Gx {
        let e = self.e;
        let u = self.rng.gen_range(0..4);
        let r = if to { self.rng.gen_range(0..4) } else { u };
        let up = self.actors[u].pubkey();
        let mut recipient = self.actors[r].pubkey();
        let mut ta = if r == 2 && self.rng.gen_bool(0.25) { self.c_extra } else { self.ata[r] };
        let mut tags: Vec<String> = vec![];
        if to && self.rng.gen_bool(0.05) {
            // mint into memo-home's Vault directly
            recipient = self.vault;
            ta = self.vault_ata;
            tags.push("recipient=vault".into());
        }
        let mut metas = self.mint_metas(&up, &ta);
        let mut data = if to {
            let mut d = disc("process_mint_to");
            d.extend(recipient.as_ref());
            d
        } else {
            disc("process_mint")
        };
        let n_mut = if self.rng.gen_bool(self.sub_rate) { if self.rng.gen_bool(0.2) { 2 } else { 1 } } else { 0 };
        // "too few accounts" is applied after the other substitutions, which index the accounts
        let mut truncate: Option<usize> = None;
        for _ in 0..n_mut {
            let which = self.rng.gen_range(0..32);
            let rk = self.random_key();
            let other = self.rng.gen_range(0..4);
            let t = match which {
                0 => { metas[1] = AccountMeta::new(self.fake_mint, false); "mint=second mint" }
                1 => { metas[1] = AccountMeta::new(self.actors[other].pubkey(), false); "mint=wallet" }
                2 => { metas[1] = AccountMeta::new(rk, false); "mint=missing" }
                3 => { metas[1] = AccountMeta::new_readonly(metas[1].pubkey, false); "mint read-only" }
                4 => { metas[2] = AccountMeta::new_readonly(self.other_pda, false); "authority=memo-burn PDA" }
                5 => { metas[2] = AccountMeta::new(metas[2].pubkey, false); "authority writable" }
                6 => { metas[2] = AccountMeta::new_readonly(rk, false); "authority=random" }
                7 => { metas[3] = AccountMeta::new(self.ata[other], false); "ta=another ATA" }
                8 => { metas[3] = AccountMeta::new(self.fake_ta[other % 2], false); "ta=second-mint account" }
                9 => { metas[3] = AccountMeta::new(e.mint, false); "ta=MEMO mint" }
                10 => { metas[3] = AccountMeta::new_readonly(metas[3].pubkey, false); "ta read-only" }
                11 => { metas[3] = AccountMeta::new(self.vault_ata, false); "ta=vault ATA" }
                12 => { metas[4] = AccountMeta::new_readonly(SPL_TOKEN_LEGACY, false); "token program=SPL Token" }
                13 => { metas[4] = AccountMeta::new_readonly(e.mint_prog, false); "token program=memo-mint" }
                14 => { metas[5] = AccountMeta::new_readonly(rk, false); "ixs=random" }
                15 => { metas[5] = AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false); "ixs=Clock" }
                16 => { metas[5] = AccountMeta::new(IXS_SYSVAR, false); "ixs writable" }
                17 => { metas[6] = AccountMeta::new(rk, false); "ms=missing" }
                18 => { metas[6] = AccountMeta::new(self.zero_state, false); "ms=zeroed" }
                19 => { metas[6] = AccountMeta::new(self.tiny_state, false); "ms=4 bytes" }
                20 => { metas[6] = AccountMeta::new(e.gbs, false); "ms=GlobalBurnStats" }
                21 => { metas[6] = AccountMeta::new(self.shadow, false); "ms=shadow" }
                22 => { metas[6] = AccountMeta::new_readonly(metas[6].pubkey, false); "ms read-only" }
                23 => { metas[0] = AccountMeta::new(metas[0].pubkey, false); "user not signer" }
                24 => { metas[0] = AccountMeta::new_readonly(metas[0].pubkey, true); "user read-only" }
                25 => { truncate = Some(self.rng.gen_range(0..7)); "too few accounts" }
                26 => {
                    let n = self.rng.gen_range(1..=12);
                    for _ in 0..n {
                        let k = match self.rng.gen_range(0..5) {
                            0 => self.random_key(),
                            1 => solana_sdk::sysvar::clock::ID,
                            2 => self.ata[self.rng.gen_range(0..4)],
                            3 => e.mint_state,
                            _ => self.actors[self.rng.gen_range(0..4)].pubkey(),
                        };
                        metas.push(if self.rng.gen_bool(0.5) { AccountMeta::new(k, false) } else { AccountMeta::new_readonly(k, false) });
                    }
                    "extra accounts"
                }
                27 => { data[0] ^= 1; "bad discriminator" }
                28 => { let n = self.rng.gen_range(0..8); data.truncate(n); "data < 8 bytes" }
                29 => { if to { let n = 8 + self.rng.gen_range(0..32); data.truncate(n); "short recipient" } else { data = { let mut d = disc("process_mint_to"); d.truncate(8); d }; "mint_to without argument" } }
                30 => { let n = self.rng.gen_range(1..=16); for _ in 0..n { data.push(self.rng.gen()); } "trailing data" }
                _ => {
                    if to {
                        let w = self.actors[other].pubkey();
                        data.truncate(8);
                        data.extend(w.as_ref());
                        "recipient argument=other actor"
                    } else {
                        data = disc("process_mint_to");
                        data.extend(up.as_ref());
                        "as process_mint_to(user)"
                    }
                }
            };
            tags.push(t.into());
        }
        if let Some(n) = truncate {
            metas.truncate(n);
        }
        let name = if to { format!("mint_to({}->{} as {})", actor_name(u), self.ta_name(&ta), short(&recipient)) } else { format!("mint({}->{})", actor_name(u), self.ta_name(&ta)) };
        let label = if tags.is_empty() { name } else { format!("{}[{}]", name, tags.join(",")) };
        Gx { k: K::Mm, ix: Instruction::new_with_bytes(e.mint_prog, &data, metas), label }
    }

    /// Memo text for index 0 (`max_bytes`, when given, caps the byte target)
    fn gen_memo_data(&mut self, max_bytes: Option<usize>) -> (Vec<u8>, String) {
        if self.rng.gen_bool(0.04) {
            let edges = edge_memos(&mut self.rng);
            let (d, data) = edges.choose(&mut self.rng).unwrap().clone();
            return (data, format!("edge {}", d));
        }
        let (style, pals, _) = STYLES[weighted(&mut self.rng, &STYLES.iter().enumerate().map(|(i, s)| (i, s.2)).collect::<Vec<_>>())];
        let expensive = matches!(style, "cjk" | "emoji" | "zero-width+ascii" | "mixed" | "cjk+ascii" | "emoji+ascii");
        let kind = weighted(&mut self.rng, &[(0u8, 20), (8, 12), (1, 18), (2, 14), (3, 8), (4, 13), (5, if expensive { 3 } else { 10 }), (6, 2), (7, 1)]);
        let target = match kind {
            0 => Target::Chars(self.rng.gen_range(66..=72)),
            8 => Target::Chars(self.rng.gen_range(68..=70)),
            1 => Target::Bytes(self.rng.gen_range(66..=72)),
            2 => Target::Bytes(self.rng.gen_range(268..=280)),
            3 => Target::Chars(self.rng.gen_range(0..=65)),
            4 => Target::Chars(self.rng.gen_range(73..=200)),
            5 | 6 => match max_bytes {
                Some(m) if kind == 6 => Target::Bytes(m),
                Some(m) => Target::Bytes(self.rng.gen_range(m / 2..=m)),
                None => Target::Bytes(self.rng.gen_range(300..=600)),
            },
            7 => Target::Bytes(0),
            _ => unreachable!(),
        };
        let prefix = if self.rng.gen_bool(0.4) { format!("fz{}.{}:", self.seed, self.step) } else { String::new() };
        let (mut data, mut desc) = if self.rng.gen_bool(0.2) {
            // two segments: a run of one style (often ASCII), then another style up to the target
            // (e.g. an ASCII first half and a multi-byte second half)
            let (style1, pals1, _) = if self.rng.gen_bool(0.5) { STYLES[0] } else { *STYLES.choose(&mut self.rng).unwrap() };
            let f = self.rng.gen_range(0.2..0.8);
            let first = match target {
                Target::Chars(n) => Target::Chars((n as f64 * f) as usize),
                Target::Bytes(b) => Target::Bytes((b as f64 * f) as usize),
            };
            let head = gen_text(&mut self.rng, pals1, first, &prefix);
            (gen_text(&mut self.rng, pals, target, &head).into_bytes(), format!("{}|{} {:?}", style1, style, target))
        } else {
            (gen_text(&mut self.rng, pals, target, &prefix).into_bytes(), format!("{} {:?}", style, target))
        };
        if self.rng.gen_bool(0.04) && !data.is_empty() {
            let bad = *INVALID.choose(&mut self.rng).unwrap();
            let s = String::from_utf8(data.clone()).unwrap();
            let bounds: Vec<usize> = s.char_indices().map(|(i, _)| i).chain(std::iter::once(s.len())).collect();
            let at = *bounds.choose(&mut self.rng).unwrap();
            let _ = data.splice(at..at, bad.iter().copied());
            if let Some(m) = max_bytes {
                if data.len() > m {
                    // keep it within the size budget: drop bytes after the invalid sequence
                    let keep = data.len() - m;
                    let end = data.len();
                    if at + bad.len() + keep <= end {
                        data.drain(end - keep..);
                    }
                }
            }
            desc.push_str(" +invalid");
        }
        (data, desc)
    }

    /// Memo that is not at index 0 (short, sometimes invalid)
    fn gen_extra_memo(&mut self) -> Gx {
        let kind = self.rng.gen_range(0..100);
        let data: Vec<u8> = if kind < 55 {
            let n = self.rng.gen_range(1..=90);
            gen_text(&mut self.rng, &[Pal::Ascii], Target::Bytes(n), "x:")
                .into_bytes()
        } else if kind < 72 {
            let (_, pals, _) = *STYLES.choose(&mut self.rng).unwrap();
            let n = self.rng.gen_range(1..=40);
            gen_text(&mut self.rng, pals, Target::Chars(n), "").into_bytes()
        } else if kind < 77 {
            vec![]
        } else {
            let n = self.rng.gen_range(0..40);
            let mut d = gen_text(&mut self.rng, &[Pal::Ascii], Target::Bytes(n), "").into_bytes();
            let bad = *INVALID.choose(&mut self.rng).unwrap();
            let at = self.rng.gen_range(0..=d.len());
            let _ = d.splice(at..at, bad.iter().copied());
            d
        };
        let mut ix = memo_ix(&data);
        self.memo_accounts(&mut ix);
        let label = format!("memo+({}B{})", data.len(), if std::str::from_utf8(&data).is_err() { " invalid" } else { "" });
        Gx { k: K::Memo, ix, label }
    }

    /// Sometimes give a memo instruction accounts (signers, or keys that may not sign)
    fn memo_accounts(&mut self, ix: &mut Instruction) {
        let r = self.rng.gen_range(0..100);
        if r < 8 {
            for _ in 0..self.rng.gen_range(1..=2) {
                let a = self.actors[self.rng.gen_range(0..4)].pubkey();
                ix.accounts.push(AccountMeta::new_readonly(a, true));
            }
        } else if r < 10 {
            let k = self.random_key();
            ix.accounts.push(AccountMeta::new_readonly(k, false));
        } else if r < 12 {
            let a = self.actors[self.rng.gen_range(0..4)].pubkey();
            ix.accounts.push(AccountMeta::new_readonly(a, false));
        }
    }

    // ---------------- transaction generation ----------------

    /// A random transaction; None to draw again (the outcome would be ambiguous, or the
    /// transaction does not fit)
    fn gen_spec(&mut self, force_ample: bool) -> Option<TxSpec> {
        let payer = self.rng.gen_range(0..4);
        let n_mints = weighted(&mut self.rng, &[(0usize, 5), (1, 66), (2, 20), (3, 7), (4, 2)]);
        let mut mints: Vec<Gx> = vec![];
        let mut app: Option<K> = None;
        for _ in 0..n_mints {
            if !mints.is_empty() && self.rng.gen_bool(0.3) {
                let d = mints[self.rng.gen_range(0..mints.len())].clone();
                mints.push(d);
                continue;
            }
            let w = self.rng.gen_range(0..100);
            let g = if w < 38 {
                self.gen_mm(false)
            } else if w < 70 {
                self.gen_mm(true)
            } else if w < 78 && !matches!(app, Some(K::Forum(_))) {
                app = Some(K::Blog);
                self.blog_ix()
            } else if w < 86 && !matches!(app, Some(K::Blog)) {
                let u = match app { Some(K::Forum(u)) => u, _ => self.rng.gen_range(0..4) };
                app = Some(K::Forum(u));
                self.forum_ix(u)
            } else if w < 88 {
                let a = self.rng.gen_range(0..4);
                let ap = self.actors[a].pubkey();
                Gx { k: K::Init, label: format!("initialize_mint_state({})", actor_name(a)),
                     ix: Instruction::new_with_bytes(self.e.mint_prog, &disc("initialize_mint_state"), vec![
                         AccountMeta::new(ap, true), AccountMeta::new(self.e.mint_state, false), AccountMeta::new_readonly(system_program(), false)]) }
            } else {
                self.gen_mine()
            };
            mints.push(g);
        }
        let mut body = mints;
        if self.rng.gen_bool(0.10) {
            let g = self.gen_extra_memo();
            body.push(g);
        }
        if self.rng.gen_bool(0.04) {
            let g = self.gen_extra_memo();
            body.push(g);
        }
        let pp = self.actors[payer].pubkey();
        if self.rng.gen_bool(0.05) {
            body.push(Gx { k: K::Transfer, ix: sys_ix::transfer(&pp, &pp, 0), label: "transfer".into() });
        }
        let price = self.rng.gen_bool(0.08);
        if price {
            let p = self.rng.gen_range(0..=5_000);
            body.push(Gx { k: K::CuPrice, ix: ComputeBudgetInstruction::set_compute_unit_price(p), label: format!("price({})", p) });
        }
        // compute unit limit mode (the value is chosen once the memo costs are known); an app's
        // BurnMemo must stay at index 0
        let mode = if force_ample { "ample" } else {
            weighted(&mut self.rng, &[("default", 42), ("ample", 33), ("starve", 15), ("tiny", 3), ("over", 4), ("index0", 3)])
        };
        let mode = if mode == "index0" && app.is_some() { "ample" } else { mode };
        let explicit = mode != "default";
        body.shuffle(&mut self.rng);
        let cu_gx = Gx { k: K::CuLimit, ix: cu(0), label: "cu".into() };
        if explicit && mode != "index0" {
            let at = self.rng.gen_range(0..=body.len());
            body.insert(at, cu_gx.clone());
        }

        // memo plan
        let plan = if app.is_some() || force_ample && self.rng.gen_bool(0.7) {
            "normal"
        } else {
            weighted(&mut self.rng, &[("normal", 80), ("absent", 4), ("late", 5), ("index0-other", 8), ("mint-first", 3)])
        };
        let plan = if mode == "index0" { "index0-cu" } else { plan };
        let memo0 = match app {
            Some(K::Blog) => Some(self.blog_memo()),
            Some(K::Forum(u)) => Some(self.forum_memo(u)),
            _ if plan == "absent" => None,
            _ => {
                let mut ix = memo_ix(b"");
                self.memo_accounts(&mut ix);
                Some(Gx { k: K::Memo, ix, label: String::new() })
            }
        };
        let memo_is_app = app.is_some();
        let mut ixs: Vec<Gx> = vec![];
        let mut memo_at: Option<usize> = None;
        match plan {
            "normal" => {
                ixs.push(memo0.clone().unwrap());
                memo_at = Some(0);
                ixs.extend(body);
            }
            "absent" => ixs = body,
            "late" => {
                ixs = body;
                let at = if ixs.is_empty() { 0 } else { self.rng.gen_range(1..=ixs.len()) };
                ixs.insert(at, memo0.clone().unwrap());
                memo_at = Some(at);
            }
            "index0-other" | "index0-cu" => {
                let first = if plan == "index0-cu" {
                    cu_gx.clone()
                } else {
                    match self.rng.gen_range(0..3) {
                        0 => Gx { k: K::CuPrice, ix: ComputeBudgetInstruction::set_compute_unit_price(1), label: "price(1)".into() },
                        1 => Gx { k: K::Transfer, ix: sys_ix::transfer(&pp, &pp, 0), label: "transfer".into() },
                        _ => {
                            let n = self.rng.gen_range(69..=120);
                            let t = gen_text(&mut self.rng, &[Pal::Ascii], Target::Bytes(n), "v1:");
                            Gx { k: K::MemoV1, ix: Instruction::new_with_bytes(LEGACY_MEMO_V1, t.as_bytes(), vec![]), label: format!("memo-v1({}B)", t.len()) }
                        }
                    }
                };
                // one price instruction at most
                if first.k == K::CuPrice && price {
                    body.retain(|g| g.k != K::CuPrice);
                }
                ixs.push(first);
                let at = if body.is_empty() { 0 } else { self.rng.gen_range(0..=body.len()) };
                body.insert(at, memo0.clone().unwrap());
                memo_at = Some(1 + at);
                ixs.extend(body);
            }
            _ => {
                // mint-first: a memo-mint or memo-home instruction at index 0
                match body.iter().position(|g| matches!(g.k, K::Mm | K::Mine { .. })) {
                    Some(p) => {
                        let g = body.remove(p);
                        ixs.push(g);
                    }
                    None => ixs.push(self.gen_mm(false)),
                }
                let at = self.rng.gen_range(0..=body.len());
                body.insert(at, memo0.clone().unwrap());
                memo_at = Some(1 + at);
                ixs.extend(body);
            }
        }
        if ixs.is_empty() {
            return None;
        }
        let v0 = self.rng.gen_bool(0.25);
        let mut spec = TxSpec { ixs, payer, v0, cu: mode.into() };

        // memo content (needs the size of the rest of the transaction for long memos)
        let mut oversize = false;
        if let (Some(at), false) = (memo_at, memo_is_app) {
            let long = self.rng.gen_bool(0.12);
            let max = if long {
                spec.ixs[at].ix.data = vec![b'x'; 200];
                let size = self.dry_size(&spec);
                if size > MAX_TX_BYTES {
                    return None;
                }
                let mut m = MAX_TX_BYTES - (size - 200);
                if !force_ample && self.rng.gen_bool(0.06) {
                    m += self.rng.gen_range(1..=8);
                    oversize = true;
                }
                Some(m)
            } else {
                None
            };
            let (data, desc) = if oversize {
                let m = max.unwrap();
                (gen_text(&mut self.rng, &[Pal::Ascii], Target::Bytes(m), "").into_bytes(), format!("oversize {}B", m))
            } else {
                self.gen_memo_data(max)
            };
            let chars = std::str::from_utf8(&data).map(|s| s.chars().count().to_string()).unwrap_or_else(|_| "invalid".into());
            spec.ixs[at].ix.data = data;
            spec.ixs[at].label = format!("memo({}; {}ch/{}B{})", desc, chars, spec.ixs[at].ix.data.len(),
                if spec.ixs[at].ix.accounts.is_empty() { "" } else { " +accounts" });
        }
        let size = self.dry_size(&spec);
        if size > MAX_TX_BYTES && !oversize {
            return None;
        }
        if oversize {
            if size <= MAX_TX_BYTES {
                return None;
            }
            spec.cu = "oversize".into();
            return Some(spec);
        }

        // compute unit limit
        let fl = self.flags_of(&spec);
        self.prepare_costs(&spec, &fl);
        let fp = [0xA5u8; 32];
        let slot = self.st.ms.1 + 1;
        let cu_at = spec.ixs.iter().position(|g| g.k == K::CuLimit);
        let full = self.walk(&spec, &fl, fp, slot, u64::MAX, &self.st.clone());
        let total_hi: u64 = full.costs.iter().map(|c| c.1).fold(0, |a, b| a.saturating_add(b));
        let value: Option<u32> = match mode {
            "default" => None,
            "ample" | "index0" => Some(if total_hi >= MAX_CU { MAX_CU as u32 } else { self.rng.gen_range(total_hi..=MAX_CU) as u32 }),
            "over" => Some(self.rng.gen_range(MAX_CU as u32 + 1..=u32::MAX)),
            "tiny" => Some(self.rng.gen_range(0..2_000)),
            _ => {
                // starve: a limit that surely runs out in instruction j and surely lets every
                // earlier one complete
                let mut cands = vec![];
                let (mut lo, mut hi) = (0u64, 0u64);
                for (j, c) in full.costs.iter().enumerate() {
                    let start = hi;
                    lo = lo.saturating_add(c.0);
                    hi = hi.saturating_add(c.1);
                    if lo >= 1 && start < lo && start <= MAX_CU {
                        cands.push((j, start, (lo - 1).min(MAX_CU)));
                    }
                }
                let &(j, a, b) = cands.choose(&mut self.rng)?;
                spec.cu = format!("starve@{}", j);
                Some(self.rng.gen_range(a..=b) as u32)
            }
        };
        if let (Some(v), Some(at)) = (value, cu_at) {
            spec.ixs[at].ix = cu(v);
            spec.ixs[at].label = format!("cu({})", v);
        }
        let limit = limit_of(&spec);
        let w = self.walk(&spec, &fl, fp, slot, limit, &self.st.clone());
        if matches!(w.out, Outcome::Ambiguous { .. }) {
            return None;
        }
        Some(spec)
    }

    /// Serialized size of the transaction (signed with a dummy blockhash)
    fn dry_size(&self, spec: &TxSpec) -> usize { self.build(spec, Hash::default()).0.size() }

    fn signers(&self) -> Vec<&Keypair> { self.actors.iter().chain(std::iter::once(&self.app)).collect() }

    fn build(&self, spec: &TxSpec, bh: Hash) -> (Wire, VersionedMessage, LoadedAddresses) {
        let ixs: Vec<Instruction> = spec.ixs.iter().map(|g| g.ix.clone()).collect();
        let payer = &self.actors[spec.payer];
        let signers = self.signers();
        if spec.v0 {
            let (tx, loaded) = self.e.build_v0(&ixs, payer, &signers, &self.alt, bh);
            let msg = tx.message.clone();
            (Wire::V0(tx), msg, loaded)
        } else {
            let tx = self.e.build(&ixs, payer, &signers, bh);
            let msg = VersionedMessage::Legacy(tx.message.clone());
            (Wire::Legacy(tx), msg, LoadedAddresses::default())
        }
    }

    /// Message-level flags: a key signs if any instruction marks it signer (or it pays the fee);
    /// it is writable if any instruction marks it writable (or it pays), unless it is a reserved
    /// key or a program id the transaction invokes
    fn flags_of(&self, spec: &TxSpec) -> Flags {
        let payer = self.actors[spec.payer].pubkey();
        let invoked: HashSet<Pubkey> = spec.ixs.iter().map(|g| g.ix.program_id).collect();
        let mut signer = HashSet::from([payer]);
        let mut writable = HashSet::from([payer]);
        for g in &spec.ixs {
            for m in &g.ix.accounts {
                if m.is_signer {
                    signer.insert(m.pubkey);
                }
                if m.is_writable {
                    writable.insert(m.pubkey);
                }
            }
        }
        writable.retain(|k| !self.reserved.contains(k) && !invoked.contains(k));
        Flags { signer, writable }
    }

    /// The model's flags must be the runtime's (SanitizedMessage); a difference is a model bug
    fn check_flags(&self, fl: &Flags, msg: &VersionedMessage, loaded: &LoadedAddresses) -> Option<String> {
        let m = SanitizedVersionedMessage::try_new(msg.clone()).ok()
            .and_then(|m| SanitizedMessage::try_new(m, SimpleAddressLoader::Enabled(loaded.clone()), &self.reserved).ok())?;
        let mut bad = vec![];
        for (i, k) in m.account_keys().iter().enumerate() {
            if m.is_writable(i) != fl.writable.contains(k) || m.is_signer(i) != fl.signer.contains(k) {
                bad.push(format!("{} runtime w{} s{} model w{} s{}", k, m.is_writable(i), m.is_signer(i), fl.writable.contains(k), fl.signer.contains(k)));
            }
        }
        if bad.is_empty() { None } else { Some(bad.join("; ")) }
    }

    // ---------------- compute costs ----------------

    fn memo_key(ix: &Instruction) -> Vec<u8> {
        let mut k = ix.program_id.to_bytes().to_vec();
        for m in &ix.accounts {
            k.extend(m.pubkey.as_ref());
            k.push(m.is_signer as u8);
            k.push(m.is_writable as u8);
        }
        k.extend((ix.data.len() as u32).to_le_bytes());
        k.extend(&ix.data);
        k
    }

    /// The memo instruction as it runs: an account signs (is writable) if it signs (is writable)
    /// anywhere in the message. Both matter for the cost: a writable signer costs the Memo
    /// program 1 CU less than a read-only one.
    fn memo_as_run(ix: &Instruction, fl: &Flags) -> Instruction {
        let mut ix = ix.clone();
        for m in ix.accounts.iter_mut() {
            m.is_signer = fl.signer.contains(&m.pubkey);
            m.is_writable = fl.writable.contains(&m.pubkey);
        }
        ix
    }

    /// Simulates every memo instruction of the transaction that is not cached yet, alone (as it
    /// runs in the transaction), with the 1.4M limit: its consumption is its exact cost; (INF, INF)
    /// if it runs out. A memo near the size limit may leave no room for the SetComputeUnitLimit
    /// instruction (in a transaction that has none): it is then simulated alone at the default
    /// 200,000, so a cost above that is only known to be at least 200,000.
    fn prepare_costs(&mut self, spec: &TxSpec, fl: &Flags) {
        for g in &spec.ixs {
            if !matches!(g.k, K::Memo | K::MemoV1) {
                continue;
            }
            let run = Self::memo_as_run(&g.ix, fl);
            let key = Self::memo_key(&run);
            if self.memo_cost.contains_key(&key) {
                continue;
            }
            let payer = self.actors[spec.payer].pubkey();
            let mut ixs = vec![run, cu(MAX_CU as u32)];
            let fits = |ixs: &[Instruction]| {
                bincode::serialize(&Transaction::new_unsigned(solana_sdk::message::Message::new(ixs, Some(&payer)))).unwrap().len() <= MAX_TX_BYTES
            };
            if !fits(&ixs) {
                ixs.pop();
            }
            if !fits(&ixs) {
                // a memo that cannot be sent on its own (only in a transaction predicted too large)
                self.memo_cost.insert(key, (0, INF));
                continue;
            }
            let at_cap = ixs.len() == 2;
            let (out, _) = self.e.simulate_unsigned(&ixs, &payer, &[]);
            let frames = top_frames(&out.logs);
            let budget = out.err == Some(json!({"InstructionError": [0, "ProgramFailedToComplete"]}))
                || out.err == Some(json!({"InstructionError": [0, "ComputationalBudgetExceeded"]}));
            let c = match frames.first().and_then(|f| f.consumed) {
                _ if budget && at_cap => (INF, INF),
                _ if budget => (DEFAULT_SBF_CU, INF),
                Some((n, _)) => (n, n),
                None => (0, INF),
            };
            self.memo_cost.insert(key, c);
        }
    }

    fn cost(&self, g: &Gx, ok: bool, spec: &TxSpec, fl: &Flags) -> (u64, u64) {
        match g.k {
            K::CuLimit | K::CuPrice | K::Transfer => (BUILTIN_CU, BUILTIN_CU),
            K::Memo | K::MemoV1 => *self.memo_cost.get(&Self::memo_key(&Self::memo_as_run(&g.ix, fl))).expect("memo cost prepared"),
            K::Mm => (if ok { MM_LO } else { FAIL_LO }, mm_hi(spec, &g.ix)),
            K::Blog | K::Forum(_) => (if ok { APP_LO } else { FAIL_LO }, APP_HI),
            K::Mine { .. } => (if ok { MINE_LO } else { FAIL_LO }, MINE_HI),
            K::Init => (FAIL_LO, INIT_HI),
        }
    }

    // ---------------- the model ----------------

    /// Runs the transaction through the model, with the compute limit `limit`
    fn walk(&self, spec: &TxSpec, fl: &Flags, fp: [u8; 32], slot: u64, limit: u64, st: &St) -> Walk {
        let mut s = st.clone();
        let mut minted = vec![];
        let mut costs = vec![];
        let (mut lo, mut hi) = (0u64, 0u64);
        for (i, g) in spec.ixs.iter().enumerate() {
            let r = self.exec(i, g, spec, fl, fp, slot, &mut s, &mut minted);
            let c = self.cost(g, r.is_ok(), spec, fl);
            costs.push(c);
            lo = lo.saturating_add(c.0);
            hi = hi.saturating_add(c.1);
            if hi <= limit {
                if let Err(o) = r {
                    return Walk { out: o, post: st.clone(), minted: vec![], costs };
                }
                continue;
            }
            let out = if lo > limit { Outcome::Budget { ix: i } } else { Outcome::Ambiguous { ix: i } };
            return Walk { out, post: st.clone(), minted: vec![], costs };
        }
        Walk { out: Outcome::Ok, post: s, minted, costs }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec(&self, i: usize, g: &Gx, spec: &TxSpec, fl: &Flags, fp: [u8; 32], slot: u64, s: &mut St, minted: &mut Vec<Minted>) -> Result<(), Outcome> {
        match g.k {
            K::CuLimit | K::CuPrice | K::Transfer => Ok(()),
            K::Memo | K::MemoV1 => {
                let program = g.ix.program_id;
                // SPL Memo: every account must sign, then the data must be UTF-8 (Memo v1: UTF-8)
                if g.k == K::Memo && g.ix.accounts.iter().any(|m| !fl.signer.contains(&m.pubkey)) {
                    return Err(Outcome::Builtin { ix: i, err: "MissingRequiredSignature", program });
                }
                if std::str::from_utf8(&g.ix.data).is_err() {
                    return Err(Outcome::Builtin { ix: i, err: "InvalidInstructionData", program });
                }
                Ok(())
            }
            K::Mm => {
                let (ta, _) = self.mm_front(i, &g.ix, fl)?;
                minted.push(self.mm_handler(i, spec, fp, slot, s, ta)?);
                Ok(())
            }
            K::Blog => {
                minted.push(self.mm_handler(i, spec, fp, slot, s, self.ata[0])?);
                Ok(())
            }
            K::Forum(u) => {
                minted.push(self.mm_handler(i, spec, fp, slot, s, self.ata[u])?);
                Ok(())
            }
            K::Mine { share, rta } => {
                if share > MAX_SHARE_BPS {
                    return Err(Outcome::Custom { ix: i, code: HOME_SHARE_TOO_HIGH, program: self.home, name: "ShareTooHigh" });
                }
                let m = self.mm_handler(i, spec, fp, slot, s, self.vault_ata)?;
                let part = (m.amount as u128 * share as u128 / 10_000) as u64;
                *s.bal.get_mut(&self.vault_ata).unwrap() -= part;
                *s.bal.get_mut(&rta).unwrap() += part;
                minted.push(m);
                Ok(())
            }
            // Anchor's `init` of an account that already has lamports: System Allocate of an
            // account that is not System-owned fails with AccountAlreadyInUse, before the admin
            // constraint
            K::Init => Err(Outcome::CustomBy { ix: i, code: 0, program: system_program() }),
        }
    }

    fn mm_err(&self, i: usize, code: u32) -> Outcome {
        Outcome::Custom { ix: i, code, program: self.e.mint_prog, name: mint_error_name(code) }
    }

    /// memo-mint before its handler: dispatch, argument, accounts (deserialization of every
    /// field in order, then the constraints of every field in order). Returns the token account
    /// and its owner.
    fn mm_front(&self, i: usize, ix: &Instruction, fl: &Flags) -> Result<(Pubkey, Pubkey), Outcome> {
        let e = self.e;
        let d = &ix.data;
        let err = |c: u32| self.mm_err(i, c);
        let to = if d.len() >= 8 && d[..8] == disc("process_mint")[..] {
            None
        } else if d.len() >= 8 && d[..8] == disc("process_mint_to")[..] {
            if d.len() < 40 {
                return Err(err(102));
            }
            Some(Pubkey::new_from_array(d[8..40].try_into().unwrap()))
        } else {
            return Err(err(101));
        };
        let m = &ix.accounts;
        let key = |f: usize| m[f].pubkey;
        // deserialization: user Signer, mint InterfaceAccount<Mint>, mint_authority AccountInfo,
        // token_account InterfaceAccount<TokenAccount>, token_program Program<Token2022>,
        // instructions AccountInfo, mint_state Account<MintState>
        for f in 0..7 {
            if f >= m.len() {
                return Err(err(3005));
            }
            let k = key(f);
            let r = self.role(&k);
            match f {
                0 if !fl.signer.contains(&k) => return Err(err(3010)),
                1 => match r {
                    Role::Missing => return Err(err(3012)),
                    Role::MemoMint | Role::OtherMint => {}
                    Role::Ta { .. } => panic!("model: a token account as the mint is not generated"),
                    _ => return Err(err(3007)),
                },
                3 => match r {
                    Role::Missing => return Err(err(3012)),
                    Role::Ta { .. } => {}
                    Role::MemoMint | Role::OtherMint => {
                        return Err(Outcome::Builtin { ix: i, err: "UninitializedAccount", program: e.mint_prog })
                    }
                    _ => return Err(err(3007)),
                },
                4 if k != t22() => return Err(err(3008)),
                6 => match r {
                    Role::Missing => return Err(err(3012)),
                    Role::MintStatePda | Role::Shadow => {}
                    Role::TinyState => return Err(err(3001)),
                    Role::ZeroState => return Err(err(3002)),
                    _ => return Err(err(3007)),
                },
                _ => {}
            }
        }
        // constraints, field by field, in Anchor 0.32.1's order within a field (mut before address).
        // user / caller (key 0) may be read-only since Revision 5 (D5)
        if !fl.writable.contains(&key(1)) {
            return Err(err(2000));
        }
        if key(1) != e.mint {
            return Err(err(UNAUTHORIZED_MINT));
        }
        if key(2) != e.mint_auth {
            return Err(err(2006));
        }
        if !fl.writable.contains(&key(3)) {
            return Err(err(2000));
        }
        let Role::Ta { mint: ta_mint, owner: ta_owner } = self.role(&key(3)) else { unreachable!() };
        if ta_mint != key(1) {
            return Err(err(INVALID_TOKEN_ACCOUNT));
        }
        if ta_owner != to.unwrap_or(key(0)) {
            return Err(err(UNAUTHORIZED_TOKEN_ACCOUNT));
        }
        if key(5) != IXS_SYSVAR {
            return Err(err(2012));
        }
        // mint_state: `mut, address = MINT_STATE_PDA @ ConstraintSeeds` since Revision 5 (D4 / O1b)
        if !fl.writable.contains(&key(6)) {
            return Err(err(2000));
        }
        if key(6) != e.mint_state {
            return Err(err(2006));
        }
        Ok((key(3), ta_owner))
    }

    /// memo-mint's handler (top level or CPI at top-level index i)
    #[allow(clippy::too_many_arguments)]
    fn mm_handler(&self, i: usize, spec: &TxSpec, fp: [u8; 32], slot: u64, s: &mut St, ta: Pubkey) -> Result<Minted, Outcome> {
        if i == 0 {
            return Err(self.mm_err(i, MEMO_REQUIRED));
        }
        let ix0 = &spec.ixs[0].ix;
        if ix0.program_id != MEMO_V2 {
            return Err(self.mm_err(i, MEMO_REQUIRED));
        }
        let text = std::str::from_utf8(&ix0.data).expect("model: the memo at index 0 ran, so it is UTF-8");
        if text.chars().count() < MEMO_MIN_CHARS {
            return Err(self.mm_err(i, MEMO_TOO_SHORT));
        }
        if s.ms == (fp, slot) {
            return Err(self.mm_err(i, ONE_MINT_PER_TRANSACTION));
        }
        s.ms = (fp, slot);
        let Some(amount) = reward(s.supply) else { return Err(self.mm_err(i, SUPPLY_LIMIT_REACHED)) };
        let before = s.supply;
        s.supply += amount;
        *s.bal.get_mut(&ta).expect("model: watched token account") += amount;
        Ok(Minted { amount, supply_before: before })
    }

    // ---------------- steps ----------------

    fn run_step(&mut self) {
        self.step += 1;
        let r = self.rng.gen_range(0..100);
        let can_resend = self.last.as_ref().map(|l| l.landed).unwrap_or(false);
        if r < 2 && can_resend {
            self.resend_exact();
        } else if r < 6 && can_resend {
            self.resend_later();
        } else if r < 10 {
            self.pair();
        } else {
            self.single();
        }
    }

    fn gen_until(&mut self, force_ample: bool) -> TxSpec {
        loop {
            if let Some(s) = self.gen_spec(force_ample) {
                return s;
            }
            self.retries += 1;
        }
    }

    fn single(&mut self) {
        let spec = self.gen_until(false);
        let _ = self.send_and_check(spec, "single");
    }

    fn send_and_check(&mut self, spec: TxSpec, kind: &str) -> (Outcome, Out, [u8; 32]) {
        let bh = self.e.c.get_latest_blockhash().expect("blockhash");
        let (wire, msg, loaded) = self.build(&spec, bh);
        let size = wire.size();
        let fl = self.flags_of(&spec);
        if size <= MAX_TX_BYTES {
            self.prepare_costs(&spec, &fl);
        }
        let flag_err = self.check_flags(&fl, &msg, &loaded);
        let fp = fingerprint_versioned(&msg, &loaded);
        let out = self.send_wire(&wire);
        let (pred, post, minted, costs) = if size > MAX_TX_BYTES {
            (Outcome::TooLarge, self.st.clone(), vec![], vec![])
        } else {
            let w = self.walk(&spec, &fl, fp, out.slot, limit_of(&spec), &self.st.clone());
            (w.out, w.post, w.minted, w.costs)
        };
        let problems = self.compare(&spec, &out, &pred, &post, &minted, &costs, flag_err);
        self.report(kind, &spec, &out, &pred, size, problems, &post);
        let landed = out.landed;
        let slot = out.slot;
        self.last = Some(Sent { spec, fp, wire, slot, sig: out.sig.clone(), bh, landed });
        (pred, out, fp)
    }

    fn send_wire(&self, w: &Wire) -> Out {
        let r = match w {
            Wire::Legacy(t) => self.e.send(t),
            Wire::V0(t) => self.e.send(t),
        };
        match r {
            Ok(sig) => self.e.wait(&sig),
            Err(err) => Out { err: Some(json!(format!("RPC rejected: {}", err))), ..Default::default() },
        }
    }

    /// Checks one transaction against the model; returns the problems found
    #[allow(clippy::too_many_arguments)]
    fn compare(&mut self, spec: &TxSpec, out: &Out, pred: &Outcome, post: &St, minted: &[Minted], costs: &[(u64, u64)],
               flag_err: Option<String>) -> Vec<String> {
        let mut problems = vec![];
        if let Some(f) = flag_err {
            problems.push(format!("model flags differ from the runtime's: {}", f));
        }
        if !pred.matches(out) {
            problems.push(format!("outcome: predicted {}, got {}", pred.label(&self.names), observed(out)));
        }
        let (chain, frozen) = self.read();
        if chain != *post {
            problems.push(format!("state: {}", st_diff(post, &chain, self)));
        }
        if frozen != self.frozen {
            let changed: Vec<String> = self.frozen_keys.iter().zip(frozen.iter().zip(self.frozen.iter()))
                .filter(|(_, (a, b))| a != b).map(|(k, _)| k.to_string()).collect();
            problems.push(format!("accounts that must never change changed: {}", changed.join(", ")));
        }
        let truncated = out.logs.iter().any(|l| l.contains("Log truncated"));
        if truncated {
            self.log_truncated += 1;
        }
        if out.ok() && !truncated {
            let got: Vec<&String> = out.logs.iter().filter(|l| l.starts_with("Program log: Minted ")).collect();
            let want: Vec<String> = minted.iter().map(|m| minted_log(m.amount, m.supply_before)).collect();
            if got.len() != want.len() || got.iter().zip(want.iter()).any(|(g, w)| *g != w) {
                problems.push(format!("mint log lines: expected {:?}, got {:?}", want, got));
            }
        }
        if out.landed && !truncated && !matches!(pred, Outcome::TooLarge) {
            self.check_cu(spec, out, costs, &mut problems);
        }
        problems
    }

    /// Compute meter and bounds against the logs: every SBF instruction's "consumed N of M" must
    /// have M = limit minus what the earlier instructions consumed (builtins 150 each); a memo
    /// consumes exactly what its simulation did; any other instruction that completes stays within
    /// its bounds
    fn check_cu(&mut self, spec: &TxSpec, out: &Out, costs: &[(u64, u64)], problems: &mut Vec<String>) {
        let frames = top_frames(&out.logs);
        let limit = limit_of(spec);
        let mut used = 0u64;
        let mut fi = 0usize;
        for (k, g) in spec.ixs.iter().enumerate() {
            let Some(f) = frames.get(fi) else { break };
            if f.program != g.ix.program_id {
                problems.push(format!("cu: log frame {} is {} but instruction {} is {}", fi, f.program, k, g.ix.program_id));
                return;
            }
            fi += 1;
            if matches!(g.k, K::CuLimit | K::CuPrice | K::Transfer) {
                if f.consumed.is_some() {
                    problems.push(format!("cu: builtin instruction {} logs a consumption", k));
                }
                used += BUILTIN_CU;
                continue;
            }
            let Some((n, m)) = f.consumed else {
                problems.push(format!("cu: no consumption logged for instruction {}", k));
                return;
            };
            if m != limit.saturating_sub(used) {
                problems.push(format!("cu: instruction {} starts with {} CU, expected limit {} - used {} = {}", k, m, limit, used, limit.saturating_sub(used)));
            }
            if f.success == Some(false) && costs.len() == k + 1 && !matches!(g.k, K::Memo | K::MemoV1) {
                // the instruction the model fails on its own: its cost up to the failure is within
                // [FAIL_LO, high] (the starved-at-failure windows rely on FAIL_LO)
                let (lo, hi) = costs[k];
                let budget = out.err_text().contains("ProgramFailedToComplete") || out.err_text().contains("ComputationalBudgetExceeded");
                if !budget && (n < lo || n > hi) {
                    problems.push(format!("cu: failing instruction {} ({}) consumed {}, outside the model's [{}, {}]", k, g.label, n, lo, hi));
                }
                if !budget {
                    let e = self.cu_seen.entry("failing instruction (non-memo)".into()).or_insert((u64::MAX, 0, 0));
                    e.0 = e.0.min(n);
                    e.1 = e.1.max(n);
                    e.2 += 1;
                }
            }
            if f.success == Some(true) {
                let (lo, hi) = costs.get(k).copied().unwrap_or((0, INF));
                if n < lo || n > hi {
                    problems.push(format!("cu: instruction {} ({}) consumed {}, outside the model's [{}, {}]", k, g.label, n, lo, hi));
                }
                let kind = match g.k {
                    K::Memo => "memo",
                    K::MemoV1 => "memo v1",
                    K::Mm => if minted_ok_mm(g) { "memo-mint" } else { "memo-mint (with substitutions)" },
                    K::Blog => "memo-blog mint_for_blog",
                    K::Forum(_) => "memo-forum mint_for_post",
                    K::Mine { .. } => "memo-home mine",
                    K::Init => "initialize_mint_state",
                    _ => "other",
                };
                let e = self.cu_seen.entry(kind.into()).or_insert((u64::MAX, 0, 0));
                e.0 = e.0.min(n);
                e.1 = e.1.max(n);
                e.2 += 1;
                if g.k == K::Mm {
                    // the high bound's margin, for calibration
                    let e = self.cu_seen.entry("memo-mint margin to high bound (min)".into()).or_insert((u64::MAX, 0, 0));
                    e.0 = e.0.min(hi.saturating_sub(n));
                    e.2 += 1;
                }
            }
            used += n;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn report(&mut self, kind: &str, spec: &TxSpec, out: &Out, pred: &Outcome, size: usize, problems: Vec<String>, post: &St) {
        let labels: Vec<&str> = spec.ixs.iter().map(|g| g.label.as_str()).collect();
        let ok = problems.is_empty();
        *self.hist.entry(pred.class(&self.names)).or_insert(0) += 1;
        *self.kinds.entry(kind.into()).or_insert(0) += 1;
        for g in spec.ixs.iter().filter(|g| g.k == K::Mm) {
            if let Some(t) = g.label.split_once('[').map(|(_, t)| t.trim_end_matches(']')) {
                for tag in t.split(',') {
                    *self.subs.entry(tag.to_string()).or_insert(0) += 1;
                }
            }
        }
        if let Some(g) = spec.ixs.first().filter(|g| g.k == K::Memo) {
            let class = match std::str::from_utf8(&g.ix.data) {
                Err(_) => "memo[0] invalid UTF-8".to_string(),
                Ok(s) => {
                    let c = s.chars().count();
                    let rel = if c < MEMO_MIN_CHARS { "<69" } else if c == MEMO_MIN_CHARS { "=69" } else { ">69" };
                    let b = g.ix.data.len();
                    let path = if b < 69 { "bytes<69" } else if b > 272 { "bytes>272" } else if s.is_ascii() { "ascii" } else { "counted" };
                    format!("memo[0] chars{} {}", rel, path)
                }
            };
            *self.memo_classes.entry(class).or_insert(0) += 1;
        }
        println!("#{} {} {} payer={} cu={} limit={} {}B slot={} | {} | pred {} | {}", self.step, kind, if spec.v0 { "v0" } else { "legacy" },
            actor_name(spec.payer), spec.cu, limit_of(spec), size, out.slot, labels.join(" ; "), pred.label(&self.names),
            if ok { "PASS".to_string() } else { format!("MISMATCH got {}", observed(out)) });
        if !ok {
            self.mismatches += 1;
            println!("  MISMATCH at step {}:", self.step);
            for p in &problems {
                println!("    - {}", p);
            }
            self.print_trace(spec, out);
            // adopt the chain's state
            self.st = self.read().0;
        } else {
            self.st = post.clone();
            if self.verbose {
                self.print_trace(spec, out);
            }
        }
    }

    fn print_trace(&self, spec: &TxSpec, out: &Out) {
        println!("    transaction ({}, payer {} {}, sig {}):", if spec.v0 { "v0" } else { "legacy" }, actor_name(spec.payer),
            self.actors[spec.payer].pubkey(), out.sig);
        for (i, g) in spec.ixs.iter().enumerate() {
            let prog = self.names.get(&g.ix.program_id).copied().unwrap_or("?");
            let accs: Vec<String> = g.ix.accounts.iter().map(|m| format!("{}{}{}", self.key_name(&m.pubkey),
                if m.is_writable { "(w)" } else { "" }, if m.is_signer { "(s)" } else { "" })).collect();
            let data = if matches!(g.k, K::Memo | K::MemoV1) { esc(&g.ix.data) } else { hex::encode(&g.ix.data) };
            println!("      [{}] {} {} accounts [{}] data {} ({} B)", i, prog, g.label, accs.join(", "), data, g.ix.data.len());
        }
        println!("    logs:");
        for l in &out.logs {
            println!("      {}", l);
        }
    }

    fn key_name(&self, k: &Pubkey) -> String {
        let e = self.e;
        if let Some(i) = self.actors.iter().position(|a| a.pubkey() == *k) {
            return actor_name(i).into();
        }
        let fixed = [(e.mint, "MEMO mint"), (e.mint_auth, "mint_authority"), (t22(), "Token-2022"), (IXS_SYSVAR, "ixs sysvar"),
            (e.mint_state, "MintState"), (self.fake_mint, "second mint"), (self.shadow, "shadow MintState"), (self.zero_state, "zeroed state"),
            (self.tiny_state, "4-byte state"), (e.gbs, "GlobalBurnStats"), (self.other_pda, "memo-burn PDA"), (SPL_TOKEN_LEGACY, "SPL Token"),
            (solana_sdk::sysvar::clock::ID, "Clock"), (e.mint_prog, "memo-mint"), (self.app.pubkey(), "app"), (self.vault, "vault"),
            (self.config, "home config"), (self.asset, "home asset")];
        if let Some((_, n)) = fixed.iter().find(|(x, _)| x == k) {
            return n.to_string();
        }
        let t = self.ta_name(k);
        if t.len() > 6 || t.contains('.') { t } else { k.to_string() }
    }

    /// The same instructions again in a later slot: memo-mint's one-mint check keys on
    /// (fingerprint, slot), so it must treat it as a new transaction. Either with a new blockhash
    /// two slots or more after the last transaction, or at once from another fee payer (a new
    /// signature, usually the same fingerprint, usually the next slot)
    fn resend_later(&mut self) {
        let last = self.last.take().unwrap();
        let mut spec = last.spec.clone();
        let mut next = false;
        if self.rng.gen_bool(0.5) {
            // another payer, as long as the transaction still fits
            let first = self.rng.gen_range(1..4);
            for d in 0..3 {
                let mut t = last.spec.clone();
                t.payer = (last.spec.payer + 1 + (first - 1 + d) % 3) % 4;
                if self.dry_size(&t) <= MAX_TX_BYTES {
                    spec = t;
                    next = true;
                    break;
                }
            }
        }
        if !next {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(30) {
                let slot = self.e.c.get_slot_with_commitment(CommitmentConfig::confirmed()).unwrap_or(0);
                let bh = self.e.c.get_latest_blockhash().unwrap_or_default();
                if slot > last.slot + 1 && bh != last.bh {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let (last_slot, last_fp) = (last.slot, last.fp);
        self.last = Some(last);
        let (pred, out, fp) = self.send_and_check(spec, if next { "resend-next" } else { "resend-later" });
        if fp == last_fp && pred == Outcome::Ok && out.ok() {
            let gap = out.slot.saturating_sub(last_slot);
            *self.kinds.entry(format!("  (same fingerprint, minted again {} slot{} later)", gap, if gap == 1 { "" } else { "s" })).or_insert(0) += 1;
        }
    }

    /// The very same signed transaction again: the runtime must not execute it twice
    fn resend_exact(&mut self) {
        let last = self.last.take().unwrap();
        let _ = match &last.wire {
            Wire::Legacy(t) => self.e.send(t),
            Wire::V0(t) => self.e.send(t),
        };
        // give it time to land if it were to
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let slot = self.e.c.get_slot_with_commitment(CommitmentConfig::confirmed()).unwrap_or(0);
            if slot > last.slot + 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let sig: solana_sdk::signature::Signature = last.sig.parse().unwrap();
        let again = self.e.wait(&sig);
        let (chain, frozen) = self.read();
        let mut problems = vec![];
        if again.slot != last.slot {
            problems.push(format!("the signature now reports slot {}, first landed in {}", again.slot, last.slot));
        }
        if chain != self.st {
            problems.push(format!("state: {}", st_diff(&self.st, &chain, self)));
        }
        if frozen != self.frozen {
            problems.push("accounts that must never change changed".into());
        }
        let ok = problems.is_empty();
        *self.kinds.entry("resend-exact".into()).or_insert(0) += 1;
        println!("#{} resend-exact of {} (slot {}) | {}", self.step, last.sig, last.slot, if ok { "PASS" } else { "MISMATCH" });
        if !ok {
            self.mismatches += 1;
            for p in &problems {
                println!("    - {}", p);
            }
            self.st = chain;
        }
        self.last = Some(last);
    }

    /// Two transactions with one blockhash, sent back to back (usually landing in one slot): the
    /// same instructions from two fee payers (same fingerprint: in one slot the later one must
    /// fail with OneMintPerTransaction), or two independent ones
    fn pair(&mut self) {
        let t1 = self.gen_until(true);
        // the same instructions from another fee payer, as long as the transaction still fits (a
        // payer that signs nothing else adds a signature and a key)
        let mut t2 = None;
        if self.rng.gen_bool(0.6) {
            let first = self.rng.gen_range(1..4);
            for d in 0..3 {
                let mut t = t1.clone();
                t.payer = (t1.payer + 1 + (first - 1 + d) % 3) % 4;
                if self.dry_size(&t) <= MAX_TX_BYTES {
                    t2 = Some(t);
                    break;
                }
            }
        }
        let t2 = match t2 {
            Some(t) => t,
            None => self.gen_until(true),
        };
        let bh = self.e.c.get_latest_blockhash().expect("blockhash");
        let (w1, m1, l1) = self.build(&t1, bh);
        let (w2, m2, l2) = self.build(&t2, bh);
        let (fp1, fp2) = (fingerprint_versioned(&m1, &l1), fingerprint_versioned(&m2, &l2));
        let s1 = match &w1 { Wire::Legacy(t) => self.e.send(t), Wire::V0(t) => self.e.send(t) };
        let s2 = match &w2 { Wire::Legacy(t) => self.e.send(t), Wire::V0(t) => self.e.send(t) };
        let o1 = match s1 { Ok(s) => self.e.wait(&s), Err(err) => Out { err: Some(json!(format!("RPC rejected: {}", err))), ..Default::default() } };
        let o2 = match s2 { Ok(s) => self.e.wait(&s), Err(err) => Out { err: Some(json!(format!("RPC rejected: {}", err))), ..Default::default() } };
        let mut problems = vec![];
        if !o1.landed || !o2.landed {
            problems.push(format!("pair did not land: {} / {}", o1.brief(), o2.brief()));
        }
        let first_is_1 = if o1.slot != o2.slot {
            o1.slot < o2.slot
        } else {
            let order = self.e.block_signatures(o1.slot);
            let (p1, p2) = (order.iter().position(|s| *s == o1.sig), order.iter().position(|s| *s == o2.sig));
            if p1.is_none() || p2.is_none() {
                problems.push(format!("block {} does not list both signatures", o1.slot));
            }
            p1 < p2
        };
        let fl1 = self.flags_of(&t1);
        let fl2 = self.flags_of(&t2);
        self.prepare_costs(&t1, &fl1);
        self.prepare_costs(&t2, &fl2);
        let st0 = self.st.clone();
        let (a, b) = if first_is_1 { ((&t1, &fl1, fp1, &o1), (&t2, &fl2, fp2, &o2)) } else { ((&t2, &fl2, fp2, &o2), (&t1, &fl1, fp1, &o1)) };
        let wa = self.walk(a.0, a.1, a.2, a.3.slot, limit_of(a.0), &st0);
        let wb = self.walk(b.0, b.1, b.2, b.3.slot, limit_of(b.0), &wa.post);
        for (n, w, o) in [(1, &wa, a.3), (2, &wb, b.3)] {
            if !w.out.matches(o) {
                problems.push(format!("tx {} in order: predicted {}, got {}", n, w.out.label(&self.names), observed(o)));
            }
            if o.ok() {
                let got: Vec<&String> = o.logs.iter().filter(|l| l.starts_with("Program log: Minted ")).collect();
                let want: Vec<String> = w.minted.iter().map(|m| minted_log(m.amount, m.supply_before)).collect();
                if got.len() != want.len() || got.iter().zip(want.iter()).any(|(g, w)| *g != w) {
                    problems.push(format!("tx {} in order: mint log lines: expected {:?}, got {:?}", n, want, got));
                }
            }
        }
        let (chain, frozen) = self.read();
        if chain != wb.post {
            problems.push(format!("state: {}", st_diff(&wb.post, &chain, self)));
        }
        if frozen != self.frozen {
            problems.push("accounts that must never change changed".into());
        }
        let ok = problems.is_empty();
        *self.kinds.entry(format!("pair ({})", if fp1 == fp2 { "same fingerprint" } else { "different fingerprints" })).or_insert(0) += 1;
        *self.hist.entry(format!("pair: {} then {}", wa.out.class(&self.names), wb.out.class(&self.names))).or_insert(0) += 1;
        let labels = |t: &TxSpec| t.ixs.iter().map(|g| g.label.as_str()).collect::<Vec<_>>().join(" ; ");
        println!("#{} pair {} slots {}/{} {} | t1 {} | t2 {} | pred {} then {} | {}", self.step,
            if fp1 == fp2 { "same-fp" } else { "diff-fp" }, o1.slot, o2.slot, if first_is_1 { "t1 first" } else { "t2 first" },
            labels(&t1), labels(&t2), wa.out.label(&self.names), wb.out.label(&self.names), if ok { "PASS" } else { "MISMATCH" });
        if !ok {
            self.mismatches += 1;
            println!("  MISMATCH at step {}:", self.step);
            for p in &problems {
                println!("    - {}", p);
            }
            self.print_trace(&t1, &o1);
            self.print_trace(&t2, &o2);
            self.st = chain;
        } else {
            self.st = wb.post;
        }
    }

    fn summary(&self) {
        println!("\n== fuzz-memo-mint seed {}: {} steps, {} mismatched ==", self.seed, self.step, self.mismatches);
        println!("final supply {} units, MintState slot {}, regenerated (ambiguous or too large) {}, log-truncated {}",
            self.st.supply, self.st.ms.1, self.retries, self.log_truncated);
        println!("step kinds:");
        for (k, n) in &self.kinds {
            println!("  {:6}  {}", n, k);
        }
        println!("predicted outcomes:");
        for (k, n) in &self.hist {
            println!("  {:6}  {}", n, k);
        }
        println!("memo at index 0 (characters vs 69, memo-mint path):");
        for (k, n) in &self.memo_classes {
            println!("  {:6}  {}", n, k);
        }
        println!("memo-mint substitutions (instructions carrying each):");
        for (k, n) in &self.subs {
            println!("  {:6}  {}", n, k);
        }
        println!("compute units of completed instructions (min, max, count):");
        for (k, (a, b, n)) in &self.cu_seen {
            println!("  {:<45} {:>9} {:>9} {:>6}", k, a, b, n);
        }
    }
}

fn minted_ok_mm(g: &Gx) -> bool { g.label.find('[').is_none() }

fn actor_name(i: usize) -> &'static str { ["A", "B", "C", "D"][i] }

/// memo-mint's high CU bound: base, the memo bytes it reads (and counts when the memo is not ASCII
/// and 69-272 bytes), the instructions sysvar it hashes, and extra accounts
fn mm_hi(spec: &TxSpec, ix: &Instruction) -> u64 {
    let memo = spec.ixs.first().filter(|g| g.ix.program_id == MEMO_V2).map(|g| g.ix.data.as_slice()).unwrap_or(&[]);
    let counting = memo.len() >= 69 && memo.len() <= 272 && !memo.is_ascii();
    let per_byte = if counting { 18 } else { 2 };
    let sysvar: u64 = spec.ixs.iter().map(|g| 70 + 33 * g.ix.accounts.len() as u64 + g.ix.data.len() as u64).sum();
    MM_HI_BASE + per_byte * memo.len() as u64 + sysvar + 150 * ix.accounts.len().saturating_sub(7) as u64
}

/// The transaction's compute unit limit (see MAX_CU)
fn limit_of(spec: &TxSpec) -> u64 {
    if let Some(g) = spec.ixs.iter().find(|g| g.k == K::CuLimit) {
        let v = u32::from_le_bytes(g.ix.data[1..5].try_into().unwrap()) as u64;
        return v.min(MAX_CU);
    }
    let sum: u64 = spec.ixs.iter().map(|g| match g.k {
        K::CuLimit | K::CuPrice | K::Transfer => DEFAULT_BUILTIN_CU,
        _ => DEFAULT_SBF_CU,
    }).sum();
    sum.min(MAX_CU)
}

fn st_diff(model: &St, chain: &St, f: &Fz) -> String {
    let mut v = vec![];
    if model.supply != chain.supply {
        v.push(format!("supply model {} chain {}", model.supply, chain.supply));
    }
    if model.ms != chain.ms {
        v.push(format!("MintState model ({}, slot {}) chain ({}, slot {})", hex::encode(&model.ms.0[..8]), model.ms.1,
            hex::encode(&chain.ms.0[..8]), chain.ms.1));
    }
    for (k, b) in &model.bal {
        let c = chain.bal.get(k).copied().unwrap_or(u64::MAX);
        if *b != c {
            v.push(format!("{} model {} chain {}", f.ta_name(k), b, c));
        }
    }
    if v.is_empty() { "same".into() } else { v.join("; ") }
}

fn main() {
    let (mut seed, mut steps, mut stop, mut verbose, mut subs) = (1u64, 200usize, false, false, 0.22f64);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = |i: usize| args.get(i + 1).cloned().unwrap_or_else(|| { eprintln!("{} needs a value", args[i]); std::process::exit(2) });
        match args[i].as_str() {
            "--seed" => { seed = val(i).parse().expect("--seed <u64>"); i += 2; }
            "--steps" => { steps = val(i).parse().expect("--steps <n>"); i += 2; }
            "--stop-on-mismatch" => { stop = true; i += 1; }
            "--verbose" => { verbose = true; i += 1; }
            "--subs" => { subs = val(i).parse().expect("--subs <probability>"); i += 2; }
            other => {
                eprintln!("unknown argument {} (--seed <u64> --steps <n> [--subs <p>] [--stop-on-mismatch] [--verbose])", other);
                std::process::exit(2);
            }
        }
    }
    let started = Instant::now();
    let e = Env::new("fuzz-memo-mint");
    println!("seed {} steps {} substitution rate {}", seed, steps, subs);
    let mut f = Fz::setup(&e, seed, verbose, subs);
    while f.step < steps {
        f.run_step();
        if stop && f.mismatches > 0 {
            break;
        }
    }
    f.summary();
    println!("elapsed {:.0} s", started.elapsed().as_secs_f64());
    std::process::exit(if f.mismatches == 0 { 0 } else { 1 });
}
