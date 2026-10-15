// Post-deployment verification: read-only checks and simulations, no transaction is sent.
//
// Uses the network and program IDs from Anchor.toml (cluster + program_env), so it works for
// testnet, prod staging (mainnet IDs on testnet) and mainnet. Needs no private key: the deploy
// admin's public key comes from ~/.config/solana/memo-token/<program_env>/authority/deploy_admin-keypair.pub.
//
// Usage:
//   cargo run --bin verify-deployment                 # simulate as the deploy admin
//   cargo run --bin verify-deployment -- <USER>       # simulate as a wallet that holds MEMO
//
// Simulations skip signature verification, so <USER> can be any address. The mint simulations
// need its MEMO token account; the burn simulations also need its user global burn stats
// account and at least 1 MEMO. Missing prerequisites are reported as SKIP.
//
// Exit codes: 0 = no check failed, 1 = at least one check failed.

use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::{Transaction, TransactionError},
};
// solana-loader-v3-interface is not a dependency of this crate
#[allow(deprecated)]
use solana_sdk::bpf_loader_upgradeable::{self, UpgradeableLoaderState};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_token_2022::{
    extension::StateWithExtensions,
    id as token_2022_id,
    state::{Account as TokenAccount, Mint},
};
use base64::{Engine as _, engine::general_purpose};
use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::str::FromStr;
use memo_token_client::{get_authority_keypair_path, get_program_id, get_rpc_url, get_token_mint};

const PROGRAMS: [&str; 7] = [
    "memo_mint", "memo_burn", "memo_chat", "memo_profile", "memo_project", "memo_blog", "memo_forum",
];

const MINT_STATE_SPACE: usize = 8 + 32 + 8 + 1 + 64;
const GLOBAL_BURN_STATS_SPACE: usize = 8 + 16 + 8 + 8 + 1 + 64;
const USER_GLOBAL_BURN_STATS_SPACE: usize = 8 + 32 + 8 + 8 + 8 + 1;

const ONE_MINT_PER_TRANSACTION: u32 = 6008; // memo-mint ErrorCode::OneMintPerTransaction
const ACCOUNT_NOT_ENOUGH_KEYS: u32 = 3005;  // Anchor ErrorCode::AccountNotEnoughKeys

const DECIMAL_FACTOR: u64 = 1_000_000;
const SIM_CU_LIMIT: u32 = 400_000;

#[derive(BorshSerialize)]
struct BurnMemo {
    version: u8,
    burn_amount: u64,
    payload: Vec<u8>,
}

#[derive(Default)]
struct Report {
    pass: u32,
    fail: u32,
    skip: u32,
}

impl Report {
    fn pass(&mut self, msg: impl AsRef<str>) {
        self.pass += 1;
        println!("  ✅ PASS  {}", msg.as_ref());
    }
    fn fail(&mut self, msg: impl AsRef<str>) {
        self.fail += 1;
        println!("  ❌ FAIL  {}", msg.as_ref());
    }
    fn skip(&mut self, msg: impl AsRef<str>) {
        self.skip += 1;
        println!("  ⏭️  SKIP  {}", msg.as_ref());
    }
    fn check(&mut self, ok: bool, msg: impl AsRef<str>) {
        if ok { self.pass(msg) } else { self.fail(msg) }
    }
}

fn main() {
    match run() {
        Ok(report) => {
            println!();
            println!("=== SUMMARY: {} passed, {} failed, {} skipped ===", report.pass, report.fail, report.skip);
            if report.fail > 0 {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("❌ {}", e);
            std::process::exit(1);
        }
    }
}

fn run() -> Result<Report, Box<dyn std::error::Error>> {
    println!("=== MEMO TOKEN DEPLOYMENT VERIFICATION (read-only, nothing is sent) ===");
    let rpc_url = get_rpc_url();
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let mut report = Report::default();

    // The deploy admin's public key, from the key tree of the current program_env (works while locked)
    let admin_keypair = get_authority_keypair_path("deploy_admin");
    let admin_pub = admin_keypair.with_extension("pub");
    let admin = match std::fs::read_to_string(&admin_pub) {
        Ok(text) => Pubkey::from_str(text.trim()).map_err(|e| format!("invalid {}: {}", admin_pub.display(), e))?,
        Err(_) => read_keypair_file(&admin_keypair)
            .map_err(|e| format!("cannot read the deploy admin ({} or {}): {}", admin_pub.display(), admin_keypair.display(), e))?
            .pubkey(),
    };
    let user = match std::env::args().nth(1) {
        Some(s) => Pubkey::from_str(&s).map_err(|e| format!("invalid user address {}: {}", s, e))?,
        None => admin,
    };
    let mint = get_token_mint("memo_token")?;
    let memo_mint_id = get_program_id("memo_mint")?;
    let memo_burn_id = get_program_id("memo_burn")?;
    println!("Upgrade authority (deploy_admin): {}", admin);
    println!("Simulation user: {}", user);
    println!("MEMO mint: {}", mint);

    // 1. Programs: deployed, upgradeable by deploy_admin, bytecode matches the local build
    println!();
    println!("[1/4] Programs");
    let deploy_dir = find_project_root().join("target/deploy");
    for name in PROGRAMS {
        let program_id = get_program_id(name)?;
        check_program(&client, &mut report, name, &program_id, &admin, &deploy_dir.join(format!("{}.so", name)));
    }

    // 2. Token mint authority is the memo-mint PDA
    println!();
    println!("[2/4] Token mint");
    let (mint_authority_pda, _) = Pubkey::find_program_address(&[b"mint_authority"], &memo_mint_id);
    match client.get_account(&mint) {
        Ok(account) => {
            report.check(account.owner == token_2022_id(), format!("MEMO mint owned by Token-2022 ({})", account.owner));
            match StateWithExtensions::<Mint>::unpack(&account.data) {
                Ok(state) => {
                    let authority: Option<Pubkey> = state.base.mint_authority.into();
                    report.check(
                        authority == Some(mint_authority_pda),
                        format!("mint authority is the memo-mint PDA {} (on-chain: {:?})", mint_authority_pda, authority),
                    );
                    println!("        supply: {} MEMO", state.base.supply as f64 / DECIMAL_FACTOR as f64);
                }
                Err(e) => report.fail(format!("cannot decode MEMO mint: {}", e)),
            }
        }
        Err(e) => report.fail(format!("MEMO mint not found: {}", e)),
    }

    // 3. Global state accounts created by the deploy script
    println!();
    println!("[3/4] Global state accounts");
    let (mint_state, _) = Pubkey::find_program_address(&[b"mint_state"], &memo_mint_id);
    if let Some(data) = check_state_account(&client, &mut report, "mint_state", &mint_state, &memo_mint_id, "MintState", MINT_STATE_SPACE, "mint") {
        println!("        last_slot: {}", u64::from_le_bytes(data[40..48].try_into().unwrap()));
    }
    let (global_burn_stats, _) = Pubkey::find_program_address(&[b"global_burn_stats"], &memo_burn_id);
    if let Some(data) = check_state_account(&client, &mut report, "global_burn_stats", &global_burn_stats, &memo_burn_id, "GlobalBurnStats", GLOBAL_BURN_STATS_SPACE, "burn") {
        let total = u128::from_le_bytes(data[8..24].try_into().unwrap());
        let count = u64::from_le_bytes(data[24..32].try_into().unwrap());
        println!("        total_burned: {} MEMO, burn_count: {}", total as f64 / DECIMAL_FACTOR as f64, count);
    }

    // 4. Simulations (never sent)
    println!();
    println!("[4/4] Simulations as {}", user);
    let token_account = get_associated_token_address_with_program_id(&user, &mint, &token_2022_id());
    let balance = match client.get_account(&token_account) {
        Ok(account) => StateWithExtensions::<TokenAccount>::unpack(&account.data).ok().map(|s| s.base.amount),
        Err(_) => None,
    };

    let mint_ix = |with_mint_state: bool| {
        let mut accounts = vec![
            AccountMeta::new(user, true),
            AccountMeta::new(mint, false),
            AccountMeta::new_readonly(mint_authority_pda, false),
            AccountMeta::new(token_account, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ];
        if with_mint_state {
            accounts.push(AccountMeta::new(mint_state, false));
        }
        Instruction::new_with_bytes(memo_mint_id, &sighash("process_mint"), accounts)
    };
    let mint_memo = spl_memo::build_memo(
        format!("verify-deployment: simulated mint, never sent. user={}", user).as_bytes(),
        &[],
    );

    match balance {
        None => {
            report.skip(format!("mint simulations: {} has no MEMO token account (pass a wallet that holds MEMO)", user));
        }
        Some(_) => {
            let result = simulate(&client, &user, vec![mint_memo.clone(), mint_ix(true)]);
            report.check(result.is_ok(), format!("single mint succeeds ({})", describe(&result)));

            let result = simulate(&client, &user, vec![mint_memo.clone(), mint_ix(true), mint_ix(true)]);
            report.check(
                custom_error(&result) == Some((2, ONE_MINT_PER_TRANSACTION)),
                format!("second mint in the same transaction rejected with OneMintPerTransaction ({})", describe(&result)),
            );

            let result = simulate(&client, &user, vec![mint_memo.clone(), mint_ix(false)]);
            report.check(
                custom_error(&result) == Some((1, ACCOUNT_NOT_ENOUGH_KEYS)),
                format!("old-client mint (no mint_state) rejected ({})", describe(&result)),
            );
        }
    }

    let (user_global_burn_stats, _) = Pubkey::find_program_address(&[b"user_global_burn_stats", user.as_ref()], &memo_burn_id);
    let has_user_stats = client
        .get_account(&user_global_burn_stats)
        .map(|a| a.owner == memo_burn_id && a.data.len() == USER_GLOBAL_BURN_STATS_SPACE)
        .unwrap_or(false);
    match balance {
        Some(amount) if amount >= DECIMAL_FACTOR && has_user_stats => {
            let burn_amount = DECIMAL_FACTOR;
            let memo = BurnMemo {
                version: 1,
                burn_amount,
                payload: format!("verify-deployment: simulated burn, never sent. user={}", user).into_bytes(),
            };
            let memo_ix = spl_memo::build_memo(general_purpose::STANDARD.encode(borsh::to_vec(&memo)?).as_bytes(), &[]);
            let burn_ix = |with_global_stats: bool| {
                let mut accounts = vec![
                    AccountMeta::new(user, true),
                    AccountMeta::new(mint, false),
                    AccountMeta::new(token_account, false),
                    AccountMeta::new(user_global_burn_stats, false),
                    AccountMeta::new_readonly(token_2022_id(), false),
                    AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
                ];
                if with_global_stats {
                    accounts.push(AccountMeta::new(global_burn_stats, false));
                }
                let mut data = sighash("process_burn");
                data.extend_from_slice(&burn_amount.to_le_bytes());
                Instruction::new_with_bytes(memo_burn_id, &data, accounts)
            };

            let result = simulate(&client, &user, vec![memo_ix.clone(), burn_ix(true)]);
            report.check(result.is_ok(), format!("burn of 1 MEMO succeeds ({})", describe(&result)));

            let result = simulate(&client, &user, vec![memo_ix, burn_ix(false)]);
            report.check(
                custom_error(&result) == Some((1, ACCOUNT_NOT_ENOUGH_KEYS)),
                format!("old-client burn (no global_burn_stats) rejected ({})", describe(&result)),
            );
        }
        _ => {
            report.skip(format!(
                "burn simulations: {} needs a user global burn stats account and at least 1 MEMO (pass a wallet that has burned before)",
                user
            ));
        }
    }

    Ok(report)
}

fn check_program(
    client: &RpcClient,
    report: &mut Report,
    name: &str,
    program_id: &Pubkey,
    admin: &Pubkey,
    local_so: &PathBuf,
) {
    println!("  {} {}", name, program_id);
    let program = match client.get_account(program_id) {
        Ok(account) => account,
        Err(e) => return report.fail(format!("{}: program not found: {}", name, e)),
    };
    if program.owner != bpf_loader_upgradeable::id() || !program.executable {
        return report.fail(format!("{}: not an upgradeable program (owner {})", name, program.owner));
    }
    let programdata_address = match bincode::deserialize(&program.data) {
        Ok(UpgradeableLoaderState::Program { programdata_address }) => programdata_address,
        _ => return report.fail(format!("{}: cannot decode program account", name)),
    };
    let programdata = match client.get_account(&programdata_address) {
        Ok(account) => account,
        Err(e) => return report.fail(format!("{}: program data not found: {}", name, e)),
    };
    let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
    let (slot, authority) = match bincode::deserialize(&programdata.data) {
        Ok(UpgradeableLoaderState::ProgramData { slot, upgrade_authority_address }) => (slot, upgrade_authority_address),
        _ => return report.fail(format!("{}: cannot decode program data", name)),
    };
    report.check(
        authority == Some(*admin),
        format!("{}: upgrade authority is deploy_admin (on-chain: {:?}), last deployed at slot {}", name, authority, slot),
    );

    // Program data is the ELF followed by zero padding (an account extended for upgrades)
    match std::fs::read(local_so) {
        Ok(local) => {
            let onchain = &programdata.data[metadata_len..];
            let matches = onchain.len() >= local.len()
                && onchain[..local.len()] == local[..]
                && onchain[local.len()..].iter().all(|b| *b == 0);
            report.check(
                matches,
                format!("{}: on-chain bytecode matches {} (sha256 {})", name, local_so.display(), short_hash(&local)),
            );
        }
        Err(_) => report.skip(format!("{}: no local build at {} to compare", name, local_so.display())),
    }
}

/// Checks owner, size and Anchor discriminator; returns the data when all match
fn check_state_account(
    client: &RpcClient,
    report: &mut Report,
    label: &str,
    address: &Pubkey,
    owner: &Pubkey,
    account_name: &str,
    space: usize,
    operation: &str,
) -> Option<Vec<u8>> {
    match client.get_account(address) {
        Ok(account) => {
            let discriminator = Sha256::digest(format!("account:{}", account_name).as_bytes());
            let ok = account.owner == *owner && account.data.len() == space && account.data[..8] == discriminator[..8];
            report.check(
                ok,
                format!("{} {} (owner {}, {} bytes, expected {})", label, address, account.owner, account.data.len(), space),
            );
            ok.then_some(account.data)
        }
        Err(_) => {
            report.fail(format!("{} {} does not exist: every {} fails until it is initialized", label, address, operation));
            None
        }
    }
}

fn simulate(client: &RpcClient, payer: &Pubkey, mut instructions: Vec<Instruction>) -> Result<u64, String> {
    // Compute budget after the memo, which must stay at index 0
    instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(SIM_CU_LIMIT));
    let transaction = Transaction::new_unsigned(Message::new(&instructions, Some(payer)));
    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        ..RpcSimulateTransactionConfig::default()
    };
    match client.simulate_transaction_with_config(&transaction, config) {
        Ok(response) => match response.value.err {
            None => Ok(response.value.units_consumed.unwrap_or(0)),
            Some(err) => Err(format!("{:?}", err)),
        },
        Err(e) => Err(format!("RPC error: {}", e)),
    }
}

/// (instruction index, custom error code) of a failed simulation
fn custom_error(result: &Result<u64, String>) -> Option<(u8, u32)> {
    let err = result.as_ref().err()?;
    for index in 0..8u8 {
        for code in [ONE_MINT_PER_TRANSACTION, ACCOUNT_NOT_ENOUGH_KEYS] {
            let expected = TransactionError::InstructionError(index, InstructionError::Custom(code));
            if *err == format!("{:?}", expected) {
                return Some((index, code));
            }
        }
    }
    None
}

fn describe(result: &Result<u64, String>) -> String {
    match result {
        Ok(units) => format!("{} CU", units),
        Err(e) => e.clone(),
    }
}

fn sighash(name: &str) -> Vec<u8> {
    Sha256::digest(format!("global:{}", name).as_bytes())[..8].to_vec()
}

fn short_hash(data: &[u8]) -> String {
    Sha256::digest(data)[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn find_project_root() -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for _ in 0..5 {
        if path.join("Anchor.toml").exists() {
            return path;
        }
        if !path.pop() {
            break;
        }
    }
    PathBuf::from("..")
}
