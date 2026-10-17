//! On-chain regression suite for the admin-only initialization of MintState (memo-mint) and
//! GlobalBurnStats (memo-burn) on a fresh deployment (local validator only).
//!
//! The validator must start without these two accounts:
//!   ./scripts/local-validator.sh start --no-core-state
//! Checks: minting and burning fail with AccountNotInitialized before initialization, and a
//! non-admin cannot initialize either account (UnauthorizedAdmin, account still absent).
//! Successful initialization by the admin is not covered: it needs the admin key.

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use harness::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    signature::Signer,
};

const MINT_UNAUTHORIZED_ADMIN: u32 = 6007;
const BURN_UNAUTHORIZED_ADMIN: u32 = 6012;

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

    e.finish();
}
