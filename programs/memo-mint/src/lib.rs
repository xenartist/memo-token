#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use anchor_lang::solana_program::pubkey;
use spl_memo::ID as MEMO_PROGRAM_ID;

declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");

// Authorized mint pubkey
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Memo length constraints
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

#[program]
pub mod memo_mint {
    use super::*;

    /// Process token minting with dynamic amount based on total supply
    /// Mints to the caller's own token account
    pub fn process_mint(ctx: Context<ProcessMint>) -> Result<()> {
        // Use shared mint logic
        execute_mint_operation(
            &ctx.accounts.instructions,
            &ctx.accounts.mint,
            &ctx.accounts.mint_authority,
            &ctx.accounts.token_account,
            &ctx.accounts.token_program,
            ctx.program_id,
            ctx.bumps.mint_authority,
        )
    }

    /// Process token minting with dynamic amount based on total supply
    /// Mints to a specified recipient's token account
    pub fn process_mint_to(ctx: Context<ProcessMintTo>) -> Result<()> {
        // Use shared mint logic
        execute_mint_operation(
            &ctx.accounts.instructions,
            &ctx.accounts.mint,
            &ctx.accounts.mint_authority,
            &ctx.accounts.recipient_token_account,
            &ctx.accounts.token_program,
            ctx.program_id,
            ctx.bumps.mint_authority,
        )
    }
}

/// Shared mint operation logic
fn execute_mint_operation<'info>(
    instructions: &AccountInfo<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    mint_authority: &AccountInfo<'info>,
    token_account: &InterfaceAccount<'info, TokenAccount>,
    token_program: &Program<'info, Token2022>,
    program_id: &Pubkey,
    mint_authority_bump: u8,
) -> Result<()> {
    // Check for memo instruction with length constraints
    let (memo_found, memo_data) = check_memo_instruction(instructions)?;
    if !memo_found {
        msg!("No memo instruction found");
        return Err(ErrorCode::MemoRequired.into());
    }
    
    // Validate PDA matches provided account
    let (expected_mint_authority, expected_bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        program_id
    );
    
    if expected_mint_authority != mint_authority.key() {
        return Err(ErrorCode::InvalidMintAuthority.into());
    }
    
    if expected_bump != mint_authority_bump {
        return Err(ErrorCode::InvalidMintAuthority.into());
    }
    
    // Get current supply and calculate dynamic mint amount
    let current_supply = mint.supply;
    let amount = calculate_dynamic_mint_amount(current_supply)?;
    
    // Execute token mint operation
    token_2022::mint_to(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            token_2022::MintTo {
                mint: mint.to_account_info(),
                to: token_account.to_account_info(),
                authority: mint_authority.to_account_info(),
            },
            &[&[b"mint_authority".as_ref(), &[mint_authority_bump]]]
        ),
        amount
    )?;
    
    // Log successful mint operation
    let token_count = amount as f64 / 1_000_000.0;
    let current_tokens = current_supply / 1_000_000;
    let recipient = token_account.owner;
    msg!("Successfully minted {} tokens ({} units) to {}, current supply: {} tokens, memo length: {} bytes", 
         token_count, amount, recipient, current_tokens, memo_data.len());
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 1
/// 
/// IMPORTANT: This contract enforces a strict instruction ordering:
/// - Index 0: Compute budget instruction (optional)
/// - Index 1: SPL Memo instruction (REQUIRED)
/// - Index 2+: memo-mint::process_mint or memo-mint::process_mint_to (other instructions)
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

/// Calculate dynamic mint amount based on current supply with hard cap
fn calculate_dynamic_mint_amount(current_supply: u64) -> Result<u64> {
    // Hard cap: 10 trillion tokens = 10_000_000_000_000 * 1_000_000 lamports
    const MAX_SUPPLY_LAMPORTS: u64 = 10_000_000_000_000 * 1_000_000;
    
    // Check hard limit
    if current_supply >= MAX_SUPPLY_LAMPORTS {
        return Err(ErrorCode::SupplyLimitReached.into());
    }
    
    // Mint amount based on current supply (in lamports)
    let amount = match current_supply {
        0..=100_000_000_000_000 => 1_000_000,           // 0-100M tokens: 1 token
        100_000_000_000_001..=1_000_000_000_000_000 => 100_000, // 100M-1B tokens: 0.1 token  
        1_000_000_000_000_001..=10_000_000_000_000_000 => 10_000, // 1B-10B tokens: 0.01 token
        10_000_000_000_000_001..=100_000_000_000_000_000 => 1_000, // 10B-100B tokens: 0.001 token
        100_000_000_000_000_001..=1_000_000_000_000_000_000 => 100, // 100B-1T tokens: 0.0001 token
        _ => 1, // 1T+ tokens: 0.000001 token (1 lamport)
    };
    
    // Double check: ensure we don't exceed the hard cap
    if current_supply + amount > MAX_SUPPLY_LAMPORTS {
        return Err(ErrorCode::SupplyLimitReached.into());
    }
    
    Ok(amount)
}

/// Account structure for token minting instruction (original version)
#[derive(Accounts)]
pub struct ProcessMint<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA serving as mint authority
    #[account(
        seeds = [b"mint_authority"],
        bump
    )]
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

/// Account structure for token minting instruction with recipient specification
#[derive(Accounts)]
#[instruction(recipient: Pubkey)]
pub struct ProcessMintTo<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,  // Can be user or contract
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA serving as mint authority
    #[account(
        seeds = [b"mint_authority"],
        bump
    )]
    pub mint_authority: AccountInfo<'info>,
    
    #[account(
        mut,
        constraint = recipient_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = recipient_token_account.owner == recipient @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub recipient_token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Error code definitions
#[error_code]
pub enum ErrorCode {
    #[msg("Transaction must include a memo instruction.")]
    MemoRequired,

    #[msg("Invalid memo format. Memo contains null bytes.")]
    InvalidMemoFormat,
    
    #[msg("Memo too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Memo too long. Must be at most 800 bytes.")]
    MemoTooLong,
    
    #[msg("Invalid token account: Account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint: Only the specified mint address can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account: User must own the token account.")]
    UnauthorizedTokenAccount,

    #[msg("Invalid mint authority: PDA does not match expected mint authority.")]
    InvalidMintAuthority,

    #[msg("Supply limit reached. Maximum supply is 10 trillion tokens.")]
    SupplyLimitReached,
} 