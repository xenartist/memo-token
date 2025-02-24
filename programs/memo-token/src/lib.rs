use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

declare_id!("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap");

#[program]
pub mod memo_token {
    use super::*;

    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // 铸造代币奖励
        token::mint_to(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
            ),
            1,  // 铸造1个代币
        )?;
        
        msg!("Congratulations! You got a token!");
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