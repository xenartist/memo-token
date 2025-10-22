#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use spl_memo::ID as MEMO_PROGRAM_ID;

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("3rncCFCJ6sGULiUKXXziLL4AExejR1UmNSvvTj8czLgB");

#[cfg(not(feature = "mainnet"))]
declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");

// Authorized mint pubkey - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// compile-time constant safety validation
const _: () = {
    // ensure max supply calculation won't overflow
    assert!(MAX_SUPPLY_TOKENS <= u64::MAX / DECIMAL_FACTOR, "MAX_SUPPLY_TOKENS too large");
    
    // ensure tier thresholds are in the correct order
    assert!(TIER_1_THRESHOLD_LAMPORTS < TIER_2_THRESHOLD_LAMPORTS, "Tier thresholds out of order");
    assert!(TIER_2_THRESHOLD_LAMPORTS < TIER_3_THRESHOLD_LAMPORTS, "Tier thresholds out of order");
    assert!(TIER_3_THRESHOLD_LAMPORTS < TIER_4_THRESHOLD_LAMPORTS, "Tier thresholds out of order");
    assert!(TIER_4_THRESHOLD_LAMPORTS < TIER_5_THRESHOLD_LAMPORTS, "Tier thresholds out of order");
    assert!(TIER_5_THRESHOLD_LAMPORTS <= MAX_SUPPLY_LAMPORTS, "Final tier exceeds max supply");
    
    // ensure mint amounts are reasonable
    assert!(TIER_1_MINT_AMOUNT > 0, "Mint amounts must be positive");
    assert!(TIER_6_MINT_AMOUNT > 0, "Minimum mint amount must be positive");
};

// Memo length constraints
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

// Token decimal factor (decimal=6 means 1 token = 1,000,000 units)
pub const DECIMAL_FACTOR: u64 = 1_000_000;

// Maximum supply cap (10 trillion tokens)
pub const MAX_SUPPLY_TOKENS: u64 = 10_000_000_000_000;
pub const MAX_SUPPLY_LAMPORTS: u64 = MAX_SUPPLY_TOKENS * DECIMAL_FACTOR;

// Supply tier thresholds (in lamports for direct comparison)
pub const TIER_1_THRESHOLD_LAMPORTS: u64 = 100_000_000 * DECIMAL_FACTOR;        // 100M tokens
pub const TIER_2_THRESHOLD_LAMPORTS: u64 = 1_000_000_000 * DECIMAL_FACTOR;      // 1B tokens  
pub const TIER_3_THRESHOLD_LAMPORTS: u64 = 10_000_000_000 * DECIMAL_FACTOR;     // 10B tokens
pub const TIER_4_THRESHOLD_LAMPORTS: u64 = 100_000_000_000 * DECIMAL_FACTOR;    // 100B tokens
pub const TIER_5_THRESHOLD_LAMPORTS: u64 = 1_000_000_000_000 * DECIMAL_FACTOR;  // 1T tokens

// Mint amounts per tier (in lamports)
pub const TIER_1_MINT_AMOUNT: u64 = 1 * DECIMAL_FACTOR;      // 1 token
pub const TIER_2_MINT_AMOUNT: u64 = DECIMAL_FACTOR / 10;     // 0.1 token
pub const TIER_3_MINT_AMOUNT: u64 = DECIMAL_FACTOR / 100;    // 0.01 token
pub const TIER_4_MINT_AMOUNT: u64 = DECIMAL_FACTOR / 1_000;  // 0.001 token
pub const TIER_5_MINT_AMOUNT: u64 = DECIMAL_FACTOR / 10_000; // 0.0001 token
pub const TIER_6_MINT_AMOUNT: u64 = 1;                       // 0.000001 token (1 lamport)

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
    let token_count = calculate_token_count_safe(amount)?;
    let current_tokens = calculate_token_count_safe(current_supply)?;
    let recipient = token_account.owner;
    msg!("Successfully minted {} tokens ({} units) to {}, current supply: {} tokens, memo length: {} bytes", 
         token_count, amount, recipient, current_tokens, memo_data.len());
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 1
/// 
/// IMPORTANT: This contract enforces a strict instruction ordering:
/// - Index 0: Compute budget instruction (REQUIRED)
/// - Index 1: SPL Memo instruction (REQUIRED)
/// - Index 2+: memo-mint::process_mint or memo-mint::process_mint_to (other instructions)
///
/// This function searches both positions to accommodate different transaction structures.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    if current_index <= 1 {
        msg!("Memo instruction must be at index 1, but current instruction is at index {}", current_index);
        return Ok((false, vec![]));
    }
    
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
    
    // Length is valid, return memo data
    msg!("Memo length validation passed: {} bytes (range: {}-{})", memo_length, min_length, max_length);
    Ok((true, memo_data.to_vec()))
}

/// Calculate dynamic mint amount based on current supply with hard cap
fn calculate_dynamic_mint_amount(current_supply: u64) -> Result<u64> {
    // Check hard limit first
    if current_supply >= MAX_SUPPLY_LAMPORTS {
        return Err(ErrorCode::SupplyLimitReached.into());
    }
    
    // Mint amount based on current supply tiers (comparing lamports directly)
    let amount = match current_supply {
        0..=TIER_1_THRESHOLD_LAMPORTS => TIER_1_MINT_AMOUNT,           // 0-100M tokens: 1 token
        _ if current_supply <= TIER_2_THRESHOLD_LAMPORTS => TIER_2_MINT_AMOUNT, // 100M-1B tokens: 0.1 token  
        _ if current_supply <= TIER_3_THRESHOLD_LAMPORTS => TIER_3_MINT_AMOUNT, // 1B-10B tokens: 0.01 token
        _ if current_supply <= TIER_4_THRESHOLD_LAMPORTS => TIER_4_MINT_AMOUNT, // 10B-100B tokens: 0.001 token
        _ if current_supply <= TIER_5_THRESHOLD_LAMPORTS => TIER_5_MINT_AMOUNT, // 100B-1T tokens: 0.0001 token
        _ => TIER_6_MINT_AMOUNT, // 1T+ tokens: 0.000001 token (1 lamport)
    };
    
    // 🔥 use checked_add to prevent overflow
    let new_supply = current_supply.checked_add(amount)
        .ok_or(ErrorCode::ArithmeticOverflow)?;
    
    // check if it exceeds the hard limit
    if new_supply > MAX_SUPPLY_LAMPORTS {
        return Err(ErrorCode::SupplyLimitReached.into());
    }
    
    Ok(amount)
}

/// safe token count calculation helper function
fn calculate_token_count_safe(lamports: u64) -> Result<f64> {
    // prevent division by zero (compile-time constant, but good practice)
    if DECIMAL_FACTOR == 0 {
        return Err(ErrorCode::ArithmeticOverflow.into());
    }
    
    // ensure conversion is safe (although for f64 u64 range is safe)
    let result = lamports as f64 / DECIMAL_FACTOR as f64;
    
    // check if result is valid (prevent NaN or infinity in extreme cases)
    if !result.is_finite() {
        return Err(ErrorCode::ArithmeticOverflow.into());
    }
    
    Ok(result)
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

    #[msg("Arithmetic overflow detected.")]
    ArithmeticOverflow,
} 

