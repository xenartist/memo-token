//! On-chain regression suite for memo-home (local validator only).
//!
//! Runs against X1 mainnet's builds of Metaplex Core, Token-2022, SPL Token and the ATA program
//! (dumped by scripts/local-validator.sh), memo-mint, memo-burn, and two test-only programs from
//! clients/regression/test-programs: a Token-2022 transfer hook and an app that calls memo-home
//! by CPI with its PDA as Authority. Covers the Burn Guard, Collections and Assets at memo-home
//! PDAs (fixed Authority, share cap, max_assets, royalties, name/URI limits, pre-funded
//! addresses), the Core paths around memo-home (including the Collection-address update
//! authority), mining with the share split, Vault outflows for any token with the Vault State
//! nonce, listing protection, Vault burns, withdraw_asset, signer checks and the app CPI path.
//! Every case asserts the exact outcome.
//!
//! Run through scripts/run-regression-tests.sh, or against a running local validator:
//!   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json \
//!     cargo run --bin regression-memo-home

#[path = "harness.rs"]
#[allow(dead_code)]
mod harness;

use base64::{engine::general_purpose, Engine as _};
use harness::*;
use memo_token_client::get_program_id;
use sha2::{Digest, Sha256};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_system_interface::instruction as sys_ix;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id as ata_of, instruction::create_associated_token_account,
};

const CORE: Pubkey = solana_sdk::pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const ATA_PROGRAM: Pubkey = spl_associated_token_account::ID;
const HOOK_PROGRAM: Pubkey = solana_sdk::pubkey!("HCwbzCbQRg4YH2pDkwa9hDDGqu64axLquG6mAKSdnbhr");
const APP_PROGRAM: Pubkey = solana_sdk::pubkey!("EZzeE6EZ7oD3Qsr5F1Wuu5TVHgbzDYL4LiZCxaSPR6c2");
const NATIVE_MINT: Pubkey = solana_sdk::pubkey!("So11111111111111111111111111111111111111112");

// memo-home error codes
const INVALID_SHARE_BPS: u32 = 6000;
const INVALID_MAX_ASSETS: u32 = 6001;
const INVALID_ROYALTY: u32 = 6002;
const UNAUTHORIZED: u32 = 6003;
const WRONG_COLLECTION: u32 = 6004;
const MAX_ASSETS_REACHED: u32 = 6006;
const NOTHING_TO_UPDATE: u32 = 6007;
const SHARE_TOO_HIGH: u32 = 6008;
const NOT_OWNER: u32 = 6010;
const LISTED: u32 = 6011;
const NOT_IN_VAULT: u32 = 6012;
const INVALID_VAULT_TOKEN_ACCOUNT: u32 = 6013;
const INVALID_MINT: u32 = 6014;
const INVALID_AMOUNT: u32 = 6015;
const INSUFFICIENT_VAULT_LAMPORTS: u32 = 6017;
const NAME_TOO_LONG: u32 = 6018;
const URI_TOO_LONG: u32 = 6019;
const INVALID_OWNER: u32 = 6020;
const MEMO_ACCOUNT_NOT_CLOSABLE: u32 = 6022;
const TOKEN_ACCOUNT_NOT_EMPTY: u32 = 6023;
const INVALID_DESTINATION: u32 = 6024;
// memo-mint / memo-burn error codes (surface at the memo-home instruction)
const MINT_MEMO_REQUIRED: u32 = 6000;
const MINT_ONE_PER_TRANSACTION: u32 = 6008;
const BURN_AMOUNT_MISMATCH: u32 = 6009;
// Anchor
const ANCHOR_CONSTRAINT_TOKEN_MINT: u32 = 2014;
const ANCHOR_CONSTRAINT_TOKEN_OWNER: u32 = 2015;
// Metaplex Core (mpl-core MplCoreError, checked against the X1 bytecode)
const CORE_INVALID_AUTHORITY: u32 = 9;
const CORE_NO_APPROVALS: u32 = 26;
const CORE_MISSING_EXTERNAL_ACCOUNT: u32 = 34;

/// A 32-byte PDA seed from a short tag
fn seed(tag: &str) -> [u8; 32] {
    let mut s = [0u8; 32];
    let b = tag.as_bytes();
    s[..b.len().min(32)].copy_from_slice(&b[..b.len().min(32)]);
    s
}

struct Home<'a> {
    e: &'a Env,
    prog: Pubkey,
    program_authority: Pubkey,
    burn_guard: Pubkey,
}

fn bstr(v: &mut Vec<u8>, s: &str) {
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
}
fn bopt_str(v: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(s) => {
            v.push(1);
            bstr(v, s);
        }
        None => v.push(0),
    }
}

/// Clears the signer flag of `key` in an instruction (the key is passed but does not sign)
fn unsigned(mut ix: Instruction, key: &Pubkey) -> Instruction {
    for m in ix.accounts.iter_mut().filter(|m| m.pubkey == *key) {
        m.is_signer = false;
    }
    ix
}

type Royalty = Option<(u16, Vec<(Pubkey, u8)>)>;

const COLLECTION_URI: &str = "https://example.com/collection.json";
const ASSET_URI: &str = "https://example.com/asset.json";

impl<'a> Home<'a> {
    fn collection_addr(&self, authority: &Pubkey, seed: &[u8; 32]) -> Pubkey {
        Pubkey::find_program_address(&[b"collection", authority.as_ref(), seed], &self.prog).0
    }
    fn asset_addr(&self, col: &Pubkey, seed: &[u8; 32]) -> Pubkey {
        Pubkey::find_program_address(&[b"asset", col.as_ref(), seed], &self.prog).0
    }
    fn config(&self, col: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"collection_config", col.as_ref()], &self.prog).0
    }
    fn vault(&self, asset: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"vault", asset.as_ref()], &self.prog).0
    }
    fn vault_state(&self, asset: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"vault_state", asset.as_ref()], &self.prog).0
    }
    fn vault_ata(&self, asset: &Pubkey) -> Pubkey { self.e.ata(&self.vault(asset)) }
    /// Outflow nonce of an Asset's Vault State
    fn nonce(&self, asset: &Pubkey) -> Option<u64> {
        self.e.data(&self.vault_state(asset)).filter(|d| d.len() == 17).map(|d| u64::from_le_bytes(d[8..16].try_into().unwrap()))
    }
    fn ix(&self, name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
        let mut d = disc(name);
        d.extend(args);
        Instruction::new_with_bytes(self.prog, &d, accounts)
    }

    fn init_burn_guard(&self, payer: &Pubkey) -> Instruction {
        self.ix("init_burn_guard", vec![], vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(self.burn_guard, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }

    /// create_collection with an explicit Collection account (for the mismatch cases)
    #[allow(clippy::too_many_arguments)]
    fn create_collection_at(&self, payer: &Pubkey, authority: &Pubkey, col: &Pubkey, seed: &[u8; 32], name: &str, uri: &str,
                            max_share_bps: u16, max_assets: Option<u32>, royalty: Royalty) -> Instruction {
        let mut a = seed.to_vec();
        bstr(&mut a, name);
        bstr(&mut a, uri);
        a.extend(max_share_bps.to_le_bytes());
        match max_assets {
            Some(m) => {
                a.push(1);
                a.extend(m.to_le_bytes());
            }
            None => a.push(0),
        }
        match royalty {
            Some((bps, creators)) => {
                a.push(1);
                a.extend(bps.to_le_bytes());
                a.extend((creators.len() as u32).to_le_bytes());
                for (k, p) in creators {
                    a.extend(k.as_ref());
                    a.push(p);
                }
            }
            None => a.push(0),
        }
        self.ix("create_collection", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*col, false),
            AccountMeta::new_readonly(self.program_authority, false),
            AccountMeta::new_readonly(self.burn_guard, false),
            AccountMeta::new(self.config(col), false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    /// create_collection at the PDA of (authority, seed(tag)); returns the Collection address
    fn create_collection(&self, payer: &Pubkey, authority: &Pubkey, tag: &str, name: &str, max_share_bps: u16,
                         max_assets: Option<u32>, royalty: Royalty) -> (Instruction, Pubkey) {
        let s = seed(tag);
        let col = self.collection_addr(authority, &s);
        (self.create_collection_at(payer, authority, &col, &s, name, COLLECTION_URI, max_share_bps, max_assets, royalty), col)
    }

    fn update_collection_with(&self, payer: &Pubkey, authority: &Pubkey, config: &Pubkey, col: &Pubkey, name: Option<&str>,
                              uri: Option<&str>) -> Instruction {
        let mut a = vec![];
        bopt_str(&mut a, name);
        bopt_str(&mut a, uri);
        self.ix("update_collection", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*config, false),
            AccountMeta::new(*col, false),
            AccountMeta::new_readonly(self.program_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn update_collection(&self, payer: &Pubkey, authority: &Pubkey, col: &Pubkey, name: Option<&str>) -> Instruction {
        self.update_collection_with(payer, authority, &self.config(col), col, name, None)
    }

    /// create_asset with explicit accounts (for the mismatch cases)
    #[allow(clippy::too_many_arguments)]
    fn create_asset_at(&self, payer: &Pubkey, authority: &Pubkey, config: &Pubkey, col: &Pubkey, asset: &Pubkey,
                       seed: &[u8; 32], owner: &Pubkey, name: &str, uri: &str) -> Instruction {
        let mut a = seed.to_vec();
        bstr(&mut a, name);
        bstr(&mut a, uri);
        self.ix("create_asset", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*config, false),
            AccountMeta::new(*col, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(self.program_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    /// create_asset at the PDA of (col, seed(tag)); returns the Asset address
    fn create_asset(&self, payer: &Pubkey, authority: &Pubkey, col: &Pubkey, tag: &str, owner: &Pubkey, name: &str) -> (Instruction, Pubkey) {
        let s = seed(tag);
        let asset = self.asset_addr(col, &s);
        (self.create_asset_at(payer, authority, &self.config(col), col, &asset, &s, owner, name, ASSET_URI), asset)
    }

    #[allow(clippy::too_many_arguments)]
    fn update_asset_with(&self, payer: &Pubkey, authority: &Pubkey, config: &Pubkey, col: &Pubkey, asset: &Pubkey,
                         name: Option<&str>, uri: Option<&str>) -> Instruction {
        let mut a = vec![];
        bopt_str(&mut a, name);
        bopt_str(&mut a, uri);
        self.ix("update_asset", a, vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*config, false),
            AccountMeta::new_readonly(*col, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new_readonly(self.program_authority, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
    fn update_asset(&self, payer: &Pubkey, authority: &Pubkey, col: &Pubkey, asset: &Pubkey, name: Option<&str>) -> Instruction {
        self.update_asset_with(payer, authority, &self.config(col), col, asset, name, None)
    }

    fn mine_accounts(&self, caller: &Pubkey, authority: &Pubkey, config: &Pubkey, asset: &Pubkey, recipient_ta: &Pubkey) -> Vec<AccountMeta> {
        let e = self.e;
        vec![
            AccountMeta::new(*caller, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*config, false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new_readonly(self.vault(asset), false),
            AccountMeta::new(self.vault_ata(asset), false),
            AccountMeta::new(*recipient_ta, false),
            AccountMeta::new(e.mint, false),
            AccountMeta::new_readonly(e.mint_auth, false),
            AccountMeta::new(e.mint_state, false),
            AccountMeta::new_readonly(IXS_SYSVAR, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(ATA_PROGRAM, false),
            AccountMeta::new_readonly(e.mint_prog, false),
            AccountMeta::new_readonly(system_program(), false),
        ]
    }
    fn mine_with(&self, caller: &Pubkey, authority: &Pubkey, config: &Pubkey, asset: &Pubkey, recipient_ta: &Pubkey,
                 share_bps: u16) -> Instruction {
        self.ix("mine", share_bps.to_le_bytes().to_vec(), self.mine_accounts(caller, authority, config, asset, recipient_ta))
    }
    fn mine(&self, caller: &Pubkey, authority: &Pubkey, col: &Pubkey, asset: &Pubkey, recipient_ta: &Pubkey, share_bps: u16) -> Instruction {
        self.mine_with(caller, authority, &self.config(col), asset, recipient_ta, share_bps)
    }

    #[allow(clippy::too_many_arguments)]
    fn withdraw_with(&self, owner: &Pubkey, config: &Pubkey, asset: &Pubkey, vault_ta: &Pubkey, dest: &Pubkey,
                     mint: &Pubkey, token_program: &Pubkey, amount: u64) -> Instruction {
        self.ix("withdraw", amount.to_le_bytes().to_vec(), vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(*config, false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new_readonly(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(*vault_ta, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*token_program, false),
        ])
    }
    fn withdraw(&self, owner: &Pubkey, col: &Pubkey, asset: &Pubkey, dest: &Pubkey, amount: u64) -> Instruction {
        self.withdraw_with(owner, &self.config(col), asset, &self.vault_ata(asset), dest, &self.e.mint, &t22(), amount)
    }

    #[allow(clippy::too_many_arguments)]
    fn withdraw_asset(&self, owner: &Pubkey, payer: &Pubkey, col: &Pubkey, asset: &Pubkey, inner: &Pubkey, inner_col: &Pubkey,
                      new_owner: &Pubkey) -> Instruction {
        self.ix("withdraw_asset", vec![], vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(self.config(col), false),
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

    fn withdraw_lamports(&self, owner: &Pubkey, col: &Pubkey, asset: &Pubkey, dest: &Pubkey, amount: u64) -> Instruction {
        self.ix("withdraw_lamports", amount.to_le_bytes().to_vec(), vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(self.config(col), false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }

    fn close_token_account(&self, owner: &Pubkey, col: &Pubkey, asset: &Pubkey, ta: &Pubkey, dest: &Pubkey, token_program: &Pubkey) -> Instruction {
        self.ix("close_token_account", vec![], vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(self.config(col), false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new_readonly(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(*ta, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*token_program, false),
        ])
    }

    fn vault_stats(&self, asset: &Pubkey) -> Pubkey { self.e.stats_pda(&self.vault(asset)) }

    fn burn(&self, owner: &Pubkey, payer: &Pubkey, col: &Pubkey, asset: &Pubkey, amount: u64) -> Instruction {
        let e = self.e;
        self.ix("burn", amount.to_le_bytes().to_vec(), vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(self.config(col), false),
            AccountMeta::new_readonly(*asset, false),
            AccountMeta::new(self.vault(asset), false),
            AccountMeta::new(self.vault_state(asset), false),
            AccountMeta::new(self.vault_ata(asset), false),
            AccountMeta::new(self.vault_stats(asset), false),
            AccountMeta::new(e.gbs, false),
            AccountMeta::new(e.mint, false),
            AccountMeta::new_readonly(IXS_SYSVAR, false),
            AccountMeta::new_readonly(t22(), false),
            AccountMeta::new_readonly(e.burn_prog, false),
            AccountMeta::new_readonly(system_program(), false),
        ])
    }
}

// ---------------- Metaplex Core instructions sent directly ----------------

/// Core instruction with the [asset, collection, payer, authority, system, log_wrapper] layout
fn core_ix(data: Vec<u8>, asset: &Pubkey, col: Option<&Pubkey>, payer: &Pubkey, authority: Option<&Pubkey>, extra: Vec<AccountMeta>) -> Instruction {
    let mut metas = vec![
        AccountMeta::new(*asset, false),
        match col { Some(c) => AccountMeta::new(*c, false), None => AccountMeta::new_readonly(CORE, false) },
        AccountMeta::new(*payer, true),
        match authority { Some(a) => AccountMeta::new_readonly(*a, true), None => AccountMeta::new_readonly(CORE, false) },
        AccountMeta::new_readonly(system_program(), false),
        AccountMeta::new_readonly(CORE, false),
    ];
    metas.extend(extra);
    Instruction { program_id: CORE, accounts: metas, data }
}

/// TransferV1 signed by `authority` (the owner, or a transfer delegate)
fn core_transfer(asset: &Pubkey, col: Option<&Pubkey>, payer: &Pubkey, authority: &Pubkey, new_owner: &Pubkey) -> Instruction {
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            match col { Some(c) => AccountMeta::new_readonly(*c, false), None => AccountMeta::new_readonly(CORE, false) },
            AccountMeta::new(*payer, true),
            if authority == payer { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new_readonly(*authority, true) },
            AccountMeta::new_readonly(*new_owner, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: vec![14, 0],
    }
}

/// AddPluginV1 of an owner-managed delegate plugin (1 freeze, 2 burn, 3 transfer), authority
/// Owner (None) or Address(delegate)
fn core_add_delegate(asset: &Pubkey, col: &Pubkey, owner: &Pubkey, plugin_type: u8, delegate: Option<&Pubkey>) -> Instruction {
    let mut d = vec![2u8, plugin_type];
    if plugin_type == 1 {
        d.push(0); // FreezeDelegate { frozen: false }
    }
    match delegate {
        Some(k) => {
            d.extend([1, 3]);
            d.extend(k.as_ref());
        }
        None => d.extend([1, 1]),
    }
    core_ix(d, asset, Some(col), owner, None, vec![])
}

/// UpdatePluginV1 FreezeDelegate { frozen } signed by the plugin authority
fn core_set_frozen(asset: &Pubkey, col: &Pubkey, payer: &Pubkey, authority: &Pubkey, frozen: bool) -> Instruction {
    let auth = if authority == payer { None } else { Some(authority) };
    core_ix(vec![6, 1, frozen as u8], asset, Some(col), payer, auth, vec![])
}

/// BurnV1 { compression_proof: None }
fn core_burn(asset: &Pubkey, col: Option<&Pubkey>, payer: &Pubkey, authority: Option<&Pubkey>, extra: Vec<AccountMeta>) -> Instruction {
    core_ix(vec![12, 0], asset, col, payer, authority, extra)
}

/// UpdateV1 { new_name, new_uri: None, new_update_authority: None } signed by `authority`
fn core_update_v1(asset: &Pubkey, col: &Pubkey, payer: &Pubkey, authority: &Pubkey, name: &str) -> Instruction {
    let mut d = vec![15u8, 1];
    bstr(&mut d, name);
    d.extend([0, 0]);
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            AccountMeta::new_readonly(*col, false),
            AccountMeta::new(*payer, true),
            if authority == payer { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new_readonly(*authority, true) },
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
    }
}

/// UpdateV2 { name: None, uri: None, new_update_authority: Address(new_ua) }: takes the Asset out
/// of its Collection
fn core_update_v2_eject(asset: &Pubkey, col: &Pubkey, payer: &Pubkey, authority: &Pubkey, new_ua: &Pubkey) -> Instruction {
    let mut d = vec![30u8, 0, 0, 1, 1];
    d.extend(new_ua.as_ref());
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, false),
            AccountMeta::new(*col, false),
            AccountMeta::new(*payer, true),
            if authority == payer { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new_readonly(*authority, true) },
            AccountMeta::new_readonly(CORE, false), // new_collection: none
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
    }
}

/// CreateCollectionV2 at a keypair address with `ua` as update authority and, optionally, an
/// Oracle (base `oracle`) that checks Transfer
fn core_create_collection(col: &Pubkey, ua: &Pubkey, payer: &Pubkey, oracle_on_transfer: Option<&Pubkey>) -> Instruction {
    let mut d = vec![21u8];
    bstr(&mut d, "plain collection");
    bstr(&mut d, "https://example.com/plain-collection.json");
    d.push(0); // plugins: None
    match oracle_on_transfer {
        Some(base) => {
            d.extend([1, 1, 0, 0, 0, 1]); // Some([Oracle])
            d.extend(base.as_ref());
            d.extend([1, 0]); // init_plugin_authority: Some(None)
            d.extend([1, 0, 0, 0, 1, 4, 0, 0, 0]); // lifecycle_checks: [(Transfer, can_reject)]
            d.push(0); // base_address_config: None
            d.extend([1, 1]); // results_offset: Some(Anchor)
        }
        None => d.push(0),
    }
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*col, true),
            AccountMeta::new_readonly(*ua, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program(), false),
        ],
        data: d,
    }
}

/// CreateV1 of a Core Asset for `owner`, in `col` (signed by `authority`) or with no collection
fn core_create(asset: &Pubkey, col: Option<&Pubkey>, authority: Option<&Pubkey>, payer: &Pubkey, owner: &Pubkey) -> Instruction {
    let mut d = vec![0u8, 0];
    bstr(&mut d, "plain core asset");
    bstr(&mut d, "https://example.com/plain.json");
    d.push(0);
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*asset, true),
            match col { Some(c) => AccountMeta::new(*c, false), None => AccountMeta::new_readonly(CORE, false) },
            match authority { Some(a) => AccountMeta::new_readonly(*a, true), None => AccountMeta::new_readonly(CORE, false) },
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(CORE, false),
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
    }
}
fn core_create_plain(asset: &Pubkey, payer: &Pubkey, owner: &Pubkey) -> Instruction { core_create(asset, None, None, payer, owner) }

/// AddCollectionPluginV1 signed by `authority`: plugin 7 PermanentTransferDelegate or 4
/// UpdateDelegate { additional_delegates: [] }, init authority Address(authority)
fn core_add_collection_plugin(col: &Pubkey, payer: &Pubkey, authority: &Pubkey, plugin_type: u8) -> Instruction {
    let mut d = vec![3u8, plugin_type];
    if plugin_type == 4 {
        d.extend(0u32.to_le_bytes());
    }
    d.extend([1, 3]);
    d.extend(authority.as_ref());
    Instruction {
        program_id: CORE,
        accounts: vec![
            AccountMeta::new(*col, false),
            AccountMeta::new(*payer, true),
            if authority == payer { AccountMeta::new_readonly(CORE, false) } else { AccountMeta::new_readonly(*authority, true) },
            AccountMeta::new_readonly(system_program(), false),
            AccountMeta::new_readonly(CORE, false),
        ],
        data: d,
    }
}

// ---------------- the test app (clients/regression/test-programs/home-app) ----------------

fn app_ix(name: &str, args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
    let mut d = disc(name);
    d.extend(args);
    Instruction::new_with_bytes(APP_PROGRAM, &d, accounts)
}

// ---------------- reading Core accounts ----------------

fn core_owner(e: &Env, asset: &Pubkey) -> Option<Pubkey> {
    e.data(asset).filter(|d| d.len() >= 33 && d[0] == 1).map(|d| Pubkey::new_from_array(d[1..33].try_into().unwrap()))
}
/// (update authority tag, key) of a Core Asset
fn core_ua(e: &Env, asset: &Pubkey) -> Option<(u8, Option<Pubkey>)> {
    let d = e.data(asset)?;
    Some((d[33], (d[33] != 0).then(|| Pubkey::new_from_array(d[34..66].try_into().unwrap()))))
}
/// (name, uri) of a Core Asset
fn core_name_uri(e: &Env, asset: &Pubkey) -> Option<(String, String)> {
    let d = e.data(asset)?;
    let mut at = if d[33] == 0 { 34 } else { 66 };
    let mut out = vec![];
    for _ in 0..2 {
        let len = u32::from_le_bytes(d[at..at + 4].try_into().unwrap()) as usize;
        out.push(String::from_utf8_lossy(&d[at + 4..at + 4 + len]).to_string());
        at += 4 + len;
    }
    Some((out[0].clone(), out[1].clone()))
}
fn core_name(e: &Env, asset: &Pubkey) -> Option<String> { core_name_uri(e, asset).map(|(n, _)| n) }
/// (num_minted, current_size) of a Core Collection
fn collection_counts(e: &Env, col: &Pubkey) -> Option<(u32, u32)> {
    let d = e.data(col)?;
    let mut p = 33;
    for _ in 0..2 {
        p += 4 + u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) as usize;
    }
    Some((u32::from_le_bytes(d[p..p + 4].try_into().unwrap()), u32::from_le_bytes(d[p + 4..p + 8].try_into().unwrap())))
}
fn collection_minted(e: &Env, col: &Pubkey) -> Option<u32> { collection_counts(e, col).map(|c| c.0) }
/// Plugin registry of a Core Collection: (internal (type, authority tag), external (type, authority
/// tag, lifecycle checks))
#[allow(clippy::type_complexity)]
fn collection_plugins(e: &Env, col: &Pubkey) -> Option<(Vec<(u8, u8)>, Vec<(u8, u8, Vec<(u8, u32)>)>)> {
    let d = e.data(col)?;
    let u32_at = |p: usize| u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
    let mut p = 33;
    for _ in 0..2 {
        p += 4 + u32_at(p) as usize;
    }
    p += 8; // num_minted, current_size
    if p >= d.len() || d[p] != 3 {
        return Some((vec![], vec![]));
    }
    let mut r = u64::from_le_bytes(d[p + 1..p + 9].try_into().unwrap()) as usize;
    if d[r] != 4 {
        return None;
    }
    r += 1;
    let auth_len = |tag: u8| if tag == 3 { 33 } else { 1 };
    let mut internal = vec![];
    let n = u32_at(r);
    r += 4;
    for _ in 0..n {
        let (t, a) = (d[r], d[r + 1]);
        r += 1 + auth_len(a) + 8;
        internal.push((t, a));
    }
    let mut external = vec![];
    let m = u32_at(r);
    r += 4;
    for _ in 0..m {
        let (t, a) = (d[r], d[r + 1]);
        r += 1 + auth_len(a);
        let mut checks = vec![];
        if d[r] == 1 {
            let n = u32_at(r + 1);
            r += 5;
            for _ in 0..n {
                checks.push((d[r], u32_at(r + 1)));
                r += 5;
            }
        } else {
            r += 1;
        }
        r += 8; // offset
        for _ in 0..2 {
            r += if d[r] == 1 { 9 } else { 1 }; // data_offset, data_len
        }
        external.push((t, a, checks));
    }
    Some((internal, external))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }
fn consumed(out: &Out, prog: &Pubkey) -> String {
    let p = format!("Program {} consumed", prog);
    out.logs.iter().filter(|l| l.starts_with(&p)).map(|l| l.split(' ').nth(3).unwrap_or("?").to_string()).last()
        .unwrap_or_else(|| "?".into())
}
/// Data of the last Anchor event named `name` in the logs (after its discriminator)
fn event(out: &Out, name: &str) -> Option<Vec<u8>> {
    let d = &Sha256::digest(format!("event:{}", name).as_bytes())[..8];
    out.logs.iter().filter_map(|l| l.strip_prefix("Program data: "))
        .filter_map(|b| general_purpose::STANDARD.decode(b).ok())
        .filter(|v| v.len() >= 8 && &v[..8] == d)
        .last()
        .map(|v| v[8..].to_vec())
}

fn main() {
    let e = Env::new("regression-memo-home");
    let prog = get_program_id("memo_home").expect("memo_home program id in Anchor.toml");
    let h = Home {
        e: &e,
        prog,
        program_authority: Pubkey::find_program_address(&[b"authority"], &prog).0,
        burn_guard: Pubkey::find_program_address(&[b"burn_guard"], &prog).0,
    };
    let w = &e.wallet;
    let wp = w.pubkey();
    println!("memo-home {}  Program Authority {}  Burn Guard {}", prog, h.program_authority, h.burn_guard);
    // sha256 of the loaded ELF (ProgramData after its 45-byte header), to compare with the pins in
    // scripts/local-validator.sh
    for (name, id) in [("Metaplex Core", CORE), ("Token-2022", t22()), ("SPL Token", SPL_TOKEN_LEGACY), ("ATA", ATA_PROGRAM)] {
        let elf = e.c.get_account(&id).ok().and_then(|a| {
            if a.data.len() == 36 {
                let pd = Pubkey::new_from_array(a.data[4..36].try_into().unwrap());
                e.c.get_account(&pd).ok().map(|p| p.data[45..].to_vec())
            } else {
                Some(a.data)
            }
        });
        match elf {
            Some(b) => println!("{} {}: sha256 {:x}", name, id, Sha256::digest(&b)),
            None => println!("{} {}: MISSING", name, id),
        }
    }
    let min_rent = e.c.get_minimum_balance_for_rent_exemption(0).unwrap();
    let sol = |k: &Pubkey| e.c.get_balance(k).unwrap_or(0);

    // Users: the app Authority, a second app, three successive owners, a market, a stranger
    let app = e.new_user("app authority", 0, false);
    let app2 = e.new_user("second app authority", 0, false);
    let b = e.new_user("B (first owner)", 0, false);
    let c = e.new_user("C (second owner)", 0, false);
    let d = e.new_user("D (buyer)", 0, false);
    let market = e.new_user("market", 0, false);
    let x = e.new_user("X (stranger)", 0, false);
    let (ap, a2p, bp, cp, dp, mp, xp) = (app.pubkey(), app2.pubkey(), b.pubkey(), c.pubkey(), d.pubkey(), market.pubkey(), x.pubkey());

    // ---------------- Burn Guard ----------------
    let out = e.exec(&[h.init_burn_guard(&wp)], w, &[]);
    let guard = e.data(&h.burn_guard);
    e.record("H01", "init_burn_guard creates the fixed Oracle content (V1: create/transfer/update Pass, burn Rejected)",
        out.ok() && guard.as_ref().map(|g| g[8..13] == [1, 2, 2, 1, 2]).unwrap_or(false),
        &format!("{} data {:?}", out.brief(), guard.map(|g| g[8..13].to_vec())));
    e.reject("H02", "init_burn_guard a second time", vec![h.init_burn_guard(&xp)], &x, &[], Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &[], &[]);

    // ---------------- Collections ----------------
    let (ix, col) = h.create_collection(&wp, &ap, "rooms", "HOME Rooms", 3000, None, None);
    let out = e.exec(&[ix], w, &[&app]);
    let cfg = e.data(&h.config(&col)).unwrap_or_default();
    let cdata = e.data(&col).unwrap_or_default();
    let created_at = cfg.get(139..147).map(|b| i64::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
    let cfg_ok = cfg.len() == 152
        && cfg[8..40] == col.to_bytes() && cfg[40..72] == ap.to_bytes() && cfg[72..104] == seed("rooms")
        && cfg[104..106] == 3000u16.to_le_bytes() && cfg[106] == 0 && cfg[107..139] == wp.to_bytes() && created_at > 0;
    let core_ok = cdata.first() == Some(&5) && cdata[1..33] == h.program_authority.to_bytes() && contains(&cdata, h.burn_guard.as_ref());
    let ev = event(&out, "CollectionCreated");
    let ev_ok = ev.as_ref().map(|v| v[0..32] == col.to_bytes() && v[32..64] == ap.to_bytes() && v[64..96] == seed("rooms")).unwrap_or(false);
    e.record("H03", "create_collection: Collection at the memo-home PDA (no key: off curve), Config fields, Core update authority = Program Authority, Burn Guard Oracle, event",
        out.ok() && cfg_ok && core_ok && ev_ok && !col.is_on_curve(),
        &format!("{} cfg_ok {} core_ok {} event_ok {} off_curve {}", out.brief(), cfg_ok, core_ok, ev_ok, !col.is_on_curve()));
    println!("     CU create_collection: {}", consumed(&out, &prog));

    let rejc = |id: &str, desc: &str, tag: &str, f: &dyn Fn(&str) -> Instruction, code: u32| {
        e.reject(id, desc, vec![f(tag)], w, &[&app], Expect::Custom(0, code), &[], &[]);
    };
    rejc("H04", "create_collection with max_share_bps 10001", "bad1", &|t| h.create_collection(&wp, &ap, t, "bad", 10_001, None, None).0, INVALID_SHARE_BPS);
    rejc("H05", "create_collection with max_assets 0", "bad2", &|t| h.create_collection(&wp, &ap, t, "bad", 0, Some(0), None).0, INVALID_MAX_ASSETS);
    rejc("H06", "create_collection with royalty percentages adding up to 90", "bad3", &|t| h.create_collection(&wp, &ap, t, "bad", 0, None,
        Some((500, vec![(bp, 90)]))).0, INVALID_ROYALTY);
    rejc("H06b", "create_collection with royalty basis points 10001", "bad4", &|t| h.create_collection(&wp, &ap, t, "bad", 0, None,
        Some((10_001, vec![(bp, 100)]))).0, INVALID_ROYALTY);
    rejc("H06c", "create_collection with the same royalty creator twice", "bad5", &|t| h.create_collection(&wp, &ap, t, "bad", 0, None,
        Some((500, vec![(bp, 50), (bp, 50)]))).0, INVALID_ROYALTY);
    rejc("H06d", "create_collection with a 254-byte name", "bad6", &|t| h.create_collection(&wp, &ap, t, &"n".repeat(254), 0, None, None).0,
        NAME_TOO_LONG);
    rejc("H06e", "create_collection with a 257-byte URI", "bad7", &|t| {
        let s = seed(t);
        h.create_collection_at(&wp, &ap, &h.collection_addr(&ap, &s), &s, "ok", &"u".repeat(257), 0, None, None)
    }, URI_TOO_LONG);

    let (ix, ltd) = h.create_collection(&wp, &ap, "limited", "Limited", 0, Some(2), Some((500, vec![(bp, 70), (cp, 30)])));
    let out = e.exec(&[ix], w, &[&app]);
    let ldata = e.data(&ltd).unwrap_or_default();
    let lcfg = e.data(&h.config(&ltd)).unwrap_or_default();
    let ltd_ok = contains(&ldata, bp.as_ref()) && contains(&ldata, cp.as_ref()) && contains(&ldata, &[10, 1, 2, 0, 0, 0])
        && lcfg.get(106..111) == Some(&[1, 2, 0, 0, 0][..]);
    e.record("H07", "create_collection with max_assets 2 and royalties: MasterEdition max_supply and Royalties plugins on the Collection",
        out.ok() && ltd_ok, &format!("{} plugins/config found {}", out.brief(), ltd_ok));

    {
        let plain = collection_plugins(&e, &col);
        let limited = collection_plugins(&e, &ltd);
        let oracle = vec![(1u8, 0u8, vec![(2u8, 4u32)])];
        let pass = plain == Some((vec![], oracle.clone()))
            && (limited == Some((vec![(10, 0), (0, 0)], oracle.clone())) || limited == Some((vec![(0, 0), (10, 0)], oracle)));
        e.record("H07b", "Collection plugins have authority None (nobody can change them): Burn Guard Oracle (Burn, can_reject), MasterEdition, Royalties",
            pass, &format!("plain {:?} limited {:?}", plain, limited));
    }

    // Authority consent, the Program Authority, and the PDA address
    {
        let s = seed("h08");
        let col8 = h.collection_addr(&ap, &s);
        let mut ix = h.create_collection_at(&wp, &ap, &col8, &s, "x", COLLECTION_URI, 0, None, None);
        ix.accounts[3] = AccountMeta::new_readonly(wp, false);
        e.reject("H08", "create_collection with another account as the Program Authority", vec![ix], w, &[&app],
            Expect::Custom(0, ANCHOR_CONSTRAINT_SEEDS), &[], &[]);
        e.record("H08a", "  ... and no Config was created", e.data(&h.config(&col8)).is_none(), "");

        // I-1: anyone can create a Core Collection naming the Program Authority, but it has no
        // Config, so memo-home never creates Assets in it, and memo-home cannot adopt it
        let kp = Keypair::new();
        e.must("X creates a Core Collection naming memo-home's Program Authority", &[core_create_collection(&kp.pubkey(), &h.program_authority, &xp, None)], &x, &[&kp]);
        e.reject("H08b", "create_collection cannot adopt a Core Collection at a keypair address (not the PDA)",
            vec![h.create_collection_at(&wp, &ap, &kp.pubkey(), &s, "x", COLLECTION_URI, 0, None, None)], w, &[&app],
            Expect::Custom(0, ANCHOR_CONSTRAINT_SEEDS), &[], &[]);
        let fake_asset = h.asset_addr(&kp.pubkey(), &seed("fake"));
        e.reject("H08c", "create_asset in a Core Collection that has no memo-home Config",
            vec![h.create_asset_at(&wp, &ap, &h.config(&kp.pubkey()), &kp.pubkey(), &fake_asset, &seed("fake"), &bp, "x", ASSET_URI)],
            w, &[&app], Expect::Custom(0, ANCHOR_ACCOUNT_NOT_INITIALIZED), &[], &[]);

        let (ix, _) = h.create_collection(&xp, &ap, "h09", "spoof", 10_000, None, None);
        e.reject("H09", "create_collection naming an Authority that does not sign", vec![unsigned(ix, &ap)], &x, &[],
            Expect::Custom(0, ANCHOR_ACCOUNT_NOT_SIGNER), &[], &[]);
        let s = seed("h09b");
        e.reject("H09b", "create_collection at the PDA of another Authority (the address binds the Authority)",
            vec![h.create_collection_at(&xp, &xp, &h.collection_addr(&ap, &s), &s, "spoof", COLLECTION_URI, 10_000, None, None)], &x, &[],
            Expect::Custom(0, ANCHOR_CONSTRAINT_SEEDS), &[], &[]);
        let (ix, _) = h.create_collection(&wp, &ap, "rooms", "again", 3000, None, None);
        e.reject("H09c", "create_collection with an Authority and seed already used", vec![ix], w, &[&app],
            Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &[], &[]);

        let (ix, pre) = h.create_collection(&wp, &ap, "prefunded", "Prefunded", 0, None, None);
        e.must("X sends 1 XNT to the Collection address before it exists", &[sys_ix::transfer(&xp, &pre, 1_000_000_000)], &x, &[]);
        let w0 = sol(&wp);
        let out = e.exec(&[ix], w, &[&app]);
        let back = sol(&wp) as i128 - w0 as i128;
        e.record("H09d", "create_collection at a pre-funded PDA: the lamports go to the payer and the Collection is created",
            out.ok() && e.data(&pre).map(|d| d[0] == 5).unwrap_or(false) && back > 990_000_000,
            &format!("{} payer balance change {}", out.brief(), back));
    }

    // ---------------- Assets ----------------
    let (ix, _) = h.create_asset(&xp, &xp, &col, "spam", &xp, "spam");
    e.reject("H10", "create_asset signed by someone other than the Authority", vec![ix], &x, &[], Expect::Custom(0, UNAUTHORIZED), &[], &[]);

    let (ix, room) = h.create_asset(&wp, &ap, &col, "room1", &bp, "Room #1");
    let out = e.exec(&[ix], w, &[&app]);
    let rd = e.data(&room).unwrap_or_default();
    let in_col = rd.len() > 66 && rd[33] == 2 && rd[34..66] == col.to_bytes();
    let ev = event(&out, "AssetCreated");
    let ev_ok = ev.as_ref().map(|v| v[32..64] == room.to_bytes() && v[64..96] == seed("room1") && v[96..128] == bp.to_bytes()).unwrap_or(false);
    e.record("H11", "create_asset by the Authority: Asset at the memo-home PDA (off curve) owned by B in the Collection, Vault State nonce 0, no Vault token account, event",
        out.ok() && core_owner(&e, &room) == Some(bp) && in_col && !room.is_on_curve() && h.nonce(&room) == Some(0)
            && e.data(&h.vault_ata(&room)).is_none() && ev_ok,
        &format!("{} owner {:?} in_col {} nonce {:?} event {}", out.brief(), core_owner(&e, &room), in_col, h.nonce(&room), ev_ok));
    println!("     CU create_asset: {}", consumed(&out, &prog));
    {
        let (ix, _) = h.create_asset(&wp, &ap, &col, "core-owner", &CORE, "x");
        e.reject("H11b", "create_asset for the Core program as owner (Core would give the Asset to the payer)", vec![ix], w, &[&app],
            Expect::Custom(0, INVALID_OWNER), &[], &[]);
        let (ix, _) = h.create_asset(&wp, &ap, &col, "room1", &cp, "dup");
        e.reject("H11c", "create_asset with a seed already used in the Collection", vec![ix], w, &[&app],
            Expect::Custom(0, SYSTEM_ACCOUNT_IN_USE), &[], &[]);
        let s = seed("h11d");
        let kp = Keypair::new();
        e.reject("H11d", "create_asset at an address that is not the PDA of (Collection, seed)",
            vec![h.create_asset_at(&wp, &ap, &h.config(&col), &col, &kp.pubkey(), &s, &bp, "x", ASSET_URI)], w, &[&app],
            Expect::Custom(0, ANCHOR_CONSTRAINT_SEEDS), &[], &[]);
        let (ix, pre) = h.create_asset(&wp, &ap, &col, "prefunded", &bp, "Prefunded");
        e.must("X sends 0.1 XNT to the Asset address before it exists", &[sys_ix::transfer(&xp, &pre, 100_000_000)], &x, &[]);
        let out = e.exec(&[ix], w, &[&app]);
        e.record("H11e", "create_asset at a pre-funded PDA: the lamports go to the payer and the Asset is created",
            out.ok() && core_owner(&e, &pre) == Some(bp), &out.brief());
        let (ix, _) = h.create_asset(&wp, &ap, &col, "long-name", &bp, &"n".repeat(254));
        e.reject("H11f", "create_asset with a 254-byte name", vec![ix], w, &[&app], Expect::Custom(0, NAME_TOO_LONG), &[], &[]);
        let s = seed("long-uri");
        let a = h.asset_addr(&col, &s);
        e.reject("H11g", "create_asset with a 257-byte URI", vec![h.create_asset_at(&wp, &ap, &h.config(&col), &col, &a, &s, &bp, "x", &"u".repeat(257))],
            w, &[&app], Expect::Custom(0, URI_TOO_LONG), &[], &[]);
        let s = seed("max-len");
        let a = h.asset_addr(&col, &s);
        let out = e.exec(&[h.create_asset_at(&wp, &ap, &h.config(&col), &col, &a, &s, &bp, &"n".repeat(253), &"u".repeat(256))], w, &[&app]);
        let out2 = e.exec(&[core_transfer(&a, Some(&col), &bp, &bp, &cp)], &b, &[]);
        e.record("H11h", "create_asset with a 253-byte name and a 256-byte URI; the owner can still transfer it",
            out.ok() && out2.ok() && core_owner(&e, &a) == Some(cp), &format!("{} / {}", out.brief(), out2.brief()));
    }

    {
        let (i1, _) = h.create_asset(&wp, &ap, &ltd, "ltd1", &bp, "Ltd #1");
        let (i2, _) = h.create_asset(&wp, &ap, &ltd, "ltd2", &bp, "Ltd #2");
        let (i3, _) = h.create_asset(&wp, &ap, &ltd, "ltd3", &bp, "Ltd #3");
        let o1 = e.exec(&[i1], w, &[&app]);
        let o2 = e.exec(&[i2], w, &[&app]);
        e.record("H12", "a max_assets 2 Collection accepts 2 Assets", o1.ok() && o2.ok() && collection_minted(&e, &ltd) == Some(2),
            &format!("{} / {} minted {:?}", o1.brief(), o2.brief(), collection_minted(&e, &ltd)));
        e.reject("H12b", "the third Asset is rejected (MaxAssetsReached)", vec![i3], w, &[&app], Expect::Custom(0, MAX_ASSETS_REACHED), &[], &[]);
    }
    {
        let s = seed("mix");
        let a = h.asset_addr(&ltd, &s);
        e.reject("H13", "create_asset with the config of one Collection and another Collection account",
            vec![h.create_asset_at(&wp, &ap, &h.config(&col), &ltd, &a, &s, &bp, "mix", ASSET_URI)], w, &[&app],
            Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
        let k = Keypair::new();
        e.reject("H14", "Core CreateV1 straight into a memo-home Collection by a stranger is rejected by Core",
            vec![core_create(&k.pubkey(), Some(&col), Some(&xp), &xp, &xp)], &x, &[&k], Expect::Custom(0, CORE_NO_APPROVALS), &[], &[]);
        e.reject("H14b", "Core AddCollectionPluginV1 (PermanentTransferDelegate) by a stranger is rejected by Core",
            vec![core_add_collection_plugin(&col, &xp, &xp, 7)], &x, &[], Expect::Custom(0, CORE_INVALID_AUTHORITY), &[], &[]);
        e.reject("H14c", "Core AddCollectionPluginV1 (UpdateDelegate) by a stranger is rejected by Core",
            vec![core_add_collection_plugin(&col, &xp, &xp, 4)], &x, &[], Expect::Custom(0, CORE_INVALID_AUTHORITY), &[], &[]);

        // Positive controls: the same Core instructions work where the signer has the authority
        let pc = Keypair::new();
        let pa = Keypair::new();
        let out = e.exec(&[core_create_collection(&pc.pubkey(), &xp, &xp, None), core_create(&pa.pubkey(), Some(&pc.pubkey()), Some(&xp), &xp, &xp),
            core_add_collection_plugin(&pc.pubkey(), &xp, &xp, 4)], &x, &[&pc, &pa]);
        e.record("H14d", "control: in X's own Core Collection, X can create an Asset and add an UpdateDelegate",
            out.ok() && core_owner(&e, &pa.pubkey()) == Some(xp), &out.brief());
        let out = e.exec(&[core_burn(&pa.pubkey(), Some(&pc.pubkey()), &xp, None, vec![])], &x, &[]);
        // Core leaves a 1-byte tombstone (Key::Uninitialized) after a burn
        e.record("H14e", "control: the owner can burn an Asset of a Collection without the Burn Guard",
            out.ok() && e.data(&pa.pubkey()).map(|d| d.len() <= 1).unwrap_or(true), &format!("{} data {:?}", out.brief(), e.data(&pa.pubkey())));

        // F01 root cause on this Core build: the Collection address signs as the Assets' update
        // authority. memo-home Collections are PDAs, so nobody can sign as them.
        let kc = Keypair::new();
        let ka = Keypair::new();
        e.must("X creates a Core Collection at a keypair address (update authority X) and an Asset in it", &[
            core_create_collection(&kc.pubkey(), &xp, &xp, None), core_create(&ka.pubkey(), Some(&kc.pubkey()), Some(&xp), &xp, &bp),
        ], &x, &[&kc, &ka]);
        let out = e.exec(&[core_update_v1(&ka.pubkey(), &kc.pubkey(), &xp, &kc.pubkey(), "renamed by the collection key")], &x, &[&kc]);
        e.record("H19", "control: Core lets a Collection's own address sign as update authority of its Assets (rename)",
            out.ok() && core_name(&e, &ka.pubkey()).as_deref() == Some("renamed by the collection key"), &out.brief());
        let out = e.exec(&[core_update_v2_eject(&ka.pubkey(), &kc.pubkey(), &xp, &kc.pubkey(), &xp)], &x, &[&kc]);
        e.record("H19b", "control: ... and take an Asset out of the Collection", out.ok() && core_ua(&e, &ka.pubkey()) == Some((1, Some(xp))),
            &out.brief());
    }

    let out = e.exec(&[h.update_asset(&wp, &ap, &col, &room, Some("Room #1 renamed"))], w, &[&app]);
    e.record("H15", "update_asset by the Authority renames the Asset", out.ok() && core_name(&e, &room).as_deref() == Some("Room #1 renamed"),
        &format!("{} name {:?}", out.brief(), core_name(&e, &room)));
    let out = e.exec(&[h.update_asset_with(&wp, &ap, &h.config(&col), &col, &room, None, Some("https://example.com/room1-v2.json"))], w, &[&app]);
    e.record("H15b", "update_asset with only a new URI keeps the name", out.ok()
        && core_name_uri(&e, &room) == Some(("Room #1 renamed".into(), "https://example.com/room1-v2.json".into())), &out.brief());
    e.reject("H15c", "update_asset with nothing to update", vec![h.update_asset_with(&wp, &ap, &h.config(&col), &col, &room, None, None)], w, &[&app],
        Expect::Custom(0, NOTHING_TO_UPDATE), &[], &[]);
    e.reject("H15d", "update_asset with a 254-byte name", vec![h.update_asset(&wp, &ap, &col, &room, Some(&"n".repeat(254)))], w, &[&app],
        Expect::Custom(0, NAME_TOO_LONG), &[], &[]);
    e.reject("H16", "update_asset signed by someone other than the Authority",
        vec![h.update_asset(&xp, &xp, &col, &room, Some("hacked"))], &x, &[], Expect::Custom(0, UNAUTHORIZED), &[], &[]);
    let (ix, col2) = h.create_collection(&wp, &ap, "other", "Other", 10_000, None, None);
    e.must("create a second Collection (Authority app, max share 100%)", &[ix], w, &[&app]);
    let (ix, other) = h.create_asset(&wp, &ap, &col2, "other1", &bp, "Other #1");
    e.must("create an Asset in the second Collection", &[ix], w, &[&app]);
    e.reject("H17", "update_asset with the config of a Collection the Asset is not in",
        vec![h.update_asset(&wp, &ap, &col, &other, Some("x"))], w, &[&app], Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
    let out = e.exec(&[h.update_collection(&wp, &ap, &col, Some("HOME Rooms v2"))], w, &[&app]);
    let renamed = e.data(&col).map(|d| contains(&d, b"HOME Rooms v2")).unwrap_or(false);
    e.record("H18", "update_collection by the Authority renames the Collection", out.ok() && renamed, &out.brief());
    e.reject("H18b", "update_collection with nothing to update", vec![h.update_collection_with(&wp, &ap, &h.config(&col), &col, None, None)], w, &[&app],
        Expect::Custom(0, NOTHING_TO_UPDATE), &[], &[]);
    e.reject("H18c", "update_collection signed by a stranger", vec![h.update_collection(&xp, &xp, &col, Some("x"))], &x, &[],
        Expect::Custom(0, UNAUTHORIZED), &[], &[]);
    e.reject("H18d", "update_collection with a 257-byte URI", vec![h.update_collection_with(&wp, &ap, &h.config(&col), &col, None, Some(&"u".repeat(257)))],
        w, &[&app], Expect::Custom(0, URI_TOO_LONG), &[], &[]);

    // A Collection of another app: no cross-app access
    let (ix, col3) = h.create_collection(&wp, &a2p, "app2", "App2 Rooms", 10_000, None, None);
    e.must("create a Collection with another Authority (app2)", &[ix], w, &[&app2]);
    let (ix, room3) = h.create_asset(&wp, &a2p, &col3, "a2-1", &bp, "App2 #1");
    e.must("app2 creates an Asset", &[ix], w, &[&app2]);
    e.reject("H18e", "update_collection by app with its own config and app2's Collection account",
        vec![h.update_collection_with(&wp, &ap, &h.config(&col), &col3, Some("x"), None)], w, &[&app], Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
    e.reject("H18f", "update_collection of app2's Collection signed by app", vec![h.update_collection(&wp, &ap, &col3, Some("x"))], w, &[&app],
        Expect::Custom(0, UNAUTHORIZED), &[], &[]);
    e.reject("H17b", "update_asset with app's config and app2's Collection account",
        vec![h.update_asset_with(&wp, &ap, &h.config(&col), &col3, &room3, Some("x"), None)], w, &[&app], Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
    e.reject("H17c", "update_asset of app2's Asset signed by app", vec![h.update_asset(&wp, &ap, &col3, &room3, Some("x"))], w, &[&app],
        Expect::Custom(0, UNAUTHORIZED), &[], &[]);

    // Nobody can use Core directly to change a memo-home Asset (no key exists for the Collection)
    for (id, who, kp) in [("H19c", "the Collection's creator (payer)", w), ("H19d", "the Authority", &app), ("H19e", "a stranger", &x)] {
        let k = kp.pubkey();
        e.reject(id, &format!("Core UpdateV1 of a memo-home Asset signed by {} is rejected by Core", who),
            vec![core_update_v1(&room, &col, &k, &k, "pwned")], kp, &[], Expect::Custom(0, CORE_NO_APPROVALS), &[], &[]);
    }
    e.reject("H19f", "Core UpdateV2 taking a memo-home Asset out of its Collection, signed by the Authority, is rejected by Core",
        vec![core_update_v2_eject(&room, &col, &ap, &ap, &ap)], &app, &[], Expect::Custom(0, CORE_NO_APPROVALS), &[], &[]);
    e.record("H19g", "  ... the Asset keeps its name and stays in the Collection",
        core_name(&e, &room).as_deref() == Some("Room #1 renamed") && core_ua(&e, &room) == Some((2, Some(col))), "");

    // ---------------- Mining ----------------
    let sender = e.new_user("message sender", 0, false);
    let sp = sender.pubkey();
    let s_ta = e.ata(&sp);
    let vault_ta = h.vault_ata(&room);
    let mine_tx = |share: u16, tag: &str| vec![e.text_memo(tag, 80), cu(400_000), h.mine(&sp, &ap, &col, &room, &s_ta, share)];
    let mine_check = |id: &str, desc: &str, ixs: Vec<Instruction>, signers: &[&Keypair], asset: &Pubkey, share: u16| {
        let vta = h.vault_ata(asset);
        let before = e.snap(&[vta, s_ta], &[]);
        let nonce = h.nonce(asset);
        let reward = curve_reference(before.supply).unwrap();
        let out = e.exec(&ixs, &sender, signers);
        let after = e.snap(&[vta, s_ta], &[]);
        let share_amt = (reward as u128 * share as u128 / 10_000) as u64;
        let pass = out.ok()
            && after.supply - before.supply == reward
            && after.bal(&vta) - before.bal(&vta) == reward - share_amt
            && after.bal(&s_ta) - before.bal(&s_ta) == share_amt
            && out.minted_log_count() == 1
            && h.nonce(asset) == nonce;
        e.record(id, desc, pass, &format!("{} reward {} share {} vault {:?}->{:?} sender {:?}->{:?}", out.brief(), reward, share_amt,
            before.bal(&vta), after.bal(&vta), before.bal(&s_ta), after.bal(&s_ta)));
        out
    };
    e.reject("H20", "mine without the SPL Memo at index 0 (memo-mint MemoRequired)",
        vec![cu(400_000), h.mine(&sp, &ap, &col, &room, &s_ta, 3000)], &sender, &[&app], Expect::Custom(1, MINT_MEMO_REQUIRED), &[s_ta], &[]);
    let out = mine_check("H21", "first mine creates the Vault token account; 30% share rounded down to the sender, the rest in the Vault (nonce unchanged)",
        mine_tx(3000, "m1"), &[&app], &room, 3000);
    println!("     CU mine (first, creates the Vault token account): memo-home {} of tx", consumed(&out, &prog));
    let out = mine_check("H22", "second mine with the Vault token account already present", mine_tx(3000, "m2"), &[&app], &room, 3000);
    println!("     CU mine: memo-home {}", consumed(&out, &prog));
    mine_check("H23", "mine with share 0: the whole reward stays in the Vault", mine_tx(0, "m3"), &[&app], &room, 0);
    mine_check("H23b", "mine with share 33.33% (on-chain rounding down)",
        vec![e.text_memo("m3b", 80), cu(400_000), h.mine(&sp, &ap, &col2, &other, &s_ta, 3333)], &[&app], &other, 3333);
    e.reject("H24", "mine with share_bps above max_share_bps (3001 > 3000)", mine_tx(3001, "m4"), &sender, &[&app],
        Expect::Custom(2, SHARE_TOO_HIGH), &[vault_ta, s_ta], &[]);
    e.reject("H25", "mine signed by someone other than the Authority",
        vec![e.text_memo("m5", 80), cu(400_000), h.mine(&sp, &xp, &col, &room, &s_ta, 3000)], &sender, &[&x],
        Expect::Custom(2, UNAUTHORIZED), &[vault_ta, s_ta], &[]);
    e.reject("H26", "mine for an Asset of another Collection",
        vec![e.text_memo("m6", 80), cu(400_000), h.mine(&sp, &ap, &col, &other, &s_ta, 3000)], &sender, &[&app],
        Expect::Custom(2, WRONG_COLLECTION), &[vault_ta, s_ta], &[]);
    e.reject("H26b", "app2 mines into app's Asset with its own config",
        vec![e.text_memo("m6b", 80), cu(400_000), h.mine(&sp, &a2p, &col3, &room, &s_ta, 3000)], &sender, &[&app2],
        Expect::Custom(2, WRONG_COLLECTION), &[vault_ta, s_ta], &[]);
    {
        let m2 = e.new_mint(&wp);
        let wrong_ta = ata_of(&sp, &m2.pubkey(), &t22());
        e.must("create a token account of another mint", &[e.create_ata_ix(&sp, &m2.pubkey())], w, &[]);
        e.reject("H27", "mine with a recipient token account of another mint",
            vec![e.text_memo("m7", 80), cu(400_000), h.mine(&sp, &ap, &col, &room, &wrong_ta, 3000)], &sender, &[&app],
            Expect::Custom(2, ANCHOR_CONSTRAINT_TOKEN_MINT), &[vault_ta, s_ta], &[]);
        let mut ix = h.mine(&sp, &ap, &col, &room, &s_ta, 3000);
        ix.accounts[5] = AccountMeta::new(ata_of(&h.vault(&room), &m2.pubkey(), &t22()), false);
        ix.accounts[6] = AccountMeta::new(wrong_ta, false);
        ix.accounts[7] = AccountMeta::new(m2.pubkey(), false);
        e.reject("H27b", "mine with another mint as the MEMO mint", vec![e.text_memo("m8", 80), cu(400_000), ix], &sender, &[&app],
            Expect::Custom(2, INVALID_MINT), &[vault_ta, s_ta], &[]);
        let mut ix = h.mine(&sp, &ap, &col, &room, &s_ta, 3000);
        ix.accounts[5] = AccountMeta::new(s_ta, false);
        e.reject("H27c", "mine with a token account other than the Vault's ATA as the Vault token account",
            vec![e.text_memo("m8b", 80), cu(400_000), ix], &sender, &[&app], Expect::Custom(2, INVALID_VAULT_TOKEN_ACCOUNT), &[vault_ta, s_ta], &[]);
        let mut ix = h.mine(&sp, &ap, &col, &room, &s_ta, 3000);
        ix.accounts[13] = AccountMeta::new_readonly(e.burn_prog, false);
        e.reject("H27d", "mine with another program as memo-mint", vec![e.text_memo("m8c", 80), cu(400_000), ix], &sender, &[&app],
            Expect::Custom(2, ANCHOR_INVALID_PROGRAM_ID), &[vault_ta, s_ta], &[]);
    }
    e.reject("H28", "two mines in one transaction (memo-mint OneMintPerTransaction)",
        vec![e.text_memo("m9", 80), cu(800_000), h.mine(&sp, &ap, &col, &room, &s_ta, 3000), h.mine(&sp, &ap, &col, &room, &s_ta, 3000)],
        &sender, &[&app], Expect::Custom(3, MINT_ONE_PER_TRANSACTION), &[vault_ta, s_ta], &[]);
    e.reject("H29", "mine with the Vault's own token account as the share recipient",
        vec![e.text_memo("m10", 80), cu(400_000), h.mine(&sp, &ap, &col, &room, &vault_ta, 3000)], &sender, &[&app],
        Expect::Custom(2, INVALID_DESTINATION), &[vault_ta, s_ta], &[]);

    // ---------------- Withdraw ----------------
    let b_ta = e.ata(&bp);
    let wd_ok = |id: &str, desc: &str, owner: &Keypair, dest: &Pubkey, amount: u64| {
        let before = e.snap(&[vault_ta, *dest], &[]);
        let n0 = h.nonce(&room).unwrap_or(u64::MAX);
        let out = e.exec(&[h.withdraw(&owner.pubkey(), &col, &room, dest, amount)], owner, &[]);
        let after = e.snap(&[vault_ta, *dest], &[]);
        let pass = out.ok() && before.bal(&vault_ta) - after.bal(&vault_ta) == amount && after.bal(dest) - before.bal(dest) == amount
            && h.nonce(&room) == Some(n0 + 1);
        e.record(id, desc, pass, &format!("{}; {} nonce {} -> {:?}", out.brief(), before.diff(&after), n0, h.nonce(&room)));
        out
    };
    let out = wd_ok("H30", "withdraw by the current owner B moves exactly the amount; the Vault State nonce goes up by 1", &b, &b_ta, 100_000);
    {
        let ev = event(&out, "Withdrawn");
        let ok = ev.as_ref().map(|v| v.len() == 112 && v[0..32] == room.to_bytes() && v[32..64] == e.mint.to_bytes()
            && v[64..96] == b_ta.to_bytes() && v[96..104] == 100_000u64.to_le_bytes() && v[104..112] == 1u64.to_le_bytes()).unwrap_or(false);
        e.record("H30b", "the Withdrawn event carries asset, mint, destination, amount and the new nonce", ok, &format!("{:?}", ev));
    }
    e.reject("H31", "withdraw by a stranger", vec![h.withdraw(&xp, &col, &room, &e.ata(&xp), 1)], &x, &[], Expect::Custom(0, NOT_OWNER), &[vault_ta], &[]);
    e.reject("H32", "withdraw amount 0", vec![h.withdraw(&bp, &col, &room, &b_ta, 0)], &b, &[], Expect::Custom(0, INVALID_AMOUNT), &[vault_ta], &[]);
    e.reject("H32b", "withdraw to the Vault token account itself", vec![h.withdraw(&bp, &col, &room, &vault_ta, 1)], &b, &[],
        Expect::Custom(0, INVALID_DESTINATION), &[vault_ta], &[]);
    let vbal = e.balance(&vault_ta).unwrap_or(0);
    e.reject("H33", "withdraw more than the Vault holds", vec![h.withdraw(&bp, &col, &room, &b_ta, vbal + 1)], &b, &[],
        Expect::Custom(0, TOKEN_INSUFFICIENT_FUNDS), &[vault_ta, b_ta], &[]);
    e.record("H33b", "  ... a failed withdrawal leaves the nonce unchanged", h.nonce(&room) == Some(1), &format!("{:?}", h.nonce(&room)));
    e.reject("H34", "withdraw with the config of another Collection",
        vec![h.withdraw_with(&bp, &h.config(&col2), &room, &vault_ta, &b_ta, &e.mint, &t22(), 1)], &b, &[],
        Expect::Custom(0, WRONG_COLLECTION), &[vault_ta, b_ta], &[]);
    e.reject("H34b", "withdraw from the Vault of another Asset (token account authority mismatch)",
        vec![h.withdraw_with(&bp, &h.config(&col2), &other, &vault_ta, &b_ta, &e.mint, &t22(), 1)], &b, &[],
        Expect::Custom(0, ANCHOR_CONSTRAINT_TOKEN_OWNER), &[vault_ta, b_ta], &[]);
    {
        let mut ix = h.withdraw(&bp, &col, &room, &b_ta, 1);
        ix.accounts[4] = AccountMeta::new(h.vault_state(&other), false);
        e.reject("H34c", "withdraw with the Vault State of another Asset", vec![ix], &b, &[], Expect::Custom(0, ANCHOR_CONSTRAINT_SEEDS), &[vault_ta, b_ta], &[]);
    }

    // Ownership follows Core transfers
    e.must("B transfers the Asset to C with Core", &[core_transfer(&room, Some(&col), &bp, &bp, &cp)], &b, &[]);
    e.reject("H35", "the previous owner B can no longer withdraw", vec![h.withdraw(&bp, &col, &room, &b_ta, 1)], &b, &[],
        Expect::Custom(0, NOT_OWNER), &[vault_ta, b_ta], &[]);
    let c_ta = e.ata(&cp);
    wd_ok("H35b", "the new owner C can withdraw right after the transfer", &c, &c_ta, 50_000);

    // ---------------- Listing protection ----------------
    e.must("C lists: TransferDelegate with Address(market)", &[core_add_delegate(&room, &col, &cp, 3, Some(&mp))], &c, &[]);
    e.reject("H36", "withdraw while a TransferDelegate is held by a market (Listed)", vec![h.withdraw(&cp, &col, &room, &c_ta, 1)], &c, &[],
        Expect::Custom(0, LISTED), &[vault_ta, c_ta], &[]);
    e.reject("H36b", "Vault burn while listed", vec![app_burn_memo(1, b"listed burn attempt payload, padded to a reasonable length......"),
        h.burn(&cp, &cp, &col, &room, 1)], &c, &[], Expect::Custom(1, LISTED), &[vault_ta], &[]);
    e.must("the market sells the Asset to D (delegate transfer)", &[core_transfer(&room, Some(&col), &wp, &mp, &dp)], w, &[&market]);
    let d_ta = e.ata(&dp);
    wd_ok("H37", "after the sale Core reset the delegate: the buyer D can withdraw", &d, &d_ta, 10_000);

    e.must("D adds an owner-managed FreezeDelegate (authority Owner, not frozen)", &[core_add_delegate(&room, &col, &dp, 1, None)], &d, &[]);
    wd_ok("H38", "an owner-managed, unfrozen FreezeDelegate does not block withdrawals", &d, &d_ta, 1_000);
    e.must("D freezes the Asset", &[core_set_frozen(&room, &col, &dp, &dp, true)], &d, &[]);
    e.reject("H38b", "withdraw while frozen (Listed)", vec![h.withdraw(&dp, &col, &room, &d_ta, 1)], &d, &[],
        Expect::Custom(0, LISTED), &[vault_ta, d_ta], &[]);
    e.must("D unfreezes the Asset", &[core_set_frozen(&room, &col, &dp, &dp, false)], &d, &[]);
    wd_ok("H38c", "withdraw works again after unfreezing", &d, &d_ta, 1_000);

    // Any token: Token-2022 and legacy SPL Token
    {
        let vault = h.vault(&room);
        let m2 = e.new_mint(&wp);
        let v2 = ata_of(&vault, &m2.pubkey(), &t22());
        let d2 = ata_of(&dp, &m2.pubkey(), &t22());
        e.must("fund the Vault with another Token-2022 token", &[
            e.create_ata_ix(&vault, &m2.pubkey()),
            e.create_ata_ix(&dp, &m2.pubkey()),
            spl_token_2022::instruction::mint_to(&t22(), &m2.pubkey(), &v2, &wp, &[], 1_000).unwrap(),
        ], w, &[]);
        let before = e.snap(&[v2, d2], &[]);
        let out = e.exec(&[h.withdraw_with(&dp, &h.config(&col), &room, &v2, &d2, &m2.pubkey(), &t22(), 400)], &d, &[]);
        let after = e.snap(&[v2, d2], &[]);
        e.record("H39", "withdraw another Token-2022 token from the Vault",
            out.ok() && before.bal(&v2) - after.bal(&v2) == 400 && after.bal(&d2) - before.bal(&d2) == 400, &out.brief());

        let m3 = Keypair::new();
        let rent = e.c.get_minimum_balance_for_rent_exemption(82).unwrap();
        let v3 = ata_of(&vault, &m3.pubkey(), &SPL_TOKEN_LEGACY);
        let d3 = ata_of(&dp, &m3.pubkey(), &SPL_TOKEN_LEGACY);
        e.must("create a legacy SPL Token mint and fund the Vault", &[
            sys_ix::create_account(&wp, &m3.pubkey(), rent, 82, &SPL_TOKEN_LEGACY),
            spl_token_2022::instruction::initialize_mint2(&SPL_TOKEN_LEGACY, &m3.pubkey(), &wp, None, 0).unwrap(),
            create_associated_token_account(&wp, &vault, &m3.pubkey(), &SPL_TOKEN_LEGACY),
            create_associated_token_account(&wp, &dp, &m3.pubkey(), &SPL_TOKEN_LEGACY),
            spl_token_2022::instruction::mint_to(&SPL_TOKEN_LEGACY, &m3.pubkey(), &v3, &wp, &[], 5).unwrap(),
        ], w, &[&m3]);
        let before = e.snap(&[v3, d3], &[]);
        let out = e.exec(&[h.withdraw_with(&dp, &h.config(&col), &room, &v3, &d3, &m3.pubkey(), &SPL_TOKEN_LEGACY, 2)], &d, &[]);
        let after = e.snap(&[v3, d3], &[]);
        e.record("H39b", "withdraw a legacy SPL Token token from the Vault (X1's SPL Token build)",
            out.ok() && before.bal(&v3) - after.bal(&v3) == 2 && after.bal(&d3) - before.bal(&d3) == 2, &out.brief());
    }

    // ---------------- Burn Guard: Assets cannot be burned ----------------
    {
        e.reject("H40", "Core BurnV1 by the owner with the Burn Guard account is rejected by the Burn Guard",
            vec![core_burn(&room, Some(&col), &dp, None, vec![AccountMeta::new_readonly(h.burn_guard, false)])], &d, &[],
            Expect::Custom(0, CORE_INVALID_AUTHORITY), &[], &[]);
        e.reject("H40b", "Core BurnV1 by the owner without the Burn Guard account is rejected (Oracle account missing)",
            vec![core_burn(&room, Some(&col), &dp, None, vec![])], &d, &[], Expect::Custom(0, CORE_MISSING_EXTERNAL_ACCOUNT), &[], &[]);
        e.must("D approves a BurnDelegate for X", &[core_add_delegate(&room, &col, &dp, 2, Some(&xp))], &d, &[]);
        e.reject("H40c", "Core BurnV1 by a BurnDelegate is rejected too",
            vec![core_burn(&room, Some(&col), &xp, Some(&xp), vec![AccountMeta::new_readonly(h.burn_guard, false)])], &x, &[],
            Expect::Custom(0, CORE_INVALID_AUTHORITY), &[], &[]);
        e.reject("H40d", "withdraw while a BurnDelegate is held by someone else (Listed)", vec![h.withdraw(&dp, &col, &room, &d_ta, 1)], &d, &[],
            Expect::Custom(0, LISTED), &[vault_ta, d_ta], &[]);
        e.record("H40e", "  ... the Asset still exists, owned by D", core_owner(&e, &room) == Some(dp), "");
        e.must("D revokes the BurnDelegate", &[core_ix(vec![10, 2], &room, Some(&col), &dp, None, vec![])], &d, &[]);
    }

    // ---------------- Vault burn ----------------
    {
        let vault = h.vault(&room);
        let amount = 7_000u64;
        // UserGlobalBurnStats size, from the wallet's own stats account
        let stats_len = e.data(&e.stats_pda(&wp)).map(|d| d.len()).expect("wallet burn stats");
        let stats_rent = e.c.get_minimum_balance_for_rent_exemption(stats_len).unwrap();
        let before = e.snap(&[vault_ta], &[vault]);
        let (v0, w0, n0) = (sol(&vault), sol(&wp), h.nonce(&room).unwrap_or(u64::MAX));
        let out = e.exec(&[app_burn_memo(amount, b"memo-home regression: first vault burn, initializes the vault burn stats"),
            cu(400_000), h.burn(&dp, &wp, &col, &room, amount)], &d, &[w]);
        let after = e.snap(&[vault_ta], &[vault]);
        let paid = w0 - sol(&wp);
        e.record("H50", "first Vault burn: Vault stats created (payer tops up exactly stats rent + empty-account rent), balance, supply and stats move by the amount, nonce +1",
            out.ok() && e.burn_deltas_ok(&before, &after, &out, &vault, &vault_ta, amount, 1)
                && v0 == 0 && paid == stats_rent + min_rent && sol(&vault) == min_rent && h.nonce(&room) == Some(n0 + 1),
            &format!("{} payer paid {} (expected {}) vault {} -> {}; {}", out.brief(), paid, stats_rent + min_rent, v0, sol(&vault), before.diff(&after)));
        println!("     CU burn (first, with stats init): memo-home {}", consumed(&out, &prog));
        let before = e.snap(&[vault_ta], &[vault]);
        let out = e.exec(&[app_burn_memo(amount, b"memo-home regression: second vault burn"), cu(400_000), h.burn(&dp, &dp, &col, &room, amount)], &d, &[]);
        let after = e.snap(&[vault_ta], &[vault]);
        e.record("H51", "second Vault burn", out.ok() && e.burn_deltas_ok(&before, &after, &out, &vault, &vault_ta, amount, 1), &out.brief());
        println!("     CU burn: memo-home {}", consumed(&out, &prog));
        e.reject("H52", "Vault burn by someone other than the owner",
            vec![app_burn_memo(1, b"stranger burn attempt payload of a reasonable length......"), h.burn(&cp, &cp, &col, &room, 1)], &c, &[],
            Expect::Custom(1, NOT_OWNER), &[vault_ta], &[vault]);
        e.reject("H53", "Vault burn with a memo amount that does not match (memo-burn)",
            vec![app_burn_memo(2, b"mismatching amount payload of a reasonable length........"), h.burn(&dp, &dp, &col, &room, 1)], &d, &[],
            Expect::Custom(1, BURN_AMOUNT_MISMATCH), &[vault_ta], &[vault]);
        let mut ix = h.burn(&dp, &dp, &col, &room, 1);
        ix.accounts[12] = AccountMeta::new_readonly(e.mint_prog, false);
        e.reject("H54", "Vault burn with another program as memo-burn",
            vec![app_burn_memo(1, b"wrong burn program payload of a reasonable length.........."), ix], &d, &[],
            Expect::Custom(1, ANCHOR_INVALID_PROGRAM_ID), &[vault_ta], &[vault]);

        // First burn with a Vault that already holds enough XNT: the payer pays nothing
        let (ix, r6) = h.create_asset(&wp, &ap, &col, "room6", &dp, "Room #6");
        e.must("create Room #6 for D", &[ix], w, &[&app]);
        let v6 = h.vault(&r6);
        let v6_ta = h.vault_ata(&r6);
        e.must("fund Room #6's Vault with 1 MEMO and 0.01 XNT", &[
            e.create_ata_ix(&v6, &e.mint),
            spl_token_2022::instruction::transfer_checked(&t22(), &e.ata(&wp), &e.mint, &v6_ta, &wp, &[], UNIT, 6).unwrap(),
            sys_ix::transfer(&wp, &v6, 10_000_000),
        ], w, &[]);
        let (w0, v0) = (sol(&wp), sol(&v6));
        let out = e.exec(&[app_burn_memo(1000, b"memo-home regression: first burn with a funded vault......"), cu(400_000),
            h.burn(&dp, &wp, &col, &r6, 1000)], &d, &[w]);
        e.record("H55", "first Vault burn when the Vault already holds the rent: the payer pays nothing, the Vault pays the stats rent",
            out.ok() && sol(&wp) == w0 && v0 - sol(&v6) == stats_rent, &format!("{} payer {} vault {} -> {}", out.brief(), w0 - sol(&wp), v0, sol(&v6)));
    }

    // ---------------- withdraw_asset ----------------
    {
        let vault = h.vault(&room);
        let plain = Keypair::new();
        e.must("create a plain Core Asset owned by the Vault", &[core_create_plain(&plain.pubkey(), &wp, &vault)], w, &[&plain]);
        e.reject("H60", "withdraw_asset by someone other than the owner",
            vec![h.withdraw_asset(&cp, &cp, &col, &room, &plain.pubkey(), &CORE, &cp)], &c, &[], Expect::Custom(0, NOT_OWNER), &[], &[]);
        let n0 = h.nonce(&room).unwrap_or(u64::MAX);
        let out = e.exec(&[h.withdraw_asset(&dp, &wp, &col, &room, &plain.pubkey(), &CORE, &xp)], &d, &[w]);
        e.record("H61", "withdraw_asset moves a plain Core Asset out of the Vault (separate payer), nonce +1",
            out.ok() && core_owner(&e, &plain.pubkey()) == Some(xp) && h.nonce(&room) == Some(n0 + 1),
            &format!("{} owner {:?}", out.brief(), core_owner(&e, &plain.pubkey())));
        e.reject("H62", "withdraw_asset of an Asset the Vault does not own",
            vec![h.withdraw_asset(&dp, &dp, &col, &room, &plain.pubkey(), &CORE, &dp)], &d, &[], Expect::Custom(0, NOT_IN_VAULT), &[], &[]);

        let (ix, nested) = h.create_asset(&wp, &ap, &col, "room2", &dp, "Room #2");
        e.must("create a memo-home Asset for D", &[ix], w, &[&app]);
        e.must("D sends it into the Vault of Room #1", &[core_transfer(&nested, Some(&col), &dp, &dp, &vault)], &d, &[]);
        e.reject("H63", "withdraw_asset with the wrong inner Collection account",
            vec![h.withdraw_asset(&dp, &dp, &col, &room, &nested, &CORE, &dp)], &d, &[], Expect::Custom(0, WRONG_COLLECTION), &[], &[]);
        let out = e.exec(&[h.withdraw_asset(&dp, &dp, &col, &room, &nested, &col, &dp)], &d, &[]);
        e.record("H64", "withdraw_asset moves a nested memo-home Asset (with its Collection) back to D",
            out.ok() && core_owner(&e, &nested) == Some(dp), &out.brief());

        // Remaining accounts reach Core: an inner Asset whose Collection has an Oracle on Transfer
        let oc = Keypair::new();
        let oa = Keypair::new();
        e.must("create a Core Collection with an Oracle on Transfer (the Burn Guard: transfer Pass) and an Asset in it owned by the Vault", &[
            core_create_collection(&oc.pubkey(), &wp, &wp, Some(&h.burn_guard)),
            core_create(&oa.pubkey(), Some(&oc.pubkey()), Some(&wp), &wp, &vault),
        ], w, &[&oc, &oa]);
        e.reject("H66", "withdraw_asset without the Oracle account Core needs fails in Core",
            vec![h.withdraw_asset(&dp, &dp, &col, &room, &oa.pubkey(), &oc.pubkey(), &dp)], &d, &[],
            Expect::Custom(0, CORE_MISSING_EXTERNAL_ACCOUNT), &[], &[]);
        let mut ix = h.withdraw_asset(&dp, &dp, &col, &room, &oa.pubkey(), &oc.pubkey(), &dp);
        ix.accounts.push(AccountMeta::new_readonly(h.burn_guard, false));
        let out = e.exec(&[ix], &d, &[]);
        e.record("H66b", "withdraw_asset with the Oracle account as a remaining account", out.ok() && core_owner(&e, &oa.pubkey()) == Some(dp),
            &out.brief());

        e.must("D lists Room #1 again (approve the existing TransferDelegate for the market)",
            &[core_ix([vec![8u8, 3, 3], mp.to_bytes().to_vec()].concat(), &room, Some(&col), &dp, None, vec![])], &d, &[]);
        let plain2 = Keypair::new();
        e.must("create another plain Core Asset owned by the Vault", &[core_create_plain(&plain2.pubkey(), &wp, &vault)], w, &[&plain2]);
        e.reject("H65", "withdraw_asset while listed", vec![h.withdraw_asset(&dp, &dp, &col, &room, &plain2.pubkey(), &CORE, &dp)], &d, &[],
            Expect::Custom(0, LISTED), &[], &[]);
    }

    // ---------------- Listing styles: unfrozen delegates vs. frozen listings ----------------
    {
        let (ix, lst) = h.create_asset(&wp, &ap, &col, "room3", &dp, "Room #3");
        e.must("create Room #3 for D", &[ix], w, &[&app]);
        let vta = h.vault_ata(&lst);
        e.must("fund Room #3's Vault with 1 MEMO", &[
            e.create_ata_ix(&h.vault(&lst), &e.mint),
            spl_token_2022::instruction::transfer_checked(&t22(), &e.ata(&wp), &e.mint, &vta, &wp, &[], UNIT, 6).unwrap(),
        ], w, &[]);
        let revoke = |t: u8| core_ix(vec![10, t], &lst, Some(&col), &dp, None, vec![]);
        let approve = |t: u8| core_ix([vec![8u8, t, 3], mp.to_bytes().to_vec()].concat(), &lst, Some(&col), &dp, None, vec![]);

        // Unfrozen TransferDelegate listing: the owner can revoke, withdraw and re-approve in one
        // transaction, but the Vault State nonce shows it
        e.must("D lists Room #3 with an unfrozen TransferDelegate held by the market", &[core_add_delegate(&lst, &col, &dp, 3, Some(&mp))], &d, &[]);
        let listed_nonce = h.nonce(&lst);
        let before = e.balance(&vta).unwrap_or(0);
        let out = e.exec(&[revoke(3), h.withdraw(&dp, &col, &lst, &d_ta, 100), approve(3)], &d, &[]);
        e.record("H80", "KNOWN LIMITATION: with an unfrozen TransferDelegate listing the owner can revoke, withdraw and re-approve in one transaction",
            out.ok() && before - e.balance(&vta).unwrap_or(0) == 100, &out.brief());
        e.record("H80b", "  ... the Vault State nonce recorded at listing has changed, so a market that checks it refuses the sale",
            listed_nonce == Some(0) && h.nonce(&lst) == Some(1), &format!("{:?} -> {:?}", listed_nonce, h.nonce(&lst)));
        e.must("D revokes the TransferDelegate", &[revoke(3)], &d, &[]);

        // Frozen listing (FreezeDelegate held and frozen by the market): the owner cannot undo it
        e.must("D adds a FreezeDelegate held by the market", &[core_add_delegate(&lst, &col, &dp, 1, Some(&mp))], &d, &[]);
        e.must("D approves the TransferDelegate for the market again", &[approve(3)], &d, &[]);
        e.must("the market freezes Room #3", &[core_set_frozen(&lst, &col, &wp, &mp, true)], w, &[&market]);
        e.reject("H81", "frozen listing: the owner cannot revoke the market's FreezeDelegate to withdraw",
            vec![revoke(1), h.withdraw(&dp, &col, &lst, &d_ta, 100)], &d, &[], Expect::Custom(0, CORE_INVALID_AUTHORITY), &[vta], &[]);
        e.reject("H81b", "frozen listing: the owner cannot remove the FreezeDelegate plugin to withdraw",
            vec![core_ix(vec![4, 1], &lst, Some(&col), &dp, None, vec![]), h.withdraw(&dp, &col, &lst, &d_ta, 100)], &d, &[],
            Expect::Custom(0, CORE_INVALID_AUTHORITY), &[vta], &[]);
        e.reject("H81c", "frozen listing: revoking only the TransferDelegate still leaves the Asset listed (frozen)",
            vec![revoke(3), h.withdraw(&dp, &col, &lst, &d_ta, 100)], &d, &[], Expect::Custom(1, LISTED), &[vta], &[]);
        e.reject("H81d", "frozen listing: the owner cannot unfreeze the market's FreezeDelegate",
            vec![revoke(3), core_set_frozen(&lst, &col, &dp, &dp, false), h.withdraw(&dp, &col, &lst, &d_ta, 100)], &d, &[],
            Expect::Custom(1, CORE_NO_APPROVALS), &[vta], &[]);

        // Sale: the market unfreezes and transfers; Core resets both delegates, the buyer can withdraw
        let out = e.exec(&[core_set_frozen(&lst, &col, &wp, &mp, false), core_transfer(&lst, Some(&col), &wp, &mp, &xp)], w, &[&market]);
        e.record("H82", "the market unfreezes and sells Room #3 to X", out.ok() && core_owner(&e, &lst) == Some(xp), &out.brief());
        let x_ta = e.ata(&xp);
        let before = e.snap(&[vta, x_ta], &[]);
        let out = e.exec(&[h.withdraw(&xp, &col, &lst, &x_ta, 1000)], &x, &[]);
        let after = e.snap(&[vta, x_ta], &[]);
        e.record("H82b", "after a frozen-listing sale Core reset the FreezeDelegate too: the buyer X can withdraw",
            out.ok() && after.bal(&x_ta) - before.bal(&x_ta) == 1000, &format!("{}; {}", out.brief(), before.diff(&after)));
    }

    // ---------------- Token-2022 transfer hook (test-programs/transfer-hook) ----------------
    {
        let present = e.c.get_account(&HOOK_PROGRAM).map(|a| a.executable).unwrap_or(false);
        e.record("H89", "the test transfer hook program is loaded (target/test-programs/transfer_hook_test.so)", present, "");
        if present {
            use spl_token_2022::extension::{transfer_hook, ExtensionType};
            let hook = HOOK_PROGRAM;
            let (ix, room4) = h.create_asset(&wp, &ap, &col, "room4", &dp, "Room #4");
            e.must("create Room #4 for D", &[ix], w, &[&app]);
            let m = Keypair::new();
            let mk = m.pubkey();
            let len = ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&[ExtensionType::TransferHook]).unwrap();
            let rent = e.c.get_minimum_balance_for_rent_exemption(len).unwrap();
            let metas = Pubkey::find_program_address(&[b"extra-account-metas", mk.as_ref()], &hook).0;
            let holder = Pubkey::find_program_address(&[b"holder", mk.as_ref()], &hook).0;
            let vault = h.vault(&room4);
            let v_ta = ata_of(&vault, &mk, &t22());
            let d_hook_ta = ata_of(&dp, &mk, &t22());
            e.must("create a Token-2022 mint with a transfer hook", &[
                sys_ix::create_account(&wp, &mk, rent, len as u64, &t22()),
                transfer_hook::instruction::initialize(&t22(), &mk, Some(wp), Some(hook)).unwrap(),
                spl_token_2022::instruction::initialize_mint2(&t22(), &mk, &wp, None, 0).unwrap(),
                Instruction::new_with_bytes(hook, &[0xA0], vec![
                    AccountMeta::new(wp, true), AccountMeta::new_readonly(mk, false), AccountMeta::new(metas, false),
                    AccountMeta::new(holder, false), AccountMeta::new_readonly(system_program(), false),
                ]),
            ], w, &[&m]);
            e.must("fund Room #4's Vault with 10 hook tokens", &[
                e.create_ata_ix(&vault, &mk),
                e.create_ata_ix(&dp, &mk),
                spl_token_2022::instruction::mint_to(&t22(), &mk, &v_ta, &wp, &[], 10).unwrap(),
            ], w, &[]);
            let wd = h.withdraw_with(&dp, &h.config(&col), &room4, &v_ta, &d_hook_ta, &mk, &t22(), 3);
            e.reject("H90", "withdraw of a transfer-hook token without the hook's extra accounts fails in Token-2022", vec![wd.clone()], &d, &[],
                Expect::ErrContains("InstructionError"), &[v_ta, d_hook_ta], &[]);
            let mut wd = wd;
            wd.accounts.extend([AccountMeta::new(holder, false), AccountMeta::new_readonly(hook, false), AccountMeta::new_readonly(metas, false)]);
            let out = e.exec(&[wd], &d, &[]);
            e.record("H91", "withdraw of a transfer-hook token with the hook's extra accounts as remaining accounts",
                out.ok() && e.balance(&v_ta) == Some(7) && e.balance(&d_hook_ta) == Some(3), &out.brief());
        }
    }

    // ---------------- Native XNT and token-account rent ----------------
    {
        let (ix, r5) = h.create_asset(&wp, &ap, &col, "room5", &dp, "Room #5");
        e.must("create Room #5 for D", &[ix], w, &[&app]);
        let vault = h.vault(&r5);
        e.must("someone sends 0.5 XNT to Room #5's Vault", &[sys_ix::transfer(&wp, &vault, 500_000_000)], w, &[]);
        let (v0, x0, n0) = (sol(&vault), sol(&xp), h.nonce(&r5).unwrap_or(u64::MAX));
        let out = e.exec(&[h.withdraw_lamports(&dp, &col, &r5, &xp, 200_000_000)], &d, &[]);
        e.record("H100", "withdraw_lamports by the owner moves exactly the amount, nonce +1", out.ok()
            && v0 - sol(&vault) == 200_000_000 && sol(&xp) - x0 == 200_000_000 && h.nonce(&r5) == Some(n0 + 1), &out.brief());
        e.reject("H101", "withdraw_lamports by a stranger", vec![h.withdraw_lamports(&xp, &col, &r5, &xp, 1)], &x, &[],
            Expect::Custom(0, NOT_OWNER), &[], &[]);
        let v = sol(&vault);
        e.reject("H102", "withdraw_lamports leaving the Vault above 0 but below the rent-exempt minimum",
            vec![h.withdraw_lamports(&dp, &col, &r5, &dp, v - min_rent + 1)], &d, &[], Expect::Custom(0, INSUFFICIENT_VAULT_LAMPORTS), &[], &[]);
        e.reject("H102b", "withdraw_lamports of more than the Vault holds",
            vec![h.withdraw_lamports(&dp, &col, &r5, &dp, v + 1)], &d, &[], Expect::Custom(0, INSUFFICIENT_VAULT_LAMPORTS), &[], &[]);
        e.reject("H102c", "withdraw_lamports to the Vault itself", vec![h.withdraw_lamports(&dp, &col, &r5, &vault, 1)], &d, &[],
            Expect::Custom(0, INVALID_DESTINATION), &[], &[]);
        e.reject("H102d", "withdraw_lamports amount 0", vec![h.withdraw_lamports(&dp, &col, &r5, &dp, 0)], &d, &[],
            Expect::Custom(0, INVALID_AMOUNT), &[], &[]);
        let out = e.exec(&[h.withdraw_lamports(&dp, &col, &r5, &dp, v - min_rent)], &d, &[]);
        e.record("H103", "withdraw_lamports down to exactly the rent-exempt minimum", out.ok() && sol(&vault) == min_rent, &out.brief());
        let out = e.exec(&[h.withdraw_lamports(&dp, &col, &r5, &dp, min_rent)], &d, &[]);
        e.record("H103b", "withdraw_lamports of everything (the Vault account disappears)", out.ok() && sol(&vault) == 0, &out.brief());

        let m4 = e.new_mint(&wp);
        let v4 = ata_of(&vault, &m4.pubkey(), &t22());
        let d4 = ata_of(&dp, &m4.pubkey(), &t22());
        e.must("fund a Token-2022 account of Room #5's Vault", &[
            e.create_ata_ix(&vault, &m4.pubkey()), e.create_ata_ix(&dp, &m4.pubkey()),
            spl_token_2022::instruction::mint_to(&t22(), &m4.pubkey(), &v4, &wp, &[], 5).unwrap(),
        ], w, &[]);
        e.reject("H110", "close_token_account of a non-empty Vault token account", vec![h.close_token_account(&dp, &col, &r5, &v4, &dp, &t22())], &d, &[],
            Expect::Custom(0, TOKEN_ACCOUNT_NOT_EMPTY), &[v4], &[]);
        e.reject("H111", "close_token_account by a stranger", vec![h.close_token_account(&xp, &col, &r5, &v4, &xp, &t22())], &x, &[],
            Expect::Custom(0, NOT_OWNER), &[v4], &[]);
        e.must("D empties it", &[h.withdraw_with(&dp, &h.config(&col), &r5, &v4, &d4, &m4.pubkey(), &t22(), 5)], &d, &[]);
        let (rent, before, n0) = (sol(&v4), sol(&xp), h.nonce(&r5).unwrap_or(u64::MAX));
        let out = e.exec(&[h.close_token_account(&dp, &col, &r5, &v4, &xp, &t22())], &d, &[]);
        e.record("H112", "close_token_account of an empty Vault token account sends its rent to the destination, nonce +1",
            out.ok() && e.data(&v4).is_none() && sol(&xp) - before == rent && h.nonce(&r5) == Some(n0 + 1), &out.brief());

        // Withdraw everything (u64::MAX) and close in one transaction
        let m5 = e.new_mint(&wp);
        let v5 = ata_of(&vault, &m5.pubkey(), &t22());
        let d5 = ata_of(&dp, &m5.pubkey(), &t22());
        e.must("fund another Token-2022 account of Room #5's Vault with 7 tokens", &[
            e.create_ata_ix(&vault, &m5.pubkey()), e.create_ata_ix(&dp, &m5.pubkey()),
            spl_token_2022::instruction::mint_to(&t22(), &m5.pubkey(), &v5, &wp, &[], 7).unwrap(),
        ], w, &[]);
        let n0 = h.nonce(&r5).unwrap_or(u64::MAX);
        let out = e.exec(&[h.withdraw_with(&dp, &h.config(&col), &r5, &v5, &d5, &m5.pubkey(), &t22(), u64::MAX),
            h.close_token_account(&dp, &col, &r5, &v5, &dp, &t22())], &d, &[]);
        e.record("H112c", "withdraw of u64::MAX moves the whole balance, so withdraw-all and close work in one transaction (nonce +2)",
            out.ok() && e.balance(&d5) == Some(7) && e.data(&v5).is_none() && h.nonce(&r5) == Some(n0 + 2), &out.brief());

        // Wrapped XNT: a native token account with a balance cannot be closed (that would unwrap it)
        let wv = ata_of(&vault, &NATIVE_MINT, &SPL_TOKEN_LEGACY);
        e.must("fund a wrapped-XNT account of Room #5's Vault", &[
            create_associated_token_account(&wp, &vault, &NATIVE_MINT, &SPL_TOKEN_LEGACY),
            sys_ix::transfer(&wp, &wv, 50_000_000),
            spl_token_2022::instruction::sync_native(&SPL_TOKEN_LEGACY, &wv).unwrap(),
        ], w, &[]);
        e.reject("H112b", "close_token_account of a wrapped-XNT account with a balance",
            vec![h.close_token_account(&dp, &col, &r5, &wv, &dp, &SPL_TOKEN_LEGACY)], &d, &[], Expect::Custom(0, TOKEN_ACCOUNT_NOT_EMPTY), &[wv], &[]);

        // The Vault's MEMO token account cannot be closed (its rent came from the first mine caller)
        let r5_ta = h.vault_ata(&r5);
        let before = e.snap(&[r5_ta], &[]);
        let reward = curve_reference(before.supply).unwrap();
        e.must("mine into Room #5 (creates its MEMO token account)", &[e.text_memo("r5-mine", 80), cu(400_000), h.mine(&sp, &ap, &col, &r5, &s_ta, 0)],
            &sender, &[&app]);
        e.must("D withdraws the mined MEMO", &[h.withdraw(&dp, &col, &r5, &d_ta, reward)], &d, &[]);
        e.reject("H113", "close_token_account of the Vault's (empty) MEMO token account", vec![h.close_token_account(&dp, &col, &r5, &r5_ta, &dp, &t22())],
            &d, &[], Expect::Custom(0, MEMO_ACCOUNT_NOT_CLOSABLE), &[], &[]);
        e.record("H113b", "  ... it still exists", e.data(&r5_ta).is_some(), "");

        // Listed Assets: both instructions are owner outflows
        e.must("send XNT to Room #1's Vault (Room #1 is listed)", &[sys_ix::transfer(&wp, &h.vault(&room), 10_000_000)], w, &[]);
        e.reject("H114", "withdraw_lamports while listed", vec![h.withdraw_lamports(&dp, &col, &room, &dp, 1_000_000)], &d, &[],
            Expect::Custom(0, LISTED), &[], &[]);
        e.reject("H114b", "close_token_account while listed", vec![h.close_token_account(&dp, &col, &room, &vault_ta, &dp, &t22())], &d, &[],
            Expect::Custom(0, LISTED), &[], &[]);
    }

    // ---------------- The correct key passed without its signature ----------------
    {
        let (ix, r7) = h.create_asset(&wp, &ap, &col, "room7", &dp, "Room #7");
        e.must("create Room #7 for D and fund its Vault", &[ix], w, &[&app]);
        let v7_ta = h.vault_ata(&r7);
        e.must("fund Room #7's Vault with 1 MEMO and 0.01 XNT", &[
            e.create_ata_ix(&h.vault(&r7), &e.mint),
            spl_token_2022::instruction::transfer_checked(&t22(), &e.ata(&wp), &e.mint, &v7_ta, &wp, &[], UNIT, 6).unwrap(),
            sys_ix::transfer(&wp, &h.vault(&r7), 10_000_000),
        ], w, &[]);
        let plain = Keypair::new();
        e.must("create a plain Core Asset owned by Room #7's Vault", &[core_create_plain(&plain.pubkey(), &wp, &h.vault(&r7))], w, &[&plain]);
        let ns = |id: &str, what: &str, ix: Instruction, key: &Pubkey, at: u64, pre: Vec<Instruction>| {
            let mut ixs = pre;
            ixs.push(unsigned(ix, key));
            e.reject(id, &format!("{} with the correct key but no signature", what), ixs, &x, &[], Expect::Custom(at, ANCHOR_ACCOUNT_NOT_SIGNER),
                &[v7_ta], &[]);
        };
        let (ci, _) = h.create_collection(&xp, &ap, "ns", "x", 0, None, None);
        ns("H120", "create_collection (Authority)", ci, &ap, 0, vec![]);
        let (ai, _) = h.create_asset(&xp, &ap, &col, "ns", &xp, "x");
        ns("H121", "create_asset (Authority)", ai, &ap, 0, vec![]);
        ns("H122", "update_asset (Authority)", h.update_asset(&xp, &ap, &col, &r7, Some("x")), &ap, 0, vec![]);
        ns("H123", "update_collection (Authority)", h.update_collection(&xp, &ap, &col, Some("x")), &ap, 0, vec![]);
        ns("H124", "mine (Authority)", h.mine(&xp, &ap, &col, &r7, &e.ata(&xp), 0), &ap, 2, vec![e.text_memo("ns", 80), cu(400_000)]);
        ns("H125", "withdraw (owner)", h.withdraw(&dp, &col, &r7, &e.ata(&xp), 1), &dp, 0, vec![]);
        ns("H126", "withdraw_lamports (owner)", h.withdraw_lamports(&dp, &col, &r7, &xp, 1), &dp, 0, vec![]);
        ns("H127", "close_token_account (owner)", h.close_token_account(&dp, &col, &r7, &v7_ta, &xp, &t22()), &dp, 0, vec![]);
        ns("H128", "withdraw_asset (owner)", h.withdraw_asset(&dp, &xp, &col, &r7, &plain.pubkey(), &CORE, &xp), &dp, 0, vec![]);
        ns("H129", "burn (owner)", h.burn(&dp, &xp, &col, &r7, 1), &dp, 1,
            vec![app_burn_memo(1, b"unsigned owner burn attempt payload, of a reasonable length")]);
        e.record("H129b", "  ... Room #7's nonce is still 0", h.nonce(&r7) == Some(0), &format!("{:?}", h.nonce(&r7)));
    }

    // ---------------- An app program as Authority and owner (test-programs/home-app) ----------------
    {
        let present = e.c.get_account(&APP_PROGRAM).map(|a| a.executable).unwrap_or(false);
        e.record("H130", "the test app program is loaded (target/test-programs/home_test_app.so)", present, "");
        if present {
            let app_pda = Pubkey::find_program_address(&[b"app"], &APP_PROGRAM).0;
            let s = seed("app-rooms");
            let acol = h.collection_addr(&app_pda, &s);
            let mut args = s.to_vec();
            args.extend(3000u16.to_le_bytes());
            let out = e.exec(&[app_ix("create_collection", args, vec![
                AccountMeta::new(wp, true),
                AccountMeta::new_readonly(app_pda, false),
                AccountMeta::new(acol, false),
                AccountMeta::new_readonly(h.program_authority, false),
                AccountMeta::new_readonly(h.burn_guard, false),
                AccountMeta::new(h.config(&acol), false),
                AccountMeta::new_readonly(CORE, false),
                AccountMeta::new_readonly(system_program(), false),
                AccountMeta::new_readonly(prog, false),
            ])], w, &[]);
            let acfg = e.data(&h.config(&acol)).unwrap_or_default();
            e.record("H131", "an app creates its Collection by CPI, its PDA signing as Authority",
                out.ok() && acfg.get(40..72) == Some(&app_pda.to_bytes()[..]), &out.brief());

            let create = |tag: &str, owner: &Pubkey| {
                let s = seed(tag);
                let asset = h.asset_addr(&acol, &s);
                (app_ix("create_asset", s.to_vec(), vec![
                    AccountMeta::new(wp, true),
                    AccountMeta::new_readonly(app_pda, false),
                    AccountMeta::new_readonly(h.config(&acol), false),
                    AccountMeta::new(acol, false),
                    AccountMeta::new(asset, false),
                    AccountMeta::new(h.vault_state(&asset), false),
                    AccountMeta::new_readonly(*owner, false),
                    AccountMeta::new_readonly(h.program_authority, false),
                    AccountMeta::new_readonly(CORE, false),
                    AccountMeta::new_readonly(system_program(), false),
                    AccountMeta::new_readonly(prog, false),
                ]), asset)
            };
            let (ix, ar) = create("app-room1", &bp);
            let out = e.exec(&[ix], w, &[]);
            e.record("H132", "the app creates an Asset by CPI", out.ok() && core_owner(&e, &ar) == Some(bp) && h.nonce(&ar) == Some(0), &out.brief());

            let mut accounts = h.mine_accounts(&sp, &app_pda, &h.config(&acol), &ar, &s_ta);
            accounts[1].is_signer = false;
            accounts.push(AccountMeta::new_readonly(prog, false));
            let mine_ix = app_ix("mine", 1000u16.to_le_bytes().to_vec(), accounts);
            let supply = e.supply();
            let out = mine_check("H133", "a message to the app mines into the Asset's Vault through app -> memo-home -> memo-mint -> Token-2022 (10% share)",
                vec![e.text_memo("app-mine", 80), cu(400_000), mine_ix], &[], &ar, 1000);
            let reward = curve_reference(supply).unwrap();
            let want = format!("Program log: mined reward {} share {}", reward, reward / 10);
            e.record("H133b", "  ... the app read the reward and the share from mine's return data", out.logs.iter().any(|l| *l == want),
                &format!("expected {:?}", want));
            println!("     CU app mine (first, creates the Vault token account): app {} memo-home {}", consumed(&out, &APP_PROGRAM), consumed(&out, &prog));

            let (ix, _) = h.create_asset(&xp, &app_pda, &acol, "steal", &xp, "x");
            e.reject("H134", "nobody else can act as the app's Authority (its PDA cannot sign outside the app)", vec![unsigned(ix, &app_pda)], &x, &[],
                Expect::Custom(0, ANCHOR_ACCOUNT_NOT_SIGNER), &[], &[]);

            // An Asset owned by the app PDA: owner outflows signed by the program
            let (ix, owned) = create("app-owned", &app_pda);
            e.must("the app creates an Asset it owns itself", &[ix], w, &[]);
            let ov = h.vault(&owned);
            e.must("send 0.2 XNT to its Vault", &[sys_ix::transfer(&wp, &ov, 200_000_000)], w, &[]);
            let x0 = sol(&xp);
            let out = e.exec(&[app_ix("withdraw_lamports", 50_000_000u64.to_le_bytes().to_vec(), vec![
                AccountMeta::new_readonly(app_pda, false),
                AccountMeta::new_readonly(h.config(&acol), false),
                AccountMeta::new_readonly(owned, false),
                AccountMeta::new(ov, false),
                AccountMeta::new(h.vault_state(&owned), false),
                AccountMeta::new(xp, false),
                AccountMeta::new_readonly(system_program(), false),
                AccountMeta::new_readonly(prog, false),
            ])], w, &[]);
            e.record("H135", "the app, owning an Asset, withdraws from its Vault by CPI (program-signed owner outflow)",
                out.ok() && sol(&xp) - x0 == 50_000_000 && h.nonce(&owned) == Some(1), &out.brief());
            e.reject("H136", "nobody else can withdraw from the app-owned Asset's Vault", vec![h.withdraw_lamports(&xp, &acol, &owned, &xp, 1)], &x, &[],
                Expect::Custom(0, NOT_OWNER), &[], &[]);
        }
    }

    // Non-memo-home Core Assets get no Vault access
    {
        let plain = Keypair::new();
        e.must("create a plain Core Asset for X and its would-be Vault token account", &[
            core_create_plain(&plain.pubkey(), &xp, &xp),
            e.create_ata_ix(&h.vault(&plain.pubkey()), &e.mint),
        ], w, &[&x, &plain]);
        e.reject("H70", "withdraw for a Core Asset outside memo-home Collections (it has no Vault State)",
            vec![h.withdraw_with(&xp, &h.config(&col), &plain.pubkey(), &h.vault_ata(&plain.pubkey()), &e.ata(&xp), &e.mint, &t22(), 1)],
            &x, &[], Expect::Custom(0, ANCHOR_ACCOUNT_NOT_INITIALIZED), &[], &[]);
    }

    e.finish();
}
