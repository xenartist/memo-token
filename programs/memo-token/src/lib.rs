use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;

declare_id!("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw");

#[program]
pub mod memo_token {
    use super::*;
    
    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // calculate memo length
        let memo_length = memo_data.len();
        
        // check memo length is not too long
        if memo_length > 700 {
            return Err(ErrorCode::MemoTooLong.into());
        }
        
        // determine the possible token count range based on length
        let max_tokens = if memo_length <= 100 {
            1
        } else if memo_length <= 200 {
            2
        } else if memo_length <= 300 {
            3
        } else if memo_length <= 400 {
            4
        } else if memo_length <= 500 {
            5
        } else if memo_length <= 600 {
            6
        } else {
            7 // max 700 bytes
        };
        
        // get PDA and bump
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            ctx.program_id
        );
        
        // verify PDA
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }
        
        // generate random number
        let clock = Clock::get()?;
        let mut hasher = solana_program::hash::Hasher::default();
        hasher.hash(&memo_data);
        hasher.hash(&clock.slot.to_le_bytes());
        hasher.hash(&clock.unix_timestamp.to_le_bytes());
        hasher.hash(ctx.accounts.user.key().as_ref());
        let hash = hasher.result();
        
        // generate random number between 1 and max_tokens
        let random_bytes = &hash.to_bytes()[0..8];
        let random_value = u64::from_le_bytes(random_bytes.try_into().unwrap());
        let token_count = (random_value % max_tokens as u64) + 1;
        
        // calculate mint amount (1 token = 10^9 units)
        let amount = token_count * 1_000_000_000;
        
        // mint tokens
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[&[b"mint_authority".as_ref(), &[bump]]]
            ),
            amount
        )?;
        
        // record the number of tokens minted
        msg!("Minted {} tokens", token_count);

        Ok(())
    }
}

// Optimized but still somewhat flexible approach
fn check_memo_instruction(instructions: &AccountInfo, min_length: usize) -> Result<(bool, Vec<u8>)> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // get current instruction index
    let current_index = solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // First check the most likely position (index 1)
    if current_index > 1 {
        match solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    if ix.data.len() >= min_length {
                        return Ok((true, ix.data.to_vec()));
                    } else {
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                }
            },
            Err(_) => {}
        }
    }
    
    // If not found at index 1, check other positions as fallback
    for i in 0..current_index {
        if i == 1 { continue; } // Skip index 1 as we already checked it
        
        match solana_program::sysvar::instructions::load_instruction_at_checked(i.into(), instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    if ix.data.len() >= min_length {
                        return Ok((true, ix.data.to_vec()));
                    } else {
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                }
            },
            Err(_) => { continue; }
        }
    }
    
    // No valid memo found
    Ok((false, vec![]))
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
    
    #[msg("Memo is too long. Must be at most 700 bytes.")]
    MemoTooLong,
    
    #[msg("Transaction must include a memo.")]
    MemoRequired,
}