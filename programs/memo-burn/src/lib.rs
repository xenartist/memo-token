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

/// Validate memo amount using comma-separated format: "amount,{user_data}"
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    // Basic length check (minimum: "X," where X is at least 1 digit)
    if memo_data.len() < 2 {
        msg!("Memo too short, minimum format: 'amount,'");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // UTF-8 validation
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check (prevent null characters)
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Find the first comma separator
    match memo_str.find(',') {
        Some(comma_pos) => {
            // Extract the amount part (before comma)
            let amount_str = memo_str[..comma_pos].trim();
            
            // Validate amount string is not empty
            if amount_str.is_empty() {
                msg!("Missing burn amount before comma");
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
            
            // Parse amount as u64
            match amount_str.parse::<u64>() {
                Ok(memo_amount) => {
                    // Verify amounts match
                    if memo_amount == expected_amount {
                        let token_count = expected_amount / 1_000_000;
                        let user_data_len = memo_str.len() - comma_pos - 1;
                        
                        msg!("Burn amount validation passed: {} tokens ({} units), user data: {} bytes", 
                             token_count, expected_amount, user_data_len);
                        Ok(())
                    } else {
                        let memo_tokens = memo_amount / 1_000_000;
                        let expected_tokens = expected_amount / 1_000_000;
                        
                        msg!("Burn amount mismatch: memo contains {} tokens ({} units), but burning {} tokens ({} units)", 
                             memo_tokens, memo_amount, expected_tokens, expected_amount);
                        Err(ErrorCode::BurnAmountMismatch.into())
                    }
                },
                Err(_) => {
                    msg!("Invalid burn amount format: '{}' is not a valid number", amount_str);
                    Err(ErrorCode::InvalidBurnAmountFormat.into())
                }
            }
        },
        None => {
            msg!("Missing comma separator in memo. Expected format: 'amount,user_data'");
            Err(ErrorCode::InvalidMemoFormat.into())
        }
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

/// Check for memo instruction with length validation
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // First check the most likely position (index 1)
    if current_index > 1 {
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == MEMO_PROGRAM_ID {
                    return validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
                }
            },
            Err(_) => {}
        }
    }
    
    // If not found at index 1, check other positions as fallback
    for i in 0..current_index {
        if i == 1 { continue; } // Skip index 1 as we already checked it
        
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(i.into(), instructions) {
            Ok(ix) => {
                if ix.program_id == MEMO_PROGRAM_ID {
                    return validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
                }
            },
            Err(_) => { continue; }
        }
    }
    
    // No valid memo found
    Ok((false, vec![]))
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
