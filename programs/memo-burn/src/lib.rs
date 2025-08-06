#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use spl_memo::ID as MEMO_PROGRAM_ID;

declare_id!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");

// Authorized mint pubkey
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = anchor_lang::solana_program::pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Memo length constraints
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

#[program]
pub mod memo_burn {
    use super::*;

    /// Process burn operation with comma-separated memo validation
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64) -> Result<()> {
        // Check burn amount is at least 1 token and is a multiple of 1_000_000 (decimal=6)
        if amount < 1_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount is a multiple of 1_000_000 (decimal=6)
        if amount % 1_000_000 != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with length validation
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref())?;
        if !memo_found {
            msg!("No memo instruction found");
            return Err(ErrorCode::MemoRequired.into());
        }

        // Validate memo contains correct amount matching the burn amount
        validate_memo_amount(&memo_data, amount)?;

        // Burn tokens
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

        let token_count = amount / 1_000_000;
        msg!("Successfully burned {} tokens ({} units) with memo validation", token_count, amount);
        
        Ok(())
    }
}

/// optimized version: minimize memory allocation and validation
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    if memo_data.len() < 2 {
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // fast scan: find comma and validate number part
    let mut comma_pos = None;
    for (i, &byte) in memo_data.iter().enumerate() {
        if byte == 0 {
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
        if byte == b',' {
            comma_pos = Some(i);
            break;
        }
        // only allow ASCII digits in the number part
        if !byte.is_ascii_digit() {
            return Err(ErrorCode::InvalidBurnAmountFormat.into());
        }
    }
    
    let comma_pos = comma_pos.ok_or(ErrorCode::InvalidMemoFormat)?;
    if comma_pos == 0 {
        return Err(ErrorCode::InvalidBurnAmountFormat.into());
    }
    
    // manually parse the number, avoid UTF-8 conversion
    let mut amount: u64 = 0;
    for &byte in &memo_data[..comma_pos] {
        if byte.is_ascii_digit() {
            amount = amount.checked_mul(10)
                .and_then(|a| a.checked_add((byte - b'0') as u64))
                .ok_or(ErrorCode::InvalidBurnAmountFormat)?;
        } else {
            return Err(ErrorCode::InvalidBurnAmountFormat.into());
        }
    }
    
    if amount == expected_amount {
        msg!("Burn amount validation passed: {} units", expected_amount);
        Ok(())
    } else {
        msg!("Burn amount mismatch: memo {} vs expected {}", amount, expected_amount);
        Err(ErrorCode::BurnAmountMismatch.into())
    }
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
    
    // Check for null bytes (security)
    if memo_data.iter().any(|&b| b == 0) {
        msg!("Memo contains null bytes");
        return Err(ErrorCode::InvalidMemoFormat.into());
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

    #[msg("Invalid memo format. Expected format: 'burn_amount,user_data'")]
    InvalidMemoFormat,
    
    #[msg("Burn amount too small. Must burn at least 1 token (1,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Invalid token account. Token account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint. Only the specified mint can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account. User must own the token account.")]
    UnauthorizedTokenAccount,

    #[msg("Invalid burn amount format. Must be a positive integer in units.")]
    InvalidBurnAmountFormat,

    #[msg("Burn amount mismatch. The amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Memo too short (minimum 69 bytes).")]
    MemoTooShort,

    #[msg("Memo too long (maximum 800 bytes).")]
    MemoTooLong,
}
