/// Prepare account files for a local solana-test-validator (used by scripts/local-validator.sh)
///
/// Copies the admin-initialized state accounts (mint state, global burn stats, counters,
/// leaderboards) and the MEMO token mint from a source cluster (testnet by default), then:
///   - sets the mint's supply to any value (to test the emission curve steps and burn minimums)
///   - creates the wallet's Token-2022 ATA with any balance (no need to mint large amounts)
/// Every account is written as <out-dir>/<pubkey>.json, loaded with `--account-dir`.
///
/// Usage:
///   prepare-local-validator --out-dir <dir> --wallet <pubkey> [--supply <tokens>]
///                           [--balance <tokens>] [--source-rpc <url>]
///
/// --supply defaults to the source cluster's current supply; --balance defaults to 1,000,000.
/// Amounts accept up to 6 decimals (e.g. "10000000.000001").

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey, rent::Rent};
use spl_token_2022::{
    extension::{
        immutable_owner::ImmutableOwner, BaseStateWithExtensionsMut, ExtensionType,
        StateWithExtensionsMut,
    },
    state::{Account as TokenAccount, AccountState, Mint},
};
use std::{fs, path::PathBuf, str::FromStr};

use memo_token_client::{format_token_amount, get_program_id, get_token_mint, parse_token_amount};

const DEFAULT_SOURCE_RPC: &str = "https://rpc.testnet.x1.xyz";
const DEFAULT_BALANCE: &str = "1000000";

/// Admin-initialized PDAs the programs expect to exist: (program, seed)
const STATE_ACCOUNTS: &[(&str, &[u8])] = &[
    ("memo_mint", b"mint_state"),
    ("memo_burn", b"global_burn_stats"),
    ("memo_chat", b"global_counter"),
    ("memo_chat", b"burn_leaderboard"),
    ("memo_project", b"global_counter"),
    ("memo_project", b"burn_leaderboard"),
    ("memo_forum", b"global_counter"),
];

struct Args {
    out_dir: PathBuf,
    wallet: Pubkey,
    supply: Option<u64>,
    balance: u64,
    source_rpc: String,
}

fn parse_args() -> Result<Args, String> {
    let mut out_dir = None;
    let mut wallet = None;
    let mut supply = None;
    let mut balance = parse_token_amount(DEFAULT_BALANCE);
    let mut source_rpc = DEFAULT_SOURCE_RPC.to_string();

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("Missing value for {}", flag))?;
        match flag.as_str() {
            "--out-dir" => out_dir = Some(PathBuf::from(value)),
            "--wallet" => wallet = Some(Pubkey::from_str(&value).map_err(|e| format!("Invalid wallet: {}", e))?),
            "--supply" => supply = Some(parse_token_amount(&value).ok_or(format!("Invalid supply: {}", value))?),
            "--balance" => balance = Some(parse_token_amount(&value).ok_or(format!("Invalid balance: {}", value))?),
            "--source-rpc" => source_rpc = value,
            _ => return Err(format!("Unknown option: {}", flag)),
        }
    }

    Ok(Args {
        out_dir: out_dir.ok_or("--out-dir is required")?,
        wallet: wallet.ok_or("--wallet is required")?,
        supply,
        balance: balance.unwrap(),
        source_rpc,
    })
}

/// Write an account in the JSON format of `solana account --output json`
fn write_account(out_dir: &PathBuf, pubkey: &Pubkey, account: &Account) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::json!({
        "pubkey": pubkey.to_string(),
        "account": {
            "lamports": account.lamports,
            "data": [BASE64.encode(&account.data), "base64"],
            "owner": account.owner.to_string(),
            "executable": account.executable,
            "rentEpoch": 0,
            "space": account.data.len(),
        }
    });
    fs::write(out_dir.join(format!("{}.json", pubkey)), serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|e| {
        eprintln!("Usage: prepare-local-validator --out-dir <dir> --wallet <pubkey> [--supply <tokens>] [--balance <tokens>] [--source-rpc <url>]");
        e
    })?;
    fs::create_dir_all(&args.out_dir)?;
    let source = RpcClient::new(args.source_rpc.clone());
    println!("Source cluster: {}", args.source_rpc);

    // State accounts, copied as-is
    for (program, seed) in STATE_ACCOUNTS {
        let program_id = get_program_id(program)?;
        let (pda, _) = Pubkey::find_program_address(&[seed], &program_id);
        match source.get_account(&pda) {
            Ok(account) => {
                write_account(&args.out_dir, &pda, &account)?;
                println!("  {} {:<18} {}", program, String::from_utf8_lossy(seed), pda);
            }
            Err(_) => println!("  {} {:<18} {} (not found on source, skipped)", program, String::from_utf8_lossy(seed), pda),
        }
    }

    // Token mint, with the supply overridden
    let mint = get_token_mint("memo_token")?;
    let mut mint_account = source.get_account(&mint)?;
    let supply = {
        let mut state = StateWithExtensionsMut::<Mint>::unpack(&mut mint_account.data)?;
        if let Some(supply) = args.supply {
            state.base.supply = supply;
            state.pack_base();
        }
        state.base.supply
    };
    write_account(&args.out_dir, &mint, &mint_account)?;
    println!("  token mint         {} (supply {} tokens)", mint, format_token_amount(supply));

    // Wallet ATA with the requested balance (the token supply is not adjusted to match)
    let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
        &args.wallet, &mint, &spl_token_2022::id(),
    );
    let space = ExtensionType::try_calculate_account_len::<TokenAccount>(&[ExtensionType::ImmutableOwner])?;
    let mut data = vec![0u8; space];
    {
        let mut state = StateWithExtensionsMut::<TokenAccount>::unpack_uninitialized(&mut data)?;
        state.base = TokenAccount {
            mint,
            owner: args.wallet,
            amount: args.balance,
            state: AccountState::Initialized,
            ..TokenAccount::default()
        };
        state.pack_base();
        state.init_account_type()?;
        state.init_extension::<ImmutableOwner>(true)?;
    }
    let ata_account = Account {
        lamports: Rent::default().minimum_balance(space),
        data,
        owner: spl_token_2022::id(),
        executable: false,
        rent_epoch: 0,
    };
    write_account(&args.out_dir, &ata, &ata_account)?;
    println!("  wallet ATA         {} (balance {} tokens)", ata, format_token_amount(args.balance));

    Ok(())
}
