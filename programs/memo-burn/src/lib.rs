#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use serde_json::Value;

declare_id!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");

// Authorized mint address
pub const AUTHORIZED_MINT: &str = "HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1";

#[program]
pub mod memo_burn {
    use super::*;

    /// Process burn operation with enhanced validation
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64) -> Result<()> {
        // Check burn amount is at least 1 token and is a multiple of 1_000_000 (decimal=6)
        if amount < 1_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount is a multiple of 1_000_000 (decimal=6)
        if amount % 1_000_000 != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with length validation (69-800 bytes)
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

/// Extract and validate memo contains correct amount (in lamports/units)
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    // Enhanced UTF-8 validation (simple but effective)
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check (prevent obvious malicious input)
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Clean the string (handle JSON escaping)
    let clean_str = memo_str
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    
    // Parse as JSON
    let json_data: Value = serde_json::from_str(&clean_str)
        .map_err(|_| ErrorCode::InvalidMemoFormat)?;

    // Extract burn_amount from memo (expecting exact units/lamports)
    let memo_burn_amount = match &json_data["burn_amount"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val // Burn amount in units (lamports)
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        Value::String(s) => {
            // Parse string as units (lamports)
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingBurnAmountField.into()),
    };

    // Check if memo burn amount matches expected burn amount (both in units)
    if memo_burn_amount != expected_amount {
        let memo_tokens = memo_burn_amount / 1_000_000;
        let expected_tokens = expected_amount / 1_000_000;
        msg!("Burn amount mismatch: memo contains {} tokens ({} units), but burning {} tokens ({} units)", 
             memo_tokens, memo_burn_amount, expected_tokens, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }

    let token_count = expected_amount / 1_000_000;
    msg!("Burn amount validation passed: {} tokens ({} units)", token_count, expected_amount);
    Ok(())
}

/// Check for memo instruction with length validation (69-800 bytes)
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // First check the most likely position (index 1)
    if current_index > 1 {
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    // Validate memo length (69-800 bytes)
                    let memo_length = ix.data.len();
                    if memo_length < 69 {
                        msg!("Memo too short: {} bytes (minimum: 69)", memo_length);
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                    if memo_length > 800 {
                        msg!("Memo too long: {} bytes (maximum: 800)", memo_length);
                        return Err(ErrorCode::MemoTooLong.into());
                    }
                    
                    msg!("Memo length validation passed: {} bytes (range: 69-800)", memo_length);
                    return Ok((true, ix.data.to_vec()));
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
                if ix.program_id == memo_program_id {
                    // Validate memo length (69-800 bytes)
                    let memo_length = ix.data.len();
                    if memo_length < 69 {
                        msg!("Memo too short: {} bytes (minimum: 69)", memo_length);
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                    if memo_length > 800 {
                        msg!("Memo too long: {} bytes (maximum: 800)", memo_length);
                        return Err(ErrorCode::MemoTooLong.into());
                    }
                    
                    msg!("Memo length validation passed: {} bytes (range: 69-800)", memo_length);
                    return Ok((true, ix.data.to_vec()));
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
        constraint = mint.key().to_string() == AUTHORIZED_MINT @ ErrorCode::UnauthorizedMint
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

    #[msg("Invalid memo format. Expected JSON format.")]
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

    #[msg("Missing burn_amount field in memo JSON.")]
    MissingBurnAmountField,

    #[msg("Invalid burn_amount format in memo. Must be a positive integer in units.")]
    InvalidBurnAmountFormat,

    #[msg("Burn amount mismatch. The burn_amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Memo too short (minimum 69 bytes).")]
    MemoTooShort,

    #[msg("Memo too long (maximum 800 bytes).")]
    MemoTooLong,
}
