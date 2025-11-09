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

// Memo length constraints
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

// Borsh serialization fixed overhead calculation
const BORSH_U8_SIZE: usize = 1;         // version (u8)
const BORSH_U64_SIZE: usize = 8;        // burn_amount (u64)
const BORSH_VEC_LENGTH_SIZE: usize = 4; // user_data.len() (u32)
const BORSH_FIXED_OVERHEAD: usize = BORSH_U8_SIZE + BORSH_U64_SIZE + BORSH_VEC_LENGTH_SIZE;

// maximum payload length = memo maximum length - borsh fixed overhead
pub const MAX_PAYLOAD_LENGTH: usize = MEMO_MAX_LENGTH - BORSH_FIXED_OVERHEAD; // 800 - 13 = 787

// Maximum allowed Borsh data size after Base64 decoding (security limit)
pub const MAX_BORSH_DATA_SIZE: usize = MEMO_MAX_LENGTH;

// Token decimal factor (decimal=6 means 1 token = 1,000,000 units)
pub const DECIMAL_FACTOR: u64 = 1_000_000;

// Minimum burn requirement (1 token)  
pub const MIN_BURN_TOKENS: u64 = 1;

// Maximum burn per transaction (1 trillion tokens = 1,000,000,000,000 * 1,000,000)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR;

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
    pub burn_count: u64,        // Number of burn transactions
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

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BurnMemo {
    /// version of the BurnMemo structure (for future compatibility)
    pub version: u8,
    
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    
    /// application payload (variable length, max 787 bytes)
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

    /// Process burn operation with Borsh memo validation
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64) -> Result<()> {
        // Check burn amount is at least 1 token and is a multiple of DECIMAL_FACTOR (decimal=6)
        if amount < DECIMAL_FACTOR * MIN_BURN_TOKENS {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount upper limit (prevent excessive burns)
        if amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        // Check burn amount is a multiple of DECIMAL_FACTOR (decimal=6)
        if amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with length validation
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref())?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Validate Borsh memo contains correct amount matching the burn amount
        validate_memo_amount(&memo_data, amount)?;

        let token_count = amount / DECIMAL_FACTOR;

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
        user_burn_stats.last_burn_time = Clock::get()?.unix_timestamp;
        
        msg!("Updated user global burn stats: total_burned={} units ({} tokens), burn_count={}", 
             user_burn_stats.total_burned, 
             user_burn_stats.total_burned / DECIMAL_FACTOR,
             user_burn_stats.burn_count);

        msg!("Successfully burned {} tokens ({} units) with Borsh+Base64 memo validation", 
             token_count, amount);
        
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
    
    // check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());

    // Then deserialize Borsh data from decoded bytes
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // validate version compatibility
    if burn_memo.version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", 
             burn_memo.version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // validate burn amount matches
    if burn_memo.burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", 
             burn_memo.burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    // validate payload length does not exceed maximum allowed value
    if burn_memo.payload.len() > MAX_PAYLOAD_LENGTH {
        msg!("Payload too long: {} bytes (max: {})", 
             burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
        return Err(ErrorCode::PayloadTooLong.into());
    }
    
    msg!("Borsh+Base64 memo validation passed: version {}, {} units, payload: {} bytes (max: {})", 
         burn_memo.version, expected_amount, burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
    
    // record payload preview
    if !burn_memo.payload.is_empty() {
        if let Ok(preview) = std::str::from_utf8(&burn_memo.payload[..burn_memo.payload.len().min(32)]) {
            msg!("Payload preview: {}...", preview);
        } else {
            msg!("Payload: [binary data, {} bytes]", burn_memo.payload.len());
        }
    }
    
    Ok(())
}

/// Validate memo data length and return result
fn validate_memo_length(memo_data: &[u8], min_length: usize, max_length: usize) -> Result<(bool, Vec<u8>)> {
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
    
    // Check maximum length requirement
    if memo_length > max_length {
        msg!("Memo too long: {} bytes (maximum: {})", memo_length, max_length);
        return Err(ErrorCode::MemoTooLong.into());
    }
    
    // Length is valid, return memo data
    msg!("Memo length validation passed: {} bytes (range: {}-{})", memo_length, min_length, max_length);
    Ok((true, memo_data.to_vec()))
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-burn::process_burn (other instructions)
/// 
/// Compute budget instructions can be placed anywhere in the transaction
/// as they are processed by Solana runtime before instruction execution.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction (process_burn) must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("process_burn must be at index 1 or later, but current instruction is at index {}", current_index);
        return Ok((false, vec![]));
    }
    
    // Check that index 0 contains the memo instruction
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at required index 0");
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH)
            } else {
                msg!("Instruction at index 0 is not a memo (program_id: {})", ix.program_id);
                Ok((false, vec![]))
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 0: {:?}", e);
            Ok((false, vec![]))
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
        bump,
        constraint = user_global_burn_stats.user == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub user_global_burn_stats: Account<'info, UserGlobalBurnStats>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
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

    #[msg("Burn amount too large. Maximum allowed: 1,000,000,000,000 tokens per transaction.")]
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

    #[msg("Memo too long (maximum 800 bytes).")]
    MemoTooLong,
    
    #[msg("Payload too long. (maximum 787 bytes).")]
    PayloadTooLong,

    #[msg("Unauthorized user. User mismatch in global burn statistics account.")]
    UnauthorizedUser,
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests;
