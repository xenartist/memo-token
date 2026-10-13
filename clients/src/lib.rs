/// Shared library for memo-token client programs
/// Provides unified configuration and utility functions

pub mod config;
pub mod mint_curve;

// Re-export commonly used functions
pub use config::{
    get_rpc_url, 
    get_wallet_path, 
    get_program_env, 
    get_program_id, 
    get_all_program_ids,
    get_token_mint,
    get_all_token_mints,
};

pub use mint_curve::{expected_mint_amount, validate_mint_amount};

/// PDA of memo-burn's global burn statistics account (seeds = [b"global_burn_stats"])
/// for the network configured in Anchor.toml. Every process_burn, and every app
/// instruction that burns via memo-burn, takes it as its last account.
pub fn global_burn_stats_pda() -> solana_sdk::pubkey::Pubkey {
    let program_id = get_program_id("memo_burn").expect("memo_burn program id must be configured in Anchor.toml");
    solana_sdk::pubkey::Pubkey::find_program_address(&[b"global_burn_stats"], &program_id).0
}
