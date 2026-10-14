//! memo-mint penetration test suite
//!
//! Every case is simulated only (nothing is sent) and has an explicit expectation:
//! - REJECT: the transaction must fail, ideally with one of the listed errors
//! - ACCEPT: the transaction is valid by design and must succeed; every successful mint's
//!   amount is checked against the emission curve
//!
//! Rules the cases are written against (memo-mint 0.2.0):
//! - the SPL Memo instruction must be at index 0 and at least 69 bytes; there is no maximum
//! - process_mint / process_mint_to must be at index 1 or later
//! - ComputeBudget instructions also take an index, so they go after the memo
//! - only one mint per transaction: a second mint in the same transaction (another top-level
//!   instruction or a CPI) fails with OneMintPerTransaction; MintState is the last account
//!
//! Requires the payer's MEMO token account (Token-2022 ATA) to exist.

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer, Keypair},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use solana_system_interface::instruction as system_instruction;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use sha2::{Sha256, Digest};
use spl_token_2022::id as token_2022_id;

use memo_token_client::{get_rpc_url, get_wallet_path, get_program_id, get_token_mint, expected_mint_amount, mint_state_pda};

const MEMO_MIN_LENGTH: usize = 69;
const INSTRUCTIONS_SYSVAR: Pubkey = solana_sdk::sysvar::instructions::ID;
const LEGACY_MEMO_V1: &str = "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo";
const SPL_TOKEN_LEGACY: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

enum Expect {
    Accept,
    Reject(&'static [&'static str]),
}

enum Outcome {
    Pass(String),
    Warn(String),
    Fail(String),
}

struct Ctx {
    client: RpcClient,
    payer: Keypair,
    program_id: Pubkey,
    mint: Pubkey,
    mint_authority: Pubkey,
    token_account: Pubkey,
    mint_state: Pubkey,
    // an existing MEMO token account owned by someone else, if one can be found
    other_token_account: Option<Pubkey>,
}

struct Case {
    category: &'static str,
    name: &'static str,
    expect: Expect,
    build: fn(&Ctx) -> (Vec<Instruction>, Vec<Keypair>),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let category = args[1].as_str();
    let valid = ["memo-attacks", "instruction-attacks", "pda-attacks", "mint-authority-attacks",
                 "account-attacks", "dos-attacks", "mint-to-attacks", "mint-state-attacks", "all"];
    if !valid.contains(&category) {
        println!("❌ Unknown test type: {}", category);
        print_usage();
        return Ok(());
    }

    let ctx = load_context()?;
    let cases: Vec<Case> = all_cases()
        .into_iter()
        .filter(|c| category == "all" || c.category == category)
        .collect();

    println!("🔥 MEMO-MINT PENETRATION TESTS: {} ({} cases, simulation only)", category, cases.len());
    let (mut pass, mut warn, mut fail) = (0, 0, 0);
    for case in &cases {
        let expect_label = match case.expect {
            Expect::Accept => "ACCEPT (by design)".to_string(),
            Expect::Reject(errs) => format!("REJECT ({})", errs.join(" / ")),
        };
        println!("\n🎯 [{}] {} — expected: {}", case.category, case.name, expect_label);
        match run_case(&ctx, case) {
            Outcome::Pass(msg) => { pass += 1; println!("   ✅ PASS: {}", msg); }
            Outcome::Warn(msg) => { warn += 1; println!("   ⚠️  WARN: {}", msg); }
            Outcome::Fail(msg) => { fail += 1; println!("   ❌ FAIL: {}", msg); }
        }
    }

    println!("\n==========================================");
    println!("Summary: {} passed, {} warnings, {} failed", pass, warn, fail);
    println!("WARN = rejected, but not for the expected reason (the case may not test what it intends).");
    if fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn print_usage() {
    println!("📋 MEMO-MINT CONTRACT PENETRATION TESTING SUITE (simulation only)");
    println!();
    println!("Usage: cargo run --bin pen-test-memo-mint -- <test_type>");
    println!();
    println!("Available test types:");
    println!("  memo-attacks           - Memo presence, position, program and length");
    println!("  instruction-attacks    - Instruction ordering and batching");
    println!("  pda-attacks            - Fake or wrong mint authority PDAs");
    println!("  mint-authority-attacks - Wrong mint, signer tricks, other users' token accounts");
    println!("  account-attacks        - Wrong programs, sysvar, account order, extra accounts and data");
    println!("  dos-attacks            - Compute, instruction trace and transaction size limits");
    println!("  mint-to-attacks        - process_mint_to recipient checks");
    println!("  mint-state-attacks     - Missing or substituted MintState account");
    println!("  all                    - Run every case");
}

fn load_context() -> Result<Ctx, Box<dyn std::error::Error>> {
    let client = RpcClient::new_with_commitment(get_rpc_url(), CommitmentConfig::confirmed());
    let payer = read_keypair_file(get_wallet_path())
        .map_err(|e| format!("Failed to read wallet keypair: {}", e))?;
    let program_id = get_program_id("memo_mint")?;
    let mint = get_token_mint("memo_token")?;
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    let token_account = get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_2022_id());
    let mint_state = mint_state_pda();

    if client.get_account(&token_account).is_err() {
        return Err(format!(
            "Payer token account {} does not exist. Create it first (spl-token create-account {} --program-id {}).",
            token_account, mint, token_2022_id()
        ).into());
    }

    // Pick an existing token account owned by someone else, for the "other user's account" cases
    let other_token_account = client
        .get_token_largest_accounts(&mint)
        .ok()
        .and_then(|accounts| {
            accounts.into_iter()
                .filter_map(|a| a.address.parse::<Pubkey>().ok())
                .find(|addr| *addr != token_account)
        });

    println!("Payer: {}", payer.pubkey());
    println!("memo-mint: {}", program_id);
    println!("Mint: {}", mint);
    println!("Token account: {}", token_account);
    match other_token_account {
        Some(a) => println!("Other user's token account: {}", a),
        None => println!("Other user's token account: none found (those cases will be skipped)"),
    }

    if client.get_account(&mint_state).is_err() {
        return Err(format!("MintState {} does not exist. Run admin-init-mint-state first.", mint_state).into());
    }
    println!("MintState: {}", mint_state);

    Ok(Ctx { client, payer, program_id, mint, mint_authority, token_account, mint_state, other_token_account })
}

fn run_case(ctx: &Ctx, case: &Case) -> Outcome {
    let (instructions, extra_signers) = (case.build)(ctx);
    if instructions.is_empty() {
        return Outcome::Warn("skipped: no suitable account available on this network".into());
    }

    let mut signers: Vec<&Keypair> = vec![&ctx.payer];
    signers.extend(extra_signers.iter());
    let blockhash = match ctx.client.get_latest_blockhash() {
        Ok(h) => h,
        Err(e) => return Outcome::Fail(format!("could not get blockhash: {}", e)),
    };
    let mut tx = Transaction::new_with_payer(&instructions, Some(&ctx.payer.pubkey()));
    if let Err(e) = tx.try_sign(&signers, blockhash) {
        return Outcome::Fail(format!("could not sign: {}", e));
    }

    // Either an RPC error (e.g. transaction too large) or a simulation result
    let (rejected, reason, logs) = match ctx.client.simulate_transaction(&tx) {
        Err(e) => (true, e.to_string(), vec![]),
        Ok(r) => {
            let logs = r.value.logs.unwrap_or_default();
            match r.value.err {
                Some(err) => (true, format!("{:?}", err), logs),
                None => (false, String::new(), logs),
            }
        }
    };

    let mint_count = instructions.iter().filter(|ix| ix.program_id == ctx.program_id).count();

    match (&case.expect, rejected) {
        (Expect::Reject(expected), true) => {
            let haystack = format!("{} {}", reason, logs.join(" "));
            match expected.iter().find(|e| haystack.contains(*e)) {
                Some(e) => Outcome::Pass(format!("rejected with {}", e)),
                None => Outcome::Warn(format!("rejected, but with: {}{}", reason, last_error_log(&logs))),
            }
        }
        (Expect::Reject(_), false) => {
            Outcome::Fail("SECURITY ISSUE: the transaction succeeded".into())
        }
        (Expect::Accept, false) => match verify_mint_logs(&logs, mint_count) {
            Ok(msg) => Outcome::Pass(msg),
            Err(msg) => Outcome::Fail(msg),
        },
        (Expect::Accept, true) => {
            Outcome::Fail(format!("valid transaction was rejected: {}{}", reason, last_error_log(&logs)))
        }
    }
}

fn last_error_log(logs: &[String]) -> String {
    logs.iter()
        .rev()
        .find(|l| l.contains("Error") || l.contains("failed"))
        .map(|l| format!(" ({})", l))
        .unwrap_or_default()
}

/// Check every "Minted {amount} units to {owner}, supply before mint: {supply} units" line
/// against the emission curve. Logs are truncated at 10 KB, so with many mints some lines
/// may be missing; those are reported as not verified.
fn verify_mint_logs(logs: &[String], mint_count: usize) -> Result<String, String> {
    let mut verified = 0;
    for line in logs.iter().filter(|l| l.contains("Program log: Minted ")) {
        let words: Vec<&str> = line.split_whitespace().collect();
        // Program log: Minted <amount> units to <owner>, supply before mint: <supply> units
        let amount = words.get(3).and_then(|w| w.parse::<u64>().ok());
        let supply = words.get(10).and_then(|w| w.parse::<u64>().ok());
        match (amount, supply) {
            (Some(amount), Some(supply)) => {
                let expected = expected_mint_amount(supply);
                if amount != expected {
                    return Err(format!("minted {} units at supply {}, curve expects {}", amount, supply, expected));
                }
                verified += 1;
            }
            _ => return Err(format!("could not parse mint log: {}", line)),
        }
    }
    if verified == mint_count {
        Ok(format!("succeeded; {} mint(s), all amounts match the curve", verified))
    } else if verified > 0 {
        Ok(format!("succeeded; {} of {} mint amounts verified (logs truncated)", verified, mint_count))
    } else {
        Err(format!("succeeded but no mint log found for {} mint instruction(s)", mint_count))
    }
}

// ---------------------------------------------------------------------------
// Instruction builders
// ---------------------------------------------------------------------------

fn memo(text: &str) -> Instruction {
    Instruction::new_with_bytes(spl_memo::ID, text.as_bytes(), vec![])
}

/// A memo of exactly `len` bytes (ASCII)
fn memo_len(prefix: &str, len: usize) -> Instruction {
    let mut text = prefix.to_string();
    while text.len() < len {
        text.push('x');
    }
    text.truncate(len);
    memo(&text)
}

/// A valid memo (minimum length)
fn valid_memo(label: &str) -> Instruction {
    memo_len(&format!("pen-test {} ", label), MEMO_MIN_LENGTH)
}

fn discriminator(name: &str) -> [u8; 8] {
    let hash = Sha256::digest(format!("global:{}", name).as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&hash[..8]);
    d
}

fn mint_accounts(ctx: &Ctx) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(ctx.payer.pubkey(), true),                 // user (signer)
        AccountMeta::new(ctx.mint, false),                          // mint
        AccountMeta::new_readonly(ctx.mint_authority, false),       // mint_authority (PDA)
        AccountMeta::new(ctx.token_account, false),                 // token_account
        AccountMeta::new_readonly(token_2022_id(), false),          // token_program
        AccountMeta::new_readonly(INSTRUCTIONS_SYSVAR, false),      // instructions sysvar
        AccountMeta::new(ctx.mint_state, false),                    // mint_state
    ]
}

fn mint_ix_with(ctx: &Ctx, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction::new_with_bytes(ctx.program_id, &discriminator("process_mint"), accounts)
}

fn mint_ix(ctx: &Ctx) -> Instruction {
    mint_ix_with(ctx, mint_accounts(ctx))
}

/// process_mint with account `index` replaced
fn mint_ix_replacing(ctx: &Ctx, index: usize, meta: AccountMeta) -> Instruction {
    let mut accounts = mint_accounts(ctx);
    accounts[index] = meta;
    mint_ix_with(ctx, accounts)
}

fn mint_to_ix(ctx: &Ctx, recipient: Pubkey, recipient_token_account: Pubkey) -> Instruction {
    let mut data = discriminator("process_mint_to").to_vec();
    data.extend_from_slice(recipient.as_ref());
    Instruction::new_with_bytes(ctx.program_id, &data, vec![
        AccountMeta::new(ctx.payer.pubkey(), true),                 // caller (signer)
        AccountMeta::new(ctx.mint, false),                          // mint
        AccountMeta::new_readonly(ctx.mint_authority, false),       // mint_authority (PDA)
        AccountMeta::new(recipient_token_account, false),           // recipient_token_account
        AccountMeta::new_readonly(token_2022_id(), false),          // token_program
        AccountMeta::new_readonly(INSTRUCTIONS_SYSVAR, false),      // instructions sysvar
        AccountMeta::new(ctx.mint_state, false),                    // mint_state
    ])
}

fn cu_limit(units: u32) -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(units)
}

fn noop_transfer(ctx: &Ctx) -> Instruction {
    system_instruction::transfer(&ctx.payer.pubkey(), &ctx.payer.pubkey(), 0)
}

fn pk(s: &str) -> Pubkey {
    s.parse().expect("valid pubkey")
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

fn all_cases() -> Vec<Case> {
    vec![
        // --- memo ---
        Case { category: "memo-attacks", name: "No memo", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Empty memo", expect: Expect::Reject(&["MemoTooShort"]),
            build: |c| (vec![memo(""), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "68-byte memo (one below minimum)", expect: Expect::Reject(&["MemoTooShort"]),
            build: |c| (vec![memo_len("short ", 68), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "69-byte memo (minimum)", expect: Expect::Accept,
            build: |c| (vec![memo_len("minimum ", 69), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "780-byte memo (no contract maximum; largest that fits with a ComputeBudget instruction)", expect: Expect::Accept,
            build: |c| (vec![memo_len("long ", 780), mint_ix(c), cu_limit(1_400_000)], vec![]) },
        Case { category: "memo-attacks", name: "Memo larger than a transaction", expect: Expect::Reject(&["too large", "TooLarge"]),
            build: |c| (vec![memo_len("huge ", 2_000), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Memo content with control characters and Unicode (not parsed)", expect: Expect::Accept,
            build: |c| (vec![memo(&format!("{}\0\n\r\t🚀🔥 {}", "injection test ".repeat(3), "x".repeat(20))), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Invalid UTF-8 memo (rejected by the Memo program)", expect: Expect::Reject(&["Invalid UTF-8", "InvalidInstructionData"]),
            build: |c| (vec![Instruction::new_with_bytes(spl_memo::ID, &[0xff; 80], vec![]), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Legacy Memo v1 program at index 0", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![Instruction::new_with_bytes(pk(LEGACY_MEMO_V1), &[b'x'; 80], vec![]), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Non-memo instruction at index 0, valid memo at index 1", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![noop_transfer(c), valid_memo("index1"), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Two memos before the mint (first one valid)", expect: Expect::Accept,
            build: |c| (vec![valid_memo("first"), valid_memo("second"), mint_ix(c)], vec![]) },
        Case { category: "memo-attacks", name: "Short memo at index 0, valid memo at index 1", expect: Expect::Reject(&["MemoTooShort"]),
            build: |c| (vec![memo("short memo"), valid_memo("second"), mint_ix(c)], vec![]) },

        // --- instruction order and batching ---
        Case { category: "instruction-attacks", name: "Mint at index 0, memo after it", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![mint_ix(c), valid_memo("after")], vec![]) },
        Case { category: "instruction-attacks", name: "ComputeBudget at index 0 (memo pushed to index 1)", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![cu_limit(200_000), valid_memo("cb-first"), mint_ix(c)], vec![]) },
        Case { category: "instruction-attacks", name: "ComputeBudget between memo and mint", expect: Expect::Accept,
            build: |c| (vec![valid_memo("cb-middle"), cu_limit(200_000), mint_ix(c)], vec![]) },
        Case { category: "instruction-attacks", name: "ComputeBudget last", expect: Expect::Accept,
            build: |c| (vec![valid_memo("cb-last"), mint_ix(c), cu_limit(200_000)], vec![]) },
        Case { category: "instruction-attacks", name: "Unrelated instruction between memo and mint", expect: Expect::Accept,
            build: |c| (vec![valid_memo("interleaved"), noop_transfer(c), mint_ix(c)], vec![]) },
        Case { category: "instruction-attacks", name: "One memo, 3 mints", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| (vec![valid_memo("batch3"), mint_ix(c), mint_ix(c), mint_ix(c), cu_limit(1_400_000)], vec![]) },
        Case { category: "instruction-attacks", name: "One memo, 31 mints", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| {
                let mut ixs = vec![valid_memo("batch31")];
                ixs.extend((0..31).map(|_| mint_ix(c)));
                ixs.push(cu_limit(1_400_000));
                (ixs, vec![])
            } },

        Case { category: "instruction-attacks", name: "process_mint then process_mint_to in one transaction", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| (vec![valid_memo("mixed"), mint_ix(c), mint_to_ix(c, c.payer.pubkey(), c.token_account)], vec![]) },
        Case { category: "instruction-attacks", name: "Two memos, two mints (only index 0 counts)", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| (vec![valid_memo("memo-a"), valid_memo("memo-b"), mint_ix(c), mint_ix(c)], vec![]) },
        Case { category: "instruction-attacks", name: "memo, mint, memo, mint", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| (vec![valid_memo("first"), mint_ix(c), valid_memo("second"), mint_ix(c)], vec![]) },
        Case { category: "instruction-attacks", name: "Mint, unrelated instructions, mint", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| (vec![valid_memo("spaced"), mint_ix(c), noop_transfer(c), cu_limit(400_000), noop_transfer(c), mint_ix(c)], vec![]) },

        // --- MintState account ---
        Case { category: "mint-state-attacks", name: "Old account list without mint_state", expect: Expect::Reject(&["AccountNotEnoughKeys"]),
            build: |c| {
                let mut accounts = mint_accounts(c);
                accounts.pop();
                (vec![valid_memo("no-state"), mint_ix_with(c, accounts)], vec![])
            } },
        Case { category: "mint-state-attacks", name: "Random account as mint_state", expect: Expect::Reject(&["AccountNotInitialized"]),
            build: |c| (vec![valid_memo("random-state"), mint_ix_replacing(c, 6, AccountMeta::new(Pubkey::new_unique(), false))], vec![]) },
        Case { category: "mint-state-attacks", name: "Mint account (other owner) as mint_state", expect: Expect::Reject(&["AccountOwnedByWrongProgram"]),
            build: |c| (vec![valid_memo("mint-as-state"), mint_ix_replacing(c, 6, AccountMeta::new(c.mint, false))], vec![]) },
        Case { category: "mint-state-attacks", name: "mint_state passed read-only", expect: Expect::Reject(&["ConstraintMut"]),
            build: |c| (vec![valid_memo("readonly-state"), mint_ix_replacing(c, 6, AccountMeta::new_readonly(c.mint_state, false))], vec![]) },

        // --- PDA ---
        Case { category: "pda-attacks", name: "Random address as mint authority", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| (vec![valid_memo("fake-pda"), mint_ix_replacing(c, 2, AccountMeta::new_readonly(Pubkey::new_unique(), false))], vec![]) },
        Case { category: "pda-attacks", name: "PDA from wrong seeds", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| {
                let (wrong, _) = Pubkey::find_program_address(&[b"wrong_authority"], &c.program_id);
                (vec![valid_memo("wrong-seeds"), mint_ix_replacing(c, 2, AccountMeta::new_readonly(wrong, false))], vec![])
            } },
        Case { category: "pda-attacks", name: "Same seeds, non-canonical bump", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| {
                let (_, bump) = Pubkey::find_program_address(&[b"mint_authority"], &c.program_id);
                let wrong = (0..bump).rev()
                    .find_map(|b| Pubkey::create_program_address(&[b"mint_authority", &[b]], &c.program_id).ok());
                match wrong {
                    Some(w) => (vec![valid_memo("bump"), mint_ix_replacing(c, 2, AccountMeta::new_readonly(w, false))], vec![]),
                    None => (vec![], vec![]),
                }
            } },
        Case { category: "pda-attacks", name: "Same seeds under another program", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| {
                let (other, _) = Pubkey::find_program_address(&[b"mint_authority"], &token_2022_id());
                (vec![valid_memo("cross-program"), mint_ix_replacing(c, 2, AccountMeta::new_readonly(other, false))], vec![])
            } },

        // --- mint / authority / token accounts ---
        Case { category: "mint-authority-attacks", name: "Random mint address", expect: Expect::Reject(&["AccountNotInitialized", "UnauthorizedMint"]),
            build: |c| (vec![valid_memo("wrong-mint"), mint_ix_replacing(c, 1, AccountMeta::new(Pubkey::new_unique(), false))], vec![]) },
        Case { category: "mint-authority-attacks", name: "Payer signs as mint authority", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| (vec![valid_memo("payer-authority"), mint_ix_replacing(c, 2, AccountMeta::new_readonly(c.payer.pubkey(), true))], vec![]) },
        Case { category: "mint-authority-attacks", name: "Extra keypair signs as mint authority", expect: Expect::Reject(&["ConstraintSeeds"]),
            build: |c| {
                let evil = Keypair::new();
                let ix = mint_ix_replacing(c, 2, AccountMeta::new_readonly(evil.pubkey(), true));
                (vec![valid_memo("evil-signer"), ix], vec![evil])
            } },
        Case { category: "mint-authority-attacks", name: "Mint into another user's token account", expect: Expect::Reject(&["UnauthorizedTokenAccount"]),
            build: |c| match c.other_token_account {
                Some(other) => (vec![valid_memo("other-account"), mint_ix_replacing(c, 3, AccountMeta::new(other, false))], vec![]),
                None => (vec![], vec![]),
            } },
        Case { category: "mint-authority-attacks", name: "Nonexistent token account", expect: Expect::Reject(&["AccountNotInitialized"]),
            build: |c| (vec![valid_memo("no-account"), mint_ix_replacing(c, 3, AccountMeta::new(Pubkey::new_unique(), false))], vec![]) },
        Case { category: "mint-authority-attacks", name: "Mint account passed as token account", expect: Expect::Reject(&["UninitializedAccount", "AccountDidNotDeserialize", "InvalidTokenAccount"]),
            build: |c| (vec![valid_memo("mint-as-account"), mint_ix_replacing(c, 3, AccountMeta::new(c.mint, false))], vec![]) },

        // --- accounts ---
        Case { category: "account-attacks", name: "Legacy SPL Token program instead of Token-2022", expect: Expect::Reject(&["InvalidProgramId"]),
            build: |c| (vec![valid_memo("spl-token"), mint_ix_replacing(c, 4, AccountMeta::new_readonly(pk(SPL_TOKEN_LEGACY), false))], vec![]) },
        Case { category: "account-attacks", name: "Fake instructions sysvar", expect: Expect::Reject(&["ConstraintAddress"]),
            build: |c| (vec![valid_memo("fake-sysvar"), mint_ix_replacing(c, 5, AccountMeta::new_readonly(Pubkey::new_unique(), false))], vec![]) },
        Case { category: "account-attacks", name: "User and mint accounts swapped", expect: Expect::Reject(&["AccountNotSigner", "AccountDidNotDeserialize", "AccountOwnedByWrongProgram"]),
            build: |c| {
                let mut accounts = mint_accounts(c);
                accounts.swap(0, 1);
                (vec![valid_memo("swapped"), mint_ix_with(c, accounts)], vec![])
            } },
        Case { category: "account-attacks", name: "Mint and token account replaced by one random account", expect: Expect::Reject(&["AccountNotInitialized"]),
            build: |c| {
                let fake = Pubkey::new_unique();
                let mut accounts = mint_accounts(c);
                accounts[1] = AccountMeta::new(fake, false);
                accounts[3] = AccountMeta::new(fake, false);
                (vec![valid_memo("substitution"), mint_ix_with(c, accounts)], vec![])
            } },
        Case { category: "account-attacks", name: "20 extra accounts appended (ignored)", expect: Expect::Accept,
            build: |c| {
                let mut accounts = mint_accounts(c);
                accounts.extend((0..20).map(|_| AccountMeta::new_readonly(Pubkey::new_unique(), false)));
                (vec![valid_memo("extra-accounts"), mint_ix_with(c, accounts)], vec![])
            } },
        Case { category: "account-attacks", name: "Extra instruction data requesting u64::MAX (ignored, normal amount)", expect: Expect::Accept,
            build: |c| {
                let mut data = discriminator("process_mint").to_vec();
                data.extend_from_slice(&u64::MAX.to_le_bytes());
                (vec![valid_memo("extra-data"), Instruction::new_with_bytes(c.program_id, &data, mint_accounts(c))], vec![])
            } },
        Case { category: "account-attacks", name: "Unknown instruction discriminator", expect: Expect::Reject(&["InstructionFallbackNotFound"]),
            build: |c| (vec![valid_memo("bad-discriminator"), Instruction::new_with_bytes(c.program_id, &[0u8; 8], mint_accounts(c))], vec![]) },

        // --- resource limits ---
        Case { category: "dos-attacks", name: "Compute limit too low (whole transaction fails, nothing minted)", expect: Expect::Reject(&["ComputationalBudgetExceeded", "exceeded CUs meter"]),
            build: |c| (vec![memo_len("long ", 780), mint_ix(c), cu_limit(1_000)], vec![]) },
        Case { category: "dos-attacks", name: "One memo, 32 mints", expect: Expect::Reject(&["OneMintPerTransaction", "MaxInstructionTraceLengthExceeded"]),
            build: |c| {
                let mut ixs = vec![valid_memo("batch32")];
                ixs.extend((0..32).map(|_| mint_ix(c)));
                ixs.push(cu_limit(1_400_000));
                (ixs, vec![])
            } },

        // --- process_mint_to ---
        Case { category: "mint-to-attacks", name: "mint_to own account with matching recipient", expect: Expect::Accept,
            build: |c| (vec![valid_memo("mint-to-self"), mint_to_ix(c, c.payer.pubkey(), c.token_account)], vec![]) },
        Case { category: "mint-to-attacks", name: "mint_to another user's account with matching recipient (allowed)", expect: Expect::Accept,
            build: |c| match c.other_token_account.and_then(|a| token_account_owner(c, &a).map(|o| (a, o))) {
                Some((account, owner)) => (vec![valid_memo("mint-to-other"), mint_to_ix(c, owner, account)], vec![]),
                None => (vec![], vec![]),
            } },
        Case { category: "mint-to-attacks", name: "mint_to with recipient not matching the account owner", expect: Expect::Reject(&["UnauthorizedTokenAccount"]),
            build: |c| (vec![valid_memo("mint-to-mismatch"), mint_to_ix(c, Pubkey::new_unique(), c.token_account)], vec![]) },
        Case { category: "mint-to-attacks", name: "mint_to without the recipient argument", expect: Expect::Reject(&["InstructionDidNotDeserialize"]),
            build: |c| {
                let mut ix = mint_to_ix(c, c.payer.pubkey(), c.token_account);
                ix.data.truncate(8);
                (vec![valid_memo("mint-to-no-arg"), ix], vec![])
            } },
        Case { category: "mint-to-attacks", name: "mint_to without a memo", expect: Expect::Reject(&["MemoRequired"]),
            build: |c| (vec![mint_to_ix(c, c.payer.pubkey(), c.token_account)], vec![]) },
        Case { category: "mint-to-attacks", name: "mint_to two different recipients in one transaction", expect: Expect::Reject(&["OneMintPerTransaction"]),
            build: |c| match c.other_token_account.and_then(|a| token_account_owner(c, &a).map(|o| (a, o))) {
                Some((account, owner)) => (vec![valid_memo("two-recipients"), mint_to_ix(c, c.payer.pubkey(), c.token_account), mint_to_ix(c, owner, account)], vec![]),
                None => (vec![], vec![]),
            } },
    ]
}

/// Owner of a Token-2022 token account (bytes 32..64 of the account data)
fn token_account_owner(ctx: &Ctx, account: &Pubkey) -> Option<Pubkey> {
    let data = ctx.client.get_account_data(account).ok()?;
    let owner: [u8; 32] = data.get(32..64)?.try_into().ok()?;
    Some(Pubkey::new_from_array(owner))
}
