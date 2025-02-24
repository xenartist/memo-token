use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

declare_id!("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap");

#[program]
pub mod memo_token {
    use super::*;

    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // Mint 1 token (1_000_000_000 represents 1 token with 9 decimals)
        token::mint_to(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
            ),
            1_000_000_000,  // Changed from 1 to 1_000_000_000
        )?;
        
        msg!("Congratulations! You got 1 token!");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub from: Signer<'info>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub mint_authority: Signer<'info>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = from,
    )]
    pub token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}