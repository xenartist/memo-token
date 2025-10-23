/// Shared library for memo-token client programs
/// Provides unified configuration and utility functions

pub mod config;

// Re-export commonly used functions
pub use config::{get_rpc_url, get_wallet_path, get_program_env, get_program_id, get_all_program_ids};

