//! Reproductions of memo-id audit findings (third audit) on a local validator.
//!
//! Each case asserts the CURRENT behaviour on X1 mainnet's builds of Metaplex Core and
//! Token-2022 (dumped by scripts/local-validator.sh), so the audit can cite it as reproduced.
//! This is not a regression suite: several cases pass only while the issue exists and must be
//! inverted once it is fixed. Case ids name the finding (S21-4 is step 4 of finding S21). S21
//! (F19) and S17 (F20) were fixed in memo-id 0.2.0 and now assert the rejections.
//!   S26  a pre-funded MEMO ID Collection PDA: admin-init-memo-id refuses for good, init works
//!        (fresh validator only: runs while the Collection address is unused; elsewhere it is
//!        skipped, which fails the run unless MEMO_REGRESSION_ALLOW_SKIP=1)
//!   S21  issue_subid while the parent's Asset, or the holder, is frozen for a lender (fixed: Listed)
//!   S17  a holder that is not a memo-home Asset, with permanent delegates (fixed: rejected)
//!   S12  an ID owned by a Core Asset Signer keeps its records across the sale of the Asset
//!   S01  update_uri and issue_subid under Core ExecuteV1 fail with reentrancy
//!   S23  a Primary ID for Address(Asset Signer) stays verified across the sale of the Asset
//!   S00 / S20 / S11  error numbers shared by memo-id, memo-home and memo-burn
//!   S08  a forged Borsh length prefix in the BurnMemo aborts with out of memory
//!
//! Run on a fresh local validator (admin-init-memo-id must be built next to this binary for S26):
//!   (cd clients && cargo build --bin repro-memo-id-audit3 --bin admin-init-memo-id)
//!   ./scripts/local-validator.sh start --no-build --supply 5000000 --balance 1000000
//!   (cd clients && MEMO_PROGRAM_ENV=testnet MEMO_RPC_URL=http://127.0.0.1:8899 \
//!     MEMO_WALLET=../target/local-validator/wallet.json cargo run --bin repro-memo-id-audit3)
//!   ./scripts/local-validator.sh stop

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use harness::*;
use memo_token_client::{get_program_id, get_rpc_url, get_wallet_path, memo_id::MemoIdAddresses};
use sha2::{Digest, Sha256};
#[allow(deprecated)]
use solana_sdk::address_lookup_table::instruction as alt_ix;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::{write_keypair_file, Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_system_interface::instruction as sys_ix;

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");

// memo-id error codes
const INVALID_MINT: u32 = 6006;
const NOT_CONTROLLER: u32 = 6008;
const NOT_IN_HOLDER_VAULT: u32 = 6009;
const INVALID_MEMO_FORMAT: u32 = 6017;
// memo-home: InvalidCoreAccount has the number of memo-id's NotInHolderVault (S00, S20)
const HOME_INVALID_CORE_ACCOUNT: u32 = 6009;
const HOME_LISTED: u32 = 6011;
// memo-id 0.2.0
const HOLDER_NOT_HOME_ASSET: u32 = 6026;
const LISTED: u32 = 6027;
// memo-burn: the numbers of memo-id's InvalidMint and NotController (S11)
const BURN_INVALID_MEMO_FORMAT: u32 = 6001;
const BURN_INVALID_TOKEN_ACCOUNT: u32 = 6006;
const BURN_UNAUTHORIZED_TOKEN_ACCOUNT: u32 = 6008;
// Metaplex Core (MplCoreError)
const CORE_INVALID_AUTHORITY: u32 = 9;
const CORE_NO_APPROVALS: u32 = 26;

const URI: &str = "https://example.com/memo-id.json";
/// Index of the memo-id instruction in [memo, compute budget, ix]
const IX: u64 = 2;

fn sha(b: &[u8]) -> [u8; 32] { Sha256::digest(b).into() }
fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Address(Pubkey),
    Asset(Pubkey),
}
impl Target {
    fn key(&self) -> Pubkey { match self { Target::Address(k) | Target::Asset(k) => *k } }
    fn encode(&self, v: &mut Vec<u8>) {
        match self {
            Target::Address(_) => v.push(0),
            Target::Asset(_) => v.push(1),
        }
        v.extend(self.key().as_ref());
    }
}

/// Registry as read from chain
#[derive(Debug, PartialEq)]
struct Reg {
    kind: u8,
    asset: Pubkey,
    parent: Pubkey,
    nft_subids_issued: u32,
    record_subids: u32,
    target_set_by: Pubkey,
    target_holder: Pubkey,
    target: Option<Target>,
    memo_id: String,
}

fn pk(d: &[u8], at: usize) -> Pubkey { Pubkey::new_from_array(d[at..at + 32].try_into().unwrap()) }
fn u32_at(d: &[u8], at: usize) -> u32 { u32::from_le_bytes(d[at..at + 4].try_into().unwrap()) }

/// Fixed layout: target_set_by 90, target_holder 122, has_target 154, target 155, memo_id 188
fn parse_reg(d: &[u8]) -> Reg {
    let target = match d[154] {
        0 => None,
        _ => {
            let k = pk(d, 156);
            Some(if d[155] == 0 { Target::Address(k) } else { Target::Asset(k) })
        }
    };
    let len = u32_at(d, 188) as usize;
    Reg {
        kind: d[8],
        asset: pk(d, 9),
        parent: pk(d, 41),
        nft_subids_issued: u32_at(d, 81),
        record_subids: u32_at(d, 85),
        target_set_by: pk(d, 90),
        target_holder: pk(d, 122),
        target,
        memo_id: String::from_utf8(d[192..192 + len].to_vec()).unwrap(),
    }
}

/// Core AssetV1: (owner, collection, name, uri)
fn parse_asset(d: &[u8]) -> (Pubkey, Option<Pubkey>, String, String) {
    let owner = pk(d, 1);
    let (col, mut p) = match d[33] {
        2 => (Some(pk(d, 34)), 66),
        1 => (None, 66),
        _ => (None, 34),
    };
    let mut s = || {
        let len = u32_at(d, p) as usize;
        let v = String::from_utf8(d[p + 4..p + 4 + len].to_vec()).unwrap();
        p += 4 + len;
        v
    };
    let name = s();
    let uri = s();
    (owner, col, name, uri)
}

struct Id<'a> {
    e: &'a Env,
    prog: Pubkey,
    home: Pubkey,
    authority: Pubkey,
    home_authority: Pubkey,
    burn_guard: Pubkey,
    collection: Pubkey,
    config: Pubkey,
}

impl<'a> Id<'a> {
    fn new(e: &'a Env) -> Id<'a> {
        let prog = get_program_id("memo_id").expect("memo_id in Anchor.toml");
        let home = get_program_id("memo_home").expect("memo_home in Anchor.toml");
        let authority = Pubkey::find_program_address(&[b"authority"], &prog).0;
        let collection = Pubkey::find_program_address(&[b"collection", authority.as_ref(), &sha(b"memo")], &home).0;
        Id {
            e,
            prog,
            home,
            authority,
            home_authority: Pubkey::find_program_address(&[b"authority"], &home).0,
            burn_guard: Pubkey::find_program_address(&[b"burn_guard"], &home).0,
            collection,
            config: Pubkey::find_program_address(&[b"collection_config", collection.as_ref()], &home).0,
        }
    }
    fn registry(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"memo_id", &sha(memo_id.as_bytes())], &self.prog).0
    }
    fn asset(&self, memo_id: &str) -> Pubkey {
        Pubkey::find_program_address(&[b"asset", self.collection.as_ref(), &sha(memo_id.as_bytes())], &self.home).0
    }
    fn vault(&self, asset: &Pubkey) -> Pubkey { Pubkey::find_program_address(&[b"vault", asset.as_ref()], &self.home).0 }
    fn vault_state(&self, asset: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"vault_state", asset.as_ref()], &self.home).0
    }
    fn subid_index(&self, parent: &Pubkey, i: u32) -> Pubkey {
        Pubkey::find_program_address(&[b"subid", parent.as_ref(), &i.to_le_bytes()], &self.prog).0
    }
    /// Primary ID of a Target: [b"primary_id", kind (0 Address, 1 Asset), key]
    fn primary(&self, target: &Target) -> Pubkey {
        let kind = match target { Target::Address(_) => 0u8, Target::Asset(_) => 1 };
        Pubkey::find_program_address(&[b"primary_id", &[kind], target.key().as_ref()], &self.prog).0
    }
    fn reg(&self, memo_id: &str) -> Option<Reg> { self.e.data(&self.registry(memo_id)).map(|d| parse_reg(&d)) }
    fn asset_data(&self, asset: &Pubkey) -> Option<(Pubkey, Option<Pubkey>, String, String)> {
        self.e.data(asset).map(|d| parse_asset(&d))
    }
    fn owner(&self, asset: &Pubkey) -> Option<Pubkey> { self.asset_data(asset).map(|a| a.0) }
    /// Price in units, from the independent curve reference: ceil(reward × centi / 100)
    fn price(&self, centi: u64) -> u64 {
        let reward = curve_reference(self.e.supply()).expect("reward") as u128;
        (reward * centi as u128).div_ceil(100) as u64
    }
    fn opt(&self, k: Option<&Pubkey>) -> AccountMeta {
        AccountMeta::new_readonly(k.copied().unwrap_or(self.prog), false)
    }
    /// holder_config: the memo-home Collection Config of the holder's Collection, as clients
    /// derive it (a holder in no Collection gives the Config of the default address, which does
    /// not exist)
    fn holder_config(&self, holder: Option<&Pubkey>) -> AccountMeta {
        let cfg = holder.map(|h| {
            let col = self.e.data(h).filter(|d| d.len() > 66 && d[0] == 1).and_then(|d| parse_asset(&d).1).unwrap_or_default();
            Pubkey::find_program_address(&[b"collection_config", col.as_ref()], &self.home).0
        });
        self.opt(cfg.as_ref())
    }
    fn ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.prog, &d, accounts)
    }

    fn init_burn_guard(&self, payer: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(self.home, &disc("init_burn_guard"), vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(self.burn_guard, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn init(&self, payer: &Pubkey) -> Instruction {
        self.ix("init", vec![], vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new(self.collection, false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(self.burn_guard, false),
            AccountMeta::new(self.config, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn home_accounts(&self, memo_id: &str) -> Vec<AccountMeta> {
        let asset = self.asset(memo_id);
        vec![
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.collection, false),
            AccountMeta::new(asset, false),
            AccountMeta::new(self.vault_state(&asset), false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
        ]
    }
    fn burn_accounts(&self, payer: &Pubkey, mint: &Pubkey, token_account: &Pubkey) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(self.e.stats_pda(payer), false),
            AccountMeta::new(self.e.gbs, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(self.e.burn_prog, false),
        ]
    }

    /// register with explicit hash, label and accounts
    #[allow(clippy::too_many_arguments)]
    fn register_raw(&self, payer: &Pubkey, hash: [u8; 32], label: &str, uri: &str, memo_id_for_accounts: &str,
                    mint: &Pubkey, token_account: &Pubkey) -> Instruction {
        let mut a = hash.to_vec();
        bstr(&mut a, label);
        bstr(&mut a, uri);
        let mut accounts = vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(self.registry(memo_id_for_accounts), false),
        ];
        accounts.extend(self.home_accounts(memo_id_for_accounts));
        accounts.extend(self.burn_accounts(payer, mint, token_account));
        accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
        accounts.push(AccountMeta::new_readonly(system_program(), false));
        self.ix("register", a, accounts)
    }
    fn register(&self, payer: &Pubkey, label: &str, uri: &str) -> Instruction {
        let memo_id = format!("{}.memo", label);
        self.register_raw(payer, sha(memo_id.as_bytes()), label, uri, &memo_id, &self.e.mint, &self.e.ata(payer))
    }

    fn payload(operation: &str, payer: &Pubkey, memo_id: &str) -> Vec<u8> {
        let mut p = vec![1u8];
        bstr(&mut p, "memo_id");
        bstr(&mut p, operation);
        bstr(&mut p, &payer.to_string());
        bstr(&mut p, memo_id);
        p
    }
    /// [BurnMemo, compute budget, ix]
    fn tx(&self, amount: u64, payload: &[u8], ix: Instruction) -> Vec<Instruction> {
        vec![app_burn_memo(amount, payload), cu(1_000_000), ix]
    }

    #[allow(clippy::too_many_arguments)]
    fn create_subid(&self, payer: &Pubkey, controller: &Pubkey, parent_id: &str, parent_asset: &Pubkey, holder: Option<&Pubkey>,
                    label: &str, target: Option<Target>) -> Instruction {
        let memo_id = format!("{}.{}", label, parent_id);
        let mut a = sha(memo_id.as_bytes()).to_vec();
        bstr(&mut a, label);
        match target {
            Some(t) => {
                a.push(1);
                t.encode(&mut a);
            }
            None => a.push(0),
        }
        self.ix("create_subid", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(self.registry(parent_id), false),
            AccountMeta::new_readonly(*parent_asset, false),
            self.opt(holder),
            self.holder_config(holder),
            AccountMeta::new(self.registry(&memo_id), false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    #[allow(clippy::too_many_arguments)]
    fn issue_subid(&self, payer: &Pubkey, controller: &Pubkey, parent_id: &str, parent_asset: &Pubkey, holder: Option<&Pubkey>,
                   memo_id: &str, index: u32, new_owner: &Pubkey, uri: &str) -> Instruction {
        let parent = self.registry(parent_id);
        let mut a = index.to_le_bytes().to_vec();
        bstr(&mut a, uri);
        let mut accounts = vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(parent, false),
            AccountMeta::new_readonly(*parent_asset, false),
            self.opt(holder),
            self.holder_config(holder),
            AccountMeta::new(self.registry(memo_id), false),
            AccountMeta::new(self.subid_index(&parent, index), false),
            AccountMeta::new_readonly(*new_owner, false),
        ];
        accounts.extend(self.home_accounts(memo_id));
        accounts.extend(self.burn_accounts(payer, &self.e.mint, &self.e.ata(payer)));
        accounts.push(AccountMeta::new_readonly(IXS_SYSVAR, false));
        accounts.push(AccountMeta::new_readonly(system_program(), false));
        self.ix("issue_subid", a, accounts)
    }
    fn set_target(&self, controller: &Pubkey, memo_id: &str, parent_id: Option<&str>, asset: &Pubkey, holder: Option<&Pubkey>,
                  target: Option<Target>) -> Instruction {
        let accounts = vec![
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new(self.registry(memo_id), false),
            self.opt(parent_id.map(|p| self.registry(p)).as_ref()),
            AccountMeta::new_readonly(*asset, false),
            self.opt(holder),
            self.holder_config(holder),
        ];
        match target {
            Some(t) => {
                let mut a = vec![];
                t.encode(&mut a);
                self.ix("set_target", a, accounts)
            }
            None => self.ix("clear_target", vec![], accounts),
        }
    }
    fn set_primary(&self, payer: &Pubkey, signer: &Pubkey, memo_id: &str, target: Target, target_asset: Option<&Pubkey>,
                   holder: Option<&Pubkey>) -> Instruction {
        let mut a = vec![];
        target.encode(&mut a);
        self.ix("set_primary", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*signer, true),
            AccountMeta::new_readonly(self.registry(memo_id), false),
            AccountMeta::new(self.primary(&target), false),
            self.opt(target_asset),
            self.opt(holder),
            self.holder_config(holder),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn clear_primary(&self, signer: &Pubkey, target: &Target, target_asset: Option<&Pubkey>, holder: Option<&Pubkey>) -> Instruction {
        self.ix("clear_primary", vec![], vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new(self.primary(target), false),
            self.opt(target_asset),
            self.opt(holder),
            self.holder_config(holder),
        ])
    }
    fn update_uri(&self, payer: &Pubkey, controller: &Pubkey, memo_id: &str, asset: &Pubkey, holder: Option<&Pubkey>, uri: &str) -> Instruction {
        let mut a = vec![];
        bstr(&mut a, uri);
        self.ix("update_uri", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*controller, true),
            AccountMeta::new_readonly(self.registry(memo_id), false),
            AccountMeta::new(*asset, false),
            self.opt(holder),
            self.holder_config(holder),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(self.collection, false),
            AccountMeta::new_readonly(self.home_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(self.home, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    /// Primary ID: (target, registry, set_by)
    fn primary_data(&self, target: &Target) -> Option<(Target, Pubkey, Pubkey)> {
        self.e.data(&self.primary(target)).map(|d| {
            let k = pk(&d, 9);
            let t = if d[8] == 0 { Target::Address(k) } else { Target::Asset(k) };
            (t, pk(&d, 41), pk(&d, 73))
        })
    }
    /// Content of the frozen lookup table that register / issue_subid clients use
    fn fixed_accounts(&self) -> Vec<Pubkey> {
        MemoIdAddresses::new(self.prog, self.home).lookup_table_addresses(self.e.mint, self.e.burn_prog, self.e.gbs)
    }

    // ---------------- memo-home instructions, against the MEMO ID Collection Config ----------------
    fn home_ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.home, &d, accounts)
    }
    fn withdraw_lamports(&self, owner: &Pubkey, asset: &Pubkey, dest: &Pubkey, amount: u64) -> Instruction {
        self.home_ix("withdraw_lamports", amount.to_le_bytes().to_vec(), vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn withdraw_asset(&self, owner: &Pubkey, asset: &Pubkey, inner: &Pubkey, inner_col: &Pubkey, new_owner: &Pubkey) -> Instruction {
        self.home_ix("withdraw_asset", vec![], vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new_readonly(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(*inner, false),
            AccountMeta::new_readonly(*inner_col, false),
            AccountMeta::new_readonly(*new_owner, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
}

// ---------------- Metaplex Core instructions sent directly ----------------

/// Core instruction with the [asset, collection, payer, authority, system, log_wrapper] layout
fn core_ix(data: Vec<u8>, asset: &Pubkey, col: Option<&Pubkey>, payer: &Pubkey, authority: Option<&Pubkey>) -> Instruction {
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            match col { Some(c) => AccountMeta::new(*c, false), None => AccountMeta::new_readonly(CORE, false) },
            AccountMeta::new(*payer, true),
            match authority { Some(a) => AccountMeta::new_readonly(*a, true), None => AccountMeta::new_readonly(CORE, false) },
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data,
    }
}

/// TransferV1 signed by `authority` (the owner, or a delegate), who also pays; `col` = None for
/// an Asset in no Collection
fn core_transfer(asset: &Pubkey, col: Option<&Pubkey>, authority: &Pubkey, new_owner: &Pubkey) -> Instruction {
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            AccountMeta::new_readonly(*col.unwrap_or(&CORE), false),
            AccountMeta::new(*authority, true),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(*new_owner, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: vec![14, 0],
    }
}

/// AddPluginV1 of an owner-managed delegate plugin (1 freeze, 3 transfer) with authority
/// Address(delegate); the FreezeDelegate starts unfrozen
fn core_add_delegate(asset: &Pubkey, col: &Pubkey, owner: &Pubkey, plugin_type: u8, delegate: &Pubkey) -> Instruction {
    let mut d = vec![2u8, plugin_type];
    if plugin_type == 1 {
        d.push(0);
    }
    d.extend([1, 3]);
    d.extend(delegate.as_ref());
    core_ix(d, asset, Some(col), owner, None)
}

/// UpdatePluginV1 FreezeDelegate { frozen } signed by `authority`, who also pays
fn core_set_frozen(asset: &Pubkey, col: &Pubkey, authority: &Pubkey, frozen: bool) -> Instruction {
    core_ix(vec![6, 1, frozen as u8], asset, Some(col), authority, None)
}

/// RevokePluginAuthorityV1 of a plugin type, signed by the owner
fn core_revoke(asset: &Pubkey, col: &Pubkey, owner: &Pubkey, plugin_type: u8) -> Instruction {
    core_ix(vec![10, plugin_type], asset, Some(col), owner, None)
}

/// BurnV1 of an Asset in no Collection, signed by `authority` (owner or burn delegate), who pays
fn core_burn(asset: &Pubkey, authority: &Pubkey) -> Instruction {
    core_ix(vec![12, 0], asset, None, authority, None)
}

/// CreateV1 of a Core Asset at keypair address `asset`, owned by `owner`, in no Collection
/// (update authority = payer), with the given plugins (already encoded, after the Option tag)
fn core_create(asset: &Pubkey, payer: &Pubkey, owner: &Pubkey, plugins: Option<Vec<u8>>) -> Instruction {
    let mut d = vec![0u8, 0];
    bstr(&mut d, "plain core asset");
    bstr(&mut d, "https://example.com/plain.json");
    match plugins {
        Some(p) => {
            d.push(1);
            d.extend(p);
        }
        None => d.push(0),
    }
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, true),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
    }
}
fn core_create_plain(asset: &Pubkey, payer: &Pubkey, owner: &Pubkey) -> Instruction { core_create(asset, payer, owner, None) }

/// CreateV1 with PermanentTransferDelegate (7) and PermanentBurnDelegate (8), both with
/// authority Address(delegate): permanent delegates can only be added at creation
fn core_create_with_permanent(asset: &Pubkey, payer: &Pubkey, owner: &Pubkey, delegate: &Pubkey) -> Instruction {
    let mut p = 2u32.to_le_bytes().to_vec();
    for t in [7u8, 8] {
        p.push(t);
        p.extend([1, 3]);
        p.extend(delegate.as_ref());
    }
    core_create(asset, payer, owner, Some(p))
}

/// Asset Signer of a Core Asset: the PDA ["mpl-core-execute", asset] that Core signs for in ExecuteV1
fn asset_signer(asset: &Pubkey) -> Pubkey { Pubkey::find_program_address(&[b"mpl-core-execute", asset.as_ref()], &CORE).0 }

/// ExecuteV1 (variant 31) on an Asset in no Collection: its owner (`payer`, also the authority)
/// has the Asset Signer sign `inner`. The inner accounts follow as remaining accounts; the Asset
/// Signer is not a transaction signer (Core marks it as signer in the CPI)
fn core_execute(asset: &Pubkey, payer: &Pubkey, inner: &Instruction) -> Instruction {
    let s = asset_signer(asset);
    let mut data = vec![31u8];
    data.extend((inner.data.len() as u32).to_le_bytes());
    data.extend(&inner.data);
    let s_writable = inner.accounts.iter().any(|m| m.pubkey == s && m.is_writable);
    let mut accounts = vec![
        AccountMeta::new(*asset, false),
        AccountMeta::new_readonly(CORE, false), // collection: none
        if s_writable { AccountMeta::new(s, false) } else { AccountMeta::new_readonly(s, false) },
        AccountMeta::new(*payer, true),
        AccountMeta::new_readonly(CORE, false), // authority: none, the payer
        AccountMeta::new_readonly(system_program(), false),
        AccountMeta::new_readonly(inner.program_id, false),
    ];
    accounts.extend(inner.accounts.iter().map(|m| AccountMeta {
        pubkey: m.pubkey,
        is_signer: m.is_signer && m.pubkey != s,
        is_writable: m.is_writable,
    }));
    Instruction { program_id: CORE, accounts, data }
}

// ---------------- lookup table and v0 transactions ----------------

fn create_frozen_alt(e: &Env, accounts: Vec<Pubkey>) -> AddressLookupTableAccount {
    let w = &e.wallet;
    let slot = e.c.get_slot_with_commitment(CommitmentConfig::finalized()).expect("slot");
    #[allow(deprecated)]
    let (create, table) = alt_ix::create_lookup_table(w.pubkey(), w.pubkey(), slot);
    #[allow(deprecated)]
    let extend = alt_ix::extend_lookup_table(table, w.pubkey(), Some(w.pubkey()), accounts.clone());
    e.must("create and extend the lookup table", &[create, extend], w, &[]);
    #[allow(deprecated)]
    let freeze = alt_ix::freeze_lookup_table(table, w.pubkey());
    e.must("freeze the lookup table", &[freeze], w, &[]);
    let after = e.c.get_slot().unwrap() + 2;
    while e.c.get_slot().unwrap() < after {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    AddressLookupTableAccount { key: table, addresses: accounts }
}

fn exec_v0(e: &Env, ixs: &[Instruction], payer: &Keypair, alt: &AddressLookupTableAccount) -> Out {
    let bh = e.c.get_latest_blockhash().expect("blockhash");
    let msg = v0::Message::try_compile(&payer.pubkey(), ixs, std::slice::from_ref(alt), bh).expect("compile v0");
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[payer]).expect("sign v0");
    match e.c.send_transaction_with_config(&tx, RpcSendTransactionConfig { skip_preflight: true, max_retries: Some(10), ..Default::default() }) {
        Ok(sig) => e.wait(&sig),
        Err(err) => Out { err: Some(serde_json::json!(format!("RPC rejected: {}", err))), ..Default::default() },
    }
}

// ---------------- outcome checks ----------------

/// Owner, lamports and data of an account
fn account(e: &Env, k: &Pubkey) -> Option<(Pubkey, u64, Vec<u8>)> { e.raw(&[*k]).pop().flatten() }
fn has_log(out: &Out, s: &str) -> bool { out.logs.iter().any(|l| l.contains(s)) }
/// Prints the log lines that name the error (evidence for the audit report)
fn evidence(out: &Out) {
    for l in out.logs.iter().filter(|l| {
        l.contains("Error") || l.contains("failed") || l.contains("eentrancy") || l.contains("memory") || l.contains("panicked")
            || l.contains("does not match")
    }) {
        println!("     log: {}", l);
    }
}

/// Exact rejection: `expect`; when `raised` is given, the innermost failing frame is that program
/// and the first AnchorError names that error; every `logs` text appears and no `absent` text
/// does; the watched balances, burn statistics, supply and the raw `accounts` are unchanged
#[allow(clippy::too_many_arguments)]
fn check_reject(e: &Env, case: &str, desc: &str, send: &dyn Fn() -> Out, expect: Expect, raised: Option<(&Pubkey, &str)>,
                logs: &[&str], absent: &[&str], tokens: &[Pubkey], users: &[Pubkey], accounts: &[Pubkey]) -> Out {
    let raw = |e: &Env| accounts.iter().map(|k| account(e, k)).collect::<Vec<_>>();
    let (before, before_raw) = (e.snap(tokens, users), raw(e));
    let out = send();
    let (after, after_raw) = (e.snap(tokens, users), raw(e));
    let named = raised.is_none_or(|(p, n)| out.failed_program() == Some(*p) && out.error_name().as_deref() == Some(n));
    let logs_ok = logs.iter().all(|s| has_log(&out, s)) && !absent.iter().any(|s| has_log(&out, s));
    let unchanged = before == after && before_raw == after_raw;
    let pass = expect.matches(&out) && named && logs_ok && unchanged;
    e.record(case, desc, pass, &format!(
        "expected {} raised by {:?}, logs {:?}, none of {:?}, no state change; got {} from {:?} ({:?}); {}; accounts unchanged {}",
        expect.describe(), raised, logs, absent, out.brief(), out.failed_program(), out.error_name(), before.diff(&after),
        before_raw == after_raw));
    evidence(&out);
    if !pass {
        println!("     all logs:\n       {}", out.logs.join("\n       "));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn reject(e: &Env, case: &str, desc: &str, ixs: Vec<Instruction>, payer: &Keypair, signers: &[&Keypair], expect: Expect,
          raised: Option<(&Pubkey, &str)>, accounts: &[Pubkey]) -> Out {
    check_reject(e, case, desc, &|| e.exec(&ixs, payer, signers), expect, raised, &[], &[], &[], &[], accounts)
}

/// Success check with the outcome printed on failure
fn ok_case(e: &Env, case: &str, desc: &str, out: &Out, pass: bool, detail: &str) {
    e.record(case, desc, out.ok() && pass, &format!("{}; {}", out.brief(), detail));
    if !(out.ok() && pass) {
        println!("     all logs:\n       {}", out.logs.join("\n       "));
    } else if !detail.is_empty() {
        println!("     {}", detail);
    }
}

/// A free label of `len` characters
fn free_label(id: &Id, len: usize, salt: u64) -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    for n in salt.. {
        let mut x = n;
        let mut s = String::new();
        for _ in 0..len {
            s.push(CH[(x % 36) as usize] as char);
            x /= 36;
        }
        if id.reg(&format!("{}.memo", s)).is_none() {
            return s;
        }
    }
    unreachable!()
}

/// Registers a free `len`-character label for `user` (setup); returns the MEMO ID
fn register_free(e: &Env, id: &Id, user: &Keypair, len: usize, price5: u64) -> String {
    let label = free_label(id, len, e.nonce());
    let memo_id = format!("{}.memo", label);
    let up = user.pubkey();
    e.must(&format!("register {}", memo_id), &id.tx(price5, &Id::payload("register", &up, &memo_id), id.register(&up, &label, URI)), user, &[]);
    memo_id
}

fn ata_of_mint(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address_with_program_id(owner, mint, &t22())
}

// ------------------------------------------------------------------------------------------------

fn main() {
    let e = Env::new("repro-memo-id-audit3");
    let id = Id::new(&e);
    let w = &e.wallet;
    let wp = w.pubkey();
    println!("memo-id {}  memo-home {}  MEMO ID Collection {}", id.prog, id.home, id.collection);

    if e.data(&id.burn_guard).is_none() {
        e.must("init memo-home Burn Guard", &[id.init_burn_guard(&wp)], w, &[]);
    }
    s26(&e, &id);
    if account(&e, &id.collection).is_none_or(|a| a.0 != CORE) {
        e.must("init the MEMO ID Collection", &[id.init(&wp)], w, &[]);
    }

    let alice = e.new_user("alice", 1_000 * UNIT, true);
    let bob = e.new_user("bob", 100 * UNIT, true);
    let price5 = id.price(6_900);

    s21(&e, &id, &alice, price5);
    s17(&e, &id, &bob, price5);
    s12_s01(&e, &id, &alice, &bob, price5);
    s23(&e, &id, &alice, &bob, price5);
    s00_s11_s20(&e, &id, &alice, &bob, price5);
    s08(&e, &id, &alice, price5);

    e.finish();
}

/// S26: anyone can pre-fund the MEMO ID Collection PDA before init. memo-home sweeps the
/// lamports, so init works; admin-init-memo-id takes the address for "already created" and
/// refuses on every run.
fn s26(e: &Env, id: &Id) {
    println!("\n-- S26: pre-funded MEMO ID Collection PDA");
    if account(e, &id.collection).is_some() {
        e.skip("S26", "the MEMO ID Collection address is already in use (run on a fresh validator)");
        return;
    }
    let w = &e.wallet;
    // a scratch HOME with a fresh deploy_admin keypair: the tool never sees the real keys
    let wallet_dir = std::path::Path::new(&get_wallet_path()).parent().unwrap().to_path_buf();
    let key_dir = wallet_dir.join("repro-home/.config/solana/memo-token/testnet/authority");
    std::fs::create_dir_all(&key_dir).expect("scratch HOME");
    let home = std::fs::canonicalize(wallet_dir.join("repro-home")).unwrap();
    let admin = Keypair::new();
    write_keypair_file(&admin, key_dir.join("deploy_admin-keypair.json")).expect("deploy_admin keypair");
    e.airdrop(&admin.pubkey(), 10);
    let tool = std::env::current_exe().unwrap().with_file_name("admin-init-memo-id");
    let rpc = get_rpc_url();
    let run_tool = || -> (Option<i32>, String) {
        match std::process::Command::new(&tool).env("HOME", &home).env("MEMO_PROGRAM_ENV", "testnet").env("MEMO_RPC_URL", &rpc).output() {
            Ok(o) => (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
            Err(err) => (None, format!("cannot run {}: {} (cargo build --bin admin-init-memo-id)", tool.display(), err)),
        }
    };

    let prefund = 1_000_000;
    e.must("pre-fund the Collection PDA with a plain System transfer", &[sys_ix::transfer(&w.pubkey(), &id.collection, prefund)], w, &[]);
    let funded = account(e, &id.collection);
    e.record("S26-1", "the MEMO ID Collection PDA is now a System account: 0 bytes, 1,000,000 lamports",
             funded == Some((system_program(), prefund, vec![])), &format!("{:?}", funded.as_ref().map(|a| (a.0, a.1, a.2.len()))));
    for case in ["S26-2", "S26-3"] {
        let (code, text) = run_tool();
        e.record(case, "admin-init-memo-id exits 1 ('unexpected MEMO ID Collection account: owner 1111..., 0 bytes'), never tries init",
                 code == Some(1) && text.contains("unexpected MEMO ID Collection account: owner 11111111111111111111111111111111, 0 bytes")
                     && account(e, &id.collection) == funded,
                 &format!("exit {:?}", code));
        for l in text.lines() {
            println!("     tool: {}", l);
        }
    }
    // init, paid by the same deploy admin the tool would use
    let ap = admin.pubkey();
    let before = e.c.get_balance(&ap).unwrap();
    let out = e.exec(&[id.init(&ap)], &admin, &[]);
    let after = e.c.get_balance(&ap).unwrap();
    let col = account(e, &id.collection).unwrap_or_default();
    let cfg = account(e, &id.config).unwrap_or_default();
    let rent = e.c.get_minimum_balance_for_rent_exemption(col.2.len()).unwrap();
    let named = col.2.len() > 44 && col.2[0] == 5 && pk(&col.2, 1) == id.home_authority && &col.2[37..44] == b"MEMO ID";
    ok_case(e, "S26-4", "init by that payer works: memo-home sweeps the 1,000,000 lamports to it; the Collection holds exactly its rent",
            &out, col.0 == CORE && named && col.1 == rent && cfg.0 == id.home && after + col.1 + cfg.1 + 5000 == before + prefund,
            &format!("payer {} -> {} (+{} swept, -{} Collection rent, -{} Config rent, -5000 fee); Collection {} lamports, rent {}",
                     before, after, prefund, col.1, cfg.1, col.1, rent));
    let (code, text) = run_tool();
    e.record("S26-5", "admin-init-memo-id now exits 0 ('already exists')",
             code == Some(0) && text.contains("MEMO ID Collection already exists"), &format!("exit {:?}", code));
    for l in text.lines().filter(|l| l.contains("Collection") || l.contains("Config")) {
        println!("     tool: {}", l);
    }
    // the scratch HOME holds only the throwaway deploy_admin keypair
    let _ = std::fs::remove_dir_all(&home);
}

/// S21: issue_subid reads only the Core owner of the parent's Asset (and of the holder), not its
/// freeze or delegate state, so a borrower strips permanent SubIds from pledged collateral
fn s21(e: &Env, id: &Id, alice: &Keypair, price5: u64) {
    println!("\n-- S21: issue_subid while the parent's Asset or the holder is frozen for a lender");
    let ap = alice.pubkey();
    let col = id.collection;
    let lender = Keypair::new();
    let lp = lender.pubkey();
    e.airdrop(&lp, 10);
    let alice2 = Keypair::new();
    let a2 = alice2.pubkey();
    e.airdrop(&a2, 1);

    // alice's ID X with the Record SubId www, pledged: TransferDelegate + frozen FreezeDelegate
    // held by the lender (an escrowless loan or a rental)
    let x_id = register_free(e, id, alice, 6, price5);
    let x = id.asset(&x_id);
    let www = format!("www.{}", x_id);
    e.must("create the Record SubId www", &[id.create_subid(&ap, &ap, &x_id, &x, None, "www", None)], alice, &[]);
    e.must("alice approves a TransferDelegate held by the lender", &[core_add_delegate(&x, &col, &ap, 3, &lp)], alice, &[]);
    e.must("alice adds a FreezeDelegate held by the lender", &[core_add_delegate(&x, &col, &ap, 1, &lp)], alice, &[]);
    e.must("the lender freezes X", &[core_set_frozen(&x, &col, &lp, true)], &lender, &[]);

    reject(e, "S21-1", "memo-home treats frozen X as listed: withdraw_lamports from its Vault -> Listed (6011)",
           vec![id.withdraw_lamports(&ap, &x, &ap, 1)], alice, &[], Expect::Custom(0, HOME_LISTED), Some((&id.home, "Listed")), &[x]);
    reject(e, "S21-2", "the lock holds: alice cannot unfreeze the lender's FreezeDelegate (Core NoApprovals)",
           vec![core_set_frozen(&x, &col, &ap, false)], alice, &[], Expect::Custom(0, CORE_NO_APPROVALS), None, &[x]);
    reject(e, "S21-3", "the lock holds: alice cannot revoke the lender's FreezeDelegate (Core InvalidAuthority)",
           vec![core_revoke(&x, &col, &ap, 1)], alice, &[], Expect::Custom(0, CORE_INVALID_AUTHORITY), None, &[x]);

    // fixed in 0.2.0 (F19): issue_subid refuses a listed parent Asset
    let n = id.reg(&x_id).unwrap().nft_subids_issued;
    reject(e, "S21-4", "frozen X: alice cannot issue www.X to her second wallet (Listed, nothing burned)",
           id.tx(price5, &Id::payload("issue_subid", &ap, &www), id.issue_subid(&ap, &ap, &x_id, &x, None, &www, n, &a2, URI)),
           alice, &[], Expect::Custom(2, LISTED), Some((&id.prog, "Listed")), &[x, id.registry(&x_id), id.registry(&www), e.ata(&ap)]);
    e.record("S21-5", "www.X is still a Record SubId; X's nft_subids_issued stays 0",
             n == 0 && id.reg(&www).is_some_and(|r| r.kind == 1) && id.reg(&x_id).is_some_and(|r| r.nft_subids_issued == 0), "");

    // default: the lender forecloses and gets X with www
    let out = e.exec(&[core_set_frozen(&x, &col, &lp, false), core_transfer(&x, Some(&col), &lp, &lp)], &lender, &[]);
    ok_case(e, "S21-6", "default: the lender unfreezes X and takes it with the TransferDelegate", &out, id.owner(&x) == Some(lp), "");
    let out = e.exec(&[id.set_target(&lp, &www, Some(&x_id), &x, None, Some(Target::Address(lp)))], &lender, &[]);
    ok_case(e, "S21-7", "the lender, now X's controller, controls www.X (still a Record)", &out,
            id.reg(&www).is_some_and(|r| r.target_set_by == lp), "");
    reject(e, "S21-8", "alice no longer does (NotController)",
           vec![id.set_target(&ap, &www, Some(&x_id), &x, None, Some(Target::Address(ap)))], alice, &[],
           Expect::Custom(0, NOT_CONTROLLER), Some((&id.prog, "NotController")), &[id.registry(&www)]);

    // holder variant: Y sits in the Vault of H, and H is the frozen collateral
    let y_id = register_free(e, id, alice, 6, price5);
    let y = id.asset(&y_id);
    let h_id = register_free(e, id, alice, 6, price5);
    let h = id.asset(&h_id);
    let pay = format!("pay.{}", y_id);
    e.must("create the Record SubId pay under Y", &[id.create_subid(&ap, &ap, &y_id, &y, None, "pay", None)], alice, &[]);
    e.must("move Y into H's Vault", &[core_transfer(&y, Some(&col), &ap, &id.vault(&h))], alice, &[]);
    e.must("alice adds a FreezeDelegate held by the lender on H", &[core_add_delegate(&h, &col, &ap, 1, &lp)], alice, &[]);
    e.must("the lender freezes H", &[core_set_frozen(&h, &col, &lp, true)], &lender, &[]);
    reject(e, "S21-9", "memo-home will not release Y from frozen H's Vault: withdraw_asset -> Listed (6011)",
           vec![id.withdraw_asset(&ap, &h, &y, &col, &ap)], alice, &[], Expect::Custom(0, HOME_LISTED), Some((&id.home, "Listed")), &[y, h]);
    let n = id.reg(&y_id).unwrap().nft_subids_issued;
    reject(e, "S21-10", "frozen holder H: alice cannot issue pay.Y through holder=H (memo-id Listed, nothing burned)",
           id.tx(price5, &Id::payload("issue_subid", &ap, &pay), id.issue_subid(&ap, &ap, &y_id, &y, Some(&h), &pay, n, &a2, URI)),
           alice, &[], Expect::Custom(2, LISTED), Some((&id.prog, "Listed")), &[y, h, id.registry(&y_id), id.registry(&pay), e.ata(&ap)]);
    e.record("S21-11", "pay.Y is still a Record SubId; Y's nft_subids_issued stays 0; Y still in H's Vault",
             n == 0 && id.reg(&pay).is_some_and(|r| r.kind == 1) && id.owner(&y) == Some(id.vault(&h)), "");
}

/// S17: the holder rule accepts any Core Asset. A seller who keeps permanent delegates on a
/// non-memo-home H sells "H with an ID inside", then claws H back, or burns it.
fn s17(e: &Env, id: &Id, bob: &Keypair, price5: u64) {
    println!("\n-- S17: a non-memo-home holder with permanent delegates");
    let bp = bob.pubkey();
    let col = id.collection;
    let mallory = e.new_user("mallory", 500 * UNIT, true);
    let mp = mallory.pubkey();

    let hk = Keypair::new();
    let h = hk.pubkey();
    e.must("mallory creates H (no Collection) with PermanentTransferDelegate + PermanentBurnDelegate held by herself",
           &[core_create_with_permanent(&h, &mp, &mp, &mp)], &mallory, &[&hk]);
    let a_id = register_free(e, id, &mallory, 6, price5);
    let a = id.asset(&a_id);
    e.must("mallory moves A into vault_of(H)", &[core_transfer(&a, Some(&col), &mp, &id.vault(&h))], &mallory, &[]);
    e.record("S17-1", "H is not a memo-home Asset (no Collection, no Vault State); A is owned by vault_of(H)",
             id.asset_data(&h).is_some_and(|d| d.0 == mp && d.1.is_none()) && e.data(&id.vault_state(&h)).is_none()
                 && id.owner(&a) == Some(id.vault(&h)),
             "");
    let set = |signer: &Pubkey, holder: Option<&Pubkey>| id.set_target(signer, &a_id, None, &a, holder, Some(Target::Address(*signer)));
    // fixed in 0.2.0 (F20): a holder must be a memo-home Asset, passed with its Collection Config
    let mut with_memo_id_config = set(&mp, Some(&h));
    with_memo_id_config.accounts[5].pubkey = id.config;
    reject(e, "S17-2", "holder=H with a real Collection Config: H is in no memo-home Collection (HolderNotHomeAsset)",
           vec![with_memo_id_config], &mallory, &[], Expect::Custom(0, HOLDER_NOT_HOME_ASSET), Some((&id.prog, "HolderNotHomeAsset")),
           &[id.registry(&a_id)]);
    check_reject(e, "S17-3", "holder=H with the Config clients derive for it: none exists (AccountNotInitialized on holder_config)",
                 &|| e.exec(&[set(&mp, Some(&h))], &mallory, &[]), Expect::Custom(0, ANCHOR_ACCOUNT_NOT_INITIALIZED),
                 Some((&id.prog, "AccountNotInitialized")), &["caused by account: holder_config"], &[], &[], &[], &[id.registry(&a_id)]);
    reject(e, "S17-4", "without the holder the controller is vault_of(H), which nobody can sign for (NotController)",
           vec![set(&mp, None)], &mallory, &[], Expect::Custom(0, NOT_CONTROLLER), Some((&id.prog, "NotController")), &[id.registry(&a_id)]);

    // a sale of H "with A inside" now gives the buyer no control, so there is nothing to claw back
    e.must("mallory sells H to bob", &[core_transfer(&h, None, &mp, &bp)], &mallory, &[]);
    let mut bob_cfg = set(&bp, Some(&h));
    bob_cfg.accounts[5].pubkey = id.config;
    reject(e, "S17-5", "bob, H's new owner, cannot control A through H either (HolderNotHomeAsset)", vec![bob_cfg], bob, &[],
           Expect::Custom(0, HOLDER_NOT_HOME_ASSET), Some((&id.prog, "HolderNotHomeAsset")), &[id.registry(&a_id)]);
    // A can never leave vault_of(H) through memo-home either: clients must refuse such transfers
    check_reject(e, "S17-6", "memo-home withdraw_asset(H, inner A) fails on H's missing Vault State (AccountNotInitialized 3012)",
                 &|| e.exec(&[id.withdraw_asset(&bp, &h, &a, &col, &bp)], bob, &[]), Expect::Custom(0, ANCHOR_ACCOUNT_NOT_INITIALIZED),
                 Some((&id.home, "AccountNotInitialized")), &["caused by account: vault_state"], &[], &[], &[], &[a]);
    let _ = price5;
}

/// S12 and S01: an ID owned by the Asset Signer S of a Core Asset Z. Records stamped with S stay
/// valid when Z is sold; Core-calling instructions cannot run under ExecuteV1.
fn s12_s01(e: &Env, id: &Id, alice: &Keypair, bob: &Keypair, price5: u64) {
    println!("\n-- S12 / S01: an ID controlled by a Core Asset Signer (ExecuteV1)");
    let (ap, bp) = (alice.pubkey(), bob.pubkey());
    let a_ta = e.ata(&ap);
    let agent = register_free(e, id, alice, 7, price5);
    let x = id.asset(&agent);
    let zk = Keypair::new();
    let z = zk.pubkey();
    let s = asset_signer(&z);
    e.must("alice creates a plain Core Asset Z", &[core_create_plain(&z, &ap, &ap)], alice, &[&zk]);
    e.must("alice moves her ID's Asset X to Z's Asset Signer S", &[core_transfer(&x, Some(&id.collection), &ap, &s)], alice, &[]);
    let exec = |payer: &Pubkey, inner: Instruction| vec![cu(400_000), core_execute(&z, payer, &inner)];

    let out = e.exec(&exec(&ap, id.set_target(&s, &agent, None, &x, None, Some(Target::Address(ap)))), alice, &[]);
    ok_case(e, "S12-1", "ExecuteV1 on Z: S, the controller, sets agent -> Address(alice); target_set_by = S (set_target works under ExecuteV1)",
            &out, id.owner(&x) == Some(s) && id.reg(&agent).is_some_and(|r| r.target_set_by == s && r.target == Some(Target::Address(ap))), "");
    let out = e.exec(&exec(&ap, id.set_primary(&ap, &s, &agent, Target::Asset(x), Some(&x), None)), alice, &[]);
    ok_case(e, "S12-2", "ExecuteV1 on Z: S sets Primary(Asset(X)) = agent; set_by = S", &out,
            id.primary_data(&Target::Asset(x)) == Some((Target::Asset(x), id.registry(&agent), s)), "");
    let out = e.exec(&[id.set_primary(&ap, &ap, &agent, Target::Address(ap), None, None)], alice, &[]);
    ok_case(e, "S12-3", "alice sets Primary(Address(alice)) = agent", &out,
            id.primary_data(&Target::Address(ap)) == Some((Target::Address(ap), id.registry(&agent), ap)), "");

    // S01: memo-id instructions that call Core (through memo-home) cannot run under ExecuteV1
    let r_id = format!("r.{}", agent);
    // the innermost failing frame is memo-home, refused when it invokes Core (already on the stack)
    let reentry = format!("Program {} failed: Cross-program invocation reentrancy not allowed", id.home);
    check_reject(e, "S01-1", "update_uri under ExecuteV1: ReentrancyNotAllowed (Core -> memo-id -> memo-home -> Core); URI unchanged",
                 &|| e.exec(&exec(&ap, id.update_uri(&ap, &s, &agent, &x, None, "ar://reentry")), alice, &[]),
                 Expect::ErrContains("[1,\"ReentrancyNotAllowed\"]"), None, &[&reentry], &[], &[], &[], &[x]);
    let out = e.exec(&exec(&ap, id.create_subid(&ap, &s, &agent, &x, None, "r", None)), alice, &[]);
    ok_case(e, "S01-2", "create_subid under ExecuteV1 works (no Core CPI)", &out,
            id.reg(&r_id).is_some_and(|r| r.kind == 1) && id.reg(&agent).is_some_and(|r| r.record_subids == 1), "");
    let ixs = vec![app_burn_memo(price5, &Id::payload("issue_subid", &ap, &r_id)), cu(1_000_000),
                   core_execute(&z, &ap, &id.issue_subid(&ap, &s, &agent, &x, None, &r_id, 0, &ap, URI))];
    let legacy = bincode::serialize(&e.build(&ixs, alice, &[], Default::default())).unwrap().len();
    let alt = if legacy > 1232 { Some(create_frozen_alt(e, id.fixed_accounts())) } else { None };
    println!("     issue_subid under ExecuteV1: legacy {} bytes, sent as {}", legacy, if alt.is_some() { "v0 with the lookup table" } else { "legacy" });
    check_reject(e, "S01-3", "issue_subid under ExecuteV1: ReentrancyNotAllowed; nothing burned, r stays a Record, no SubId Index",
                 &|| match &alt { Some(t) => exec_v0(e, &ixs, alice, t), None => e.exec(&ixs, alice, &[]) },
                 Expect::ErrContains("[2,\"ReentrancyNotAllowed\"]"), None, &[&reentry], &[], &[a_ta], &[ap],
                 &[id.registry(&agent), id.registry(&r_id), id.subid_index(&id.registry(&agent), 0), x]);
    // register under ExecuteV1 (payer alice, inner memo-id register): the same reentrancy
    let l = free_label(id, 7, e.nonce());
    let m = format!("{}.memo", l);
    let ixs = vec![app_burn_memo(price5, &Id::payload("register", &ap, &m)), cu(1_000_000), core_execute(&z, &ap, &id.register(&ap, &l, URI))];
    check_reject(e, "S01-4", "register under ExecuteV1: ReentrancyNotAllowed; nothing burned, no Registry, no Asset",
                 &|| e.exec(&ixs, alice, &[]), Expect::ErrContains("[2,\"ReentrancyNotAllowed\"]"), None, &[&reentry], &[],
                 &[a_ta], &[ap], &[id.registry(&m), id.asset(&m)]);

    // the sale of Z: X stays owned by S, so everything S stamped stays valid
    e.must("alice sells Z to bob", &[core_transfer(&z, None, &ap, &bp)], alice, &[]);
    let r = id.reg(&agent).unwrap();
    let controller = id.owner(&x);
    let verified = controller == Some(s) && r.target_set_by == s && r.target == Some(Target::Address(ap))
        && id.primary_data(&Target::Address(ap)) == Some((Target::Address(ap), id.registry(&agent), ap));
    e.record("S12-4", "after Z's sale: X is still owned by S = target_set_by, so agent <-> alice stays verified; \
                       Primary(Asset(X)) set by S stays valid",
             verified && id.primary_data(&Target::Asset(x)).is_some_and(|p| p.2 == s) && id.owner(&z) == Some(bp),
             &format!("controller {:?} {:?}", controller, r));
    reject(e, "S12-5", "alice can no longer drive S: her ExecuteV1 on Z is refused by Core (NoApprovals)",
           exec(&ap, id.set_target(&s, &agent, None, &x, None, None)), alice, &[], Expect::Custom(1, CORE_NO_APPROVALS), None,
           &[id.registry(&agent)]);
    let out = e.exec(&exec(&bp, id.set_target(&s, &agent, None, &x, None, None)), bob, &[]);
    ok_case(e, "S12-6", "only the buyer ends the seller's record: bob's ExecuteV1 on Z clears agent's target", &out,
            id.reg(&agent).is_some_and(|r| r.target.is_none()), "");
}

/// S23: a Primary ID for Address(S), S the Asset Signer of Y, is set by S and never lapses: after
/// Y's sale the seller's ID stays verified to an address the buyer signs for
fn s23(e: &Env, id: &Id, alice: &Keypair, bob: &Keypair, price5: u64) {
    println!("\n-- S23: Address(Asset Signer) as a Primary ID target");
    let (ap, bp) = (alice.pubkey(), bob.pubkey());
    let yk = Keypair::new();
    let y = yk.pubkey();
    let s = asset_signer(&y);
    e.must("alice creates a plain Core Asset Y", &[core_create_plain(&y, &ap, &ap)], alice, &[&yk]);
    let sig = register_free(e, id, alice, 7, price5);
    let sig_asset = id.asset(&sig);
    e.must("alice points sig at Address(S), Y's Asset Signer", &[id.set_target(&ap, &sig, None, &sig_asset, None, Some(Target::Address(s)))], alice, &[]);
    e.must("contrast: alice, Y's owner, sets Primary(Asset(Y)) = sig", &[id.set_primary(&ap, &ap, &sig, Target::Asset(y), Some(&y), None)], alice, &[]);
    let primary = id.primary(&Target::Address(s));
    let verified = || {
        id.reg(&sig).is_some_and(|r| r.target == Some(Target::Address(s)) && r.target_set_by == ap) && id.owner(&sig_asset) == Some(ap)
            && id.primary_data(&Target::Address(s)) == Some((Target::Address(s), id.registry(&sig), s))
    };
    let out = e.exec(&[cu(400_000), core_execute(&y, &ap, &id.set_primary(&ap, &s, &sig, Target::Address(s), None, None))], alice, &[]);
    ok_case(e, "S23-1", "ExecuteV1 on Y: S sets Primary(Address(S)) = sig; set_by = S; sig <-> Address(S) verified", &out, verified(), "");
    let before = account(e, &primary);
    e.must("alice sells Y to bob", &[core_transfer(&y, None, &ap, &bp)], alice, &[]);
    e.record("S23-2", "after Y's sale: Primary(Address(S)) unchanged and still valid (set_by = S); sig stays verified to the address bob signs for",
             verified() && account(e, &primary) == before && id.owner(&y) == Some(bp), "");
    e.record("S23-3", "contrast: Primary(Asset(Y)) set by alice has lapsed (Y's controller is now bob)",
             id.primary_data(&Target::Asset(y)).is_some_and(|p| p.2 == ap) && id.owner(&y) == Some(bp), "");
    reject(e, "S23-4", "alice's ExecuteV1 on Y (clear_primary) after the sale: Core NoApprovals",
           vec![cu(400_000), core_execute(&y, &ap, &id.clear_primary(&s, &Target::Address(s), None, None))], alice, &[],
           Expect::Custom(1, CORE_NO_APPROVALS), None, &[primary]);
    reject(e, "S23-5", "alice cannot clear it directly (NotController)", vec![id.clear_primary(&ap, &Target::Address(s), None, None)], alice, &[],
           Expect::Custom(0, NOT_CONTROLLER), Some((&id.prog, "NotController")), &[primary]);
    let rent = e.c.get_balance(&primary).unwrap();
    let s_before = e.c.get_balance(&s).unwrap();
    let out = e.exec(&[cu(400_000), core_execute(&y, &bp, &id.clear_primary(&s, &Target::Address(s), None, None))], bob, &[]);
    ok_case(e, "S23-6", "bob's ExecuteV1 on Y closes the Primary ID that names the seller's ID; the rent goes to S", &out,
            account(e, &primary).is_none() && e.c.get_balance(&s).unwrap() == s_before + rent,
            &format!("rent {} S {} -> {}", rent, s_before, e.c.get_balance(&s).unwrap()));
}

/// S00, S20, S11: memo-home helper errors raised in memo-id's frame, and memo-burn errors raised
/// by CPI, surface as Custom codes under memo-id's instruction index with memo-id's own numbers
fn s00_s11_s20(e: &Env, id: &Id, alice: &Keypair, bob: &Keypair, price5: u64) {
    println!("\n-- S00 / S20 / S11: error numbers shared by memo-id, memo-home and memo-burn");
    let (ap, bp) = (alice.pubkey(), bob.pubkey());
    let (a_ta, b_ta) = (e.ata(&ap), e.ata(&bp));
    let a_id = register_free(e, id, alice, 6, price5);
    let a_asset = id.asset(&a_id);

    check_reject(e, "S20-1", "set_primary(Asset(bob's wallet)): Custom(6009), the number of NotInHolderVault, is memo-home InvalidCoreAccount",
                 &|| e.exec(&[id.set_primary(&bp, &bp, &a_id, Target::Asset(bp), Some(&bp), None)], bob, &[]),
                 Expect::Custom(0, NOT_IN_HOLDER_VAULT), Some((&id.prog, "InvalidCoreAccount")),
                 &["AnchorError thrown in programs/memo-home/src/lib.rs"], &["NotInHolderVault"], &[], &[], &[id.primary(&Target::Asset(bp))]);
    check_reject(e, "S20-2", "control: a genuine NotInHolderVault (a memo-home holder whose Vault does not hold the Asset) is also Custom(6009)",
                 &|| e.exec(&[id.set_target(&ap, &a_id, None, &a_asset, Some(&a_asset), Some(Target::Address(ap)))], alice, &[]),
                 Expect::Custom(0, NOT_IN_HOLDER_VAULT), Some((&id.prog, "NotInHolderVault")),
                 &["AnchorError thrown in programs/memo-id/src/lib.rs"], &["InvalidCoreAccount"], &[], &[], &[id.registry(&a_id)]);

    // S00 (b), the F05 situation: the Primary ID of a burned Asset cannot be cleared
    let qk = Keypair::new();
    let q = qk.pubkey();
    e.must("bob creates a plain Core Asset Q", &[core_create_plain(&q, &bp, &bp)], bob, &[&qk]);
    e.must("bob sets Primary(Asset(Q))", &[id.set_primary(&bp, &bp, &a_id, Target::Asset(q), Some(&q), None)], bob, &[]);
    e.must("bob burns Q", &[core_burn(&q, &bp)], bob, &[]);
    check_reject(e, "S00-1", "clear_primary of a burned Asset: Custom(6009) = memo-home InvalidCoreAccount; Primary ID and its rent stay",
                 &|| e.exec(&[id.clear_primary(&bp, &Target::Asset(q), Some(&q), None)], bob, &[]),
                 Expect::Custom(0, HOME_INVALID_CORE_ACCOUNT), Some((&id.prog, "InvalidCoreAccount")),
                 &["AnchorError thrown in programs/memo-home/src/lib.rs"], &[], &[], &[], &[id.primary(&Target::Asset(q))]);

    // S11 / S00 (c): memo-burn errors under memo-id's instruction index
    let l = free_label(id, 6, e.nonce());
    let m = format!("{}.memo", l);
    let pl = Id::payload("register", &ap, &m);
    let reg_with = |mint: &Pubkey, ta: &Pubkey| id.tx(price5, &pl, id.register_raw(&ap, sha(m.as_bytes()), &l, URI, &m, mint, ta));
    check_reject(e, "S11-1", "register paying from bob's MEMO account: Custom(6008), memo-id's NotController number, raised by memo-burn UnauthorizedTokenAccount",
                 &|| e.exec(&reg_with(&e.mint, &b_ta), alice, &[]), Expect::Custom(IX, BURN_UNAUTHORIZED_TOKEN_ACCOUNT),
                 Some((&e.burn_prog, "UnauthorizedTokenAccount")), &[&format!("Program {} failed: custom program error: 0x1778", id.prog)],
                 &["NotController"], &[a_ta, b_ta], &[ap, bp], &[id.registry(&m)]);
    let other = e.new_mint(&e.wallet.pubkey());
    let other_ta = ata_of_mint(&ap, &other.pubkey());
    e.must("alice's ATA of a second mint", &[e.create_ata_ix(&ap, &other.pubkey())], &e.wallet, &[]);
    check_reject(e, "S11-2", "register with alice's token account of another mint: Custom(6006) raised by memo-burn InvalidTokenAccount",
                 &|| e.exec(&reg_with(&e.mint, &other_ta), alice, &[]), Expect::Custom(IX, BURN_INVALID_TOKEN_ACCOUNT),
                 Some((&e.burn_prog, "InvalidTokenAccount")), &[], &["InvalidMint"], &[a_ta], &[ap], &[id.registry(&m)]);
    check_reject(e, "S11-3", "contrast: register with another mint: the same Custom(6006), raised by memo-id InvalidMint",
                 &|| e.exec(&reg_with(&other.pubkey(), &other_ta), alice, &[]), Expect::Custom(IX, INVALID_MINT),
                 Some((&id.prog, "InvalidMint")), &[], &[], &[a_ta], &[ap], &[id.registry(&m)]);
    let nostats = e.new_user("nostats", price5, false);
    let np = nostats.pubkey();
    check_reject(e, "S11-4", "register by a payer without burn stats: Custom(3012) raised by memo-burn (user_global_burn_stats)",
                 &|| e.exec(&id.tx(price5, &Id::payload("register", &np, &m), id.register(&np, &l, URI)), &nostats, &[]),
                 Expect::Custom(IX, ANCHOR_ACCOUNT_NOT_INITIALIZED), Some((&e.burn_prog, "AccountNotInitialized")),
                 &["caused by account: user_global_burn_stats"], &[], &[e.ata(&np)], &[], &[id.registry(&m)]);
}

/// S08: memo-id decodes the BurnMemo with Borsh without checking its length prefixes; a forged
/// prefix makes Borsh pre-allocate up to 1 MiB on the 32 KiB heap
fn s08(e: &Env, id: &Id, alice: &Keypair, price5: u64) {
    println!("\n-- S08: forged Borsh length prefix in the BurnMemo");
    let ap = alice.pubkey();
    let a_ta = e.ata(&ap);
    let l = free_label(id, 6, e.nonce());
    let m = format!("{}.memo", l);
    let pl = Id::payload("register", &ap, &m);
    let forged = |declared: u32| memo_ix(&b64(&burn_memo_raw(1, price5, declared, &pl)));
    let reg = || id.register(&ap, &l, URI);
    let abort = Expect::ErrContains("[2,\"ProgramFailedToComplete\"]");
    let oom = "memory allocation failed, out of memory";
    let watch = [id.registry(&m), id.asset(&m)];
    println!("     payload: {} bytes", pl.len());
    for (case, declared) in [("S08-1", 65_536u32), ("S08-2", u32::MAX)] {
        check_reject(e, case, &format!("BurnMemo payload prefix {} ({} bytes present): out-of-memory abort instead of InvalidMemoFormat", declared, pl.len()),
                     &|| e.exec(&[forged(declared), cu(1_000_000), reg()], alice, &[]), abort, None, &[oom], &["InvalidMemoFormat"],
                     &[a_ta], &[ap], &watch);
    }
    check_reject(e, "S08-3", "control: prefix one byte too long gives the clean InvalidMemoFormat (6017)",
                 &|| e.exec(&[forged(pl.len() as u32 + 1), cu(1_000_000), reg()], alice, &[]), Expect::Custom(IX, INVALID_MEMO_FORMAT),
                 Some((&id.prog, "InvalidMemoFormat")), &[], &[oom], &[a_ta], &[ap], &watch);
    check_reject(e, "S08-3b", "control: prefix 20,000 still gives the clean InvalidMemoFormat: the abort is a heap threshold, not a check",
                 &|| e.exec(&[forged(20_000), cu(1_000_000), reg()], alice, &[]), Expect::Custom(IX, INVALID_MEMO_FORMAT),
                 Some((&id.prog, "InvalidMemoFormat")), &[], &[oom], &[a_ta], &[ap], &watch);
    // payload-level: the category String prefix
    let mut f = pl.clone();
    f[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
    check_reject(e, "S08-4", "payload category prefix u32::MAX: out-of-memory abort",
                 &|| e.exec(&id.tx(price5, &f, reg()), alice, &[]), abort, None, &[oom], &[], &[a_ta], &[ap], &watch);
    let mut g = pl.clone();
    g[1..5].copy_from_slice(&((pl.len() - 5 + 1) as u32).to_le_bytes());
    check_reject(e, "S08-5", "control: category prefix one byte past the end gives the clean InvalidMemoFormat (6017)",
                 &|| e.exec(&id.tx(price5, &g, reg()), alice, &[]), Expect::Custom(IX, INVALID_MEMO_FORMAT),
                 Some((&id.prog, "InvalidMemoFormat")), &[], &[oom], &[a_ta], &[ap], &watch);
    check_reject(e, "S08-6", "memo-burn rejects the same forged memo cleanly (InvalidMemoFormat 6001)",
                 &|| e.exec(&[forged(65_536), e.burn_ix(&ap, price5)], alice, &[]), Expect::Custom(1, BURN_INVALID_MEMO_FORMAT),
                 Some((&e.burn_prog, "InvalidMemoFormat")), &["Payload length prefix 65536 does not match"], &[oom], &[a_ta], &[ap], &[]);

    // the heap threshold: largest prefix that still fails cleanly (simulated, nothing lands)
    let sim = |declared: u32| -> String {
        let tx = e.build(&[forged(declared), cu(1_000_000), reg()], alice, &[], e.c.get_latest_blockhash().unwrap());
        match e.c.simulate_transaction(&tx) {
            Ok(r) => r.value.err.map(|x| format!("{:?}", x)).unwrap_or_else(|| "ok".into()),
            Err(x) => format!("rpc error {}", x),
        }
    };
    let clean = sim(pl.len() as u32 + 1);
    let (mut lo, mut hi) = (pl.len() as u32 + 1, 65_536u32);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if sim(mid) == clean { lo = mid } else { hi = mid }
    }
    println!("     heap threshold: prefix {} -> {}; prefix {} -> {}", lo, sim(lo), hi, sim(hi));
}
