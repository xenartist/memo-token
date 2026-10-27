//! memo-id addresses for the network configured in Anchor.toml, shared by the admin clients and
//! the regression suite so that the deployed lookup table is the one the tests use.

use crate::{get_program_id, get_token_mint, global_burn_stats_pda};
use sha2::{Digest, Sha256};
use solana_sdk::{pubkey, pubkey::Pubkey, sysvar};

pub const CORE_PROGRAM_ID: Pubkey = pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const TOKEN_2022_PROGRAM_ID: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const SYSTEM_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");

/// The v1 suffix; the MEMO ID Collection's seed is sha256 of it
pub const SUFFIX: &str = "memo";

#[derive(Debug, Clone, Copy)]
pub struct MemoIdAddresses {
    pub program: Pubkey,
    pub home: Pubkey,
    /// memo-id Authority PDA (Authority of the MEMO ID Collection)
    pub authority: Pubkey,
    /// memo-home's program Authority PDA
    pub home_authority: Pubkey,
    pub burn_guard: Pubkey,
    pub collection: Pubkey,
    pub config: Pubkey,
}

impl MemoIdAddresses {
    pub fn new(program: Pubkey, home: Pubkey) -> Self {
        let authority = Pubkey::find_program_address(&[b"authority"], &program).0;
        let seed: [u8; 32] = Sha256::digest(SUFFIX.as_bytes()).into();
        let collection = Pubkey::find_program_address(&[b"collection", authority.as_ref(), &seed], &home).0;
        MemoIdAddresses {
            program,
            home,
            authority,
            home_authority: Pubkey::find_program_address(&[b"authority"], &home).0,
            burn_guard: Pubkey::find_program_address(&[b"burn_guard"], &home).0,
            collection,
            config: Pubkey::find_program_address(&[b"collection_config", collection.as_ref()], &home).0,
        }
    }

    /// From the program IDs in Anchor.toml
    pub fn from_config() -> Result<Self, String> {
        Ok(Self::new(get_program_id("memo_id")?, get_program_id("memo_home")?))
    }

    /// Content of the frozen Address Lookup Table that register / issue_subid transactions use:
    /// every account they need whoever sends them. Programs invoked at the top level (memo-id,
    /// SPL Memo, Compute Budget) must stay in the message itself, so they are not listed.
    pub fn lookup_table_addresses(&self, memo_mint: Pubkey, memo_burn: Pubkey, global_burn_stats: Pubkey) -> Vec<Pubkey> {
        vec![
            self.authority,
            self.config,
            self.collection,
            self.home_authority,
            CORE_PROGRAM_ID,
            self.home,
            memo_mint,
            global_burn_stats,
            TOKEN_2022_PROGRAM_ID,
            memo_burn,
            sysvar::instructions::ID,
            SYSTEM_PROGRAM_ID,
        ]
    }

    /// `lookup_table_addresses` with the MEMO mint, memo-burn and its GlobalBurnStats from Anchor.toml
    pub fn lookup_table_addresses_from_config(&self) -> Result<Vec<Pubkey>, String> {
        let mint = get_token_mint("memo_token").map_err(|e| e.to_string())?;
        Ok(self.lookup_table_addresses(mint, get_program_id("memo_burn")?, global_burn_stats_pda()))
    }
}
