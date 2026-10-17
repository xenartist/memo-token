//! On-chain regression suite for memo-burn (local validator only).
//!
//! Covers burn amount bounds (1-unit minimum, fractional amounts, 1T + 1, u64::MAX), the memo
//! binding (amount, version, payload length prefix, strict Base64, length boundaries), memo
//! position rules, owner/delegate/mint/stats-PDA constraints, exact user and global burn
//! statistics deltas, CPI Guard and durable-nonce behaviour. Every case asserts the exact outcome.
//! The exact 1T burn and the 18T user statistics cap are covered by regression-memo-curve.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-burn

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use base64::{engine::general_purpose, Engine as _};
use harness::*;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::get_associated_token_address_with_program_id as ata_of;

// memo-burn error codes
const MEMO_REQUIRED: u32 = 6000;
const INVALID_MEMO_FORMAT: u32 = 6001;
const UNSUPPORTED_MEMO_VERSION: u32 = 6002;
const BURN_AMOUNT_TOO_SMALL: u32 = 6003;
const BURN_AMOUNT_TOO_LARGE: u32 = 6004;
const INVALID_TOKEN_ACCOUNT: u32 = 6006;
const UNAUTHORIZED_MINT: u32 = 6007;
const UNAUTHORIZED_TOKEN_ACCOUNT: u32 = 6008;
const BURN_AMOUNT_MISMATCH: u32 = 6009;
const MEMO_TOO_SHORT: u32 = 6010;

fn main() {
    let e = Env::new("regression-memo-burn");
    // A is a fresh user (the wallet's statistics may already be at the 18T cap after
    // regression-memo-curve, which would hide total_burned deltas)
    let a_user = e.new_user("A", 10_000 * UNIT, true);
    let a = &a_user;
    let ap = a.pubkey();
    let a_ta = e.ata(&ap);
    let x: u64 = 1_234_567; // 1.234567 tokens, a fractional amount

    // ---------------- setup ----------------
    let b = e.new_user("B", 1000 * UNIT, true);
    let bp = b.pubkey();
    let b_ta = e.ata(&bp);
    let c = e.new_user("C (no burn stats)", 100 * UNIT, false);
    let cp = c.pubkey();
    let watch = [a_ta, b_ta];
    let users = [ap, bp];
    let rej = |id: &str, desc: &str, memo: Vec<u8>, amount: u64, expect: Expect| {
        e.reject(id, desc, vec![memo_ix(&memo), e.burn_ix(&ap, amount)], a, &[], expect, &watch, &users);
    };
    let at1 = |code: u32| Expect::Custom(1, code);

    // ---------------- first burn of a new user sets every statistic ----------------
    {
        let f = e.new_user("F (fresh stats)", 10 * UNIT, true);
        let fp = f.pubkey();
        let s0 = e.user_stats(&fp);
        let fresh = s0.map(|s| s.user == fp && s.total == 0 && s.count == 0 && s.last_time == 0).unwrap_or(false);
        e.record("B00a", "initialize_user_global_burn_stats creates zeroed stats for the user", fresh, &format!("{:?}", s0));
        e.burn_ok("B00b", "first burn of a fresh user: total, count, last_burn_time and global stats updated exactly",
            vec![memo_ix(&e.burn_memo(1, x, "fresh")), e.burn_ix(&fp, x)], &f, &[], &fp, x, 1);
    }

    // ---------------- amount bounds ----------------
    rej("B01", "amount 0", e.burn_memo(1, 0, "zero"), 0, at1(BURN_AMOUNT_TOO_SMALL));
    e.burn_ok("B02", "amount 1 unit (the minimum)", vec![memo_ix(&e.burn_memo(1, 1, "one-unit")), e.burn_ix(&ap, 1)], a, &[], &ap, 1, 1);
    e.burn_ok("B02b", "fractional amount 1.234567 tokens", vec![memo_ix(&e.burn_memo(1, x, "frac")), e.burn_ix(&ap, x)], a, &[], &ap, x, 1);
    e.burn_ok("B02c", "amount 999,999 units (just below one token)",
        vec![memo_ix(&e.burn_memo(1, 999_999, "sub-token")), e.burn_ix(&ap, 999_999)], a, &[], &ap, 999_999, 1);
    e.burn_ok("B02d", "amount 1 token", vec![memo_ix(&e.burn_memo(1, UNIT, "one-token")), e.burn_ix(&ap, UNIT)], a, &[], &ap, UNIT, 1);
    rej("B04", "amount 1T + 1 unit", e.burn_memo(1, ONE_T + 1, "1T+1"), ONE_T + 1, at1(BURN_AMOUNT_TOO_LARGE));
    rej("B04b", "amount u64::MAX", e.burn_memo(1, u64::MAX, "u64max"), u64::MAX, at1(BURN_AMOUNT_TOO_LARGE));
    {
        let bb = e.balance(&b_ta).unwrap();
        e.reject("B05", "B burns its balance + 1 unit", vec![memo_ix(&e.burn_memo(1, bb + 1, "insufficient")), e.burn_ix(&bp, bb + 1)],
            &b, &[], Expect::Custom(1, TOKEN_INSUFFICIENT_FUNDS), &watch, &users);
    }

    // ---------------- memo content ----------------
    rej("B06", "memo amount = amount + 1", e.burn_memo(1, x + 1, "mm+1"), x, at1(BURN_AMOUNT_MISMATCH));
    rej("B07", "memo amount = amount - 1", e.burn_memo(1, x - 1, "mm-1"), x, at1(BURN_AMOUNT_MISMATCH));
    rej("B07b", "memo amount = whole-token rounding of a fractional amount", e.burn_memo(1, UNIT, "rounded"), x, at1(BURN_AMOUNT_MISMATCH));
    rej("B08", "memo version 0", e.burn_memo(0, x, "v0"), x, at1(UNSUPPORTED_MEMO_VERSION));
    rej("B09", "memo version 2", e.burn_memo(2, x, "v2"), x, at1(UNSUPPORTED_MEMO_VERSION));
    {
        let p = e.payload("len", 60);
        let forged = |declared: u32| b64(&burn_memo_raw(1, x, declared, &p));
        rej("B10", "payload length prefix = actual + 1", forged(61), x, at1(INVALID_MEMO_FORMAT));
        rej("B11", "payload length prefix = actual - 1", forged(59), x, at1(INVALID_MEMO_FORMAT));
        rej("B12", "payload length prefix = u32::MAX", forged(u32::MAX), x, at1(INVALID_MEMO_FORMAT));
        rej("B12b", "payload length prefix = 0 with 60 payload bytes", forged(0), x, at1(INVALID_MEMO_FORMAT));
    }
    {
        let mut m = e.burn_memo(1, x, "trail-b64");
        m.extend(b"AAAA");
        rej("B13", "valid memo + 'AAAA' (three extra decoded bytes)", m, x, at1(INVALID_MEMO_FORMAT));
        let mut m = e.burn_memo(1, x, "trail-junk");
        m.extend(b"!!");
        rej("B14", "valid memo + '!!'", m, x, at1(INVALID_MEMO_FORMAT));
        let m = format!("this memo is plain text, not base64 at all, nonce {} .........................", e.nonce()).into_bytes();
        rej("B15", "plain-text memo", m, x, at1(INVALID_MEMO_FORMAT));
    }
    {
        // bytes FB EF BE encode to "++++" in the standard alphabet and "----" in the URL-safe one
        let mut p = e.payload("urlsafe", 60);
        p[2] = 0xFB;
        p[3] = 0xEF;
        p[4] = 0xBE;
        let raw = burn_memo_raw(1, x, p.len() as u32, &p);
        let std_enc = general_purpose::STANDARD.encode(&raw);
        let url_enc = general_purpose::URL_SAFE.encode(&raw);
        rej("B16", "URL-safe Base64 alphabet", url_enc.into_bytes(), x, at1(INVALID_MEMO_FORMAT));
        e.burn_ok("B16b", "control: the same bytes in the standard alphabet", vec![memo_ix(std_enc.as_bytes()), e.burn_ix(&ap, x)], a, &[], &ap, x, 1);
    }
    {
        let m = String::from_utf8(e.burn_memo(1, x, "ws-mid")).unwrap();
        rej("B17", "newline inside the Base64", format!("{}\n{}", &m[..40], &m[40..]).into_bytes(), x, at1(INVALID_MEMO_FORMAT));
        let m = String::from_utf8(e.burn_memo(1, x, "ws-end")).unwrap();
        rej("B17b", "trailing newline", format!("{}\n", m).into_bytes(), x, at1(INVALID_MEMO_FORMAT));
        let m = String::from_utf8(e.burn_memo(1, x, "ws-lead")).unwrap();
        rej("B17c", "leading space", format!(" {}", m).into_bytes(), x, at1(INVALID_MEMO_FORMAT));
        // 13 + 58 = 71 decoded bytes: the canonical encoding needs one '='
        let p = e.payload("nopad", 58);
        let raw = burn_memo_raw(1, x, 58, &p);
        rej("B18", "Base64 without '=' padding", general_purpose::STANDARD_NO_PAD.encode(&raw).into_bytes(), x, at1(INVALID_MEMO_FORMAT));
        let p = e.payload("trailbits", 58);
        let mut enc = b64(&burn_memo_raw(1, x, 58, &p));
        let pad = enc.iter().filter(|&&ch| ch == b'=').count();
        let idx = enc.len() - pad - 1;
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let pos = alphabet.iter().position(|&ch| ch == enc[idx]).unwrap();
        enc[idx] = alphabet[pos ^ 1];
        rej("B18b", "Base64 with non-zero trailing bits", enc, x, at1(INVALID_MEMO_FORMAT));
    }
    {
        // BURN-02: 68 bytes -> MemoTooShort; 69-71 bytes -> InvalidMemoFormat; 72 bytes -> ok
        let raw = |plen: usize, tag: &str| -> Vec<u8> { let p = e.payload(tag, plen); burn_memo_raw(1, x, plen as u32, &p) };
        let m68 = b64(&raw(38, "l68"));
        let mut m69 = b64(&raw(38, "l69"));
        m69.push(b'=');
        let m70 = general_purpose::STANDARD_NO_PAD.encode(raw(39, "l70")).into_bytes();
        let m71 = general_purpose::STANDARD_NO_PAD.encode(raw(40, "l71")).into_bytes();
        let m72 = b64(&raw(39, "l72"));
        let lengths = [m68.len(), m69.len(), m70.len(), m71.len(), m72.len()];
        e.record("B19-len", "BURN-02 memos have lengths 68, 69, 70, 71, 72", lengths == [68, 69, 70, 71, 72], &format!("{:?}", lengths));
        rej("B19", "well-formed 68-byte burn memo", m68, x, at1(MEMO_TOO_SHORT));
        rej("B19-69", "69-byte memo (canonical 68 + '=')", m69, x, at1(INVALID_MEMO_FORMAT));
        rej("B19-70", "70-byte memo (unpadded Base64)", m70, x, at1(INVALID_MEMO_FORMAT));
        rej("B19-71", "71-byte memo (unpadded Base64)", m71, x, at1(INVALID_MEMO_FORMAT));
        e.burn_ok("B19b", "72-byte memo (shortest valid)", vec![memo_ix(&m72), e.burn_ix(&ap, x)], a, &[], &ap, x, 1);
        e.reject("B19c", "non-UTF-8 memo bytes (rejected by the Memo program)", vec![memo_ix(&[0xff; 80]), e.burn_ix(&ap, x)],
            a, &[], Expect::ErrContains("InvalidInstructionData"), &watch, &users);
    }

    // ---------------- instruction position ----------------
    e.reject("B20", "ComputeBudget at index 0, valid memo at index 1",
        vec![cu(200_000), memo_ix(&e.burn_memo(1, x, "idx1")), e.burn_ix(&ap, x)], a, &[], Expect::Custom(2, MEMO_REQUIRED), &watch, &users);
    e.reject("B21", "process_burn at index 0, memo at index 1",
        vec![e.burn_ix(&ap, x), memo_ix(&e.burn_memo(1, x, "idx0"))], a, &[], Expect::Custom(0, MEMO_REQUIRED), &watch, &users);
    e.reject("B22", "no memo", vec![e.burn_ix(&ap, x)], a, &[], Expect::Custom(0, MEMO_REQUIRED), &watch, &users);
    e.reject("B22b", "legacy Memo v1 at index 0 with a valid burn memo body",
        vec![Instruction::new_with_bytes(LEGACY_MEMO_V1, &e.burn_memo(1, x, "legacy"), vec![]), e.burn_ix(&ap, x)],
        a, &[], Expect::Custom(1, MEMO_REQUIRED), &watch, &users);
    e.burn_ok("B22c", "ComputeBudget between the memo (index 0) and the burn",
        vec![memo_ix(&e.burn_memo(1, x, "cb-mid")), cu(300_000), e.burn_ix(&ap, x)], a, &[], &ap, x, 1);

    // ---------------- several burns bound to one memo ----------------
    e.burn_ok("B23", "two process_burn(X) bound to one memo (both burn, by design)",
        vec![memo_ix(&e.burn_memo(1, x, "double")), e.burn_ix(&ap, x), e.burn_ix(&ap, x)], a, &[], &ap, 2 * x, 2);
    e.reject("B23b", "process_burn(X) + process_burn(X + 1) with a memo for X (second fails, transaction reverted)",
        vec![memo_ix(&e.burn_memo(1, x, "double-diff")), e.burn_ix(&ap, x), e.burn_ix(&ap, x + 1)], a, &[], Expect::Custom(2, BURN_AMOUNT_MISMATCH), &watch, &users);
    {
        let before = e.snap(&[a_ta, b_ta], &[ap, bp]);
        let out = e.exec(&[memo_ix(&e.burn_memo(1, x, "two-users")), e.burn_ix(&ap, x), e.burn_ix(&bp, x)], a, &[&b]);
        let after = e.snap(&[a_ta, b_ta], &[ap, bp]);
        let (ua0, ua1, ub0, ub1) = (before.user(&ap), after.user(&ap), before.user(&bp), after.user(&bp));
        let (g0, g1) = (before.global(), after.global());
        let pass = out.ok()
            && before.bal(&a_ta) - after.bal(&a_ta) == x && before.bal(&b_ta) - after.bal(&b_ta) == x
            && before.supply - after.supply == 2 * x
            && ua1.total == ua0.total + x && ua1.count == ua0.count + 1 && Some(ua1.last_time) == out.block_time
            && ub1.total == ub0.total + x && ub1.count == ub0.count + 1 && Some(ub1.last_time) == out.block_time
            && g1.total == g0.total + 2 * x as u128 && g1.count == g0.count + 2 && Some(g1.last_time) == out.block_time;
        e.record("B23c", "A and B each burn X with one memo: each user's stats +X/+1, global +2X/+2", pass,
            &format!("{}; {}", out.brief(), before.diff(&after)));
    }

    // ---------------- authority and accounts ----------------
    e.reject("B24", "A signs, burns from B's token account (A's stats)",
        vec![memo_ix(&e.burn_memo(1, x, "other-ta")), e.burn_ix_with(x, e.burn_accounts(&ap, &b_ta, &e.stats_pda(&ap)))],
        a, &[], at1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &users);
    e.must("B approves A as delegate for 10 MEMO",
        &[spl_token_2022::instruction::approve_checked(&t22(), &b_ta, &e.mint, &ap, &bp, &[], 10 * UNIT, 6).unwrap()], &b, &[]);
    e.reject("B25", "delegate A burns from owner B's account through memo-burn (A's stats)",
        vec![memo_ix(&e.burn_memo(1, x, "delegate")), e.burn_ix_with(x, e.burn_accounts(&ap, &b_ta, &e.stats_pda(&ap)))],
        a, &[], at1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &users);
    e.reject("B25b", "delegate A burns from B's account passing B's stats",
        vec![memo_ix(&e.burn_memo(1, x, "delegate2")), e.burn_ix_with(x, e.burn_accounts(&ap, &b_ta, &e.stats_pda(&bp)))],
        a, &[], at1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &users);
    e.reject("B26", "A burns its own tokens but passes B's stats",
        vec![memo_ix(&e.burn_memo(1, x, "other-stats")), e.burn_ix_with(x, e.burn_accounts(&ap, &a_ta, &e.stats_pda(&bp)))],
        a, &[], at1(ANCHOR_CONSTRAINT_SEEDS), &watch, &users);
    e.reject("B27", "C (no burn stats) burns", vec![memo_ix(&e.burn_memo(1, x, "no-stats")), e.burn_ix(&cp, x)],
        &c, &[], at1(ANCHOR_ACCOUNT_NOT_INITIALIZED), &[e.ata(&cp)], &[]);
    {
        let g = e.new_mint(&ap);
        let g_ta = ata_of(&ap, &g.pubkey(), &t22());
        e.must("create A's token account of mint G and mint 1000 G", &[
            e.create_ata_ix(&ap, &g.pubkey()),
            spl_token_2022::instruction::mint_to(&t22(), &g.pubkey(), &g_ta, &ap, &[], 1000 * UNIT).unwrap()], &e.wallet, &[a]);
        let mut acc = e.burn_accounts(&ap, &g_ta, &e.stats_pda(&ap));
        acc[1] = AccountMeta::new(g.pubkey(), false);
        e.reject("B28", "other mint G + A's G account (burn worthless tokens to inflate stats)",
            vec![memo_ix(&e.burn_memo(1, x, "fake-mint")), e.burn_ix_with(x, acc)], a, &[], at1(UNAUTHORIZED_MINT), &watch, &users);
        e.reject("B28b", "real mint + A's token account of mint G",
            vec![memo_ix(&e.burn_memo(1, x, "wrong-ta-mint")), e.burn_ix_with(x, e.burn_accounts(&ap, &g_ta, &e.stats_pda(&ap)))],
            a, &[], at1(INVALID_TOKEN_ACCOUNT), &watch, &users);
    }
    {
        let with = |i: usize, m: AccountMeta| {
            let mut acc = e.burn_accounts(&ap, &a_ta, &e.stats_pda(&ap));
            acc[i] = m;
            acc
        };
        e.reject("B29", "legacy SPL Token program as token_program",
            vec![memo_ix(&e.burn_memo(1, x, "legacy-token")), e.burn_ix_with(x, with(4, AccountMeta::new_readonly(SPL_TOKEN_LEGACY, false)))],
            a, &[], at1(ANCHOR_INVALID_PROGRAM_ID), &watch, &users);
        e.reject("B30", "random account as instructions sysvar",
            vec![memo_ix(&e.burn_memo(1, x, "fake-sysvar")), e.burn_ix_with(x, with(5, AccountMeta::new_readonly(Pubkey::new_unique(), false)))],
            a, &[], at1(ANCHOR_CONSTRAINT_ADDRESS), &watch, &users);
        e.reject("B31", "B's UserGlobalBurnStats as global_burn_stats",
            vec![memo_ix(&e.burn_memo(1, x, "fake-gbs")), e.burn_ix_with(x, with(6, AccountMeta::new(e.stats_pda(&bp), false)))],
            a, &[], at1(ANCHOR_DISCRIMINATOR_MISMATCH), &watch, &users);
        e.reject("B31b", "memo-mint MintState as global_burn_stats",
            vec![memo_ix(&e.burn_memo(1, x, "ms-as-gbs")), e.burn_ix_with(x, with(6, AccountMeta::new(e.mint_state, false)))],
            a, &[], at1(ANCHOR_OWNED_BY_WRONG_PROGRAM), &watch, &users);
        e.reject("B31c", "global_burn_stats read-only",
            vec![memo_ix(&e.burn_memo(1, x, "gbs-ro")), e.burn_ix_with(x, with(6, AccountMeta::new_readonly(e.gbs, false)))],
            a, &[], at1(ANCHOR_CONSTRAINT_MUT), &watch, &users);
        e.reject("B32b", "token_account read-only",
            vec![memo_ix(&e.burn_memo(1, x, "ta-ro")), e.burn_ix_with(x, with(2, AccountMeta::new_readonly(a_ta, false)))],
            a, &[], at1(ANCHOR_CONSTRAINT_MUT), &watch, &users);
        let mut acc = e.burn_accounts(&bp, &b_ta, &e.stats_pda(&bp));
        acc[0] = AccountMeta::new(bp, false);
        e.reject("B32", "user B not a signer (fee payer A)", vec![memo_ix(&e.burn_memo(1, x, "not-signer")), e.burn_ix_with(x, acc)],
            a, &[], at1(ANCHOR_ACCOUNT_NOT_SIGNER), &watch, &users);
    }

    // ---------------- stats initialization ----------------
    e.reject("B33", "initialize_user_global_burn_stats twice", vec![e.init_stats_ix(&ap)], a, &[],
        Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &watch, &users);
    e.reject("B34", "non-admin initialize_global_burn_stats while GlobalBurnStats exists",
        vec![Instruction::new_with_bytes(e.burn_prog, &disc("initialize_global_burn_stats"), vec![
            AccountMeta::new(ap, true), AccountMeta::new(e.gbs, false), AccountMeta::new_readonly(system_program(), false)])],
        a, &[], Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &watch, &users);
    {
        let d = Keypair::new();
        e.airdrop(&d.pubkey(), 2);
        let pda = e.stats_pda(&d.pubkey());
        e.must("A pre-funds D's stats PDA with 0.01 SOL", &[sys_ix::transfer(&ap, &pda, 10_000_000)], a, &[]);
        let out = e.exec(&[e.init_stats_ix(&d.pubkey())], &d, &[]);
        let owner = e.c.get_account(&pda).ok().map(|acc| acc.owner);
        let s = e.user_stats(&d.pubkey());
        let pass = out.ok() && owner == Some(e.burn_prog) && s.map(|s| s.user == d.pubkey() && s.total == 0 && s.count == 0).unwrap_or(false);
        e.record("B35", "a pre-funded stats PDA does not block initialize_user_global_burn_stats", pass,
            &format!("{}; owner {:?}; stats {:?}", out.brief(), owner, s));
    }

    // ---------------- mint and burn with one memo ----------------
    {
        let memo = e.burn_memo(1, x, "mint+burn");
        let before = e.snap(&[a_ta], &[ap]);
        let reward = curve_reference(before.supply).unwrap_or(0);
        let out = e.exec(&[memo_ix(&memo), e.mint_ix(&ap), e.burn_ix(&ap, x)], a, &[]);
        let after = e.snap(&[a_ta], &[ap]);
        let pass = out.ok()
            && after.bal(&a_ta) as i128 - before.bal(&a_ta) as i128 == reward as i128 - x as i128
            && after.user(&ap).total == before.user(&ap).total + x
            && after.global().count == before.global().count + 1;
        e.record("B36", "one burn memo backs process_mint and process_burn(X) (balance delta = reward - X)", pass,
            &format!("{}; reward {}; {}", out.brief(), reward, before.diff(&after)));
    }

    // ---------------- CPI Guard (BURN-06) ----------------
    {
        let g = e.new_user("G (CPI Guard)", 100 * UNIT, true);
        let gp = g.pubkey();
        let g_ta = e.ata(&gp);
        e.must("G enables CPI Guard on its token account", &[
            spl_token_2022::instruction::reallocate(&t22(), &g_ta, &gp, &gp, &[], &[spl_token_2022::extension::ExtensionType::CpiGuard]).unwrap(),
            spl_token_2022::extension::cpi_guard::instruction::enable_cpi_guard(&t22(), &g_ta, &gp, &[]).unwrap(),
        ], &g, &[]);
        e.reject("X1a", "process_burn from an account with CPI Guard enabled (Token-2022 error 43)",
            vec![memo_ix(&e.burn_memo(1, x, "cpiguard")), e.burn_ix(&gp, x)], &g, &[], Expect::Custom(1, TOKEN_CPI_GUARD_BURN_BLOCKED), &[g_ta], &[gp]);
        e.mint_ok("X1b", "process_mint into an account with CPI Guard enabled still works",
            vec![e.text_memo("cpiguard-mint", 69), e.mint_ix(&gp)], &g, &[], &g_ta, &[a_ta]);
    }

    // ---------------- durable nonce (INT-04) ----------------
    {
        let nonce = Keypair::new();
        let rent = e.c.get_minimum_balance_for_rent_exemption(80).unwrap();
        e.must("create durable nonce account", &sys_ix::create_nonce_account(&ap, &nonce.pubkey(), &ap, rent), a, &[&nonce]);
        let d = e.data(&nonce.pubkey()).unwrap();
        let nonce_hash = Hash::new_from_array(d[40..72].try_into().unwrap());
        let ixs = vec![sys_ix::advance_nonce_account(&nonce.pubkey(), &ap), memo_ix(&e.burn_memo(1, x, "nonce")), e.burn_ix(&ap, x)];
        let tx = e.build(&ixs, a, &[], nonce_hash);
        let before = e.snap(&watch, &users);
        let out = e.send_tx(&tx);
        let after = e.snap(&watch, &users);
        e.record("X3c", "durable-nonce burn [AdvanceNonce, memo, process_burn] is rejected (memo not at index 0)",
            Expect::Custom(2, MEMO_REQUIRED).matches(&out) && before == after, &format!("{}; {}", out.brief(), before.diff(&after)));
    }

    e.finish();
}
