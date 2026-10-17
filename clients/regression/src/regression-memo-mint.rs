//! On-chain regression suite for memo-mint (local validator only).
//!
//! Covers one mint per transaction (top-level and through app CPI: memo-blog mint_for_blog,
//! memo-forum mint_for_post), the memo position and length rules, account substitution,
//! admin-only initialization with existing state, exact minted amounts against an independent
//! curve reference and durable-nonce transactions. Every case asserts the exact outcome.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-mint
//! The supply must be below the 10T cap.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use borsh::BorshSerialize;
use harness::*;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::get_associated_token_address_with_program_id as ata_of;

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

    let one_mint = |ix: u64| Expect::Custom(ix, ONE_MINT_PER_TRANSACTION);

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
        vec![blog_mint_memo(&e, &ap), e.mint_ix(&ap), blog_mint_ix(&e, &ap), cu(600_000)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M04d", "memo-blog mint_for_blog twice (two CPI mints)",
        vec![blog_mint_memo(&e, &ap), blog_mint_ix(&e, &ap), blog_mint_ix(&e, &ap), cu(800_000)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M04d2", "top-level process_mint_to + memo-blog mint_for_blog (CPI mint)",
        vec![blog_mint_memo(&e, &ap), e.mint_to_ix(&ap, &ap, &a_ta), blog_mint_ix(&e, &ap), cu(600_000)], a, &[], one_mint(2), &watch, &[]);
    e.mint_ok("M04e", "control: memo-forum mint_for_post alone mints the curve amount through CPI",
        vec![post_mint_memo(&e, &ap, post_id), post_mint_ix(&e, &ap, post_id), cu(400_000)], a, &[], &a_ta, &[b_ta]);
    e.reject("M04f", "memo-forum mint_for_post (CPI mint) + top-level process_mint",
        vec![post_mint_memo(&e, &ap, post_id), post_mint_ix(&e, &ap, post_id), e.mint_ix(&ap), cu(600_000)], a, &[], one_mint(2), &watch, &[]);
    e.reject("M04g", "top-level process_mint_to(B) + memo-forum mint_for_post (CPI mint)",
        vec![post_mint_memo(&e, &ap, post_id), e.mint_to_ix(&ap, &bp, &b_ta), post_mint_ix(&e, &ap, post_id), cu(600_000)], a, &[], one_mint(2), &watch, &[]);

    // ---------------- memo position and length ----------------
    let memo_required = |ix: u64| Expect::Custom(ix, MEMO_REQUIRED);
    e.reject("M05", "ComputeBudget at index 0, valid memo at index 1",
        vec![cu(200_000), e.text_memo("idx1", 69), e.mint_ix(&ap)], a, &[], memo_required(2), &watch, &[]);
    e.reject("M06", "68-byte memo", vec![e.text_memo("68", 68), e.mint_ix(&ap)], a, &[], Expect::Custom(1, MEMO_TOO_SHORT), &watch, &[]);
    e.mint_ok("M07", "69-byte memo (minimum)", vec![e.text_memo("69", 69), e.mint_ix(&ap)], a, &[], &a_ta, &[b_ta]);
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
        a, &[], Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &watch, &[]);

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

    e.finish();
}
