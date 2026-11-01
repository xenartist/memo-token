//! On-chain regression suite for memo-mint (local validator only).
//!
//! Covers one mint per transaction (top-level and through app CPI: memo-blog mint_for_blog,
//! memo-forum mint_for_post), the memo position and length rules, account substitution,
//! admin-only initialization with existing state, exact minted amounts against an independent
//! curve reference and durable-nonce transactions; then (M20-M28) account substitution through
//! process_mint_to, a shadow MintState (seeds constraint), read-only accounts, Anchor framework
//! rejections, the Revision 5 character minimum at its encoding boundaries (and the CU its fast
//! paths save), write locks requested on reserved keys and v0 transactions over an address lookup
//! table, two mints in one slot, the same instructions again in a later slot, and the exact
//! MemoTooShort log lines. 107 cases. Every case asserts the exact outcome.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-mint
//! The supply must be below the 10T cap, and the validator must carry the shadow MintState that
//! prepare-local-validator plants (scripts/local-validator.sh).

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use borsh::BorshSerialize;
use harness::*;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0::LoadedAddresses, AddressLookupTableAccount, Message, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::get_associated_token_address_with_program_id as ata_of;
use std::{collections::HashSet, thread::sleep, time::{Duration, Instant}};

// memo-mint error codes
const MEMO_REQUIRED: u32 = 6000;
const MEMO_TOO_SHORT: u32 = 6001;
const INVALID_TOKEN_ACCOUNT: u32 = 6002;
const UNAUTHORIZED_MINT: u32 = 6003;
const UNAUTHORIZED_TOKEN_ACCOUNT: u32 = 6004;
const ONE_MINT_PER_TRANSACTION: u32 = 6008;

#[derive(BorshSerialize)]
struct BlogCreationData { version: u8, category: String, operation: String, creator: String, name: String, description: String, image: String }
#[derive(BorshSerialize)]
struct BlogMintData { version: u8, category: String, operation: String, minter: String, message: String }
#[derive(BorshSerialize)]
struct PostCreationData { version: u8, category: String, operation: String, creator: String, post_id: u64, title: String, content: String, image: String }
#[derive(BorshSerialize)]
struct PostMintData { version: u8, category: String, operation: String, user: String, post_id: u64, message: String }

fn blog_pda(e: &Env, owner: &Pubkey) -> Pubkey { Pubkey::find_program_address(&[b"blog", owner.as_ref()], &e.blog_prog).0 }
fn forum_counter_pda(e: &Env) -> Pubkey { Pubkey::find_program_address(&[b"global_counter"], &e.forum_prog).0 }
fn post_pda(e: &Env, id: u64) -> Pubkey { Pubkey::find_program_address(&[b"post", id.to_le_bytes().as_ref()], &e.forum_prog).0 }

/// Burn amount for an app create operation: twice the current mint reward (the minimum is 1x)
fn app_burn_amount(e: &Env) -> u64 { 2 * curve_reference(e.supply()).unwrap_or(1) }

fn ensure_blog(e: &Env, who: &Keypair) {
    let wp = who.pubkey();
    if e.data(&blog_pda(e, &wp)).is_some() {
        return;
    }
    let amount = app_burn_amount(e);
    let p = BlogCreationData { version: 1, category: "blog".into(), operation: "create_blog".into(), creator: wp.to_string(),
        name: "regression blog".into(), description: "regression test blog".into(), image: "".into() };
    let mut d = disc("create_blog");
    d.extend(amount.to_le_bytes());
    let ix = Instruction::new_with_bytes(e.blog_prog, &d, vec![
        AccountMeta::new(wp, true), AccountMeta::new(blog_pda(e, &wp), false), AccountMeta::new(e.mint, false),
        AccountMeta::new(e.ata(&wp), false), AccountMeta::new(e.stats_pda(&wp), false), AccountMeta::new_readonly(t22(), false),
        AccountMeta::new_readonly(e.burn_prog, false), AccountMeta::new_readonly(system_program(), false),
        AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.gbs, false)]);
    e.must("create blog", &[app_burn_memo(amount, &p.try_to_vec().unwrap()), ix, cu(600_000)], who, &[]);
}

fn create_post(e: &Env, who: &Keypair) -> u64 {
    let wp = who.pubkey();
    let id = u64::from_le_bytes(e.data(&forum_counter_pda(e)).expect("forum global counter")[8..16].try_into().unwrap());
    let amount = app_burn_amount(e);
    let p = PostCreationData { version: 1, category: "forum".into(), operation: "create_post".into(), creator: wp.to_string(), post_id: id,
        title: "regression post".into(), content: "regression test post".into(), image: "".into() };
    let mut d = disc("create_post");
    d.extend(id.to_le_bytes());
    d.extend(amount.to_le_bytes());
    let ix = Instruction::new_with_bytes(e.forum_prog, &d, vec![
        AccountMeta::new(wp, true), AccountMeta::new(forum_counter_pda(e), false), AccountMeta::new(post_pda(e, id), false),
        AccountMeta::new(e.mint, false), AccountMeta::new(e.ata(&wp), false), AccountMeta::new(e.stats_pda(&wp), false),
        AccountMeta::new_readonly(t22(), false), AccountMeta::new_readonly(e.burn_prog, false), AccountMeta::new_readonly(system_program(), false),
        AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.gbs, false)]);
    e.must("create forum post", &[app_burn_memo(amount, &p.try_to_vec().unwrap()), ix, cu(600_000)], who, &[]);
    id
}

fn blog_mint_memo(e: &Env, minter: &Pubkey) -> Instruction {
    let p = BlogMintData { version: 1, category: "blog".into(), operation: "mint_for_blog".into(), minter: minter.to_string(),
        message: format!("regression mint {}", e.nonce()) };
    app_burn_memo(0, &p.try_to_vec().unwrap())
}

fn blog_mint_ix(e: &Env, minter: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(e.blog_prog, &disc("mint_for_blog"), vec![
        AccountMeta::new(*minter, true), AccountMeta::new(blog_pda(e, minter), false), AccountMeta::new(e.mint, false),
        AccountMeta::new_readonly(e.mint_auth, false), AccountMeta::new(e.ata(minter), false), AccountMeta::new_readonly(t22(), false),
        AccountMeta::new_readonly(e.mint_prog, false), AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.mint_state, false)])
}

fn post_mint_memo(e: &Env, user: &Pubkey, id: u64) -> Instruction {
    let p = PostMintData { version: 1, category: "forum".into(), operation: "mint_for_post".into(), user: user.to_string(), post_id: id,
        message: format!("regression mint {}", e.nonce()) };
    app_burn_memo(0, &p.try_to_vec().unwrap())
}

fn post_mint_ix(e: &Env, user: &Pubkey, id: u64) -> Instruction {
    let mut d = disc("mint_for_post");
    d.extend(id.to_le_bytes());
    Instruction::new_with_bytes(e.forum_prog, &d, vec![
        AccountMeta::new(*user, true), AccountMeta::new(post_pda(e, id), false), AccountMeta::new(e.mint, false),
        AccountMeta::new_readonly(e.mint_auth, false), AccountMeta::new(e.ata(user), false), AccountMeta::new_readonly(t22(), false),
        AccountMeta::new_readonly(e.mint_prog, false), AccountMeta::new_readonly(IXS_SYSVAR, false), AccountMeta::new(e.mint_state, false)])
}

fn main() {
    let e = Env::new("regression-memo-mint");
    let a = &e.wallet;
    let ap = a.pubkey();
    let a_ta = e.ata(&ap);
    let supply = e.supply();
    if curve_reference(supply).is_none() {
        eprintln!("supply {} is at or above the 10T cap; regression-memo-mint needs a lower supply", supply);
        std::process::exit(2);
    }

    // ---------------- setup ----------------
    let b = e.new_user("B", 100 * UNIT, true);
    let bp = b.pubkey();
    let b_ta = e.ata(&bp);
    let watch = [a_ta, b_ta];
    ensure_blog(&e, a);
    let post_id = create_post(&e, a);
    // a second mint whose mint authority IS the memo-mint PDA, and A's token account of it
    let fake = e.new_mint(&e.mint_auth);
    let fake_ta = ata_of(&ap, &fake.pubkey(), &t22());
    e.must("create A's token account of the second mint", &[e.create_ata_ix(&ap, &fake.pubkey())], a, &[]);
    // a zeroed account with MintState's size, owned by memo-mint
    let fake_state = Keypair::new();
    let rent = e.c.get_minimum_balance_for_rent_exemption(113).unwrap();
    e.must("create zeroed 113-byte account owned by memo-mint",
        &[sys_ix::create_account(&ap, &fake_state.pubkey(), rent, 113, &e.mint_prog)], a, &[&fake_state]);
    // a 4-byte account owned by memo-mint (shorter than an account discriminator)
    let tiny_state = Keypair::new();
    let rent4 = e.c.get_minimum_balance_for_rent_exemption(4).unwrap();
    e.must("create 4-byte account owned by memo-mint",
        &[sys_ix::create_account(&ap, &tiny_state.pubkey(), rent4, 4, &e.mint_prog)], a, &[&tiny_state]);
    // the shadow MintState planted by prepare-local-validator: memo-mint-owned, MintState's
    // discriminator, size and the PDA's bump, at an address that is not the PDA
    let shadow = shadow_mint_state();
    match e.raw(&[shadow]).remove(0) {
        Some((owner, _, d)) if owner == e.mint_prog && d.len() == 113 && d[..8] == account_disc("MintState")[..]
            && d[48] == Pubkey::find_program_address(&[b"mint_state"], &e.mint_prog).1 => {}
        other => {
            eprintln!("SETUP FAILED: shadow MintState {} missing or wrong ({:?}); restart the validator with", shadow, other.map(|o| (o.0, o.2.len())));
            eprintln!("scripts/local-validator.sh (prepare-local-validator plants it)");
            std::process::exit(2);
        }
    }

    let one_mint = |ix: u64| Expect::Custom(ix, ONE_MINT_PER_TRANSACTION);
    // At a memo-blog / memo-forum instruction the number alone is ambiguous (6008 is also memo-blog's
    // UnsupportedMemoVersion and memo-forum's InvalidMemoFormat): memo-mint must raise it in the CPI
    let one_mint_cpi = |ix: u64| Expect::Raised(ix, ONE_MINT_PER_TRANSACTION, e.mint_prog, "OneMintPerTransaction");

    // ---------------- one mint per transaction: top level ----------------
    e.reject("M01", "two process_mint in one transaction",
        vec![e.text_memo("two-mints", 69), e.mint_ix(&ap), e.mint_ix(&ap)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M02", "process_mint + process_mint_to(self) in one transaction",
        vec![e.text_memo("mint+mint_to", 69), e.mint_ix(&ap), e.mint_to_ix(&ap, &ap, &a_ta)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M03", "process_mint_to A + process_mint_to B in one transaction",
        vec![e.text_memo("mint_to x2", 69), e.mint_to_ix(&ap, &ap, &a_ta), e.mint_to_ix(&ap, &bp, &b_ta)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M03b", "process_mint_to(B) by A + process_mint by B (two signers) in one transaction",
        vec![e.text_memo("two signers", 69), e.mint_to_ix(&ap, &bp, &b_ta), e.mint_ix(&bp)], a, &[&b], one_mint(2), &watch, &[]);
    {
        let mut ixs = vec![e.text_memo("30 mints", 69)];
        for _ in 0..30 {
            ixs.push(e.mint_ix(&ap));
        }
        ixs.push(cu(1_400_000));
        e.reject("M03c", "30 process_mint in one transaction", ixs, a, &[], one_mint(2), &watch, &[]);
    }

    // ---------------- one mint per transaction: app CPI ----------------
    e.mint_ok("M04a", "control: memo-blog mint_for_blog alone mints the curve amount through CPI",
        vec![blog_mint_memo(&e, &ap), blog_mint_ix(&e, &ap), cu(400_000)], a, &[], &a_ta, &[b_ta]);
    e.reject("M04b", "memo-blog mint_for_blog (CPI mint) + top-level process_mint",
        vec![blog_mint_memo(&e, &ap), blog_mint_ix(&e, &ap), e.mint_ix(&ap), cu(600_000)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M04c", "top-level process_mint + memo-blog mint_for_blog (CPI mint)",
        vec![blog_mint_memo(&e, &ap), e.mint_ix(&ap), blog_mint_ix(&e, &ap), cu(600_000)], a, &[], one_mint_cpi(2), &watch, &[]);
    e.reject("M04d", "memo-blog mint_for_blog twice (two CPI mints)",
        vec![blog_mint_memo(&e, &ap), blog_mint_ix(&e, &ap), blog_mint_ix(&e, &ap), cu(800_000)], a, &[], one_mint_cpi(2), &watch, &[]);
    e.reject("M04d2", "top-level process_mint_to + memo-blog mint_for_blog (CPI mint)",
        vec![blog_mint_memo(&e, &ap), e.mint_to_ix(&ap, &ap, &a_ta), blog_mint_ix(&e, &ap), cu(600_000)], a, &[], one_mint_cpi(2), &watch, &[]);
    e.mint_ok("M04e", "control: memo-forum mint_for_post alone mints the curve amount through CPI",
        vec![post_mint_memo(&e, &ap, post_id), post_mint_ix(&e, &ap, post_id), cu(400_000)], a, &[], &a_ta, &[b_ta]);
    e.reject("M04f", "memo-forum mint_for_post (CPI mint) + top-level process_mint",
        vec![post_mint_memo(&e, &ap, post_id), post_mint_ix(&e, &ap, post_id), e.mint_ix(&ap), cu(600_000)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M04g", "top-level process_mint_to(B) + memo-forum mint_for_post (CPI mint)",
        vec![post_mint_memo(&e, &ap, post_id), e.mint_to_ix(&ap, &bp, &b_ta), post_mint_ix(&e, &ap, post_id), cu(600_000)], a, &[], one_mint_cpi(2), &watch, &[]);

    // ---------------- memo position and length ----------------
    let memo_required = |ix: u64| Expect::Custom(ix, MEMO_REQUIRED);
    e.reject("M05", "ComputeBudget at index 0, valid memo at index 1",
        vec![cu(200_000), e.text_memo("idx1", 69), e.mint_ix(&ap)], a, &[], memo_required(2), &watch, &[]);
    let out = e.reject("M06", "68-byte memo", vec![e.text_memo("68", 68), e.mint_ix(&ap)], a, &[], Expect::Custom(1, MEMO_TOO_SHORT), &watch, &[]);
    too_short_log(&e, "M28a", "M06's log names the bytes (fewer bytes than the minimum)", &out, "Memo too short: 68 bytes");
    e.mint_ok("M07", "69-byte memo (minimum)", vec![e.text_memo("69", 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta]);
    // The minimum counts characters (code points), not bytes
    let memo = fill_bytes(&e, "e69b", 'é', 69);
    let chars = std::str::from_utf8(&memo.data).map(|s| s.chars().count()).unwrap_or(0);
    let out = e.reject("M06b", "69-byte memo of 2-byte characters (fewer than 69 characters)",
        vec![memo, e.mint_ix(&ap)], a, &[], Expect::Custom(1, MEMO_TOO_SHORT), &watch, &[]);
    too_short_log(&e, "M28b", "M06b's log names the characters counted (69 bytes, fewer characters)", &out,
        &format!("Memo too short: {} characters (minimum: 69)", chars));
    e.reject("M06c", "68-character memo of 2-byte characters",
        vec![fill_chars(&e, "e68c", 'é', 68), e.mint_ix(&ap)], a, &[], Expect::Custom(1, MEMO_TOO_SHORT), &watch, &[]);
    e.mint_ok("M07b", "69-character memo of 2-byte characters (minimum)",
        vec![fill_chars(&e, "e69c", 'é', 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M07c", "69-character memo of 3-byte characters",
        vec![fill_chars(&e, "z69c", '中', 69), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.reject("M08", "no memo", vec![e.mint_ix(&ap)], a, &[], memo_required(0), &watch, &[]);
    e.reject("M09", "System transfer at index 0, valid memo at index 1",
        vec![sys_ix::transfer(&ap, &ap, 0), e.text_memo("sys0", 69), e.mint_ix(&ap)], a, &[], memo_required(2), &watch, &[]);
    e.reject("M09b", "legacy Memo v1 program at index 0 (80 bytes)",
        vec![Instruction::new_with_bytes(LEGACY_MEMO_V1, format!("regression {} legacy memo v1 {}", e.nonce(), "x".repeat(60)).as_bytes(), vec![]), e.mint_ix(&ap)],
        a, &[], memo_required(1), &watch, &[]);
    e.reject("M10", "process_mint at index 0, memo at index 1",
        vec![e.mint_ix(&ap), e.text_memo("after", 69)], a, &[], memo_required(0), &watch, &[]);
    e.reject("M10b", "empty memo at index 0", vec![memo_ix(b""), e.mint_ix(&ap)], a, &[], Expect::Custom(1, MEMO_TOO_SHORT), &watch, &[]);
    e.reject("M10c", "68-byte memo at index 0, valid memo at index 1",
        vec![e.text_memo("short0", 68), e.text_memo("valid1", 69), e.mint_ix(&ap)], a, &[], Expect::Custom(2, MEMO_TOO_SHORT), &watch, &[]);

    // ---------------- account substitution ----------------
    {
        let mut acc = e.mint_accounts(&ap, &fake_ta);
        acc[1] = AccountMeta::new(fake.pubkey(), false);
        e.reject("M11", "second mint whose authority is the memo-mint PDA, via process_mint",
            vec![e.text_memo("fakemint", 69), e.mint_ix_with(acc.clone())], a, &[], Expect::Custom(1, UNAUTHORIZED_MINT), &watch, &[]);
        e.reject("M11b", "same second mint via process_mint_to",
            vec![e.text_memo("fakemint2", 69), e.mint_to_ix_with(&ap, acc)], a, &[], Expect::Custom(1, UNAUTHORIZED_MINT), &watch, &[]);
        let fake_supply = e.data(&fake.pubkey()).map(|d| u64::from_le_bytes(d[36..44].try_into().unwrap()));
        e.record("M11c", "the second mint's supply stays 0", fake_supply == Some(0), &format!("supply {:?}", fake_supply));
    }
    let custom1 = |code: u32| Expect::Custom(1, code);
    e.reject("M12", "process_mint signed by A with B's token account",
        vec![e.text_memo("other-ta", 69), e.mint_ix_with(e.mint_accounts(&ap, &b_ta))], a, &[], custom1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &[]);
    e.reject("M12b", "process_mint with A's token account of the second mint",
        vec![e.text_memo("wrong-mint-ta", 69), e.mint_ix_with(e.mint_accounts(&ap, &fake_ta))], a, &[], custom1(INVALID_TOKEN_ACCOUNT), &watch, &[]);
    e.reject("M12c", "process_mint_to(A) with A's token account of the second mint",
        vec![e.text_memo("wrong-mint-rta", 69), e.mint_to_ix(&ap, &ap, &fake_ta)], a, &[], custom1(INVALID_TOKEN_ACCOUNT), &watch, &[]);
    e.reject("M13", "process_mint_to with recipient B but A's token account",
        vec![e.text_memo("rcpt-mismatch", 69), e.mint_to_ix(&ap, &bp, &a_ta)], a, &[], custom1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &[]);
    e.reject("M13b", "process_mint_to with recipient A but B's token account",
        vec![e.text_memo("rcpt-mismatch2", 69), e.mint_to_ix(&ap, &ap, &b_ta)], a, &[], custom1(UNAUTHORIZED_TOKEN_ACCOUNT), &watch, &[]);
    let with = |i: usize, m: AccountMeta| {
        let mut acc = e.mint_accounts(&ap, &a_ta);
        acc[i] = m;
        acc
    };
    e.reject("M14", "random account as instructions sysvar",
        vec![e.text_memo("fake-sysvar", 69), e.mint_ix_with(with(5, AccountMeta::new_readonly(Pubkey::new_unique(), false)))],
        a, &[], custom1(ANCHOR_CONSTRAINT_ADDRESS), &watch, &[]);
    e.reject("M14b", "Clock sysvar as instructions sysvar",
        vec![e.text_memo("clock-sysvar", 69), e.mint_ix_with(with(5, AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)))],
        a, &[], custom1(ANCHOR_CONSTRAINT_ADDRESS), &watch, &[]);
    e.reject("M15", "legacy SPL Token program as token_program",
        vec![e.text_memo("legacy-token", 69), e.mint_ix_with(with(4, AccountMeta::new_readonly(SPL_TOKEN_LEGACY, false)))],
        a, &[], custom1(ANCHOR_INVALID_PROGRAM_ID), &watch, &[]);
    e.reject("M16", "nonexistent account as mint_state",
        vec![e.text_memo("ms-random", 69), e.mint_ix_with(with(6, AccountMeta::new(Pubkey::new_unique(), false)))],
        a, &[], custom1(ANCHOR_ACCOUNT_NOT_INITIALIZED), &watch, &[]);
    e.reject("M16b", "zeroed memo-mint-owned account as mint_state",
        vec![e.text_memo("ms-zero", 69), e.mint_ix_with(with(6, AccountMeta::new(fake_state.pubkey(), false)))],
        a, &[], custom1(ANCHOR_DISCRIMINATOR_MISMATCH), &watch, &[]);
    e.reject("M16c", "memo-burn GlobalBurnStats as mint_state",
        vec![e.text_memo("ms-gbs", 69), e.mint_ix_with(with(6, AccountMeta::new(e.gbs, false)))],
        a, &[], custom1(ANCHOR_OWNED_BY_WRONG_PROGRAM), &watch, &[]);
    e.reject("M16d", "mint_state passed read-only",
        vec![e.text_memo("ms-ro", 69), e.mint_ix_with(with(6, AccountMeta::new_readonly(e.mint_state, false)))],
        a, &[], custom1(ANCHOR_CONSTRAINT_MUT), &watch, &[]);
    {
        let mut acc = e.mint_accounts(&bp, &b_ta);
        acc[0] = AccountMeta::new(bp, false);
        e.reject("M16e", "user B not a signer (fee payer A)", vec![e.text_memo("not-signer", 69), e.mint_ix_with(acc)],
            a, &[], custom1(ANCHOR_ACCOUNT_NOT_SIGNER), &watch, &[]);
    }
    {
        let other_pda = Pubkey::find_program_address(&[b"mint_authority"], &e.burn_prog).0;
        e.reject("M16f", "mint_authority PDA of another program (same seeds)",
            vec![e.text_memo("auth-other", 69), e.mint_ix_with(with(2, AccountMeta::new_readonly(other_pda, false)))],
            a, &[], custom1(ANCHOR_CONSTRAINT_SEEDS), &watch, &[]);
    }

    // ---------------- admin-only initialization (state exists) ----------------
    e.reject("M17a", "non-admin initialize_mint_state while MintState exists",
        vec![Instruction::new_with_bytes(e.mint_prog, &disc("initialize_mint_state"), vec![
            AccountMeta::new(ap, true), AccountMeta::new(e.mint_state, false), AccountMeta::new_readonly(system_program(), false)])],
        a, &[], Expect::RaisedBy(0, SYSTEM_ACCOUNT_IN_USE, system_program()), &watch, &[]);

    // ---------------- allowed operations, exact amounts ----------------
    e.mint_ok("M18a", "successive mint #1", vec![e.text_memo("seq1", 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M18b", "successive mint #2", vec![e.text_memo("seq2", 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M19", "A calls process_mint_to for B (B receives, A unchanged)",
        vec![e.text_memo("mint_to B", 69), e.mint_to_ix(&ap, &bp, &b_ta)], a, &[], &b_ta, &[a_ta]);
    e.mint_ok("M19b", "ComputeBudget between memo and mint, 20 extra accounts appended",
        vec![e.text_memo("cb-mid", 69), cu(300_000), {
            let mut acc = e.mint_accounts(&ap, &a_ta);
            acc.extend((0..20).map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false)));
            e.mint_ix_with(acc)
        }], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M19c", "trailing instruction data (ignored)",
        vec![e.text_memo("extra-data", 69), {
            let mut d = disc("process_mint");
            d.extend(u64::MAX.to_le_bytes());
            Instruction::new_with_bytes(e.mint_prog, &d, e.mint_accounts(&ap, &a_ta))
        }], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M19d", "780-byte memo (no maximum memo length)",
        vec![e.text_memo("long", 780), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M19e", "memo-blog mint_for_blog in a separate transaction after top-level mints",
        vec![blog_mint_memo(&e, &ap), blog_mint_ix(&e, &ap), cu(400_000)], a, &[], &a_ta, &[b_ta]);

    // ---------------- durable nonce (INT-04) ----------------
    {
        let nonce = Keypair::new();
        let rent = e.c.get_minimum_balance_for_rent_exemption(80).unwrap();
        e.must("create durable nonce account", &sys_ix::create_nonce_account(&ap, &nonce.pubkey(), &ap, rent), a, &[&nonce]);
        let d = e.data(&nonce.pubkey()).unwrap();
        let nonce_hash = Hash::new_from_array(d[40..72].try_into().unwrap());
        let ixs = vec![sys_ix::advance_nonce_account(&nonce.pubkey(), &ap), e.text_memo("nonce", 69), e.mint_ix(&ap)];
        let tx = e.build(&ixs, a, &[], nonce_hash);
        let before = e.snap(&watch, &[]);
        let out = e.send_tx(&tx);
        let after = e.snap(&watch, &[]);
        let pass = memo_required(2).matches(&out) && before == after;
        e.record("X3a", "durable-nonce transaction [AdvanceNonce, memo, process_mint] is rejected (memo not at index 0)", pass,
            &format!("{}; {}", out.brief(), before.diff(&after)));
    }

    // From here on every rejection with a custom error names the failing program and Anchor error
    // (Expect::Raised; a non-custom error is matched exactly, with the failing program,
    // Expect::BuiltinBy), and most also check that no account the transaction references changed
    // (reject_untouched).
    let raised = |ix: u64, code: u32, name: &'static str| Expect::Raised(ix, code, e.mint_prog, name);

    // ---------------- process_mint_to account substitution (S50) ----------------
    // ProcessMintTo is an Accounts struct of its own: the checks of M14-M16f, through process_mint_to
    let with_to = |i: usize, m: AccountMeta| {
        let mut acc = e.mint_accounts(&ap, &a_ta);
        acc[i] = m;
        e.mint_to_ix_with(&ap, acc)
    };
    e.reject_untouched("M20a", "process_mint_to with a random account as instructions sysvar",
        vec![e.text_memo("to-fake-sysvar", 69), with_to(5, AccountMeta::new_readonly(Pubkey::new_unique(), false))],
        a, &[], raised(1, ANCHOR_CONSTRAINT_ADDRESS, "ConstraintAddress"), &watch, &[]);
    // (reject, not reject_untouched: the Clock sysvar changes every slot)
    e.reject("M20b", "process_mint_to with the Clock sysvar as instructions sysvar",
        vec![e.text_memo("to-clock-sysvar", 69), with_to(5, AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false))],
        a, &[], raised(1, ANCHOR_CONSTRAINT_ADDRESS, "ConstraintAddress"), &watch, &[]);
    e.reject_untouched("M20c", "process_mint_to with the legacy SPL Token program as token_program",
        vec![e.text_memo("to-legacy-token", 69), with_to(4, AccountMeta::new_readonly(SPL_TOKEN_LEGACY, false))],
        a, &[], raised(1, ANCHOR_INVALID_PROGRAM_ID, "InvalidProgramId"), &watch, &[]);
    e.reject_untouched("M20d", "process_mint_to with a nonexistent account as mint_state",
        vec![e.text_memo("to-ms-random", 69), with_to(6, AccountMeta::new(Pubkey::new_unique(), false))],
        a, &[], raised(1, ANCHOR_ACCOUNT_NOT_INITIALIZED, "AccountNotInitialized"), &watch, &[]);
    e.reject_untouched("M20e", "process_mint_to with a zeroed memo-mint-owned account as mint_state",
        vec![e.text_memo("to-ms-zero", 69), with_to(6, AccountMeta::new(fake_state.pubkey(), false))],
        a, &[], raised(1, ANCHOR_DISCRIMINATOR_MISMATCH, "AccountDiscriminatorMismatch"), &watch, &[]);
    e.reject_untouched("M20f", "process_mint_to with memo-burn GlobalBurnStats as mint_state",
        vec![e.text_memo("to-ms-gbs", 69), with_to(6, AccountMeta::new(e.gbs, false))],
        a, &[], raised(1, ANCHOR_OWNED_BY_WRONG_PROGRAM, "AccountOwnedByWrongProgram"), &watch, &[]);
    e.reject_untouched("M20g", "process_mint_to with mint_state passed read-only",
        vec![e.text_memo("to-ms-ro", 69), with_to(6, AccountMeta::new_readonly(e.mint_state, false))],
        a, &[], raised(1, ANCHOR_CONSTRAINT_MUT, "ConstraintMut"), &watch, &[]);
    {
        let mut acc = e.mint_accounts(&bp, &b_ta);
        acc[0] = AccountMeta::new(bp, false);
        e.reject_untouched("M20h", "process_mint_to with caller B not a signer (fee payer A)",
            vec![e.text_memo("to-not-signer", 69), e.mint_to_ix_with(&bp, acc)],
            a, &[], raised(1, ANCHOR_ACCOUNT_NOT_SIGNER, "AccountNotSigner"), &watch, &[]);
    }
    {
        let other_pda = Pubkey::find_program_address(&[b"mint_authority"], &e.burn_prog).0;
        e.reject_untouched("M20i", "process_mint_to with the mint_authority PDA of another program (same seeds)",
            vec![e.text_memo("to-auth-other", 69), with_to(2, AccountMeta::new_readonly(other_pda, false))],
            a, &[], raised(1, ANCHOR_CONSTRAINT_SEEDS, "ConstraintSeeds"), &watch, &[]);
    }

    // ---------------- shadow MintState (seeds constraint, S51) ----------------
    // Account<MintState> only checks owner and discriminator; `seeds = [b"mint_state"], bump =
    // mint_state.bump` is what ties mint_state to the PDA. Without it, a second MintState would
    // allow a second mint in one transaction. Nothing on a real network can create one (memo-mint
    // creates MintState only at the PDA), so prepare-local-validator plants it here.
    let seeds = |ix: u64| raised(ix, ANCHOR_CONSTRAINT_SEEDS, "ConstraintSeeds");
    e.reject_untouched("M21a", "process_mint with the shadow MintState",
        vec![e.text_memo("shadow", 69), e.mint_ix_with(with(6, AccountMeta::new(shadow, false)))], a, &[], seeds(1), &watch, &[]);
    e.reject_untouched("M21b", "process_mint (MintState) + process_mint (shadow MintState) in one transaction: nothing minted",
        vec![e.text_memo("shadow2", 69), e.mint_ix(&ap), e.mint_ix_with(with(6, AccountMeta::new(shadow, false)))],
        a, &[], seeds(2), &watch, &[]);
    e.reject_untouched("M21c", "process_mint_to with the shadow MintState",
        vec![e.text_memo("to-shadow", 69), with_to(6, AccountMeta::new(shadow, false))], a, &[], seeds(1), &watch, &[]);
    e.reject_untouched("M21d", "process_mint_to (MintState) + process_mint_to (shadow MintState) in one transaction: nothing minted",
        vec![e.text_memo("to-shadow2", 69), e.mint_to_ix(&ap, &ap, &a_ta), with_to(6, AccountMeta::new(shadow, false))],
        a, &[], seeds(2), &watch, &[]);

    // ---------------- read-only accounts (`mut` constraints, S52) ----------------
    // A read-only mint or token account would also make the Token-2022 mint_to fail, but with
    // another error; the account-meta contract is pinned here: mint, token account and MintState
    // writable, user / caller read-only allowed (memo-mint never writes them, MINT-12 / D5)
    let mut_err = raised(1, ANCHOR_CONSTRAINT_MUT, "ConstraintMut");
    e.reject_untouched("M22a", "process_mint with the MEMO mint read-only",
        vec![e.text_memo("mint-ro", 69), e.mint_ix_with(with(1, AccountMeta::new_readonly(e.mint, false)))], a, &[], mut_err, &watch, &[]);
    e.reject_untouched("M22b", "process_mint with the token account read-only",
        vec![e.text_memo("ta-ro", 69), e.mint_ix_with(with(3, AccountMeta::new_readonly(a_ta, false)))], a, &[], mut_err, &watch, &[]);
    {
        let mut acc = e.mint_accounts(&bp, &b_ta);
        acc[0] = AccountMeta::new_readonly(bp, true);
        e.mint_ok("M22c", "process_mint with user B read-only (B signs, fee payer A): accepted",
            vec![e.text_memo("user-ro", 69), e.mint_ix_with(acc)], a, &[&b], &b_ta, &[]);
    }
    e.reject_untouched("M22d", "process_mint_to with the MEMO mint read-only",
        vec![e.text_memo("to-mint-ro", 69), with_to(1, AccountMeta::new_readonly(e.mint, false))], a, &[], mut_err, &watch, &[]);
    e.reject_untouched("M22e", "process_mint_to with the recipient token account read-only",
        vec![e.text_memo("to-ta-ro", 69), with_to(3, AccountMeta::new_readonly(a_ta, false))], a, &[], mut_err, &watch, &[]);
    {
        let mut acc = e.mint_accounts(&bp, &b_ta);
        acc[0] = AccountMeta::new_readonly(bp, true);
        e.mint_ok("M22f", "process_mint_to with caller B read-only (B signs, fee payer A): accepted",
            vec![e.text_memo("to-caller-ro", 69), e.mint_to_ix_with(&bp, acc)], a, &[&b], &b_ta, &[]);
    }

    // ---------------- Anchor framework rejections (S53) ----------------
    {
        let mut acc = e.mint_accounts(&ap, &a_ta);
        acc.truncate(6);
        e.reject_untouched("M23a", "process_mint with the pre-Revision-3 account list (no mint_state)",
            vec![e.text_memo("old-accounts", 69), e.mint_ix_with(acc.clone())], a, &[],
            raised(1, ANCHOR_ACCOUNT_NOT_ENOUGH_KEYS, "AccountNotEnoughKeys"), &watch, &[]);
        e.reject_untouched("M23b", "process_mint_to with the pre-Revision-3 account list (no mint_state)",
            vec![e.text_memo("to-old-accounts", 69), e.mint_to_ix_with(&ap, acc)], a, &[],
            raised(1, ANCHOR_ACCOUNT_NOT_ENOUGH_KEYS, "AccountNotEnoughKeys"), &watch, &[]);
    }
    {
        let mut ix = e.mint_to_ix(&ap, &ap, &a_ta);
        ix.data.truncate(8 + 31);
        e.reject_untouched("M23c", "process_mint_to with a 31-byte recipient argument",
            vec![e.text_memo("to-short-arg", 69), ix], a, &[], raised(1, ANCHOR_INSTRUCTION_DID_NOT_DESERIALIZE, "InstructionDidNotDeserialize"), &watch, &[]);
    }
    e.reject_untouched("M23d", "unknown instruction discriminator (8 zero bytes)",
        vec![e.text_memo("bad-disc", 69), Instruction::new_with_bytes(e.mint_prog, &[0u8; 8], e.mint_accounts(&ap, &a_ta))], a, &[],
        raised(1, ANCHOR_INSTRUCTION_FALLBACK_NOT_FOUND, "InstructionFallbackNotFound"), &watch, &[]);
    e.reject_untouched("M23e", "a System-owned account (wallet A) as the mint",
        vec![e.text_memo("mint-system", 69), e.mint_ix_with(with(1, AccountMeta::new(ap, false)))], a, &[],
        raised(1, ANCHOR_OWNED_BY_WRONG_PROGRAM, "AccountOwnedByWrongProgram"), &watch, &[]);
    // anchor-spl unpacks InterfaceAccount<TokenAccount> with spl-token-2022's StateWithExtensions
    // inside memo-mint, so the mint fails there, before any CPI, with
    // ProgramError::UninitializedAccount rather than AccountDidNotDeserialize. Token-2022's
    // mint_to would raise the same error for this account, so the case also pins the program.
    e.reject_untouched("M23f", "the MEMO mint as the token account",
        vec![e.text_memo("mint-as-ta", 69), e.mint_ix_with(with(3, AccountMeta::new(e.mint, false)))], a, &[],
        Expect::BuiltinBy(1, "UninitializedAccount", e.mint_prog), &watch, &[]);
    e.reject_untouched("M23g", "a 4-byte memo-mint-owned account as mint_state (shorter than a discriminator)",
        vec![e.text_memo("ms-tiny", 69), e.mint_ix_with(with(6, AccountMeta::new(tiny_state.pubkey(), false)))], a, &[],
        raised(1, ANCHOR_DISCRIMINATOR_NOT_FOUND, "AccountDiscriminatorNotFound"), &watch, &[]);

    // ---------------- Revision 5: the character minimum at its encoding boundaries ----------------
    // At least 69 characters (code points) of the memo, which the Memo program at index 0 has
    // already checked to be valid UTF-8. validate_memo_length: under 69 bytes too short; 273 bytes
    // (more than 4 x 68) or more enough without counting; all ASCII enough; otherwise it counts the bytes
    // that are not UTF-8 continuation bytes. Memos of emoji such as '🌍' need the 1.4M CU limit in
    // the Memo program (about 1.19M CU for 69 of them; plane-2 ideographs cost about 20k); the
    // other cases reuse the same limit.
    let too_short = raised(1, MEMO_TOO_SHORT, "MemoTooShort");
    let earth = |n: usize| "🌍".repeat(n);
    let memo_of = |s: &str, chars: usize, bytes: usize| {
        assert_eq!((s.chars().count(), s.len()), (chars, bytes), "memo {:?}", s);
        memo_ix(s.as_bytes())
    };
    let out = e.reject_untouched("M24a", "68 four-byte characters (272 bytes, the longest memo that is counted)",
        vec![memo_of(&format!("{}{}", plane2_char(&e), earth(67)), 68, 272), e.mint_ix(&ap), cu(1_400_000)], a, &[], too_short, &watch, &[]);
    too_short_log(&e, "M28c", "M24a's log names the characters counted (68 of 272 bytes)", &out, "Memo too short: 68 characters (minimum: 69)");
    e.mint_ok("M24b", "67 four-byte characters + 4 ASCII (272 bytes, 71 characters: counted)",
        vec![memo_of(&format!("{}{}abcd", plane2_char(&e), earth(66)), 71, 272), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M24c", "69 four-byte characters (276 bytes: accepted by length without counting)",
        vec![memo_of(&format!("{}{}", plane2_char(&e), earth(68)), 69, 276), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M24d", "68 x '🌍' + 'x' (273 bytes, 69 characters: the shortest memo accepted by length without counting)",
        vec![memo_of(&format!("{}x", earth(68)), 69, 273), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M24e", "35 'e' + 34 combining acute accents (69 code points, 35 visible letters, 103 bytes)",
        vec![memo_of(&format!("{}e", "e\u{301}".repeat(34)), 69, 103), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.reject_untouched("M24f", "34 'e' + 34 combining acute accents (68 code points, 102 bytes)",
        vec![memo_of(&"e\u{301}".repeat(34), 68, 102), e.mint_ix(&ap), cu(1_400_000)], a, &[], too_short, &watch, &[]);
    e.mint_ok("M24g", "69 zero-width spaces U+200B (invisible, 207 bytes)",
        vec![memo_of(&"\u{200B}".repeat(69), 69, 207), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M24h", "69 NUL characters (ASCII)",
        vec![memo_of(&"\0".repeat(69), 69, 69), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M24i", "'é' + 68 NUL (70 bytes, 69 characters: counted, NUL is a character)",
        vec![memo_of(&format!("é{}", "\0".repeat(68)), 69, 70), e.mint_ix(&ap), cu(1_400_000)], a, &[], &a_ta, &[b_ta]);
    let mut bad = vec![0xFFu8];
    bad.extend([b'a'; 68]);
    e.reject_untouched("M24j", "invalid UTF-8 at index 0 (0xFF + 68 'a'): the Memo program rejects it",
        vec![memo_ix(&bad), e.mint_ix(&ap), cu(1_400_000)], a, &[],
        Expect::BuiltinBy(0, "InvalidInstructionData", spl_memo::ID), &watch, &[]);
    e.reject_untouched("M24k", "276 continuation bytes 0x80 at index 0 (memo-mint's length fast path alone would accept them): the Memo program rejects them",
        vec![memo_ix(&[0x80u8; 276]), e.mint_ix(&ap), cu(1_400_000)], a, &[],
        Expect::BuiltinBy(0, "InvalidInstructionData", spl_memo::ID), &watch, &[]);
    // Mostly ASCII and still too short: the ASCII fast path must cover the whole memo (a check of
    // only its first or last half would accept these)
    let ascii = |n: usize| {
        let mut s = format!("r{}:", e.nonce());
        while s.len() < n {
            s.push('x');
        }
        s
    };
    e.reject_untouched("M24l", "46 ASCII + 12 'é' (70 bytes, 58 characters; the first half is ASCII)",
        vec![memo_of(&format!("{}{}", ascii(46), "é".repeat(12)), 58, 70), e.mint_ix(&ap)], a, &[], too_short, &watch, &[]);
    e.reject_untouched("M24m", "12 'é' + 46 ASCII (70 bytes, 58 characters; the second half is ASCII)",
        vec![memo_of(&format!("{}{}", "é".repeat(12), ascii(46)), 58, 70), e.mint_ix(&ap)], a, &[], too_short, &watch, &[]);
    // The fast paths exist for compute units, so a change that drops one accepts and rejects the
    // same memos and shows only in CU: memo-mint's own CU in simulated mints (sigVerify off,
    // nothing lands). On the final Revision 5, counting a 272-byte memo costs about 530 CU (8 bytes
    // at a time) and the ASCII scan about 1 CU per byte; on the length fast path a longer memo
    // costs about 0.5 CU per byte more (hashing and copying the longer sysvar).
    //   M24n: the counted memo costs about 530 more; without the ASCII fast path, about the same
    //   M24o: about +210; without the length fast path both are counted, about +1,400
    //   M24p: about 780 more at 272 bytes than at 273; if the length fast path started later, about
    //         the same
    let relations: [(&str, &str, String, String, fn(u64, u64) -> bool); 3] = [
        ("M24n", "ASCII fast path: 270 ASCII + 'é' (272 bytes, counted) costs memo-mint at least 300 CU more than 272 ASCII bytes (simulated)",
            ascii(272), format!("{}é", ascii(270)), |a, b| b >= a + 300),
        ("M24o", "length fast path: memo-mint's CU for 699 ASCII + 'é' (701 bytes) is within 1,000 of 275 ASCII + 'é' (277 bytes) (simulated)",
            format!("{}é", ascii(275)), format!("{}é", ascii(699)), |a, b| a.abs_diff(b) < 1_000),
        ("M24p", "the length fast path starts at 273 bytes: 270 ASCII + 'é' (272 bytes, counted) costs memo-mint at least 300 CU more than 271 ASCII + 'é' (273 bytes) (simulated)",
            format!("{}é", ascii(271)), format!("{}é", ascii(270)), |a, b| b >= a + 300),
    ];
    for (id, desc, first, second, relation) in relations {
        let sim = |memo: &str| e.simulate_unsigned(&[memo_ix(memo.as_bytes()), e.mint_ix(&ap), cu(1_400_000)], &ap, &[]).0;
        let (o1, o2) = (sim(&first), sim(&second));
        let (c1, c2) = (o1.consumed(&e.mint_prog), o2.consumed(&e.mint_prog));
        let pass = o1.ok() && o2.ok() && o1.minted_log_count() == 1 && o2.minted_log_count() == 1
            && matches!((c1, c2), (Some(x), Some(y)) if relation(x, y));
        e.record(id, desc, pass, &format!("{} bytes: {} memo-mint CU {:?}; {} bytes: {} memo-mint CU {:?}",
            first.len(), o1.brief(), c1, second.len(), o2.brief(), c2));
        println!("     CU memo-mint: {:?} ({} bytes) / {:?} ({} bytes)", c1, first.len(), c2, second.len());
    }

    // ---------------- reserved keys and v0 transactions (S07, MEMO-CORE-AUDIT 8.2) ----------------
    // The runtime demotes a write lock requested on a reserved key (sysvars, builtin programs) to
    // read-only, in the instructions sysvar memo-mint hashes too, and resolves v0 accounts through
    // the lookup table; the harness fingerprint is built the same way, so mint_ok checks
    // MintState = (fingerprint, slot) exactly in both cases.
    // The demotion cases are not vacuous: without demotion their fingerprint differs (the harness
    // before S07 would have reported a false FAIL)
    let demoted = |ixs: &[Instruction], alt: Option<&AddressLookupTableAccount>| {
        let (msg, loaded) = match alt {
            None => (VersionedMessage::Legacy(Message::new(ixs, Some(&ap))), LoadedAddresses::default()),
            Some(alt) => {
                let (tx, loaded) = e.build_v0(ixs, a, &[], alt, Hash::default());
                (tx.message, loaded)
            }
        };
        if fingerprint_versioned(&msg, &loaded) == fingerprint_with_reserved(&msg, &loaded, &HashSet::new()) {
            eprintln!("SETUP FAILED: no reserved key is requested writable in {:?}", ixs);
            std::process::exit(2);
        }
        ixs.to_vec()
    };
    e.mint_ok("M25a", "instructions sysvar requested writable (demoted to read-only by the runtime)",
        demoted(&[e.text_memo("ixs-writable", 69), e.mint_ix_with(with(5, AccountMeta::new(IXS_SYSVAR, false)))], None), a, &[], &a_ta, &[b_ta]);
    e.mint_ok("M25b", "Clock sysvar requested writable as an extra account (demoted)",
        demoted(&[e.text_memo("clock-writable", 69), {
            let mut acc = e.mint_accounts(&ap, &a_ta);
            acc.push(AccountMeta::new(solana_sdk::sysvar::clock::ID, false));
            e.mint_ix_with(acc)
        }], None), a, &[], &a_ta, &[b_ta]);
    let alt = e.create_alt(&[e.mint, e.mint_auth, a_ta, t22(), IXS_SYSVAR, e.mint_state]);
    {
        // every mint account but the user (a signer) comes from the table
        let (_, loaded) = e.build_v0(&[e.text_memo("probe", 69), e.mint_ix(&ap)], a, &[], &alt, Hash::default());
        if (loaded.writable.len(), loaded.readonly.len()) != (3, 3) {
            eprintln!("SETUP FAILED: the v0 mint loads {:?} from the lookup table, expected 3 writable and 3 read-only", loaded);
            std::process::exit(2);
        }
    }
    e.mint_ok_v0("M25c", "v0 transaction: mint, mint authority, token account, Token-2022, instructions sysvar and MintState from a lookup table",
        vec![e.text_memo("v0", 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta], &alt);
    e.mint_ok_v0("M25d", "v0 transaction with the instructions sysvar loaded writable from the lookup table (demoted)",
        demoted(&[e.text_memo("v0-ixs-writable", 69), e.mint_ix_with(with(5, AccountMeta::new(IXS_SYSVAR, false)))], Some(&alt)),
        a, &[], &a_ta, &[b_ta], &alt);
    e.reject_v0("M25e", "v0 transaction with two process_mint",
        vec![e.text_memo("v0-two-mints", 69), e.mint_ix(&ap), e.mint_ix(&ap)], a, &[],
        raised(2, ONE_MINT_PER_TRANSACTION, "OneMintPerTransaction"), &watch, &[], &alt);
    e.reject_v0("M25f", "v0 transaction with MintState loaded read-only from the lookup table",
        vec![e.text_memo("v0-ms-ro", 69), e.mint_ix_with(with(6, AccountMeta::new_readonly(e.mint_state, false)))], a, &[],
        raised(1, ANCHOR_CONSTRAINT_MUT, "ConstraintMut"), &watch, &[], &alt);

    // ---------------- two transactions mint in one slot (MM02 / MM03) ----------------
    // record_mint rejects only the same fingerprint in the same slot: two different transactions
    // in one slot must both mint, and MintState then holds the later one (block order). Sent back
    // to back with one blockhash until both land in one slot (on the local validator nearly always
    // the first attempt); if they never do, the case is skipped loudly (a skip fails the suite
    // unless MEMO_REGRESSION_ALLOW_SKIP=1).
    {
        const ATTEMPTS: u32 = 20;
        let mut done = false;
        for attempt in 1..=ATTEMPTS {
            let bh = e.c.get_latest_blockhash().expect("blockhash");
            let t1 = e.build(&[e.text_memo("slot-1", 69), e.mint_ix(&ap)], a, &[], bh);
            let t2 = e.build(&[e.text_memo("slot-2", 69), e.mint_to_ix(&ap, &bp, &b_ta)], a, &[], bh);
            let before = e.snap(&watch, &[]);
            let (o1, o2) = match (e.send(&t1), e.send(&t2)) {
                (Ok(s1), Ok(s2)) => (e.wait(&s1), e.wait(&s2)),
                (r1, r2) => {
                    // let the one that was sent land before the next attempt's snapshot
                    for s in [r1, r2].into_iter().flatten() {
                        e.wait(&s);
                    }
                    println!("     M26 attempt {}: the RPC refused a transaction", attempt);
                    continue;
                }
            };
            if o1.slot != o2.slot || !o1.landed || !o2.landed {
                println!("     M26 attempt {}: slots {} / {}", attempt, o1.slot, o2.slot);
                continue;
            }
            let after = e.snap(&watch, &[]);
            let order = e.block_signatures(o1.slot);
            let (p1, p2) = (order.iter().position(|s| *s == o1.sig), order.iter().position(|s| *s == o2.sig));
            let one_first = p1 < p2;
            let r_first = curve_reference(before.supply).unwrap_or(0);
            let r_second = curve_reference(before.supply + r_first).unwrap_or(0);
            let (r1, r2) = if one_first { (r_first, r_second) } else { (r_second, r_first) };
            let later_fp = fingerprint(if one_first { &t2.message } else { &t1.message });
            let (sb1, sb2) = if one_first { (before.supply, before.supply + r_first) } else { (before.supply + r_first, before.supply) };
            let pass = o1.ok() && o2.ok() && p1.is_some() && p2.is_some()
                && o1.minted_log_count() == 1 && o2.minted_log_count() == 1
                && o1.logs.contains(&minted_log(r1, sb1)) && o2.logs.contains(&minted_log(r2, sb2))
                && after.bal(&a_ta) == before.bal(&a_ta) + r1 && after.bal(&b_ta) == before.bal(&b_ta) + r2
                && after.supply == before.supply + r1 + r2 && after.mint_state == Some((later_fp, o1.slot));
            e.record("M26", &format!("two different transactions both mint in one slot (slot {}, attempt {}); MintState holds the later one",
                o1.slot, attempt), pass, &format!("{} / {}; block order {:?} / {:?}; {}", o1.brief(), o2.brief(), p1, p2, before.diff(&after)));
            done = true;
            break;
        }
        if !done {
            e.skip("M26", &format!("skipped: different slots - the two mints never landed in one slot in {} attempts", ATTEMPTS));
        }
    }

    // ---------------- the same instructions in a later slot (MM01 / NM11) ----------------
    // The fingerprint covers the instructions, not the blockhash or the signatures, so a client
    // that sends the same memo and accounts again (memo-miner changes its memo once per second)
    // sends the same fingerprint again; record_mint must reject it only in the same slot
    {
        let ixs = vec![e.text_memo("again", 69), e.mint_ix(&ap)];
        e.mint_ok("M27a", "[memo, process_mint]", ixs.clone(), a, &[], &a_ta, &[b_ta]);
        let first = e.mint_state_value();
        let s1 = first.map(|f| f.1).unwrap_or(0);
        // two slots later the latest blockhash is one the first transaction did not use
        let start = Instant::now();
        while e.c.get_slot_with_commitment(CommitmentConfig::confirmed()).unwrap_or(0) <= s1 + 1 && start.elapsed() < Duration::from_secs(60) {
            sleep(Duration::from_millis(100));
        }
        e.mint_ok("M27b", "the same [memo, process_mint] again in a later slot mints again", ixs, a, &[], &a_ta, &[b_ta]);
        let second = e.mint_state_value();
        e.record("M27c", "  ... the two transactions have the same fingerprint, the second a later slot",
            matches!((first, second), (Some((f1, s1)), Some((f2, s2))) if f1 == f2 && s2 > s1),
            &format!("MintState slot {:?} -> {:?}, same fingerprint {}", first.map(|f| f.1), second.map(|f| f.1),
                first.map(|f| f.0) == second.map(|f| f.0)));
    }

    e.finish();
}

/// The MemoTooShort log line of a rejected mint (D3 / O4) must be exactly `line`: the bytes when the
/// memo has fewer bytes than the minimum, else the characters counted
fn too_short_log(e: &Env, id: &str, desc: &str, out: &Out, line: &str) {
    let want = format!("Program log: {}", line);
    let got: Vec<&String> = out.logs.iter().filter(|l| l.contains("too short")).collect();
    e.record(id, desc, out.logs.contains(&want), &format!("expected {:?}; got {:?}", want, got));
}

/// A 4-byte character (CJK Extension B, plane 2) that differs from call to call, so that memos
/// made of 4-byte characters stay unique
fn plane2_char(e: &Env) -> char {
    char::from_u32(0x20000 + (e.nonce() % 0xA6D0) as u32).unwrap()
}

/// Memo of a unique ASCII prefix followed by `ch` up to `chars` characters
fn fill_chars(e: &Env, tag: &str, ch: char, chars: usize) -> Instruction {
    let mut s = format!("r{}:{}:", e.nonce(), tag);
    while s.chars().count() < chars {
        s.push(ch);
    }
    memo_ix(s.as_bytes())
}

/// Memo of a unique ASCII prefix followed by `ch` up to `bytes` bytes, topped up with ASCII
fn fill_bytes(e: &Env, tag: &str, ch: char, bytes: usize) -> Instruction {
    let mut s = format!("r{}:{}:", e.nonce(), tag);
    while s.len() + ch.len_utf8() <= bytes {
        s.push(ch);
    }
    while s.len() < bytes {
        s.push('x');
    }
    assert!(s.chars().count() < 69, "fill_bytes must stay below 69 characters");
    memo_ix(s.as_bytes())
}
