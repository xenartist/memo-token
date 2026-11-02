//! On-chain regression suite for the admin-only initialization of MintState (memo-mint) and
//! GlobalBurnStats (memo-burn) on a fresh deployment (local validator only).
//!
//! The validator must start without these two accounts:
//!   ./scripts/local-validator.sh start --no-core-state
//! Checks: minting and burning fail with AccountNotInitialized before initialization, and a
//! non-admin cannot initialize either account (UnauthorizedAdmin, account still absent).
//! The admin's successful initialization of each account runs without the admin key, in
//! simulateTransaction with signature verification off (nothing lands): the account it creates
//! (owner, size, discriminator, canonical bump, zero fields), a mint or burn right after it in the
//! same transaction, a second initialization, initialization at an address that is not the PDA, and
//! that nothing persists. Also: the admin not signing, and the shadow MintState on a fresh
//! deployment. 17 cases.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use harness::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
};

const MINT_UNAUTHORIZED_ADMIN: u32 = 6007;
const BURN_UNAUTHORIZED_ADMIN: u32 = 6012;
/// memo-mint's AUTHORIZED_ADMIN_PUBKEY (testnet build, the one the local validator runs)
const MINT_ADMIN: Pubkey = solana_sdk::pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");
const MINT_STATE_SPACE: usize = 8 + 32 + 8 + 1 + 64;
/// memo-burn's AUTHORIZED_ADMIN_PUBKEY (testnet build): the same key as memo-mint's
const BURN_ADMIN: Pubkey = MINT_ADMIN;
const GLOBAL_BURN_STATS_SPACE: usize = 8 + 16 + 8 + 8 + 1 + 64;

fn main() {
    let e = Env::new("regression-admin-init");
    let a = &e.wallet;
    let ap = a.pubkey();
    let a_ta = e.ata(&ap);
    if e.data(&e.mint_state).is_some() || e.data(&e.gbs).is_some() {
        eprintln!("regression-admin-init needs a validator without MintState and GlobalBurnStats;");
        eprintln!("start it with: ./scripts/local-validator.sh start --no-core-state");
        std::process::exit(2);
    }
    let watch = [a_ta];
    let users = [ap];

    e.reject("TEST-09.mint-before-init", "process_mint before MintState is initialized",
        vec![e.text_memo("fresh-mint", 69), e.mint_ix(&ap)], a, &[], Expect::Custom(1, ANCHOR_ACCOUNT_NOT_INITIALIZED), &watch, &users);
    e.reject("TEST-09.burn-before-init", "process_burn before GlobalBurnStats is initialized",
        vec![memo_ix(&e.burn_memo(1, UNIT, "fresh-burn")), e.burn_ix(&ap, UNIT)], a, &[], Expect::Custom(1, ANCHOR_ACCOUNT_NOT_INITIALIZED), &watch, &users);

    let cases = [
        ("TEST-09.mint-non-admin", e.mint_prog, "initialize_mint_state", e.mint_state, MINT_UNAUTHORIZED_ADMIN),
        ("TEST-09.burn-non-admin", e.burn_prog, "initialize_global_burn_stats", e.gbs, BURN_UNAUTHORIZED_ADMIN),
    ];
    for (id, program, name, account, code) in cases {
        let ix = Instruction::new_with_bytes(program, &disc(name), vec![
            AccountMeta::new(ap, true), AccountMeta::new(account, false), AccountMeta::new_readonly(system_program(), false)]);
        let out = e.exec(&[ix], a, &[]);
        let exists = e.data(&account).is_some();
        e.record(id, &format!("non-admin {} on a fresh deployment fails with {} and creates nothing", name, code),
            Expect::Custom(0, code).matches(&out) && !exists, &format!("{}; account exists afterwards: {}", out.brief(), exists));
    }

    // ---------------- memo-mint initialization by the admin, simulated (S49) ----------------
    // The admin's signature is the only thing a test lacks: simulateTransaction with sigVerify off
    // runs the transaction with empty signatures, so the success path runs here without the admin
    // key. A simulation lands nothing, so no later real transaction can use the MintState it shows
    // (TEST-09.init-not-persisted checks that). The admin needs lamports for the fee and the rent.
    e.airdrop(&MINT_ADMIN, 1);
    let init_ix = |admin: &Pubkey, state: &Pubkey, admin_signs: bool| Instruction::new_with_bytes(e.mint_prog, &disc("initialize_mint_state"), vec![
        AccountMeta::new(*admin, admin_signs), AccountMeta::new(*state, false), AccountMeta::new_readonly(system_program(), false)]);
    let bump = Pubkey::find_program_address(&[b"mint_state"], &e.mint_prog).1;
    let rent = e.c.get_minimum_balance_for_rent_exemption(MINT_STATE_SPACE).unwrap();
    // MintState as created: memo-mint-owned, rent-exempt, 113 bytes, discriminator, the given
    // fingerprint and slot, the canonical bump, reserved zero
    let state_is = |acc: &Option<(Pubkey, u64, Vec<u8>)>, fp: [u8; 32], slot: u64| match acc {
        Some((owner, lamports, d)) => *owner == e.mint_prog && *lamports == rent && d.len() == MINT_STATE_SPACE
            && d[..8] == account_disc("MintState")[..] && d[8..40] == fp && d[40..48] == slot.to_le_bytes() && d[48] == bump
            && d[49..].iter().all(|&x| x == 0),
        None => false,
    };
    let show = |acc: &Option<(Pubkey, u64, Vec<u8>)>| acc.as_ref().map(|(o, l, d)| format!("owner {} lamports {} data {}", o, l, hex::encode(d)))
        .unwrap_or_else(|| "absent".into());

    let (out, acc) = e.simulate_unsigned(&[init_ix(&MINT_ADMIN, &e.mint_state, true)], &MINT_ADMIN, &[e.mint_state]);
    e.record("TEST-09.admin-init", &format!("the admin initializes MintState (simulated): memo-mint-owned, {} bytes, discriminator, zero fingerprint and slot, canonical bump {}, zero reserved", MINT_STATE_SPACE, bump),
        out.ok() && state_is(&acc[0], [0; 32], 0), &format!("{}; {}; logs {:?}", out.brief(), show(&acc[0]), out.logs));

    // [memo, initialize_mint_state, process_mint]: the new MintState works for the first mint
    {
        let ixs = vec![e.text_memo("init-then-mint", 69), init_ix(&MINT_ADMIN, &e.mint_state, true), e.mint_ix(&ap)];
        let before = e.snap(&[a_ta], &[]);
        let reward = curve_reference(before.supply).unwrap_or(0);
        let (out, acc) = e.simulate_unsigned(&ixs, &MINT_ADMIN, &[e.mint_state, a_ta, e.mint]);
        let amount = |i: usize, at: usize| acc[i].as_ref().filter(|x| x.2.len() >= at + 8).map(|x| u64::from_le_bytes(x.2[at..at + 8].try_into().unwrap()));
        let fp = fingerprint(&Message::new(&ixs, Some(&MINT_ADMIN)));
        let pass = out.ok() && out.minted_log_count() == 1 && out.logs.contains(&minted_log(reward, before.supply))
            && state_is(&acc[0], fp, out.slot)
            && amount(1, 64) == Some(before.bal(&a_ta) + reward) && amount(2, 36) == Some(before.supply + reward);
        e.record("TEST-09.admin-init-then-mint", "[memo, initialize_mint_state, process_mint] (simulated): the first mint uses the new MintState, which then holds (fingerprint, slot)",
            pass, &format!("{}; reward {}; token account {:?} (was {}); supply {:?} (was {}); {}", out.brief(), reward, amount(1, 64),
                before.bal(&a_ta), amount(2, 36), before.supply, show(&acc[0])));
    }

    // A failed simulation returns no account state (see simulate_unsigned); these two assert the
    // exact error, and TEST-09.init-not-persisted that no simulation left anything behind
    let (out, _) = e.simulate_unsigned(&[init_ix(&MINT_ADMIN, &e.mint_state, true), init_ix(&MINT_ADMIN, &e.mint_state, true)], &MINT_ADMIN, &[]);
    let expect = Expect::RaisedBy(1, SYSTEM_ACCOUNT_IN_USE, system_program());
    e.record("TEST-09.admin-init-twice", "the admin initializes MintState twice in one transaction (simulated): the second fails in the System program",
        expect.matches(&out), &format!("expected {}; got {} ({:?})", expect.describe(), out.brief(), out.failed_program()));

    let (out, _) = e.simulate_unsigned(&[init_ix(&MINT_ADMIN, &Pubkey::new_unique(), true)], &MINT_ADMIN, &[]);
    let expect = Expect::Raised(0, ANCHOR_CONSTRAINT_SEEDS, e.mint_prog, "ConstraintSeeds");
    e.record("TEST-09.admin-init-not-pda", "the admin initializes MintState at an address that is not the PDA (simulated)",
        expect.matches(&out), &format!("expected {}; got {} ({:?} in {:?})", expect.describe(), out.brief(), out.error_name(), out.failed_program()));

    let out = e.exec(&[init_ix(&MINT_ADMIN, &e.mint_state, false)], a, &[]);
    let expect = Expect::Raised(0, ANCHOR_ACCOUNT_NOT_SIGNER, e.mint_prog, "AccountNotSigner");
    let exists = e.data(&e.mint_state).is_some();
    e.record("TEST-09.admin-not-signer", "initialize_mint_state naming the admin, who does not sign (fee payer A): creates nothing",
        expect.matches(&out) && !exists, &format!("expected {}; got {} ({:?}); MintState exists afterwards: {}", expect.describe(), out.brief(), out.error_name(), exists));

    e.reject("TEST-09.init-not-persisted", "after the simulations MintState still does not exist: process_mint fails as before initialization",
        vec![e.text_memo("after-sim", 69), e.mint_ix(&ap)], a, &[], Expect::Raised(1, ANCHOR_ACCOUNT_NOT_INITIALIZED, e.mint_prog, "AccountNotInitialized"), &watch, &users);

    // ---------------- memo-burn initialization by the admin, simulated ----------------
    // Same technique as for memo-mint above (MEMO-BURN-AUDIT.md, re-audit finding BURN-R01)
    let gbs_init_ix = |admin: &Pubkey, stats: &Pubkey, admin_signs: bool| Instruction::new_with_bytes(e.burn_prog, &disc("initialize_global_burn_stats"), vec![
        AccountMeta::new(*admin, admin_signs), AccountMeta::new(*stats, false), AccountMeta::new_readonly(system_program(), false)]);
    let gbs_bump = Pubkey::find_program_address(&[b"global_burn_stats"], &e.burn_prog).1;
    let gbs_rent = e.c.get_minimum_balance_for_rent_exemption(GLOBAL_BURN_STATS_SPACE).unwrap();
    // GlobalBurnStats as created: memo-burn-owned, rent-exempt, 105 bytes, discriminator, the given
    // total and count, last_burn_time set exactly when count > 0, the canonical bump, reserved zero
    let gbs_is = |acc: &Option<(Pubkey, u64, Vec<u8>)>, total: u128, count: u64| match acc {
        Some((owner, lamports, d)) => *owner == e.burn_prog && *lamports == gbs_rent && d.len() == GLOBAL_BURN_STATS_SPACE
            && d[..8] == account_disc("GlobalBurnStats")[..] && d[8..24] == total.to_le_bytes() && d[24..32] == count.to_le_bytes()
            && (i64::from_le_bytes(d[32..40].try_into().unwrap()) > 0) == (count > 0) && d[40] == gbs_bump
            && d[41..].iter().all(|&x| x == 0),
        None => false,
    };

    let (out, acc) = e.simulate_unsigned(&[gbs_init_ix(&BURN_ADMIN, &e.gbs, true)], &BURN_ADMIN, &[e.gbs]);
    e.record("TEST-09.burn-admin-init", &format!("the admin initializes GlobalBurnStats (simulated): memo-burn-owned, {} bytes, discriminator, zero counters, canonical bump {}, zero reserved", GLOBAL_BURN_STATS_SPACE, gbs_bump),
        out.ok() && gbs_is(&acc[0], 0, 0), &format!("{}; {}; logs {:?}", out.brief(), show(&acc[0]), out.logs));

    // [memo, initialize_global_burn_stats, process_burn]: the new GlobalBurnStats works for the first burn
    {
        let ixs = vec![memo_ix(&e.burn_memo(1, UNIT, "init-then-burn")), gbs_init_ix(&BURN_ADMIN, &e.gbs, true), e.burn_ix(&ap, UNIT)];
        let before = e.snap(&[a_ta], &[]);
        let (out, acc) = e.simulate_unsigned(&ixs, &BURN_ADMIN, &[e.gbs, a_ta, e.mint]);
        let amount = |i: usize, at: usize| acc[i].as_ref().filter(|x| x.2.len() >= at + 8).map(|x| u64::from_le_bytes(x.2[at..at + 8].try_into().unwrap()));
        let pass = out.ok() && out.logs.contains(&burned_log(UNIT)) && gbs_is(&acc[0], UNIT as u128, 1)
            && amount(1, 64) == before.bal(&a_ta).checked_sub(UNIT) && amount(2, 36) == before.supply.checked_sub(UNIT);
        e.record("TEST-09.burn-admin-init-then-burn", "[memo, initialize_global_burn_stats, process_burn] (simulated): the first burn uses the new GlobalBurnStats, which then counts it",
            pass, &format!("{}; token account {:?} (was {}); supply {:?} (was {}); {}", out.brief(), amount(1, 64),
                before.bal(&a_ta), amount(2, 36), before.supply, show(&acc[0])));
    }

    let (out, _) = e.simulate_unsigned(&[gbs_init_ix(&BURN_ADMIN, &e.gbs, true), gbs_init_ix(&BURN_ADMIN, &e.gbs, true)], &BURN_ADMIN, &[]);
    let expect = Expect::RaisedBy(1, SYSTEM_ACCOUNT_IN_USE, system_program());
    e.record("TEST-09.burn-admin-init-twice", "the admin initializes GlobalBurnStats twice in one transaction (simulated): the second fails in the System program",
        expect.matches(&out), &format!("expected {}; got {} ({:?})", expect.describe(), out.brief(), out.failed_program()));

    let (out, _) = e.simulate_unsigned(&[gbs_init_ix(&BURN_ADMIN, &Pubkey::new_unique(), true)], &BURN_ADMIN, &[]);
    let expect = Expect::Raised(0, ANCHOR_CONSTRAINT_SEEDS, e.burn_prog, "ConstraintSeeds");
    e.record("TEST-09.burn-admin-init-not-pda", "the admin initializes GlobalBurnStats at an address that is not the PDA (simulated)",
        expect.matches(&out), &format!("expected {}; got {} ({:?} in {:?})", expect.describe(), out.brief(), out.error_name(), out.failed_program()));

    let out = e.exec(&[gbs_init_ix(&BURN_ADMIN, &e.gbs, false)], a, &[]);
    let expect = Expect::Raised(0, ANCHOR_ACCOUNT_NOT_SIGNER, e.burn_prog, "AccountNotSigner");
    let exists = e.data(&e.gbs).is_some();
    e.record("TEST-09.burn-admin-not-signer", "initialize_global_burn_stats naming the admin, who does not sign (fee payer A): creates nothing",
        expect.matches(&out) && !exists, &format!("expected {}; got {} ({:?}); GlobalBurnStats exists afterwards: {}", expect.describe(), out.brief(), out.error_name(), exists));

    e.reject("TEST-09.burn-init-not-persisted", "after the simulations GlobalBurnStats still does not exist: process_burn fails as before initialization",
        vec![memo_ix(&e.burn_memo(1, UNIT, "after-sim")), e.burn_ix(&ap, UNIT)], a, &[], Expect::Raised(1, ANCHOR_ACCOUNT_NOT_INITIALIZED, e.burn_prog, "AccountNotInitialized"), &watch, &users);

    // The shadow MintState (planted by prepare-local-validator, not the PDA) cannot stand in for
    // the missing one
    let mut acc = e.mint_accounts(&ap, &a_ta);
    acc[6] = AccountMeta::new(shadow_mint_state(), false);
    e.reject_untouched("TEST-09.mint-shadow-state", "process_mint with the shadow MintState on a fresh deployment",
        vec![e.text_memo("fresh-shadow", 69), e.mint_ix_with(acc)], a, &[], Expect::Raised(1, ANCHOR_CONSTRAINT_SEEDS, e.mint_prog, "ConstraintSeeds"), &watch, &users);

    e.finish();
}
