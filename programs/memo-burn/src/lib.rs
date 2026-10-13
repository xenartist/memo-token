#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use spl_memo::ID as MEMO_PROGRAM_ID;
use base64::{Engine as _, engine::general_purpose};

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh");

#[cfg(not(feature = "mainnet"))]
declare_id!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");

// Authorized mint pubkey - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Authorized admin pubkey (only used to initialize the global burn statistics account)
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");

// Memo length constraint: minimum only; the maximum is set by the transaction size limit
pub const MEMO_MIN_LENGTH: usize = 69;

// Borsh serialization fixed overhead calculation
const BORSH_U8_SIZE: usize = 1;         // version (u8)
const BORSH_U64_SIZE: usize = 8;        // burn_amount (u64)
const BORSH_VEC_LENGTH_SIZE: usize = 4; // payload.len() (u32)
const BORSH_FIXED_OVERHEAD: usize = BORSH_U8_SIZE + BORSH_U64_SIZE + BORSH_VEC_LENGTH_SIZE;

// Offset of the payload length prefix inside the Borsh data (after version and burn_amount)
const BORSH_PAYLOAD_LEN_OFFSET: usize = BORSH_U8_SIZE + BORSH_U64_SIZE;

// No maximum memo or payload length: the transaction size limit bounds both
// (a memo decodes to 3/4 of its length, and the payload is that minus the 13-byte header)

// Token decimal factor (decimal=6 means 1 token = 1,000,000 units)
pub const DECIMAL_FACTOR: u64 = 1_000_000;

// Minimum burn requirement (1 token)  
pub const MIN_BURN_TOKENS: u64 = 1;

// Maximum burn per process_burn instruction (1 trillion tokens = 1,000,000,000,000 * 1,000,000)
// A transaction may contain several burn instructions, so this is not a per-transaction total
pub const MAX_BURN_PER_INSTRUCTION: u64 = 1_000_000_000_000 * DECIMAL_FACTOR;

// Current version of BurnMemo structure
pub const BURN_MEMO_VERSION: u8 = 1;

// Maximum user global burn amount (prevent overflow, set to reasonable limit)
// Note: This is set to 18 trillion tokens (1.8x of max supply) because:
// 1. It tracks CUMULATIVE burns across the token's lifetime
// 2. In a dynamic economy with mint+burn cycles, cumulative burns can exceed max supply
// 3. Example: User burns 5T, ecosystem remints 5T, user burns 5T again = 10T cumulative
// 4. This higher limit ensures active users' contributions are fully tracked
pub const MAX_USER_GLOBAL_BURN_AMOUNT: u64 = 18_000_000_000_000 * DECIMAL_FACTOR; // Reserve space for safety

/// User global burn statistics tracking account
#[account]
pub struct UserGlobalBurnStats {
    pub user: Pubkey,           // User's public key
    pub total_burned: u64,      // Total amount burned by this user (in units)
    pub burn_count: u64,        // Number of burn instructions
    pub last_burn_time: i64,    // Timestamp of last burn
    pub bump: u8,               // PDA bump
}

impl UserGlobalBurnStats {
    pub const SPACE: usize = 8 + // discriminator
        32 + // user (Pubkey)
        8 +  // total_burned (u64)
        8 +  // burn_count (u64)
        8 +  // last_burn_time (i64)
        1;   // bump (u8)
}

/// Global burn statistics across all users (one account, seeds = [b"global_burn_stats"])
/// Counts every burn made through memo-burn, including burns from programs calling it via CPI.
/// Both counters saturate at their type's maximum instead of overflowing.
#[account]
pub struct GlobalBurnStats {
    pub total_burned: u128,     // Total amount burned by all users (in units)
    pub burn_count: u64,        // Number of burn instructions
    pub last_burn_time: i64,    // Timestamp of last burn
    pub bump: u8,               // PDA bump
    pub reserved: [u8; 64],     // Reserved for future fields (no realloc needed)
}

impl GlobalBurnStats {
    pub const SPACE: usize = 8 + // discriminator
        16 + // total_burned (u128)
        8 +  // burn_count (u64)
        8 +  // last_burn_time (i64)
        1 +  // bump (u8)
        64;  // reserved

    /// Record one burn; counters stop at their maximum instead of overflowing
    pub fn record_burn(&mut self, amount: u64, timestamp: i64) {
        self.total_burned = self.total_burned.saturating_add(amount as u128);
        self.burn_count = self.burn_count.saturating_add(1);
        self.last_burn_time = timestamp;
    }
}

/// Memo layout expected by process_burn (Base64-encoded Borsh of this struct).
/// The program reads only the 13-byte header (version, burn_amount, payload length);
/// the struct is public for clients and calling programs that build the memo.
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BurnMemo {
    /// version of the BurnMemo structure (for future compatibility)
    pub version: u8,
    
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    
    /// application payload (variable length, bounded only by the transaction size)
    pub payload: Vec<u8>,
}

#[program]
pub mod memo_burn {
    use super::*;

    /// Initialize user global burn statistics tracking
    pub fn initialize_user_global_burn_stats(ctx: Context<InitializeUserGlobalBurnStats>) -> Result<()> {
        let user_burn_stats = &mut ctx.accounts.user_global_burn_stats;
        user_burn_stats.user = ctx.accounts.user.key();
        user_burn_stats.total_burned = 0;
        user_burn_stats.burn_count = 0;
        user_burn_stats.last_burn_time = 0;
        user_burn_stats.bump = ctx.bumps.user_global_burn_stats;
        
        msg!("Initialized global burn statistics tracking for user: {}", ctx.accounts.user.key());
        Ok(())
    }

    /// Initialize the global burn statistics account (admin only, once, right after deployment)
    pub fn initialize_global_burn_stats(ctx: Context<InitializeGlobalBurnStats>) -> Result<()> {
        let global_burn_stats = &mut ctx.accounts.global_burn_stats;
        global_burn_stats.total_burned = 0;
        global_burn_stats.burn_count = 0;
        global_burn_stats.last_burn_time = 0;
        global_burn_stats.bump = ctx.bumps.global_burn_stats;
        global_burn_stats.reserved = [0u8; 64];
        
        msg!("Initialized global burn statistics");
        Ok(())
    }

    /// Process burn operation with Borsh memo validation
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64) -> Result<()> {
        // Check burn amount is at least 1 token and is a multiple of DECIMAL_FACTOR (decimal=6)
        if amount < DECIMAL_FACTOR * MIN_BURN_TOKENS {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount upper limit (prevent excessive burns)
        if amount > MAX_BURN_PER_INSTRUCTION {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        // Check burn amount is a multiple of DECIMAL_FACTOR (decimal=6)
        if amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with length validation
        let memo_data = match check_memo_instruction(ctx.accounts.instructions.as_ref())? {
            Some(data) => data,
            None => return Err(ErrorCode::MemoRequired.into()),
        };

        // Validate Borsh memo contains correct amount matching the burn amount
        validate_memo_amount(&memo_data, amount)?;

        token_2022::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token_2022::Burn {
                    mint: ctx.accounts.mint.to_account_info(),
                    from: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        // Update user global burn statistics tracking (now required)
        let user_burn_stats = &mut ctx.accounts.user_global_burn_stats;
        
        // Check for overflow before adding
        let new_total = user_burn_stats.total_burned.saturating_add(amount);
        
        // Apply maximum limit
        if new_total > MAX_USER_GLOBAL_BURN_AMOUNT {
            user_burn_stats.total_burned = MAX_USER_GLOBAL_BURN_AMOUNT;
            msg!("User global burn amount reached maximum limit: {}", MAX_USER_GLOBAL_BURN_AMOUNT);
        } else {
            user_burn_stats.total_burned = new_total;
        }
        
        // Update burn count with overflow protection
        user_burn_stats.burn_count = user_burn_stats.burn_count.saturating_add(1);
        
        // Update last burn time
        let timestamp = Clock::get()?.unix_timestamp;
        user_burn_stats.last_burn_time = timestamp;
        
        // Update global burn statistics
        ctx.accounts.global_burn_stats.record_burn(amount, timestamp);
        
        // Log successful burn (integer units, 1 token = 1,000,000 units)
        msg!("Burned {} units from {}, user total burned: {} units, burn count: {}",
             amount, ctx.accounts.user.key(), user_burn_stats.total_burned, user_burn_stats.burn_count);
        
        Ok(())
    }
}

/// validate Borsh-formatted memo data (with Base64 decoding)
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // check decoded borsh data holds at least the fixed header
    if decoded_data.len() < BORSH_FIXED_OVERHEAD {
        msg!("Decoded data too short: {} bytes (minimum: {})", decoded_data.len(), BORSH_FIXED_OVERHEAD);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Check the payload length prefix against the bytes actually present
    // (a forged prefix must be rejected cleanly; Borsh would allocate the declared length)
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&decoded_data[BORSH_PAYLOAD_LEN_OFFSET..BORSH_FIXED_OVERHEAD]);
    let declared_payload_len = u32::from_le_bytes(len_bytes) as usize;
    if declared_payload_len != decoded_data.len() - BORSH_FIXED_OVERHEAD {
        msg!("Payload length prefix {} does not match {} payload bytes", declared_payload_len, decoded_data.len() - BORSH_FIXED_OVERHEAD);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }

    // Read the fixed Borsh header directly: version (u8) and burn_amount (u64, little-endian).
    // The payload is not needed on-chain, so it is not copied; with the length prefix checked
    // above, this accepts exactly the memos that BurnMemo::try_from_slice would accept.
    let version = decoded_data[0];
    let mut amount_bytes = [0u8; 8];
    amount_bytes.copy_from_slice(&decoded_data[BORSH_U8_SIZE..BORSH_PAYLOAD_LEN_OFFSET]);
    let burn_amount = u64::from_le_bytes(amount_bytes);
    
    // validate version compatibility
    if version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // validate burn amount matches
    if burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    Ok(())
}

/// Validate memo data length; Ok(true) when the memo is at least `min_length` bytes
/// There is no maximum: the transaction size limit bounds the memo length
fn validate_memo_length(memo_data: &[u8], min_length: usize) -> Result<bool> {
    let memo_length = memo_data.len();
    
    // Ensure data is not empty
    if memo_data.is_empty() {
        msg!("Memo data is empty");
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    // Check minimum length requirement
    if memo_length < min_length {
        msg!("Memo too short: {} bytes (minimum: {})", memo_length, min_length);
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    Ok(true)
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-burn::process_burn (other instructions)
/// 
/// Compute budget instructions also occupy an instruction index, so they must be
/// placed at index 1 or later; a compute budget instruction at index 0 makes the burn fail.
///
/// Returns the memo data when a valid memo is at index 0, otherwise None.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<Option<Vec<u8>>> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction (process_burn) must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("process_burn must be at index 1 or later, but current instruction is at index {}", current_index);
        return Ok(None);
    }
    
    // Check that index 0 contains the memo instruction
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH)?;
                Ok(Some(ix.data))
            } else {
                msg!("Instruction at index 0 is not a memo (program_id: {})", ix.program_id);
                Ok(None)
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 0: {:?}", e);
            Ok(None)
        }
    }
}

/// Account structure for initializing user global burn statistics
#[derive(Accounts)]
pub struct InitializeUserGlobalBurnStats<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        init,
        payer = user,
        space = UserGlobalBurnStats::SPACE,
        seeds = [b"user_global_burn_stats", user.key().as_ref()],
        bump
    )]
    pub user_global_burn_stats: Account<'info, UserGlobalBurnStats>,
    
    pub system_program: Program<'info, System>,
}

/// Account structure for initializing global burn statistics (admin only)
#[derive(Accounts)]
pub struct InitializeGlobalBurnStats<'info> {
    #[account(
        mut,
        constraint = admin.key() == AUTHORIZED_ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin
    )]
    pub admin: Signer<'info>,
    
    #[account(
        init,
        payer = admin,
        space = GlobalBurnStats::SPACE,
        seeds = [b"global_burn_stats"],
        bump
    )]
    pub global_burn_stats: Account<'info, GlobalBurnStats>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ProcessBurn<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = token_account.owner == user.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// User global burn statistics tracking account (now required)
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", user.key().as_ref()],
        bump = user_global_burn_stats.bump,
        constraint = user_global_burn_stats.user == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub user_global_burn_stats: Account<'info, UserGlobalBurnStats>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
    
    /// Global burn statistics account (last, so existing account order is unchanged)
    #[account(
        mut,
        seeds = [b"global_burn_stats"],
        bump = global_burn_stats.bump
    )]
    pub global_burn_stats: Account<'info, GlobalBurnStats>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Transaction must include a memo.")]
    MemoRequired,

    #[msg("Invalid memo format. Expected Borsh-serialized structure.")]
    InvalidMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,
    
    #[msg("Burn amount too small. Must burn at least 1 token (1,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Burn amount too large. Maximum allowed: 1,000,000,000,000 tokens per burn instruction.")]
    BurnAmountTooLarge,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Invalid token account. Token account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint. Only the specified mint can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account. User must own the token account.")]
    UnauthorizedTokenAccount,

    #[msg("Burn amount mismatch. The burn_amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Memo too short (minimum 69 bytes).")]
    MemoTooShort,

    #[msg("Unauthorized user. User mismatch in global burn statistics account.")]
    UnauthorizedUser,

    #[msg("Unauthorized admin. Only the authorized admin can initialize global burn statistics.")]
    UnauthorizedAdmin,
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests;
