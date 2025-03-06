use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::{self, Token2022},
    token_interface::{Mint, TokenAccount},
};

declare_id!("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap");

#[program]
pub mod memo_token {
    use super::*;
    
    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
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
        
        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token_2022::MintTo {
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

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    /// Interface for the SPL Token 2022 Mint account
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA as mint authority
    pub mint_authority: AccountInfo<'info>,
    
    #[account(mut)]
    /// Interface for the SPL Token 2022 Token account
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
}