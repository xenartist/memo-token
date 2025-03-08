#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022,
    token_interface::{Mint, TokenAccount, Token2022},
};
use spl_token_2022::extension::{
    metadata_pointer::instruction as metadata_pointer_instruction,
};
use spl_token_metadata_interface::instruction as metadata_instruction;

declare_id!("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw");

#[program]
pub mod memo_token {
    use super::*;

    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // verify mint authority PDA
        let (mint_authority, bump) = Pubkey::find_program_address(&[b"mint_authority"], ctx.program_id);
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }

        // mint token
        let amount = 1_000_000_000; // 1 token (9 decimals)
        let mint_authority_seeds = &[b"mint_authority".as_ref(), &[bump]];
        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token_2022::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[mint_authority_seeds],
            ),
            amount,
        )?;

        Ok(())
    }

    pub fn set_metadata(ctx: Context<SetMetadata>, name: String, symbol: String, uri: String) -> Result<()> {
        // verify mint authority PDA
        let (mint_authority, bump) = Pubkey::find_program_address(&[b"mint_authority"], ctx.program_id);
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }

        let mint_authority_seeds = &[b"mint_authority".as_ref(), &[bump]];

        // 1. initialize metadata pointer
        let init_metadata_pointer_ix = metadata_pointer_instruction::initialize(
            &spl_token_2022::id(),
            &ctx.accounts.mint.key(),
            Some(mint_authority),
            Some(ctx.accounts.mint.key()),
        )?;

        // execute initialize metadata pointer CPI call
        solana_program::program::invoke_signed(
            &init_metadata_pointer_ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.mint_authority.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
            ],
            &[mint_authority_seeds],
        )?;

        // 2. update metadata
        let update_metadata_ix = metadata_instruction::initialize(
            &spl_token_2022::id(),
            &ctx.accounts.mint.key(),
            &mint_authority,
            &mint_authority,
            &ctx.accounts.user.key(),
            symbol,
            uri,
            name,
        );

        // execute update metadata CPI call
        solana_program::program::invoke_signed(
            &update_metadata_ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.mint_authority.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
            ],
            &[mint_authority_seeds],
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: This is the mint authority PDA
    pub mint_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct SetMetadata<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: This is the mint authority PDA
    pub mint_authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token2022>,
}