#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use solana_sha256_hasher::hash;
use spl_memo::ID as MEMO_PROGRAM_ID;

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX");

#[cfg(not(feature = "mainnet"))]
declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");

// Authorized mint pubkey - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Authorized admin pubkey (initializes MintState once) - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");

// compile-time constant safety validation
const _: () = {
    // ensure max supply calculation won't overflow
    assert!(MAX_SUPPLY_TOKENS <= u64::MAX / DECIMAL_FACTOR, "MAX_SUPPLY_TOKENS too large");
    
    // ensure the emission curve ends exactly at the max supply
    assert!(CURVE_START_LAMPORTS * 10u64.pow(CURVE_DECADES) == MAX_SUPPLY_LAMPORTS, "Curve must end at max supply");

    // ensure every step in every decade is a whole number of lamports and the smallest reward is >= 1 lamport
    assert!(BASE_MINT_AMOUNT % 10u64.pow(CURVE_DECADES) == 0, "Base mint amount must be divisible across all decades");
    assert!(BASE_MINT_AMOUNT / 10u64.pow(CURVE_DECADES) >= 1, "Minimum mint amount must be positive");
};

// Memo length constraint: minimum only; the maximum is set by the transaction size limit
pub const MEMO_MIN_LENGTH: usize = 69;

// Token decimal factor (decimal=6 means 1 token = 1,000,000 units)
pub const DECIMAL_FACTOR: u64 = 1_000_000;

// Maximum supply cap (10 trillion tokens)
pub const MAX_SUPPLY_TOKENS: u64 = 10_000_000_000_000;
pub const MAX_SUPPLY_LAMPORTS: u64 = MAX_SUPPLY_TOKENS * DECIMAL_FACTOR;

// Emission curve (in lamports for direct comparison)
//
// Below CURVE_START every mint pays BASE_MINT_AMOUNT (1 token).
// From CURVE_START on, supply is split into decades [start * 10^n, start * 10^(n+1)).
// Inside a decade, supply in [d * 10^k, (d+1) * 10^k) (d = leading digit 1..9) pays
//     decade_base * (11 - d) / 10,   decade_base = BASE_MINT_AMOUNT / 10^n
// so the reward drops by a tenth of the decade base at every round number:
//     10M: 1, 20M: 0.9, ..., 90M: 0.2, 100M: 0.1, 200M: 0.09, ..., 1B: 0.01, ...
// and would reach exactly 1 lamport at MAX_SUPPLY (10T).
pub const CURVE_START_TOKENS: u64 = 10_000_000;                         // 10M tokens
pub const CURVE_START_LAMPORTS: u64 = CURVE_START_TOKENS * DECIMAL_FACTOR;
pub const CURVE_DECADES: u32 = 6;                                       // 10M -> 10T
pub const BASE_MINT_AMOUNT: u64 = 1 * DECIMAL_FACTOR;                   // 1 token

/// One mint per transaction.
///
/// A transaction's instructions sysvar holds the serialized top-level instructions followed by
/// a 2-byte current instruction index. Everything except those last 2 bytes is the same for
/// every instruction and every CPI in the transaction, cannot be changed by any program, and
/// includes every instruction's program id, accounts (signers included) and data. Its hash is
/// therefore a fingerprint of the transaction.
///
/// MintState stores the fingerprint and slot of the last mint. A mint whose transaction has the
/// same fingerprint in the same slot is a second mint in the same transaction and is rejected,
/// whether it comes from another top-level instruction or from a CPI (state written by the first
/// mint is visible to later instructions and CPIs of the same transaction, and every mint
/// write-locks MintState, so no other transaction can run in between).
#[account]
pub struct MintState {
    pub last_tx_fingerprint: [u8; 32], // fingerprint of the last transaction that minted
    pub last_slot: u64,                // slot of that transaction
    pub bump: u8,                      // PDA bump, seeds = [b"mint_state"]
    pub reserved: [u8; 64],            // reserved for future fields
}

impl MintState {
    pub const SPACE: usize = 8 + 32 + 8 + 1 + 64;

    /// Record a mint; fails if this transaction (same fingerprint, same slot) already minted
    pub fn record_mint(&mut self, fingerprint: [u8; 32], slot: u64) -> Result<()> {
        if self.last_slot == slot && self.last_tx_fingerprint == fingerprint {
            msg!("Only one mint is allowed per transaction");
            return Err(ErrorCode::OneMintPerTransaction.into());
        }
        self.last_tx_fingerprint = fingerprint;
        self.last_slot = slot;
        Ok(())
    }
}

/// Fingerprint of the current transaction: hash of the instructions sysvar data without the
/// trailing 2-byte current instruction index (the bytes `load_current_index_checked` reads)
pub fn transaction_fingerprint(instructions_sysvar_data: &[u8]) -> Result<[u8; 32]> {
    let len = instructions_sysvar_data.len();
    if len < 2 {
        return Err(ProgramError::InvalidAccountData.into());
    }
    Ok(hash(&instructions_sysvar_data[..len - 2]).to_bytes())
}

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
            &mut ctx.accounts.mint_state,
            ctx.bumps.mint_authority,
        )
    }

    /// Process token minting with dynamic amount based on total supply
    /// Mints to `recipient`'s token account, so anyone can mint for someone else;
    /// `recipient` is checked against recipient_token_account.owner in ProcessMintTo
    pub fn process_mint_to(ctx: Context<ProcessMintTo>, recipient: Pubkey) -> Result<()> {
        // Keep the IDL argument name `recipient`; the value is only used by the account constraint
        let _ = recipient;

        // Use shared mint logic
        execute_mint_operation(
            &ctx.accounts.instructions,
            &ctx.accounts.mint,
            &ctx.accounts.mint_authority,
            &ctx.accounts.recipient_token_account,
            &ctx.accounts.token_program,
            &mut ctx.accounts.mint_state,
            ctx.bumps.mint_authority,
        )
    }

    /// Create the MintState account (admin only, once; run right after deploying this version)
    pub fn initialize_mint_state(ctx: Context<InitializeMintState>) -> Result<()> {
        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.last_tx_fingerprint = [0u8; 32];
        mint_state.last_slot = 0;
        mint_state.bump = ctx.bumps.mint_state;
        mint_state.reserved = [0u8; 64];
        Ok(())
    }
}

/// Shared mint operation logic
fn execute_mint_operation<'info>(
    instructions: &AccountInfo<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    mint_authority: &AccountInfo<'info>,
    token_account: &InterfaceAccount<'info, TokenAccount>,
    token_program: &Program<'info, Token2022>,
    mint_state: &mut Account<'info, MintState>,
    mint_authority_bump: u8,
) -> Result<()> {
    // Check for memo instruction with length constraints
    if !check_memo_instruction(instructions)? {
        msg!("No memo instruction found");
        return Err(ErrorCode::MemoRequired.into());
    }

    // One mint per transaction (see MintState); checked and recorded before minting
    let fingerprint = transaction_fingerprint(&instructions.try_borrow_data()?)?;
    mint_state.record_mint(fingerprint, Clock::get()?.slot)?;
    
    // mint_authority address and canonical bump are already verified by the
    // `seeds = [b"mint_authority"], bump` constraint in the Accounts struct
    
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
    
    // Log successful mint operation (integer units, 1 token = 1,000,000 units)
    msg!("Minted {} units to {}, supply before mint: {} units", amount, token_account.owner, current_supply);
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-mint::process_mint or memo-mint::process_mint_to (other instructions)
/// 
/// Compute budget instructions also occupy an instruction index, so they must be
/// placed at index 1 or later; a compute budget instruction at index 0 makes the mint fail.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<bool> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction (process_mint) must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("process_mint must be at index 1 or later, but current instruction is at index {}", current_index);
        return Ok(false);
    }
    
    // Check that index 0 contains the memo instruction
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH)
            } else {
                msg!("Instruction at index 0 is not a memo (program_id: {})", ix.program_id);
                Ok(false)
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 0: {:?}", e);
            Ok(false)
        }
    }
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

/// Calculate dynamic mint amount based on current supply with hard cap
fn calculate_dynamic_mint_amount(current_supply: u64) -> Result<u64> {
    // Check hard limit first
    if current_supply >= MAX_SUPPLY_LAMPORTS {
        return Err(ErrorCode::SupplyLimitReached.into());
    }
    
    // Mint amount based on the emission curve (comparing lamports directly)
    let amount = if current_supply < CURVE_START_LAMPORTS {
        BASE_MINT_AMOUNT
    } else {
        // Find the decade: magnitude <= current_supply < magnitude * 10
        // (at most CURVE_DECADES - 1 iterations; magnitude * 10 <= current_supply < MAX_SUPPLY, so no overflow)
        let mut magnitude = CURVE_START_LAMPORTS;
        let mut decade_base = BASE_MINT_AMOUNT;
        while current_supply / 10 >= magnitude {
            magnitude *= 10;
            decade_base /= 10;
        }

        // Leading digit, always 1..=9 here
        let digit = current_supply / magnitude;
        decade_base * (11 - digit) / 10
    };
    
    // use checked_add to prevent overflow
    let new_supply = current_supply.checked_add(amount)
        .ok_or(ErrorCode::ArithmeticOverflow)?;
    
    // check if it exceeds the hard limit
    if new_supply > MAX_SUPPLY_LAMPORTS {
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

    /// One-mint-per-transaction state (last, so existing account order is unchanged)
    #[account(
        mut,
        seeds = [b"mint_state"],
        bump = mint_state.bump
    )]
    pub mint_state: Account<'info, MintState>,
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

    /// One-mint-per-transaction state (last, so existing account order is unchanged)
    #[account(
        mut,
        seeds = [b"mint_state"],
        bump = mint_state.bump
    )]
    pub mint_state: Account<'info, MintState>,
}

/// Account structure for creating MintState (admin only, once)
#[derive(Accounts)]
pub struct InitializeMintState<'info> {
    #[account(
        mut,
        constraint = admin.key() == AUTHORIZED_ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin
    )]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = MintState::SPACE,
        seeds = [b"mint_state"],
        bump
    )]
    pub mint_state: Account<'info, MintState>,

    pub system_program: Program<'info, System>,
}

/// Error code definitions
#[error_code]
pub enum ErrorCode {
    #[msg("Transaction must include a memo instruction.")]
    MemoRequired,

    #[msg("Memo too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Invalid token account: Account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint: Only the specified mint address can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account: Token account must be owned by the minting user or the declared recipient.")]
    UnauthorizedTokenAccount,

    #[msg("Supply limit reached. Maximum supply is 10 trillion tokens.")]
    SupplyLimitReached,

    #[msg("Arithmetic overflow detected.")]
    ArithmeticOverflow,

    #[msg("Unauthorized admin: Only the authorized admin can perform this operation.")]
    UnauthorizedAdmin,

    #[msg("Only one mint is allowed per transaction.")]
    OneMintPerTransaction,
}

// Unit tests in separate file
#[cfg(test)]
mod tests;
