#![allow(unexpected_cfgs)]

//! Test-only app program for the memo-home regression suite (local validator only, never
//! deployed). It is the smallest app built the intended way: its PDA [b"app"] is the Authority
//! of its Collections and signs memo-home's create_collection, create_asset and mine by CPI.
//! Its PDA can also own Assets, so the suite can check owner outflows signed by a program.

use anchor_lang::prelude::*;

declare_id!("EZzeE6EZ7oD3Qsr5F1Wuu5TVHgbzDYL4LiZCxaSPR6c2");

pub const APP_SEED: &[u8] = b"app";

#[program]
pub mod home_test_app {
    use super::*;

    pub fn create_collection(ctx: Context<AppCreateCollection>, seed: [u8; 32], max_share_bps: u16) -> Result<()> {
        let a = &ctx.accounts;
        memo_home::cpi::create_collection(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::CreateCollection {
                    payer: a.payer.to_account_info(),
                    authority: a.app.to_account_info(),
                    collection: a.collection.to_account_info(),
                    program_authority: a.program_authority.to_account_info(),
                    burn_guard: a.burn_guard.to_account_info(),
                    config: a.config.to_account_info(),
                    core_program: a.core_program.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[APP_SEED, &[ctx.bumps.app]]],
            ),
            seed,
            "App Rooms".into(),
            "https://example.com/app-collection.json".into(),
            max_share_bps,
            None,
            None,
        )
    }

    pub fn create_asset(ctx: Context<AppCreateAsset>, seed: [u8; 32]) -> Result<()> {
        let a = &ctx.accounts;
        memo_home::cpi::create_asset(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::CreateAsset {
                    payer: a.payer.to_account_info(),
                    authority: a.app.to_account_info(),
                    config: a.config.to_account_info(),
                    collection: a.collection.to_account_info(),
                    asset: a.asset.to_account_info(),
                    vault_state: a.vault_state.to_account_info(),
                    owner: a.owner.to_account_info(),
                    program_authority: a.program_authority.to_account_info(),
                    core_program: a.core_program.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[APP_SEED, &[ctx.bumps.app]]],
            ),
            seed,
            "App Room".into(),
            "https://example.com/app-room.json".into(),
        )
    }

    /// The app's message action: mines into the Asset's Vault with a share for the sender, and
    /// reads the result from memo-home's return data
    pub fn mine(ctx: Context<AppMine>, share_bps: u16) -> Result<()> {
        let a = &ctx.accounts;
        let result = memo_home::cpi::mine(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::Mine {
                    caller: a.caller.to_account_info(),
                    authority: a.app.to_account_info(),
                    config: a.config.to_account_info(),
                    asset: a.asset.to_account_info(),
                    vault: a.vault.to_account_info(),
                    vault_token_account: a.vault_token_account.to_account_info(),
                    recipient_token_account: a.recipient_token_account.to_account_info(),
                    mint: a.mint.to_account_info(),
                    mint_authority: a.mint_authority.to_account_info(),
                    mint_state: a.mint_state.to_account_info(),
                    instructions: a.instructions.to_account_info(),
                    token_program: a.token_program.to_account_info(),
                    associated_token_program: a.associated_token_program.to_account_info(),
                    memo_mint_program: a.memo_mint_program.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[APP_SEED, &[ctx.bumps.app]]],
            ),
            share_bps,
        )?
        .get();
        msg!("mined reward {} share {}", result.reward, result.share);
        Ok(())
    }

    /// Owner outflow signed by the app PDA, for an Asset the app owns
    pub fn withdraw_lamports(ctx: Context<AppWithdrawLamports>, amount: u64) -> Result<()> {
        let a = &ctx.accounts;
        memo_home::cpi::withdraw_lamports(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::WithdrawLamports {
                    owner: a.app.to_account_info(),
                    config: a.config.to_account_info(),
                    asset: a.asset.to_account_info(),
                    vault: a.vault.to_account_info(),
                    vault_state: a.vault_state.to_account_info(),
                    destination: a.destination.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[APP_SEED, &[ctx.bumps.app]]],
            ),
            amount,
        )
    }
}

/// Pass-through accounts: memo-home checks every one of them
#[derive(Accounts)]
pub struct AppCreateCollection<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: the app PDA, Authority of the Collection
    #[account(seeds = [APP_SEED], bump)]
    pub app: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub collection: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub program_authority: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub burn_guard: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub config: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
}

#[derive(Accounts)]
pub struct AppCreateAsset<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: the app PDA, Authority of the Collection
    #[account(seeds = [APP_SEED], bump)]
    pub app: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub config: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub collection: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub vault_state: UncheckedAccount<'info>,
    /// CHECK: any owner
    pub owner: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub program_authority: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
}

#[derive(Accounts)]
pub struct AppMine<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,
    /// CHECK: the app PDA, Authority of the Collection
    #[account(seeds = [APP_SEED], bump)]
    pub app: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub config: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub asset: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub vault: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub vault_token_account: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub recipient_token_account: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked by memo-mint
    pub mint_authority: UncheckedAccount<'info>,
    /// CHECK: checked by memo-mint
    #[account(mut)]
    pub mint_state: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub instructions: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub token_program: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub associated_token_program: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub memo_mint_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
}

#[derive(Accounts)]
pub struct AppWithdrawLamports<'info> {
    /// CHECK: the app PDA, owner of the Asset
    #[account(seeds = [APP_SEED], bump)]
    pub app: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub config: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    pub asset: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: checked by memo-home
    #[account(mut)]
    pub vault_state: UncheckedAccount<'info>,
    /// CHECK: any destination
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
}
