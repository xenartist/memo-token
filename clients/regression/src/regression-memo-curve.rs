//! On-chain regression suite for the emission curve, the 10T supply cap, the 1T per-instruction
//! burn maximum and the 18T user burn statistics cap (local validator only).
//!
//! The validator must start with a supply above 10T held by the test wallet:
//!   ./scripts/local-validator.sh start --supply 18400000000000 --balance 18400000000000
//! The suite then walks the supply down with exact 1T burns (each one checked for exact balance,
//! supply, user and global statistics deltas) and, at each tier, mints twice and compares the
//! minted amount with hand-written values and with an independent curve reference:
//!   above 10T, exactly 10T, 10T - 1 unit, 9T, 5T, 1T + 1 unit, 1B, 100M, 20M, 10M + 1 unit, 10M, 5M.
//! The wallet's cumulative burns pass 18T on the way, which checks the user statistics cap.
//! It ends at a supply of about 5M tokens, so the other suites can run afterwards.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use harness::*;
use solana_sdk::signature::Signer;

const SUPPLY_LIMIT_REACHED: u32 = 6005;
const START_SUPPLY_MIN: u64 = 18_100_000_000_000 * UNIT;
const T: u64 = 1_000_000_000_000 * UNIT; // 1T tokens in units
const M: u64 = 1_000_000 * UNIT; // 1M tokens in units

/// Burns the wallet's tokens until the supply is exactly `target`, in transactions of up to six
/// 1T burns bound to one memo, then one burn for the remainder. Every transaction is checked with
/// burn_ok (exact deltas, user statistics capped at 18T).
fn burn_down_to(e: &Env, target: u64, tag: &str) {
    let ap = e.wallet.pubkey();
    let mut n = 0;
    loop {
        let supply = e.supply();
        if supply <= target {
            if supply < target {
                e.record(&format!("D-{}", tag), "supply reached the target exactly", false, &format!("supply {} target {}", supply, target));
            }
            return;
        }
        n += 1;
        let rest = supply - target;
        let id = format!("D-{}-{}", tag, n);
        if rest >= ONE_T {
            let k = (rest / ONE_T).min(6);
            let mut ixs = vec![memo_ix(&e.burn_memo(1, ONE_T, &id))];
            for _ in 0..k {
                ixs.push(e.burn_ix(&ap, ONE_T));
            }
            ixs.push(cu(1_400_000));
            let out = e.burn_ok(&id, &format!("burn {} x 1T (MAX_BURN_PER_INSTRUCTION) toward {}", k, tag), ixs, &e.wallet, &[], &ap, k * ONE_T, k);
            if !out.ok() {
                e.finish(); // the supply cannot be walked down any further
            }
        } else {
            let out = e.burn_ok(&id, &format!("burn {} units down to {}", rest, tag),
                vec![memo_ix(&e.burn_memo(1, rest, &id)), e.burn_ix(&ap, rest)], &e.wallet, &[], &ap, rest, 1);
            if !out.ok() {
                e.finish();
            }
        }
    }
}

fn mint_expect(e: &Env, id: &str, expected: u64) {
    let ap = e.wallet.pubkey();
    let a_ta = e.ata(&ap);
    let supply = e.supply();
    let minted = e.mint_ok(id, &format!("mint at supply {} units matches the curve reference", supply),
        vec![e.text_memo(id, 69), e.mint_ix(&ap)], &e.wallet, &[], &a_ta, &[]);
    e.record(&format!("{}-x", id), &format!("mint at supply {} units pays exactly {} units (hand-written)", supply, expected),
        minted == expected, &format!("minted {}", minted));
}

fn reject_mint(e: &Env, id: &str, desc: &str, use_mint_to: bool) {
    let ap = e.wallet.pubkey();
    let a_ta = e.ata(&ap);
    let ix = if use_mint_to { e.mint_to_ix(&ap, &ap, &a_ta) } else { e.mint_ix(&ap) };
    e.reject(id, desc, vec![e.text_memo(id, 69), ix], &e.wallet, &[], Expect::Custom(1, SUPPLY_LIMIT_REACHED), &[a_ta], &[ap]);
}

fn main() {
    let e = Env::new("regression-memo-curve");
    let ap = e.wallet.pubkey();
    let a_ta = e.ata(&ap);
    let supply = e.supply();
    let balance = e.balance(&a_ta).unwrap_or(0);
    if supply < START_SUPPLY_MIN || balance + 5 * M < supply {
        eprintln!("regression-memo-curve needs a supply of at least 18.1T tokens held by the wallet (supply {}, balance {});", supply, balance);
        eprintln!("start the validator with: ./scripts/local-validator.sh start --supply 18400000000000 --balance 18400000000000");
        std::process::exit(2);
    }
    let stats0 = e.user_stats(&ap).unwrap_or_default();

    // ---------------- above and at the cap ----------------
    reject_mint(&e, "T01", "process_mint with supply above 10T", false);
    burn_down_to(&e, MAX_SUPPLY, "10T");
    reject_mint(&e, "C01", "process_mint at supply exactly 10T", false);
    reject_mint(&e, "C01b", "process_mint_to at supply exactly 10T", true);
    e.burn_ok("C02", "burn 1 unit at the cap", vec![memo_ix(&e.burn_memo(1, 1, "cap-burn")), e.burn_ix(&ap, 1)], &e.wallet, &[], &ap, 1, 1);
    mint_expect(&e, "C03", 1);
    e.record("C03-supply", "supply back at exactly 10T", e.supply() == MAX_SUPPLY, &format!("supply {}", e.supply()));
    reject_mint(&e, "C04", "process_mint at 10T again", false);

    // ---------------- curve tiers ----------------
    let tiers: &[(&str, u64, [u64; 2])] = &[
        ("9T", 9 * T, [2, 1]),
        ("5T", 5 * T, [6, 5]),
        ("1T+1", T + 1, [9, 9]),
        ("1B", 1_000 * M, [10_000, 9_000]),
        ("100M", 100 * M, [100_000, 90_000]),
        ("20M", 20 * M, [900_000, 800_000]),
        ("10M+1", 10 * M + 1, [900_000, 900_000]),
        ("10M", 10 * M, [1_000_000, 900_000]),
        ("5M", 5 * M, [1_000_000, 1_000_000]),
    ];
    for (tag, target, expected) in tiers {
        burn_down_to(&e, *target, tag);
        mint_expect(&e, &format!("S-{}-1", tag), expected[0]);
        mint_expect(&e, &format!("S-{}-2", tag), expected[1]);
    }

    // ---------------- user statistics cap (18T) ----------------
    let stats = e.user_stats(&ap).unwrap_or_default();
    e.record("T03", "after burning more than 18T in total, the user's total_burned is exactly 18T",
        stats0.total < MAX_USER_GLOBAL_BURN && stats.total == MAX_USER_GLOBAL_BURN, &format!("before {:?} now {:?}", stats0, stats));
    e.burn_ok("T04", "burn 1 unit past the cap: tokens burn, user total stays 18T, count +1, global +1",
        vec![memo_ix(&e.burn_memo(1, 1, "past-cap")), e.burn_ix(&ap, 1)], &e.wallet, &[], &ap, 1, 1);
    let after = e.user_stats(&ap).unwrap_or_default();
    e.record("T05", "user total still exactly 18T", after.total == MAX_USER_GLOBAL_BURN && after.count == stats.count + 1,
        &format!("{:?}", after));

    e.finish();
}
