use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;

declare_id!("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw");

#[program]
pub mod memo_token {
    use super::*;
    
    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // Check if there's a memo instruction in the transaction
        let memo_found = check_memo_before_current_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // Get PDA and bump
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            ctx.program_id
        );
        
        // Verify PDA
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }

        // Mint tokens (fixed amount of 1)
        let amount = 1_000_000_000; // 1 token (9 decimals)
        let mint_authority_seeds = &[
            b"mint_authority".as_ref(),
            &[bump],
        ];
        
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[mint_authority_seeds]
            ),
            amount
        )?;

        Ok(())
    }
}

// only check instructions before current instruction
fn check_memo_before_current_instruction(instructions: &AccountInfo, min_length: usize) -> Result<bool> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // current instruction index
    let current_index = solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // only check instructions before current instruction
    for i in 0..current_index {
        let index = i as usize;
        
        if let Ok(ix) = solana_program::sysvar::instructions::load_instruction_at_checked(index, instructions) {
            // check if it's a memo instruction
            if ix.program_id == memo_program_id {
                // check memo length
                if ix.data.len() >= min_length {
                    return Ok(true);
                } else {
                    return Err(ErrorCode::MemoTooShort.into());
                }
            }
        }
    }

    Ok(false)
}

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    /// CHECK: PDA as mint authority
    pub mint_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Memo is too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Transaction must include a memo instruction before the mint instruction.")]
    MemoRequired,
}