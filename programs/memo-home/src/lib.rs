#![allow(unexpected_cfgs)]

//! memo-home: the base-layer NFT program of the MEMO system (see MEMO-HOME-DESIGN.md).
//!
//! Every Asset is a Metaplex Core Asset in a memo-home Collection. memo-home is the Core update
//! authority of every Collection it creates, and each Collection has an immutable Authority
//! (usually an app program's PDA) that may create and update Assets and mine into their Vaults.
//! Each Asset has a Vault PDA whose holdings only the Asset's current Core owner can move, and a
//! Vault State that counts the Vault's outflows.
//! Collection and Asset addresses are memo-home PDAs, so no private key exists for them: Core
//! lets the Collection address sign as the update authority of its Assets.
//! There is no admin key, no fee and no app data in this program.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    sysvar::instructions::ID as INSTRUCTIONS_ID,
};
use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id, AssociatedToken};
use anchor_spl::token_2022::Token2022;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("ArrrsGh5biKJYpiKMYJ6FD3AxCY7Fhd9Gv761ZCymzzs");

#[cfg(not(feature = "mainnet"))]
declare_id!("J1pxwy8V4xKvPgS5HMUn5jFo3u7At3MW4BvFWKg48Qcv");

/// Metaplex Core program (same address on X1 testnet and mainnet)
pub const CORE_PROGRAM_ID: Pubkey = pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");

/// The MEMO mint (memo-mint only mints this token)
pub const MEMO_MINT: Pubkey = memo_mint::AUTHORIZED_MINT_PUBKEY;

// PDA seeds
pub const AUTHORITY_SEED: &[u8] = b"authority";
pub const BURN_GUARD_SEED: &[u8] = b"burn_guard";
pub const COLLECTION_SEED: &[u8] = b"collection";
pub const COLLECTION_CONFIG_SEED: &[u8] = b"collection_config";
pub const ASSET_SEED: &[u8] = b"asset";
pub const VAULT_SEED: &[u8] = b"vault";
pub const VAULT_STATE_SEED: &[u8] = b"vault_state";

/// Basis points denominator (100%)
pub const MAX_BPS: u16 = 10_000;

/// Royalty creators per Collection (Metaplex convention)
pub const MAX_ROYALTY_CREATORS: usize = 5;

/// Name and URI limits (bytes) for Collections and Assets
pub const MAX_NAME_LEN: usize = 64;
pub const MAX_URI_LEN: usize = 256;

// memo-home must be built for the same network as the memo-mint and memo-burn it calls. An app
// that enables memo-mint/mainnet or memo-burn/mainnet without memo-home/mainnet fails here.
const fn same_key(a: &Pubkey, b: &Pubkey) -> bool {
    let (a, b) = (a.as_array(), b.as_array());
    let mut i = 0;
    while i < 32 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
#[cfg(feature = "mainnet")]
const _: () = assert!(
    same_key(&memo_mint::ID_CONST, &pubkey!("8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX"))
        && same_key(&memo_burn::ID_CONST, &pubkey!("2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh"))
        && same_key(&MEMO_MINT, &pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick")),
    "memo-home/mainnet needs memo-mint/mainnet and memo-burn/mainnet"
);
#[cfg(not(feature = "mainnet"))]
const _: () = assert!(
    same_key(&memo_mint::ID_CONST, &pubkey!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy"))
        && same_key(&memo_burn::ID_CONST, &pubkey!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP"))
        && same_key(&MEMO_MINT, &pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")),
    "memo-mint or memo-burn is built for mainnet: enable memo-home/mainnet too"
);

// Metaplex Core account keys (first byte of every Core account)
const CORE_KEY_ASSET_V1: u8 = 1;
const CORE_KEY_PLUGIN_HEADER_V1: u8 = 3;
const CORE_KEY_PLUGIN_REGISTRY_V1: u8 = 4;
const CORE_KEY_COLLECTION_V1: u8 = 5;

// Metaplex Core instruction discriminators
const CORE_IX_CREATE_V1: u8 = 0;
const CORE_IX_TRANSFER_V1: u8 = 14;
const CORE_IX_UPDATE_V1: u8 = 15;
const CORE_IX_UPDATE_COLLECTION_V1: u8 = 16;
const CORE_IX_CREATE_COLLECTION_V2: u8 = 21;

// Metaplex Core enum values
const CORE_UA_NONE: u8 = 0;
const CORE_UA_ADDRESS: u8 = 1;
const CORE_UA_COLLECTION: u8 = 2;
const CORE_PLUGIN_ROYALTIES: u8 = 0;
const CORE_PLUGIN_FREEZE_DELEGATE: u8 = 1;
const CORE_PLUGIN_BURN_DELEGATE: u8 = 2;
const CORE_PLUGIN_TRANSFER_DELEGATE: u8 = 3;
const CORE_PLUGIN_PERMANENT_FREEZE_DELEGATE: u8 = 5;
const CORE_PLUGIN_PERMANENT_TRANSFER_DELEGATE: u8 = 7;
const CORE_PLUGIN_PERMANENT_BURN_DELEGATE: u8 = 8;
const CORE_PLUGIN_MASTER_EDITION: u8 = 10;
/// Highest Core plugin type known to this program (Groups); a higher one counts as listed
const CORE_PLUGIN_LAST_KNOWN: u8 = 18;
const CORE_AUTHORITY_NONE: u8 = 0;
const CORE_AUTHORITY_OWNER: u8 = 1;
const CORE_AUTHORITY_UPDATE_AUTHORITY: u8 = 2;
const CORE_AUTHORITY_ADDRESS: u8 = 3;
const CORE_RULE_SET_NONE: u8 = 0;
const CORE_EXTERNAL_ORACLE: u8 = 1;
const CORE_LIFECYCLE_BURN: u8 = 2;
const CORE_CHECK_CAN_REJECT: u32 = 4;
const CORE_RESULTS_OFFSET_ANCHOR: u8 = 1;
const CORE_DATA_STATE_ACCOUNT: u8 = 0;

// Oracle validation values read by Core from the Burn Guard (OracleValidation::V1)
const ORACLE_VALIDATION_V1: u8 = 1;
const ORACLE_REJECTED: u8 = 1;
const ORACLE_PASS: u8 = 2;

#[program]
pub mod memo_home {
    use super::*;

    /// Create the global Burn Guard once. Its content is fixed (burn Rejected, everything else
    /// Pass), so anyone may create it and no admin is needed.
    pub fn init_burn_guard(ctx: Context<InitBurnGuard>) -> Result<()> {
        let guard = &mut ctx.accounts.burn_guard;
        guard.validation = ORACLE_VALIDATION_V1;
        guard.create = ORACLE_PASS;
        guard.transfer = ORACLE_PASS;
        guard.burn = ORACLE_REJECTED;
        guard.update = ORACLE_PASS;
        guard.bump = ctx.bumps.burn_guard;
        msg!("Burn Guard created: {}", guard.key());
        Ok(())
    }

    /// Create a Core Collection at the PDA [COLLECTION_SEED, authority, seed] with memo-home as
    /// update authority, the Burn Guard Oracle and, optionally, a fixed supply limit and fixed
    /// royalties. The Authority signs (an app PDA by CPI). `authority`, `max_share_bps` and
    /// `max_assets` can never change.
    pub fn create_collection(
        ctx: Context<CreateCollection>,
        seed: [u8; 32],
        name: String,
        uri: String,
        max_share_bps: u16,
        max_assets: Option<u32>,
        royalty: Option<RoyaltyArgs>,
    ) -> Result<()> {
        require_name_uri(Some(&name), Some(&uri))?;
        require!(max_share_bps <= MAX_BPS, ErrorCode::InvalidShareBps);
        require!(max_assets != Some(0), ErrorCode::InvalidMaxAssets);
        if let Some(r) = &royalty {
            r.validate()?;
        }

        let authority = ctx.accounts.authority.key();
        let collection_seeds: &[&[u8]] = &[COLLECTION_SEED, authority.as_ref(), &seed, &[ctx.bumps.collection]];
        sweep_prefunded(
            &ctx.accounts.collection,
            &ctx.accounts.payer,
            &ctx.accounts.system_program,
            collection_seeds,
        )?;

        let data = encode_create_collection_v2(
            &name,
            &uri,
            &ctx.accounts.burn_guard.key(),
            max_assets,
            royalty.as_ref(),
        );
        let ix = Instruction {
            program_id: CORE_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(ctx.accounts.collection.key(), true),
                AccountMeta::new_readonly(ctx.accounts.program_authority.key(), false),
                AccountMeta::new(ctx.accounts.payer.key(), true),
                AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
            ],
            data,
        };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.collection.to_account_info(),
                ctx.accounts.program_authority.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[collection_seeds],
        )?;

        let config = &mut ctx.accounts.config;
        config.collection = ctx.accounts.collection.key();
        config.authority = authority;
        config.seed = seed;
        config.max_share_bps = max_share_bps;
        config.max_assets = max_assets;
        config.creator = ctx.accounts.payer.key();
        config.created_at = Clock::get()?.unix_timestamp;
        config.bump = ctx.bumps.config;

        emit!(CollectionCreated {
            collection: config.collection,
            authority,
            seed,
            max_share_bps,
            max_assets,
            creator: config.creator,
        });
        Ok(())
    }

    /// Rename the Collection or change its URI (Authority only).
    pub fn update_collection(
        ctx: Context<UpdateCollection>,
        new_name: Option<String>,
        new_uri: Option<String>,
    ) -> Result<()> {
        require!(new_name.is_some() || new_uri.is_some(), ErrorCode::NothingToUpdate);
        require_name_uri(new_name.as_deref(), new_uri.as_deref())?;
        let mut data = vec![CORE_IX_UPDATE_COLLECTION_V1];
        borsh_opt_str(&mut data, new_name.as_deref());
        borsh_opt_str(&mut data, new_uri.as_deref());
        let core = CORE_PROGRAM_ID;
        let ix = Instruction {
            program_id: core,
            accounts: vec![
                AccountMeta::new(ctx.accounts.collection.key(), false),
                AccountMeta::new(ctx.accounts.payer.key(), true),
                AccountMeta::new_readonly(ctx.accounts.program_authority.key(), true),
                AccountMeta::new_readonly(core, false), // new_update_authority: none
                AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(core, false), // log_wrapper: none
            ],
            data,
        };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.collection.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.program_authority.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.core_program.to_account_info(),
            ],
            &[&[AUTHORITY_SEED, &[ctx.bumps.program_authority]]],
        )?;
        emit!(CollectionUpdated { collection: ctx.accounts.collection.key() });
        Ok(())
    }

    /// Create an Asset at the PDA [ASSET_SEED, collection, seed] in the Collection for `owner`,
    /// with its Vault State (Authority only). Rejected once the Collection has minted
    /// `max_assets` Assets.
    pub fn create_asset(ctx: Context<CreateAsset>, seed: [u8; 32], name: String, uri: String) -> Result<()> {
        require_name_uri(Some(&name), Some(&uri))?;
        // Core gives the Asset to the payer when the owner account is Core's address ("none")
        require_keys_neq!(ctx.accounts.owner.key(), CORE_PROGRAM_ID, ErrorCode::InvalidOwner);
        let collection = parse_collection(&ctx.accounts.collection.try_borrow_data()?)?;
        require_keys_eq!(
            collection.update_authority,
            ctx.accounts.program_authority.key(),
            ErrorCode::NotHomeCollection
        );
        check_max_assets(&collection, ctx.accounts.config.max_assets)?;

        let collection_key = ctx.accounts.collection.key();
        let asset_seeds: &[&[u8]] = &[ASSET_SEED, collection_key.as_ref(), &seed, &[ctx.bumps.asset]];
        sweep_prefunded(&ctx.accounts.asset, &ctx.accounts.payer, &ctx.accounts.system_program, asset_seeds)?;

        let core = CORE_PROGRAM_ID;
        let ix = Instruction {
            program_id: core,
            accounts: vec![
                AccountMeta::new(ctx.accounts.asset.key(), true),
                AccountMeta::new(collection_key, false),
                AccountMeta::new_readonly(ctx.accounts.program_authority.key(), true),
                AccountMeta::new(ctx.accounts.payer.key(), true),
                AccountMeta::new_readonly(ctx.accounts.owner.key(), false),
                AccountMeta::new_readonly(core, false), // update_authority: none (the Collection)
                AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(core, false), // log_wrapper: none
            ],
            data: encode_create_v1(&name, &uri),
        };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.asset.to_account_info(),
                ctx.accounts.collection.to_account_info(),
                ctx.accounts.program_authority.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.owner.to_account_info(),
                ctx.accounts.core_program.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[&[AUTHORITY_SEED, &[ctx.bumps.program_authority]], asset_seeds],
        )?;

        let state = &mut ctx.accounts.vault_state;
        state.nonce = 0;
        state.bump = ctx.bumps.vault_state;

        emit!(AssetCreated {
            collection: collection_key,
            asset: ctx.accounts.asset.key(),
            seed,
            owner: ctx.accounts.owner.key(),
        });
        Ok(())
    }

    /// Rename an Asset or change its URI (Authority only).
    pub fn update_asset(
        ctx: Context<UpdateAsset>,
        new_name: Option<String>,
        new_uri: Option<String>,
    ) -> Result<()> {
        require!(new_name.is_some() || new_uri.is_some(), ErrorCode::NothingToUpdate);
        require_name_uri(new_name.as_deref(), new_uri.as_deref())?;
        require_home_asset(&ctx.accounts.asset, &ctx.accounts.config)?;
        let mut data = vec![CORE_IX_UPDATE_V1];
        borsh_opt_str(&mut data, new_name.as_deref());
        borsh_opt_str(&mut data, new_uri.as_deref());
        data.push(0); // new_update_authority: None
        let core = CORE_PROGRAM_ID;
        let ix = Instruction {
            program_id: core,
            accounts: vec![
                AccountMeta::new(ctx.accounts.asset.key(), false),
                AccountMeta::new_readonly(ctx.accounts.collection.key(), false),
                AccountMeta::new(ctx.accounts.payer.key(), true),
                AccountMeta::new_readonly(ctx.accounts.program_authority.key(), true),
                AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
                AccountMeta::new_readonly(core, false), // log_wrapper: none
            ],
            data,
        };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.asset.to_account_info(),
                ctx.accounts.collection.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.program_authority.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.core_program.to_account_info(),
            ],
            &[&[AUTHORITY_SEED, &[ctx.bumps.program_authority]]],
        )?;
        emit!(AssetUpdated {
            collection: ctx.accounts.collection.key(),
            asset: ctx.accounts.asset.key(),
        });
        Ok(())
    }

    /// Mine MEMO into the Asset's Vault (Authority only): memo-mint mints the current reward to
    /// the Vault, `share_bps` of it (rounded down, at most the Collection's `max_share_bps`) goes
    /// to `recipient_token_account`, the rest stays in the Vault. The transaction must start
    /// with the SPL Memo that memo-mint requires. A recipient that requires incoming memos
    /// (Token-2022 MemoTransfer) cannot receive a share. Returns the reward and the share
    /// (Anchor return data, read by a calling app with `.get()`).
    pub fn mine(ctx: Context<Mine>, share_bps: u16) -> Result<MineResult> {
        require!(share_bps <= ctx.accounts.config.max_share_bps, ErrorCode::ShareTooHigh);
        require_home_asset(&ctx.accounts.asset, &ctx.accounts.config)?;
        require_keys_neq!(
            ctx.accounts.recipient_token_account.key(),
            ctx.accounts.vault_token_account.key(),
            ErrorCode::InvalidDestination
        );

        // The Vault's MEMO token account is created on first use (caller pays); it can never be
        // closed, so this happens once per Asset
        associated_token::create_idempotent(CpiContext::new(
            ctx.accounts.associated_token_program.to_account_info(),
            associated_token::Create {
                payer: ctx.accounts.caller.to_account_info(),
                associated_token: ctx.accounts.vault_token_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
        ))?;

        let before = token_amount(&ctx.accounts.vault_token_account)?;
        memo_mint::cpi::process_mint_to(
            CpiContext::new(
                ctx.accounts.memo_mint_program.to_account_info(),
                memo_mint::cpi::accounts::ProcessMintTo {
                    caller: ctx.accounts.caller.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    mint_authority: ctx.accounts.mint_authority.to_account_info(),
                    recipient_token_account: ctx.accounts.vault_token_account.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    instructions: ctx.accounts.instructions.to_account_info(),
                    mint_state: ctx.accounts.mint_state.to_account_info(),
                },
            ),
            ctx.accounts.vault.key(),
        )?;
        let reward = token_amount(&ctx.accounts.vault_token_account)?
            .checked_sub(before)
            .ok_or(ErrorCode::ArithmeticOverflow)?;

        let share = share_of(reward, share_bps)?;
        if share > 0 {
            let asset_key = ctx.accounts.asset.key();
            let seeds: &[&[u8]] = &[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]];
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_token_account.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.recipient_token_account.to_account_info(),
                        authority: ctx.accounts.vault.to_account_info(),
                    },
                    &[seeds],
                ),
                share,
                ctx.accounts.mint.decimals,
            )?;
        }

        emit!(Mined {
            collection: ctx.accounts.config.collection,
            asset: ctx.accounts.asset.key(),
            recipient_token_account: ctx.accounts.recipient_token_account.key(),
            reward,
            share,
        });
        Ok(MineResult { reward, share })
    }

    /// Move tokens out of the Vault (current Core owner only, Asset not listed). Works for any
    /// SPL Token or Token-2022 mint. For a Token-2022 mint with a transfer hook, the hook's extra
    /// accounts (resolved off-chain, as for any transfer) are passed as remaining accounts and
    /// forwarded to the token program as non-signers: hooks that need an extra signer, and
    /// destinations that require incoming memos, are not supported. `amount` u64::MAX withdraws
    /// the whole balance, so "withdraw all, then close the account" works in one transaction
    /// even if someone sends more tokens just before it.
    pub fn withdraw<'info>(ctx: Context<'_, '_, 'info, 'info, Withdraw<'info>>, amount: u64) -> Result<()> {
        let amount = if amount == u64::MAX { ctx.accounts.vault_token_account.amount } else { amount };
        require!(amount > 0, ErrorCode::InvalidAmount);
        require_owner_not_listed(&ctx.accounts.asset, &ctx.accounts.config, &ctx.accounts.owner.key())?;
        require_keys_neq!(
            ctx.accounts.destination.key(),
            ctx.accounts.vault_token_account.key(),
            ErrorCode::InvalidDestination
        );
        let nonce = bump_nonce(&mut ctx.accounts.vault_state)?;
        let asset_key = ctx.accounts.asset.key();
        let seeds: &[&[u8]] = &[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]];
        let mut ix = anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked(
            &ctx.accounts.token_program.key(),
            &ctx.accounts.vault_token_account.key(),
            &ctx.accounts.mint.key(),
            &ctx.accounts.destination.key(),
            &ctx.accounts.vault.key(),
            &[],
            amount,
            ctx.accounts.mint.decimals,
        )?;
        let mut infos = vec![
            ctx.accounts.vault_token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.destination.to_account_info(),
            ctx.accounts.vault.to_account_info(),
        ];
        forward_remaining(&mut ix, &mut infos, ctx.remaining_accounts);
        invoke_signed(&ix, &infos, &[seeds])?;
        emit!(Withdrawn {
            asset: asset_key,
            mint: ctx.accounts.mint.key(),
            destination: ctx.accounts.destination.key(),
            amount,
            nonce,
        });
        Ok(())
    }

    /// Move native XNT out of the Vault (current Core owner only, Asset not listed). The Vault
    /// keeps either nothing or at least the rent-exempt minimum of an empty account.
    pub fn withdraw_lamports(ctx: Context<WithdrawLamports>, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::InvalidAmount);
        require_owner_not_listed(&ctx.accounts.asset, &ctx.accounts.config, &ctx.accounts.owner.key())?;
        require_keys_neq!(ctx.accounts.destination.key(), ctx.accounts.vault.key(), ErrorCode::InvalidDestination);
        let left = ctx.accounts.vault.lamports().checked_sub(amount).ok_or(ErrorCode::InsufficientVaultLamports)?;
        require!(left == 0 || left >= Rent::get()?.minimum_balance(0), ErrorCode::InsufficientVaultLamports);
        let nonce = bump_nonce(&mut ctx.accounts.vault_state)?;
        let asset_key = ctx.accounts.asset.key();
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                },
                &[&[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]]],
            ),
            amount,
        )?;
        emit!(LamportsWithdrawn { asset: asset_key, destination: ctx.accounts.destination.key(), amount, nonce });
        Ok(())
    }

    /// Close an empty token account of the Vault and send its rent to `destination` (current
    /// Core owner only, Asset not listed). The Vault's MEMO token account cannot be closed: its
    /// rent was paid by the first `mine` caller and stays with the Asset.
    pub fn close_token_account(ctx: Context<CloseTokenAccount>) -> Result<()> {
        require_owner_not_listed(&ctx.accounts.asset, &ctx.accounts.config, &ctx.accounts.owner.key())?;
        require_keys_neq!(
            ctx.accounts.token_account.key(),
            get_associated_token_address_with_program_id(
                &ctx.accounts.vault.key(),
                &MEMO_MINT,
                &anchor_spl::token_2022::ID
            ),
            ErrorCode::MemoAccountNotClosable
        );
        // Value leaves only through `withdraw` (also for wrapped XNT, which closing would unwrap)
        require!(ctx.accounts.token_account.amount == 0, ErrorCode::TokenAccountNotEmpty);
        let nonce = bump_nonce(&mut ctx.accounts.vault_state)?;
        let asset_key = ctx.accounts.asset.key();
        token_interface::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token_interface::CloseAccount {
                account: ctx.accounts.token_account.to_account_info(),
                destination: ctx.accounts.destination.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            },
            &[&[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]]],
        ))?;
        emit!(TokenAccountClosed { asset: asset_key, token_account: ctx.accounts.token_account.key(), nonce });
        Ok(())
    }

    /// Move a Core Asset owned by the Vault to `new_owner` (current Core owner only, Asset not
    /// listed). Accounts that the inner Asset's plugins need are passed as remaining accounts.
    pub fn withdraw_asset<'info>(ctx: Context<'_, '_, 'info, 'info, WithdrawAsset<'info>>) -> Result<()> {
        require_owner_not_listed(&ctx.accounts.asset, &ctx.accounts.config, &ctx.accounts.owner.key())?;
        let inner = parse_asset(&ctx.accounts.inner_asset.try_borrow_data()?)?;
        require_keys_eq!(inner.owner, ctx.accounts.vault.key(), ErrorCode::NotInVault);
        // Core needs the inner Asset's Collection when it has one; otherwise Core's address as "none"
        let expected_collection = inner.collection.unwrap_or(CORE_PROGRAM_ID);
        require_keys_eq!(ctx.accounts.inner_collection.key(), expected_collection, ErrorCode::WrongCollection);
        let nonce = bump_nonce(&mut ctx.accounts.vault_state)?;

        let core = CORE_PROGRAM_ID;
        let accounts = vec![
            AccountMeta::new(ctx.accounts.inner_asset.key(), false),
            AccountMeta::new_readonly(ctx.accounts.inner_collection.key(), false),
            AccountMeta::new(ctx.accounts.payer.key(), true),
            AccountMeta::new_readonly(ctx.accounts.vault.key(), true),
            AccountMeta::new_readonly(ctx.accounts.new_owner.key(), false),
            AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
            AccountMeta::new_readonly(core, false), // log_wrapper: none
        ];
        let mut infos = vec![
            ctx.accounts.inner_asset.to_account_info(),
            ctx.accounts.inner_collection.to_account_info(),
            ctx.accounts.payer.to_account_info(),
            ctx.accounts.vault.to_account_info(),
            ctx.accounts.new_owner.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.core_program.to_account_info(),
        ];
        let mut ix = Instruction { program_id: core, accounts, data: vec![CORE_IX_TRANSFER_V1, 0] };
        forward_remaining(&mut ix, &mut infos, ctx.remaining_accounts);
        let asset_key = ctx.accounts.asset.key();
        invoke_signed(&ix, &infos, &[&[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]]])?;
        emit!(AssetWithdrawn {
            asset: asset_key,
            inner_asset: ctx.accounts.inner_asset.key(),
            new_owner: ctx.accounts.new_owner.key(),
            nonce,
        });
        Ok(())
    }

    /// Burn MEMO held by the Vault through memo-burn, the Vault signing as the user (current
    /// Core owner only, Asset not listed). The transaction must start with the BurnMemo that
    /// memo-burn requires. On first use the payer funds the Vault's UserGlobalBurnStats.
    pub fn burn(ctx: Context<Burn>, amount: u64) -> Result<()> {
        require_owner_not_listed(&ctx.accounts.asset, &ctx.accounts.config, &ctx.accounts.owner.key())?;
        let nonce = bump_nonce(&mut ctx.accounts.vault_state)?;
        let asset_key = ctx.accounts.asset.key();
        let seeds: &[&[u8]] = &[VAULT_SEED, asset_key.as_ref(), &[ctx.bumps.vault]];

        if ctx.accounts.vault_burn_stats.data_is_empty() {
            // The Vault pays for its own stats account, so it must hold that rent and stay
            // rent-exempt itself; the payer tops it up
            let rent = Rent::get()?;
            let need = rent
                .minimum_balance(memo_burn::UserGlobalBurnStats::SPACE)
                .checked_add(rent.minimum_balance(0))
                .ok_or(ErrorCode::ArithmeticOverflow)?;
            let have = ctx.accounts.vault.lamports();
            if need > have {
                anchor_lang::system_program::transfer(
                    CpiContext::new(
                        ctx.accounts.system_program.to_account_info(),
                        anchor_lang::system_program::Transfer {
                            from: ctx.accounts.payer.to_account_info(),
                            to: ctx.accounts.vault.to_account_info(),
                        },
                    ),
                    need - have,
                )?;
            }
            memo_burn::cpi::initialize_user_global_burn_stats(CpiContext::new_with_signer(
                ctx.accounts.memo_burn_program.to_account_info(),
                memo_burn::cpi::accounts::InitializeUserGlobalBurnStats {
                    user: ctx.accounts.vault.to_account_info(),
                    user_global_burn_stats: ctx.accounts.vault_burn_stats.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[seeds],
            ))?;
        }

        memo_burn::cpi::process_burn(
            CpiContext::new_with_signer(
                ctx.accounts.memo_burn_program.to_account_info(),
                memo_burn::cpi::accounts::ProcessBurn {
                    user: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    token_account: ctx.accounts.vault_token_account.to_account_info(),
                    user_global_burn_stats: ctx.accounts.vault_burn_stats.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    instructions: ctx.accounts.instructions.to_account_info(),
                    global_burn_stats: ctx.accounts.global_burn_stats.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;
        emit!(VaultBurned { asset: asset_key, amount, nonce });
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Instruction arguments
// ---------------------------------------------------------------------------------------------

/// One royalty receiver; percentages of all creators add up to 100
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct RoyaltyCreator {
    pub address: Pubkey,
    pub percentage: u8,
}

/// Royalties for marketplace sales, written to a Core Royalties plugin that nobody can change
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct RoyaltyArgs {
    pub basis_points: u16,
    pub creators: Vec<RoyaltyCreator>,
}

impl RoyaltyArgs {
    pub fn validate(&self) -> Result<()> {
        require!(self.basis_points <= MAX_BPS, ErrorCode::InvalidRoyalty);
        require!(
            !self.creators.is_empty() && self.creators.len() <= MAX_ROYALTY_CREATORS,
            ErrorCode::InvalidRoyalty
        );
        let total: u32 = self.creators.iter().map(|c| c.percentage as u32).sum();
        require!(total == 100, ErrorCode::InvalidRoyalty);
        // Core rejects duplicate creators
        for (i, c) in self.creators.iter().enumerate() {
            require!(!self.creators[..i].iter().any(|o| o.address == c.address), ErrorCode::InvalidRoyalty);
        }
        Ok(())
    }
}

/// What `mine` minted into the Vault and paid out of it
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq)]
pub struct MineResult {
    pub reward: u64,
    pub share: u64,
}

// ---------------------------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitBurnGuard<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = BurnGuard::SPACE,
        seeds = [BURN_GUARD_SEED],
        bump
    )]
    pub burn_guard: Account<'info, BurnGuard>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seed: [u8; 32])]
pub struct CreateCollection<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The Collection's fixed Authority, signing to consent (an app PDA signs by CPI)
    pub authority: Signer<'info>,

    /// CHECK: the new Core Collection, a memo-home PDA (no private key exists), created by Core
    #[account(mut, seeds = [COLLECTION_SEED, authority.key().as_ref(), &seed], bump)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: memo-home PDA that becomes the Collection's Core update authority
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub program_authority: UncheckedAccount<'info>,

    /// The global Burn Guard, added to the Collection as an Oracle
    #[account(seeds = [BURN_GUARD_SEED], bump = burn_guard.bump)]
    pub burn_guard: Account<'info, BurnGuard>,

    #[account(
        init,
        payer = payer,
        space = CollectionConfig::SPACE,
        seeds = [COLLECTION_CONFIG_SEED, collection.key().as_ref()],
        bump
    )]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Metaplex Core
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateCollection<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(constraint = authority.key() == config.authority @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: the Core Collection of this config
    #[account(mut, address = config.collection @ ErrorCode::WrongCollection)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: memo-home PDA, the Collection's Core update authority
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: Metaplex Core
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seed: [u8; 32])]
pub struct CreateAsset<'info> {
    /// Pays for the Asset and its Vault State
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(constraint = authority.key() == config.authority @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: the Core Collection of this config, parsed in the handler
    #[account(mut, address = config.collection @ ErrorCode::WrongCollection, owner = CORE_PROGRAM_ID)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: the new Core Asset, a memo-home PDA (no private key exists), created by Core
    #[account(mut, seeds = [ASSET_SEED, collection.key().as_ref(), &seed], bump)]
    pub asset: UncheckedAccount<'info>,

    #[account(
        init,
        payer = payer,
        space = VaultState::SPACE,
        seeds = [VAULT_STATE_SEED, asset.key().as_ref()],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: any address except Core's may own the new Asset
    pub owner: UncheckedAccount<'info>,

    /// CHECK: memo-home PDA, the Collection's Core update authority
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: Metaplex Core
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateAsset<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(constraint = authority.key() == config.authority @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: the Core Collection of this config
    #[account(address = config.collection @ ErrorCode::WrongCollection)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: Core Asset, checked to be in the Collection in the handler
    #[account(mut, owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: memo-home PDA, the Collection's Core update authority
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: Metaplex Core
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Mine<'info> {
    /// memo-mint caller; pays for the Vault's MEMO token account on first use
    #[account(mut)]
    pub caller: Signer<'info>,

    #[account(constraint = authority.key() == config.authority @ ErrorCode::Unauthorized)]
    pub authority: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset, checked to be in the Collection in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset
    #[account(seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    /// CHECK: the Vault's MEMO associated token account, created here if missing
    #[account(
        mut,
        address = get_associated_token_address_with_program_id(&vault.key(), &mint.key(), &token_program.key())
            @ ErrorCode::InvalidVaultTokenAccount
    )]
    pub vault_token_account: UncheckedAccount<'info>,

    /// Receives the share (any MEMO token account other than the Vault's)
    #[account(mut, token::mint = mint, token::token_program = token_program)]
    pub recipient_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, address = MEMO_MINT @ ErrorCode::InvalidMint)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// CHECK: checked by memo-mint
    pub mint_authority: UncheckedAccount<'info>,

    /// CHECK: checked by memo-mint
    #[account(mut)]
    pub mint_state: UncheckedAccount<'info>,

    /// CHECK: instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub memo_mint_program: Program<'info, memo_mint::program::MemoMint>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub owner: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset, checked in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset
    #[account(seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    #[account(mut, seeds = [VAULT_STATE_SEED, asset.key().as_ref()], bump = vault_state.bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = vault,
        token::token_program = token_program
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = mint, token::token_program = token_program)]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    pub mint: InterfaceAccount<'info, Mint>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct WithdrawLamports<'info> {
    pub owner: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset, checked in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset (a system account)
    #[account(mut, seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    #[account(mut, seeds = [VAULT_STATE_SEED, asset.key().as_ref()], bump = vault_state.bump)]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: any address other than the Vault may receive the XNT
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseTokenAccount<'info> {
    pub owner: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset, checked in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset
    #[account(seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    #[account(mut, seeds = [VAULT_STATE_SEED, asset.key().as_ref()], bump = vault_state.bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(mut, token::authority = vault, token::token_program = token_program)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: any address may receive the rent
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct WithdrawAsset<'info> {
    pub owner: Signer<'info>,

    /// Pays Core's fees for the transfer
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset whose Vault holds the inner Asset, checked in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset
    #[account(seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    #[account(mut, seeds = [VAULT_STATE_SEED, asset.key().as_ref()], bump = vault_state.bump)]
    pub vault_state: Account<'info, VaultState>,

    /// CHECK: Core Asset owned by the Vault, checked in the handler
    #[account(mut, owner = CORE_PROGRAM_ID)]
    pub inner_asset: UncheckedAccount<'info>,

    /// CHECK: the inner Asset's Collection, or the Core program address when it has none
    pub inner_collection: UncheckedAccount<'info>,

    /// CHECK: any address may receive the inner Asset
    pub new_owner: UncheckedAccount<'info>,

    /// CHECK: Metaplex Core
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Burn<'info> {
    pub owner: Signer<'info>,

    /// Funds the Vault's burn stats account on first use
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [COLLECTION_CONFIG_SEED, config.collection.as_ref()], bump = config.bump)]
    pub config: Account<'info, CollectionConfig>,

    /// CHECK: Core Asset, checked in the handler
    #[account(owner = CORE_PROGRAM_ID)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: Vault PDA of the Asset (signs memo-burn as the user)
    #[account(mut, seeds = [VAULT_SEED, asset.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,

    #[account(mut, seeds = [VAULT_STATE_SEED, asset.key().as_ref()], bump = vault_state.bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: memo-burn UserGlobalBurnStats of the Vault, checked by memo-burn
    #[account(mut)]
    pub vault_burn_stats: UncheckedAccount<'info>,

    /// CHECK: checked by memo-burn
    #[account(mut)]
    pub global_burn_stats: UncheckedAccount<'info>,

    #[account(mut, address = MEMO_MINT @ ErrorCode::InvalidMint)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// CHECK: instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub memo_burn_program: Program<'info, memo_burn::program::MemoBurn>,
    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------------------------
// Account data
// ---------------------------------------------------------------------------------------------

/// Oracle account that Core reads (after the 8-byte discriminator) on every burn of an Asset in
/// a memo-home Collection: OracleValidation::V1 { create, transfer, burn, update }
#[account]
pub struct BurnGuard {
    pub validation: u8,
    pub create: u8,
    pub transfer: u8,
    pub burn: u8,
    pub update: u8,
    pub bump: u8,
}

impl BurnGuard {
    pub const SPACE: usize = 8 + // discriminator
        5 + // OracleValidation::V1
        1; // bump
}

/// Per-Collection settings, fixed at creation
#[account]
pub struct CollectionConfig {
    pub collection: Pubkey,       // Core Collection, PDA [COLLECTION_SEED, authority, seed]
    pub authority: Pubkey,        // may create / update Assets and mine
    pub seed: [u8; 32],           // seed of the Collection address
    pub max_share_bps: u16,       // cap of the mine share
    pub max_assets: Option<u32>,  // supply limit, None = unlimited
    pub creator: Pubkey,          // payer of create_collection
    pub created_at: i64,
    pub bump: u8,
}

impl CollectionConfig {
    pub const SPACE: usize = 8 + // discriminator
        32 + // collection
        32 + // authority
        32 + // seed
        2 +  // max_share_bps
        5 +  // max_assets (Option<u32>)
        32 + // creator
        8 +  // created_at
        1;   // bump
}

/// Per-Asset Vault State, created with the Asset and never closed. `nonce` grows by one on every
/// owner outflow (withdraw, withdraw_lamports, close_token_account, withdraw_asset, burn), so a
/// market or bidder that records it can require at the sale that nothing has left the Vault.
#[account]
pub struct VaultState {
    pub nonce: u64,
    pub bump: u8,
}

impl VaultState {
    pub const SPACE: usize = 8 + // discriminator
        8 + // nonce
        1; // bump
}

// ---------------------------------------------------------------------------------------------
// Events (program logs: hints for indexers, not proof; read balances from chain)
// ---------------------------------------------------------------------------------------------

#[event]
pub struct CollectionCreated {
    pub collection: Pubkey,
    pub authority: Pubkey,
    pub seed: [u8; 32],
    pub max_share_bps: u16,
    pub max_assets: Option<u32>,
    pub creator: Pubkey,
}

#[event]
pub struct CollectionUpdated {
    pub collection: Pubkey,
}

#[event]
pub struct AssetCreated {
    pub collection: Pubkey,
    pub asset: Pubkey,
    pub seed: [u8; 32],
    pub owner: Pubkey,
}

#[event]
pub struct AssetUpdated {
    pub collection: Pubkey,
    pub asset: Pubkey,
}

#[event]
pub struct Mined {
    pub collection: Pubkey,
    pub asset: Pubkey,
    pub recipient_token_account: Pubkey,
    pub reward: u64,
    pub share: u64,
}

#[event]
pub struct Withdrawn {
    pub asset: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub nonce: u64,
}

#[event]
pub struct LamportsWithdrawn {
    pub asset: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub nonce: u64,
}

#[event]
pub struct TokenAccountClosed {
    pub asset: Pubkey,
    pub token_account: Pubkey,
    pub nonce: u64,
}

#[event]
pub struct AssetWithdrawn {
    pub asset: Pubkey,
    pub inner_asset: Pubkey,
    pub new_owner: Pubkey,
    pub nonce: u64,
}

#[event]
pub struct VaultBurned {
    pub asset: Pubkey,
    pub amount: u64,
    pub nonce: u64,
}

// ---------------------------------------------------------------------------------------------
// Read-only helpers (also used by apps through the `cpi` feature)
// ---------------------------------------------------------------------------------------------

/// The fields of a Core AssetV1 that memo-home uses
#[derive(Clone, Debug, PartialEq)]
pub struct CoreAsset {
    pub owner: Pubkey,
    /// Some(collection) when the update authority is `Collection(collection)`
    pub collection: Option<Pubkey>,
    /// Offset of the PluginHeaderV1, if the Asset has plugins
    pub plugin_header: Option<usize>,
}

/// The fields of a Core CollectionV1 that memo-home uses
#[derive(Clone, Debug, PartialEq)]
pub struct CoreCollection {
    pub update_authority: Pubkey,
    pub num_minted: u32,
    pub current_size: u32,
}

fn read_bytes<'a>(d: &'a [u8], at: usize, len: usize) -> Result<&'a [u8]> {
    let end = at.checked_add(len).ok_or(ErrorCode::InvalidCoreAccount)?;
    d.get(at..end).ok_or(error!(ErrorCode::InvalidCoreAccount))
}
fn read_u8(d: &[u8], at: usize) -> Result<u8> { Ok(read_bytes(d, at, 1)?[0]) }
fn read_u32(d: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_bytes(d, at, 4)?.try_into().unwrap()))
}
fn read_u64(d: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(read_bytes(d, at, 8)?.try_into().unwrap()))
}
fn read_pubkey(d: &[u8], at: usize) -> Result<Pubkey> {
    Ok(Pubkey::new_from_array(read_bytes(d, at, 32)?.try_into().unwrap()))
}
fn read_offset(d: &[u8], at: usize) -> Result<usize> {
    usize::try_from(read_u64(d, at)?).map_err(|_| error!(ErrorCode::InvalidCoreAccount))
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(error!(ErrorCode::InvalidCoreAccount))
}
/// Offset just past a Borsh string starting at `at`
fn skip_str(d: &[u8], at: usize) -> Result<usize> {
    let len = read_u32(d, at)? as usize;
    let end = add(add(at, 4)?, len)?;
    require!(end <= d.len(), ErrorCode::InvalidCoreAccount);
    Ok(end)
}

/// Parse a Core AssetV1: key, owner, update authority, name, uri, seq, [plugin header].
/// Trusted bytes only: the caller checks that the account is owned by Core (see `owner_of`).
pub fn parse_asset(d: &[u8]) -> Result<CoreAsset> {
    require!(read_u8(d, 0)? == CORE_KEY_ASSET_V1, ErrorCode::InvalidCoreAccount);
    let owner = read_pubkey(d, 1)?;
    let ua_tag = read_u8(d, 33)?;
    let (collection, mut p) = match ua_tag {
        CORE_UA_NONE => (None, 34),
        CORE_UA_ADDRESS => {
            read_pubkey(d, 34)?;
            (None, 66)
        }
        CORE_UA_COLLECTION => (Some(read_pubkey(d, 34)?), 66),
        _ => return err!(ErrorCode::InvalidCoreAccount),
    };
    p = skip_str(d, p)?; // name
    p = skip_str(d, p)?; // uri
    p = match read_u8(d, p)? {
        0 => add(p, 1)?, // seq: None
        1 => {
            read_u64(d, add(p, 1)?)?; // seq: Some(u64)
            add(p, 9)?
        }
        _ => return err!(ErrorCode::InvalidCoreAccount),
    };
    let plugin_header = if p < d.len() {
        require!(read_u8(d, p)? == CORE_KEY_PLUGIN_HEADER_V1, ErrorCode::InvalidCoreAccount);
        Some(p)
    } else {
        None
    };
    Ok(CoreAsset { owner, collection, plugin_header })
}

/// Parse a Core CollectionV1: key, update authority, name, uri, num_minted, current_size.
/// Trusted bytes only: the caller checks that the account is owned by Core.
pub fn parse_collection(d: &[u8]) -> Result<CoreCollection> {
    require!(read_u8(d, 0)? == CORE_KEY_COLLECTION_V1, ErrorCode::InvalidCoreAccount);
    let update_authority = read_pubkey(d, 1)?;
    let mut p = skip_str(d, 33)?; // name
    p = skip_str(d, p)?; // uri
    Ok(CoreCollection { update_authority, num_minted: read_u32(d, p)?, current_size: read_u32(d, add(p, 4)?)? })
}

/// True when the Asset is (or may be) listed or locked: a Freeze, Transfer or Burn Delegate is
/// held by anyone but the owner, a Freeze Delegate is frozen, a permanent delegate exists, or a
/// plugin type or authority unknown to this program is present (fail closed).
/// Trusted bytes only: the caller checks that the account is owned by Core (see `is_listed`).
pub fn asset_data_is_listed(d: &[u8]) -> Result<bool> {
    let header = match parse_asset(d)?.plugin_header {
        Some(h) => h,
        None => return Ok(false),
    };
    let mut r = read_offset(d, add(header, 1)?)?;
    require!(read_u8(d, r)? == CORE_KEY_PLUGIN_REGISTRY_V1, ErrorCode::InvalidCoreAccount);
    let records = read_u32(d, add(r, 1)?)?;
    r = add(r, 5)?;
    for _ in 0..records {
        let plugin_type = read_u8(d, r)?;
        let auth_tag = read_u8(d, add(r, 1)?)?;
        let auth_len = match auth_tag {
            CORE_AUTHORITY_NONE | CORE_AUTHORITY_OWNER | CORE_AUTHORITY_UPDATE_AUTHORITY => 1,
            CORE_AUTHORITY_ADDRESS => 33,
            _ => return Ok(true),
        };
        let offset = read_offset(d, add(add(r, 1)?, auth_len)?)?;
        r = add(r, 1 + auth_len + 8)?;
        let delegated = auth_tag != CORE_AUTHORITY_OWNER;
        match plugin_type {
            CORE_PLUGIN_FREEZE_DELEGATE => {
                // Plugin::FreezeDelegate { frozen: bool }
                let frozen = read_u8(d, add(offset, 1)?)? != 0;
                if frozen || delegated {
                    return Ok(true);
                }
            }
            CORE_PLUGIN_BURN_DELEGATE | CORE_PLUGIN_TRANSFER_DELEGATE => {
                if delegated {
                    return Ok(true);
                }
            }
            CORE_PLUGIN_PERMANENT_FREEZE_DELEGATE
            | CORE_PLUGIN_PERMANENT_TRANSFER_DELEGATE
            | CORE_PLUGIN_PERMANENT_BURN_DELEGATE => return Ok(true),
            t if t > CORE_PLUGIN_LAST_KNOWN => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

/// Current owner of a Core Asset account
pub fn owner_of(asset: &AccountInfo) -> Result<Pubkey> {
    require_keys_eq!(*asset.owner, CORE_PROGRAM_ID, ErrorCode::InvalidCoreAccount);
    Ok(parse_asset(&asset.try_borrow_data()?)?.owner)
}

/// Collection of a Core Asset account (None if it has none)
pub fn collection_of(asset: &AccountInfo) -> Result<Option<Pubkey>> {
    require_keys_eq!(*asset.owner, CORE_PROGRAM_ID, ErrorCode::InvalidCoreAccount);
    Ok(parse_asset(&asset.try_borrow_data()?)?.collection)
}

/// True when the account is a Core Asset in the Collection of this memo-home Collection Config.
/// The config must be an `Account` (owned by memo-home, so created by memo-home).
pub fn is_home_asset(asset: &AccountInfo, config: &Account<CollectionConfig>) -> bool {
    matches!(collection_of(asset), Ok(Some(c)) if c == config.collection)
}

/// True when the Asset is listed or locked (see `asset_data_is_listed`)
pub fn is_listed(asset: &AccountInfo) -> Result<bool> {
    require_keys_eq!(*asset.owner, CORE_PROGRAM_ID, ErrorCode::InvalidCoreAccount);
    asset_data_is_listed(&asset.try_borrow_data()?)
}

/// The gate of every owner outflow, for apps that act on behalf of an Asset's owner: a Core
/// Asset in this memo-home Collection, owned by `signer` (who must have signed), not listed
pub fn require_owner_not_listed(asset: &AccountInfo, config: &Account<CollectionConfig>, signer: &Pubkey) -> Result<()> {
    require_keys_eq!(*asset.owner, CORE_PROGRAM_ID, ErrorCode::InvalidCoreAccount);
    let data = asset.try_borrow_data()?;
    let parsed = parse_asset(&data)?;
    require!(parsed.collection == Some(config.collection), ErrorCode::WrongCollection);
    require_keys_eq!(parsed.owner, *signer, ErrorCode::NotOwner);
    require!(!asset_data_is_listed(&data)?, ErrorCode::Listed);
    Ok(())
}

/// Outflow count of an Asset's Vault, read from its Vault State account (checked to belong to
/// the Asset). A market records it at listing and requires it unchanged at the sale; nested
/// Assets in the Vault have their own Vault States.
pub fn vault_nonce_of(vault_state: &AccountInfo, asset: &Pubkey) -> Result<u64> {
    require_keys_eq!(*vault_state.owner, crate::ID, ErrorCode::InvalidVaultState);
    let state = VaultState::try_deserialize(&mut &vault_state.try_borrow_data()?[..])?;
    let expected = Pubkey::create_program_address(&[VAULT_STATE_SEED, asset.as_ref(), &[state.bump]], &crate::ID)
        .map_err(|_| error!(ErrorCode::InvalidVaultState))?;
    require_keys_eq!(vault_state.key(), expected, ErrorCode::InvalidVaultState);
    Ok(state.nonce)
}

/// Collection PDA of an Authority and seed, and its bump
pub fn collection_address(authority: &Pubkey, seed: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[COLLECTION_SEED, authority.as_ref(), seed], &crate::ID)
}

/// Asset PDA of a Collection and seed, and its bump
pub fn asset_address(collection: &Pubkey, seed: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ASSET_SEED, collection.as_ref(), seed], &crate::ID)
}

/// Vault PDA of an Asset and its bump. Send XNT here, never to the Asset address itself (Core
/// lets anyone collect the lamports above rent of a Core account).
pub fn vault_of(asset: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED, asset.as_ref()], &crate::ID)
}

/// Vault State PDA of an Asset and its bump
pub fn vault_state_of(asset: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_STATE_SEED, asset.as_ref()], &crate::ID)
}

/// The Vault's associated token account for a mint (MEMO: `MEMO_MINT` with Token-2022)
pub fn vault_token_account(asset: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(&vault_of(asset).0, mint, token_program)
}

/// Collection Config PDA of a Collection
pub fn collection_config_of(collection: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[COLLECTION_CONFIG_SEED, collection.as_ref()], &crate::ID).0
}

/// `reward × share_bps / 10_000`, rounded down
pub fn share_of(reward: u64, share_bps: u16) -> Result<u64> {
    require!(share_bps <= MAX_BPS, ErrorCode::InvalidShareBps);
    let share = (reward as u128)
        .checked_mul(share_bps as u128)
        .ok_or(ErrorCode::ArithmeticOverflow)?
        / MAX_BPS as u128;
    u64::try_from(share).map_err(|_| error!(ErrorCode::ArithmeticOverflow))
}

/// The supply limit counts every Asset ever created (Core's num_minted, which never decreases),
/// not the current size
fn check_max_assets(collection: &CoreCollection, max_assets: Option<u32>) -> Result<()> {
    if let Some(max) = max_assets {
        require!(collection.num_minted < max, ErrorCode::MaxAssetsReached);
    }
    Ok(())
}

fn require_name_uri(name: Option<&str>, uri: Option<&str>) -> Result<()> {
    require!(name.map_or(0, str::len) <= MAX_NAME_LEN, ErrorCode::NameTooLong);
    require!(uri.map_or(0, str::len) <= MAX_URI_LEN, ErrorCode::UriTooLong);
    Ok(())
}

fn require_home_asset(asset: &AccountInfo, config: &Account<CollectionConfig>) -> Result<()> {
    require!(is_home_asset(asset, config), ErrorCode::WrongCollection);
    Ok(())
}

fn bump_nonce(state: &mut Account<VaultState>) -> Result<u64> {
    state.nonce = state.nonce.checked_add(1).ok_or(ErrorCode::ArithmeticOverflow)?;
    Ok(state.nonce)
}

/// Core creates Assets and Collections with System create_account, which fails when the address
/// already holds lamports, and anyone can send lamports to a PDA in advance. Move them to the
/// payer first, signed by the PDA (nobody else can have assigned or allocated it).
fn sweep_prefunded<'info>(
    account: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    seeds: &[&[u8]],
) -> Result<()> {
    let lamports = account.lamports();
    if lamports == 0 {
        return Ok(());
    }
    require!(
        *account.owner == anchor_lang::system_program::ID && account.data_is_empty(),
        ErrorCode::AddressInUse
    );
    anchor_lang::system_program::transfer(
        CpiContext::new_with_signer(
            system_program.clone(),
            anchor_lang::system_program::Transfer { from: account.clone(), to: payer.clone() },
            &[seeds],
        ),
        lamports,
    )
}

/// Append remaining accounts to a CPI as non-signers (writable as passed), so they never receive
/// a PDA signature from memo-home
fn forward_remaining<'info>(ix: &mut Instruction, infos: &mut Vec<AccountInfo<'info>>, remaining: &[AccountInfo<'info>]) {
    for a in remaining {
        ix.accounts.push(if a.is_writable {
            AccountMeta::new(a.key(), false)
        } else {
            AccountMeta::new_readonly(a.key(), false)
        });
        infos.push(a.clone());
    }
}

/// Amount of a token account (SPL Token and Token-2022 share the base layout)
fn token_amount(account: &AccountInfo) -> Result<u64> {
    let data = account.try_borrow_data()?;
    let acct = TokenAccount::try_deserialize(&mut &data[..])?;
    Ok(acct.amount)
}

// ---------------------------------------------------------------------------------------------
// Core instruction data
// ---------------------------------------------------------------------------------------------

fn borsh_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn borsh_opt_str(out: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(s) => {
            out.push(1);
            borsh_str(out, s);
        }
        None => out.push(0),
    }
}

/// CreateV1 { data_state: AccountState, name, uri, plugins: None }
pub(crate) fn encode_create_v1(name: &str, uri: &str) -> Vec<u8> {
    let mut d = vec![CORE_IX_CREATE_V1, CORE_DATA_STATE_ACCOUNT];
    borsh_str(&mut d, name);
    borsh_str(&mut d, uri);
    d.push(0);
    d
}

/// CreateCollectionV2 { name, uri, plugins, external_plugin_adapters: [Burn Guard Oracle] }
///
/// plugins (each with plugin authority None, so nobody can change them):
///   MasterEdition { max_supply: Some(max_assets), name: None, uri: None } when max_assets is set
///   Royalties { basis_points, creators, rule_set: None } when royalty is set
/// Oracle: base_address = Burn Guard, plugin authority None, lifecycle check Burn → can_reject,
///   no base address config, results offset Anchor (after the discriminator)
pub(crate) fn encode_create_collection_v2(
    name: &str,
    uri: &str,
    burn_guard: &Pubkey,
    max_assets: Option<u32>,
    royalty: Option<&RoyaltyArgs>,
) -> Vec<u8> {
    let mut d = vec![CORE_IX_CREATE_COLLECTION_V2];
    borsh_str(&mut d, name);
    borsh_str(&mut d, uri);

    let mut plugins: Vec<Vec<u8>> = vec![];
    if let Some(max) = max_assets {
        let mut p = vec![CORE_PLUGIN_MASTER_EDITION, 1];
        p.extend_from_slice(&max.to_le_bytes());
        p.extend_from_slice(&[0, 0]); // name: None, uri: None
        p.extend_from_slice(&[1, CORE_AUTHORITY_NONE]);
        plugins.push(p);
    }
    if let Some(r) = royalty {
        let mut p = vec![CORE_PLUGIN_ROYALTIES];
        p.extend_from_slice(&r.basis_points.to_le_bytes());
        p.extend_from_slice(&(r.creators.len() as u32).to_le_bytes());
        for c in &r.creators {
            p.extend_from_slice(c.address.as_ref());
            p.push(c.percentage);
        }
        p.push(CORE_RULE_SET_NONE);
        p.extend_from_slice(&[1, CORE_AUTHORITY_NONE]);
        plugins.push(p);
    }
    if plugins.is_empty() {
        d.push(0);
    } else {
        d.push(1);
        d.extend_from_slice(&(plugins.len() as u32).to_le_bytes());
        for p in plugins {
            d.extend_from_slice(&p);
        }
    }

    d.push(1); // external_plugin_adapters: Some
    d.extend_from_slice(&1u32.to_le_bytes());
    d.push(CORE_EXTERNAL_ORACLE);
    d.extend_from_slice(burn_guard.as_ref());
    d.extend_from_slice(&[1, CORE_AUTHORITY_NONE]); // init_plugin_authority: Some(None)
    d.extend_from_slice(&1u32.to_le_bytes()); // lifecycle_checks
    d.push(CORE_LIFECYCLE_BURN);
    d.extend_from_slice(&CORE_CHECK_CAN_REJECT.to_le_bytes());
    d.push(0); // base_address_config: None
    d.extend_from_slice(&[1, CORE_RESULTS_OFFSET_ANCHOR]);
    d
}

#[error_code]
pub enum ErrorCode {
    #[msg("max_share_bps or share_bps above 10000")]
    InvalidShareBps,
    #[msg("max_assets must be at least 1")]
    InvalidMaxAssets,
    #[msg("Royalty: basis points above 10000, 1 to 5 distinct creators, percentages adding up to 100")]
    InvalidRoyalty,
    #[msg("Signer is not the Collection's Authority")]
    Unauthorized,
    #[msg("Account is not the expected Collection, or the Asset is not in it")]
    WrongCollection,
    #[msg("Collection is not managed by memo-home")]
    NotHomeCollection,
    #[msg("The Collection has reached max_assets")]
    MaxAssetsReached,
    #[msg("Nothing to update")]
    NothingToUpdate,
    #[msg("share_bps above the Collection's max_share_bps")]
    ShareTooHigh,
    #[msg("Not a valid Metaplex Core account")]
    InvalidCoreAccount,
    #[msg("Signer is not the current owner of the Asset")]
    NotOwner,
    #[msg("Asset is listed or locked (delegated or frozen)")]
    Listed,
    #[msg("The inner Asset is not owned by this Vault")]
    NotInVault,
    #[msg("Not the Vault's associated token account")]
    InvalidVaultTokenAccount,
    #[msg("Not the MEMO mint")]
    InvalidMint,
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
    #[msg("The Vault holds too few lamports, or would be left below the rent-exempt minimum")]
    InsufficientVaultLamports,
    #[msg("Name longer than 64 bytes")]
    NameTooLong,
    #[msg("URI longer than 256 bytes")]
    UriTooLong,
    #[msg("The Core program cannot own an Asset")]
    InvalidOwner,
    #[msg("The Collection or Asset address is already in use")]
    AddressInUse,
    #[msg("The Vault's MEMO token account cannot be closed")]
    MemoAccountNotClosable,
    #[msg("Token account is not empty")]
    TokenAccountNotEmpty,
    #[msg("Destination is the source account")]
    InvalidDestination,
    #[msg("Not the Vault State of this Asset")]
    InvalidVaultState,
}

#[cfg(test)]
mod tests;
