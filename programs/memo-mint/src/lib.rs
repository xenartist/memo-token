#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;

declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");

// Authorized mint address
pub const AUTHORIZED_MINT: &str = "memoX1g5dtnxeN6zVdHMYWCCg3Qgre8WGFNs7YF2Mbc";

#[program]
pub mod memo_mint {
    use super::*;

    /// Process token minting
    /// Mints exactly 1 token per call, requires memo instruction
    pub fn process_mint(ctx: Context<ProcessMint>) -> Result<()> {
        // Check for memo instruction with length constraints (69-800 bytes)
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69, 800)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // Fixed token count - always mint exactly 1 token
        let token_count = 1u64;
        
        // Derive PDA and bump seed
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            ctx.program_id
        );
        
        // Validate PDA matches provided account
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ErrorCode::InvalidMintAuthority.into());
        }
        
        // For decimal=0 tokens, amount equals token count directly
        let amount = token_count;
        
        // Execute token mint operation
        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token_2022::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[&[b"mint_authority".as_ref(), &[bump]]]
            ),
            amount
        )?;
        
        // Log successful mint operation
        msg!("Successfully minted {} tokens with memo length: {} bytes", token_count, memo_data.len());
        
        Ok(())
    }
}

/// Check for memo instruction in transaction with length validation
fn check_memo_instruction(
    instructions: &AccountInfo, 
    min_length: usize, 
    max_length: usize
) -> Result<(bool, Vec<u8>)> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Check most likely position first (index 1)
    if current_index > 1 {
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    return validate_memo_length(&ix.data, min_length, max_length);
                }
            },
            Err(_) => {}
        }
    }
    
    // If not found at index 1, check other positions as fallback
    for i in 0..current_index {
        if i == 1 { continue; } // Skip index 1 since already checked
        
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(i.into(), instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    return validate_memo_length(&ix.data, min_length, max_length);
                }
            },
            Err(_) => { continue; }
        }
    }
    
    // No valid memo instruction found
    Ok((false, vec![]))
}

/// Validate memo data length and return result
fn validate_memo_length(memo_data: &[u8], min_length: usize, max_length: usize) -> Result<(bool, Vec<u8>)> {
    let memo_length = memo_data.len();
    
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

/// Account structure for token minting instruction
#[derive(Accounts)]
pub struct ProcessMint<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = mint.key().to_string() == AUTHORIZED_MINT @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA serving as mint authority
    pub mint_authority: AccountInfo<'info>,
    
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

/// Error code definitions
#[error_code]
pub enum ErrorCode {
    #[msg("Memo too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Memo too long. Must be at most 800 bytes.")]
    MemoTooLong,
    
    #[msg("Transaction must include a memo instruction.")]
    MemoRequired,
    
    #[msg("Invalid token account: Account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint: Only the specified mint address can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account: User must own the token account.")]
    UnauthorizedTokenAccount,

    #[msg("Invalid mint authority: PDA does not match expected mint authority.")]
    InvalidMintAuthority,
} 