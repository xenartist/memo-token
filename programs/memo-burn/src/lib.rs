#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use anchor_lang::solana_program::pubkey;
use spl_memo::ID as MEMO_PROGRAM_ID;

declare_id!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");

// Authorized mint pubkey
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Memo length constraints
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

// Borsh serialization fixed overhead calculation
const BORSH_U64_SIZE: usize = 8;        // burn_amount (u64)
const BORSH_VEC_LENGTH_SIZE: usize = 4; // user_data.len() (u32)
const BORSH_FIXED_OVERHEAD: usize = BORSH_U64_SIZE + BORSH_VEC_LENGTH_SIZE;

// maximum user data length = memo maximum length - borsh fixed overhead
pub const MAX_USER_DATA_LENGTH: usize = MEMO_MAX_LENGTH - BORSH_FIXED_OVERHEAD; // 800 - 12 = 788

// Token decimal factor (decimal=6 means 1 token = 1,000,000 units)
pub const DECIMAL_FACTOR: u64 = 1_000_000;

// Minimum burn requirement (1 token)  
pub const MIN_BURN_TOKENS: u64 = 1;

// Maximum burn per transaction (1 trillion tokens = 1,000,000,000,000 * 1,000,000)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR;

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BurnMemo {
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    
    /// user data (variable length, max 788 bytes)
    pub user_data: Vec<u8>,
}

#[program]
pub mod memo_burn {
    use super::*;

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

        msg!("Successfully burned {} tokens ({} units) with Borsh memo validation", 
             token_count, amount);
        
        Ok(())
    }
}

/// validate Borsh-formatted memo data
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    // deserialize Borsh data
    let burn_memo = BurnMemo::try_from_slice(memo_data)
        .map_err(|_| {
            msg!("Invalid memo format");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // validate burn amount matches
    if burn_memo.burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", 
             burn_memo.burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    // validate user_data length does not exceed maximum allowed value (fully utilize space)
    if burn_memo.user_data.len() > MAX_USER_DATA_LENGTH {
        msg!("User data too long: {} bytes (max: {})", 
             burn_memo.user_data.len(), MAX_USER_DATA_LENGTH);
        return Err(ErrorCode::UserDataTooLong.into());
    }
    
    msg!("Borsh memo validation passed: {} units, user_data: {} bytes (max: {})", 
         expected_amount, burn_memo.user_data.len(), MAX_USER_DATA_LENGTH);
    
    // record user_data preview
    if !burn_memo.user_data.is_empty() {
        if let Ok(preview) = std::str::from_utf8(&burn_memo.user_data[..burn_memo.user_data.len().min(32)]) {
            msg!("User data preview: {}...", preview);
        } else {
            msg!("User data: [binary data, {} bytes]", burn_memo.user_data.len());
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

/// Check for memo instruction at REQUIRED index 1
/// 
/// IMPORTANT: This contract enforces a strict instruction ordering:
/// - Index 0: Compute budget instruction (optional)
/// - Index 1: SPL Memo instruction (REQUIRED)
/// - Index 2+: memo-burn::process_burn (other instructions)
///
/// Any deviation from this pattern will result in transaction failure.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Ensure there are enough instructions (at least index 1 must exist)
    if current_index <= 1 {
        msg!("Memo instruction must be at index 1, but transaction only has {} instructions", current_index);
        return Ok((false, vec![]));
    }
    
    // Check fixed position: index 1
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at required index 1");
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH)
            } else {
                msg!("Instruction at index 1 is not a memo (program_id: {})", ix.program_id);
                Ok((false, vec![]))
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 1: {:?}", e);
            Ok((false, vec![]))
        }
    }
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
    
    #[msg("User data too long. (maximum 788 bytes).")]
    UserDataTooLong,
}
