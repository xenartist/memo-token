#![allow(unexpected_cfgs)]

//! memo-id: globally unique IDs such as `xen-artist.memo`, owned as memo-home NFTs (see
//! MEMO-ID-DESIGN.md).
//!
//! A registered MEMO ID is a Core Asset in the MEMO ID Collection, whose Authority is this
//! program's PDA, plus a Registry account at [b"memo_id", sha256(memo_id)]. Whoever controls the
//! Asset controls the ID: they point it at a wallet or an Asset (forward record), create Record
//! SubIds under it, and issue NFT SubIds (one way). The Target confirms the link with a Primary
//! ID (reverse record); apps show only IDs whose forward and reverse records match.
//! There is no admin key and no fee beyond the MEMO burned through memo-burn.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked, ID as INSTRUCTIONS_ID,
};
use anchor_spl::token_2022::Token2022;
use anchor_spl::token_interface::{Mint, TokenAccount};
use base64::{engine::general_purpose, Engine as _};
use solana_sha256_hasher::hash;
use spl_memo::ID as MEMO_PROGRAM_ID;

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("Gji1DjBFybT3NWvuToyrvQkRHhzvRmkqWeHDsqxWRenp");

#[cfg(not(feature = "mainnet"))]
declare_id!("Gji1DjBFybT3NWvuToyrvQkRHhzvRmkqWeHDsqxWRenp");

/// The MEMO mint (the one memo-burn accepts)
pub const MEMO_MINT: Pubkey = memo_burn::AUTHORIZED_MINT_PUBKEY;

// memo-id must be built for the same network as the memo-home, memo-mint and memo-burn it calls
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
    same_key(&memo_home::ID_CONST, &pubkey!("ArrrsGh5biKJYpiKMYJ6FD3AxCY7Fhd9Gv761ZCymzzs"))
        && same_key(&memo_mint::ID_CONST, &pubkey!("8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX"))
        && same_key(&memo_burn::ID_CONST, &pubkey!("2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh"))
        && same_key(&MEMO_MINT, &pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick")),
    "memo-id/mainnet needs memo-home/mainnet, memo-mint/mainnet and memo-burn/mainnet"
);
#[cfg(not(feature = "mainnet"))]
const _: () = assert!(
    same_key(&memo_home::ID_CONST, &pubkey!("J1pxwy8V4xKvPgS5HMUn5jFo3u7At3MW4BvFWKg48Qcv"))
        && same_key(&memo_mint::ID_CONST, &pubkey!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy"))
        && same_key(&memo_burn::ID_CONST, &pubkey!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP"))
        && same_key(&MEMO_MINT, &pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")),
    "memo-home, memo-mint or memo-burn is built for mainnet: enable memo-id/mainnet too"
);

// PDA seeds
pub const AUTHORITY_SEED: &[u8] = b"authority";
pub const MEMO_ID_SEED: &[u8] = b"memo_id";
pub const SUBID_SEED: &[u8] = b"subid";
pub const PRIMARY_ID_SEED: &[u8] = b"primary_id";

/// The v1 suffix; the MEMO ID Collection's seed is sha256 of it
pub const SUFFIX: &str = "memo";
pub const COLLECTION_NAME: &str = "MEMO ID";
/// URI of the MEMO ID Collection, fixed at `init` (no admin can change it later): the shared
/// MEMO ID metadata JSON on Arweave. Must be set before the mainnet deployment.
pub const COLLECTION_URI: &str = "";
#[cfg(feature = "mainnet")]
const _: () = assert!(!COLLECTION_URI.is_empty(), "set COLLECTION_URI before building for mainnet");

/// Label: 1-63 bytes; full ID: at most 253 bytes (the DNS hostname limits). The Asset is named
/// after the full `memo_id`, so memo-home must accept names that long.
pub const MAX_LABEL_LEN: usize = 63;
pub const MAX_MEMO_ID_LEN: usize = 253;
const _: () = assert!(MAX_MEMO_ID_LEN <= memo_home::MAX_NAME_LEN, "memo-home must accept a full MEMO ID as an Asset name");
pub const MAX_URI_LEN: usize = memo_home::MAX_URI_LEN;
pub const URI_SCHEMES: [&str; 3] = ["https://", "ar://", "ipfs://"];

/// Prices in centi-mints (1/100 of the current memo-mint reward), by label length (MEMO-ID-DESIGN.md §4)
pub const PRICE_CENTI_MINTS_1: u64 = 42_069_000; // 420,690 mints
pub const PRICE_CENTI_MINTS_2: u64 = 4_206_900; // 42,069 mints
pub const PRICE_CENTI_MINTS_3: u64 = 420_690; // 4,206.9 mints
pub const PRICE_CENTI_MINTS_4: u64 = 42_069; // 420.69 mints
pub const PRICE_CENTI_MINTS_5_PLUS: u64 = 6_900; // 69 mints
pub const PRICE_CENTI_MINTS_ISSUE_SUBID: u64 = 6_900; // 69 mints

// Memo (BurnMemo at instruction index 0, Base64 Borsh, as memo-burn requires)
pub const MEMO_MIN_LENGTH: usize = 69;
pub const BURN_MEMO_VERSION: u8 = 1;
pub const PAYLOAD_VERSION: u8 = 1;
pub const EXPECTED_CATEGORY: &str = "memo_id";
pub const OPERATION_REGISTER: &str = "register";
pub const OPERATION_ISSUE_SUBID: &str = "issue_subid";

#[program]
pub mod memo_id {
    use super::*;

    /// Create the MEMO ID Collection once (anyone may call it; memo-home rejects a second call).
    /// The memo-id Authority PDA signs memo-home's create_collection: no share, no supply limit,
    /// no royalties.
    pub fn init(ctx: Context<Init>) -> Result<()> {
        let a = &ctx.accounts;
        memo_home::cpi::create_collection(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::CreateCollection {
                    payer: a.payer.to_account_info(),
                    authority: a.authority.to_account_info(),
                    collection: a.collection.to_account_info(),
                    program_authority: a.program_authority.to_account_info(),
                    burn_guard: a.burn_guard.to_account_info(),
                    config: a.config.to_account_info(),
                    core_program: a.core_program.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[AUTHORITY_SEED, &[ctx.bumps.authority]]],
            ),
            collection_seed(),
            COLLECTION_NAME.into(),
            COLLECTION_URI.into(),
            0,
            None,
            None,
        )?;
        emit!(Initialized { collection: a.collection.key() });
        Ok(())
    }

    /// Register `<label>.memo` for the payer: burns the price for the label's length through
    /// memo-burn (BurnMemo at index 0) and creates the Registry and the Asset (owner = payer,
    /// name = the MEMO ID). `memo_id_hash` is sha256 of the full ID, for the Registry address.
    pub fn register(ctx: Context<Register>, memo_id_hash: [u8; 32], label: String, uri: String) -> Result<()> {
        require!(valid_label(&label), ErrorCode::InvalidLabel);
        let memo_id = format!("{}.{}", label, SUFFIX);
        require!(memo_id.len() <= MAX_MEMO_ID_LEN, ErrorCode::MemoIdTooLong);
        require!(hash(memo_id.as_bytes()).to_bytes() == memo_id_hash, ErrorCode::HashMismatch);
        require_uri(&uri)?;

        let price = burn_price(price_centi_mints(label.len()), ctx.accounts.burn.mint.supply)?;
        let payer = ctx.accounts.payer.key();
        require_burn_memo(&ctx.accounts.instructions, price, OPERATION_REGISTER, &payer, &memo_id)?;
        burn(&ctx.accounts.burn, &ctx.accounts.payer, &ctx.accounts.instructions, price)?;
        let a = &ctx.accounts;
        create_asset(&a.home, &a.payer, &a.system_program, ctx.bumps.home.authority, &a.payer.to_account_info(), &memo_id, uri)?;

        let registry = &mut ctx.accounts.registry;
        registry.kind = MemoIdKind::Nft;
        registry.asset = ctx.accounts.home.asset.key();
        registry.parent = Pubkey::default();
        registry.created_at = Clock::get()?.unix_timestamp;
        registry.nft_subids_issued = 0;
        registry.record_subids = 0;
        registry.bump = ctx.bumps.registry;
        registry.target_set_by = Pubkey::default();
        registry.target = None;
        registry.memo_id = memo_id;

        emit!(Registered {
            memo_id: registry.memo_id.clone(),
            registry: registry.key(),
            asset: registry.asset,
            owner: payer,
            burned: price,
        });
        Ok(())
    }

    /// Create the Record SubId `<label>.<parent>` (parent's controller only; the parent must be
    /// an NFT ID). It optionally points at `target` right away. Rent only, refunded on delete.
    pub fn create_subid(
        ctx: Context<CreateSubid>,
        memo_id_hash: [u8; 32],
        label: String,
        target: Option<Target>,
    ) -> Result<()> {
        require!(valid_label(&label), ErrorCode::InvalidLabel);
        let parent = &mut ctx.accounts.parent;
        require!(parent.kind == MemoIdKind::Nft, ErrorCode::ParentNotNft);
        let controller = require_nft_controller(
            parent,
            &ctx.accounts.parent_asset,
            ctx.accounts.holder.as_deref(),
            &ctx.accounts.controller.key(),
        )?;
        let memo_id = format!("{}.{}", label, parent.memo_id);
        require!(memo_id.len() <= MAX_MEMO_ID_LEN, ErrorCode::MemoIdTooLong);
        require!(hash(memo_id.as_bytes()).to_bytes() == memo_id_hash, ErrorCode::HashMismatch);
        parent.record_subids = parent.record_subids.checked_add(1).ok_or(ErrorCode::ArithmeticOverflow)?;

        let registry = &mut ctx.accounts.registry;
        registry.kind = MemoIdKind::Record;
        registry.asset = Pubkey::default();
        registry.parent = parent.key();
        registry.created_at = Clock::get()?.unix_timestamp;
        registry.nft_subids_issued = 0;
        registry.record_subids = 0;
        registry.bump = ctx.bumps.registry;
        registry.target_set_by = if target.is_some() { controller } else { Pubkey::default() };
        registry.target = target;
        registry.memo_id = memo_id;

        emit!(SubidCreated { memo_id: registry.memo_id.clone(), registry: registry.key(), parent: parent.key() });
        if let Some(t) = target {
            emit!(TargetSet { registry: registry.key(), target: t, set_by: controller });
        }
        Ok(())
    }

    /// Delete a Record SubId (parent's controller only); its rent goes to the controller
    pub fn delete_subid(ctx: Context<DeleteSubid>) -> Result<()> {
        let parent = &mut ctx.accounts.parent;
        require_nft_controller(
            parent,
            &ctx.accounts.parent_asset,
            ctx.accounts.holder.as_deref(),
            &ctx.accounts.controller.key(),
        )?;
        parent.record_subids = parent.record_subids.saturating_sub(1);
        emit!(SubidDeleted { memo_id: ctx.accounts.registry.memo_id.clone(), registry: ctx.accounts.registry.key() });
        Ok(())
    }

    /// Turn a Record SubId into an NFT owned by `new_owner` (parent's controller only; one way):
    /// burns 69 mints (BurnMemo at index 0, paid by the payer), creates the Asset and the SubId
    /// Index entry `index`, which must be the parent's `nft_subids_issued`.
    pub fn issue_subid(ctx: Context<IssueSubid>, index: u32, uri: String) -> Result<()> {
        require_uri(&uri)?;
        let parent = &mut ctx.accounts.parent;
        require!(index == parent.nft_subids_issued, ErrorCode::WrongSubidIndex);
        require_nft_controller(
            parent,
            &ctx.accounts.parent_asset,
            ctx.accounts.holder.as_deref(),
            &ctx.accounts.controller.key(),
        )?;
        parent.nft_subids_issued = parent.nft_subids_issued.checked_add(1).ok_or(ErrorCode::ArithmeticOverflow)?;
        parent.record_subids = parent.record_subids.saturating_sub(1);
        let parent_key = parent.key();

        let memo_id = ctx.accounts.registry.memo_id.clone();
        let price = burn_price(PRICE_CENTI_MINTS_ISSUE_SUBID, ctx.accounts.burn.mint.supply)?;
        let payer = ctx.accounts.payer.key();
        require_burn_memo(&ctx.accounts.instructions, price, OPERATION_ISSUE_SUBID, &payer, &memo_id)?;
        burn(&ctx.accounts.burn, &ctx.accounts.payer, &ctx.accounts.instructions, price)?;
        let a = &ctx.accounts;
        create_asset(&a.home, &a.payer, &a.system_program, ctx.bumps.home.authority, &a.new_owner, &memo_id, uri)?;

        let registry = &mut ctx.accounts.registry;
        registry.kind = MemoIdKind::Nft;
        registry.asset = ctx.accounts.home.asset.key();

        let entry = &mut ctx.accounts.subid_index;
        entry.registry = registry.key();
        entry.bump = ctx.bumps.subid_index;

        emit!(SubidIssued {
            memo_id,
            registry: registry.key(),
            parent: parent_key,
            index,
            asset: registry.asset,
            owner: ctx.accounts.new_owner.key(),
            burned: price,
        });
        Ok(())
    }

    /// Point a MEMO ID at a wallet or an Asset (its controller only). The record is valid only
    /// while the signer stays the controller.
    pub fn set_target(mut ctx: Context<SetTarget>, target: Target) -> Result<()> {
        let a = &mut ctx.accounts;
        let controller = require_controller(&a.registry, a.parent.as_ref(), &a.asset, a.holder.as_deref(), &a.controller.key())?;
        a.registry.target = Some(target);
        a.registry.target_set_by = controller;
        emit!(TargetSet { registry: a.registry.key(), target, set_by: controller });
        Ok(())
    }

    /// Remove the forward record (controller only)
    pub fn clear_target(mut ctx: Context<SetTarget>) -> Result<()> {
        let a = &mut ctx.accounts;
        require_controller(&a.registry, a.parent.as_ref(), &a.asset, a.holder.as_deref(), &a.controller.key())?;
        a.registry.target = None;
        a.registry.target_set_by = Pubkey::default();
        emit!(TargetCleared { registry: a.registry.key() });
        Ok(())
    }

    /// Choose the MEMO ID shown for `target` (the Target's controller only: the address itself,
    /// or the controller of the Asset). Creates or replaces the Primary ID.
    pub fn set_primary(mut ctx: Context<SetPrimary>, target: Target) -> Result<()> {
        let a = &mut ctx.accounts;
        let signer = a.signer.key();
        require_target_controller(&target, a.target_asset.as_deref(), a.holder.as_deref(), &signer)?;
        let primary = &mut a.primary;
        primary.target = target;
        primary.memo_id_registry = a.registry.key();
        primary.set_by = signer;
        primary.bump = ctx.bumps.primary;
        emit!(PrimarySet { target, registry: primary.memo_id_registry, set_by: signer });
        Ok(())
    }

    /// Remove the Primary ID (the Target's current controller only); its rent goes to the signer
    pub fn clear_primary(ctx: Context<ClearPrimary>) -> Result<()> {
        let a = &ctx.accounts;
        let target = a.primary.target;
        require_target_controller(&target, a.target_asset.as_deref(), a.holder.as_deref(), &a.signer.key())?;
        emit!(PrimaryCleared { target });
        Ok(())
    }

    /// Change the Asset's metadata URI (controller only); the name is always the MEMO ID
    pub fn update_uri(ctx: Context<UpdateUri>, uri: String) -> Result<()> {
        require_uri(&uri)?;
        let a = &ctx.accounts;
        require!(a.registry.kind == MemoIdKind::Nft, ErrorCode::NotNft);
        require_nft_controller(&a.registry, &a.asset, a.holder.as_deref(), &a.controller.key())?;
        memo_home::cpi::update_asset(
            CpiContext::new_with_signer(
                a.memo_home_program.to_account_info(),
                memo_home::cpi::accounts::UpdateAsset {
                    payer: a.payer.to_account_info(),
                    authority: a.authority.to_account_info(),
                    config: a.config.to_account_info(),
                    collection: a.collection.to_account_info(),
                    asset: a.asset.to_account_info(),
                    program_authority: a.program_authority.to_account_info(),
                    core_program: a.core_program.to_account_info(),
                    system_program: a.system_program.to_account_info(),
                },
                &[&[AUTHORITY_SEED, &[ctx.bumps.authority]]],
            ),
            None,
            Some(uri),
        )?;
        emit!(UriUpdated { registry: a.registry.key(), asset: a.asset.key() });
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// What a MEMO ID points to. The kind is part of the record, so a wallet and an Asset with the
/// same address are never confused: an `Address` is confirmed by its own signature, an `Asset`
/// by its controller.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Address(Pubkey),
    Asset(Pubkey),
}

impl Target {
    pub fn key(&self) -> &Pubkey {
        match self {
            Target::Address(k) | Target::Asset(k) => k,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoIdKind {
    /// Backed by its own Asset in the MEMO ID Collection
    Nft,
    /// A Record SubId, controlled by its parent's controller
    Record,
}

/// Payload of the BurnMemo for `register` and `issue_subid` (Borsh, inside the Base64 memo)
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct MemoIdPayload {
    pub version: u8,
    /// "memo_id"
    pub category: String,
    /// "register" or "issue_subid"
    pub operation: String,
    /// The payer (base58), who burns the MEMO
    pub user_pubkey: String,
    /// The full MEMO ID being registered or issued
    pub memo_id: String,
}

impl MemoIdPayload {
    pub fn validate(&self, operation: &str, payer: &Pubkey, memo_id: &str) -> Result<()> {
        require!(self.version == PAYLOAD_VERSION, ErrorCode::UnsupportedPayloadVersion);
        require!(self.category == EXPECTED_CATEGORY, ErrorCode::InvalidCategory);
        require!(self.operation == operation, ErrorCode::InvalidOperation);
        require!(self.user_pubkey == payer.to_string(), ErrorCode::UserPubkeyMismatch);
        require!(self.memo_id == memo_id, ErrorCode::MemoIdMismatch);
        Ok(())
    }
}

/// BurnMemo as memo-burn reads it
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BurnMemo {
    pub version: u8,
    pub burn_amount: u64,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------------------------

/// memo-home accounts for creating or updating an Asset in the MEMO ID Collection
#[derive(Accounts)]
pub struct HomeAccounts<'info> {
    /// CHECK: memo-id PDA, Authority of the MEMO ID Collection (signs memo-home CPIs)
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: memo-home Collection Config of the MEMO ID Collection, checked by memo-home
    pub config: UncheckedAccount<'info>,

    /// CHECK: the MEMO ID Collection
    #[account(mut, address = memo_id_collection() @ ErrorCode::WrongCollection)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: the new Asset at memo-home asset_address(collection, sha256(memo_id)), checked by memo-home
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: the new Asset's Vault State, created by memo-home
    #[account(mut)]
    pub vault_state: UncheckedAccount<'info>,

    /// CHECK: memo-home PDA, checked by memo-home
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: Metaplex Core, checked by memo-home
    pub core_program: UncheckedAccount<'info>,

    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
}

/// memo-burn accounts for burning the payer's MEMO
#[derive(Accounts)]
pub struct BurnAccounts<'info> {
    #[account(mut, address = MEMO_MINT @ ErrorCode::InvalidMint)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// The payer's MEMO token account (checked by memo-burn)
    #[account(mut)]
    pub payer_token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: the payer's memo-burn UserGlobalBurnStats, checked by memo-burn
    #[account(mut)]
    pub payer_burn_stats: UncheckedAccount<'info>,

    /// CHECK: memo-burn GlobalBurnStats, checked by memo-burn
    #[account(mut)]
    pub global_burn_stats: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub memo_burn_program: Program<'info, memo_burn::program::MemoBurn>,
}

#[derive(Accounts)]
pub struct Init<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: memo-id PDA, Authority of the MEMO ID Collection
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: the MEMO ID Collection, created by memo-home
    #[account(mut, address = memo_id_collection() @ ErrorCode::WrongCollection)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub burn_guard: UncheckedAccount<'info>,

    /// CHECK: the Collection Config, created by memo-home
    #[account(mut)]
    pub config: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub core_program: UncheckedAccount<'info>,

    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(memo_id_hash: [u8; 32], label: String)]
pub struct Register<'info> {
    /// Pays the rent and burns the MEMO; owns the new Asset
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = Registry::space_for(label.len() + 1 + SUFFIX.len()),
        seeds = [MEMO_ID_SEED, memo_id_hash.as_ref()],
        bump
    )]
    pub registry: Account<'info, Registry>,

    pub home: HomeAccounts<'info>,
    pub burn: BurnAccounts<'info>,

    /// CHECK: instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(memo_id_hash: [u8; 32], label: String)]
pub struct CreateSubid<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The parent's controller
    pub controller: Signer<'info>,

    #[account(mut)]
    pub parent: Account<'info, Registry>,

    /// CHECK: the parent's Asset, checked in the handler
    pub parent_asset: UncheckedAccount<'info>,

    /// CHECK: the memo-home Asset whose Vault holds the parent's Asset, if it is held (checked in the handler)
    pub holder: Option<UncheckedAccount<'info>>,

    #[account(
        init,
        payer = payer,
        space = Registry::space_for(label.len() + 1 + parent.memo_id.len()),
        seeds = [MEMO_ID_SEED, memo_id_hash.as_ref()],
        bump
    )]
    pub registry: Account<'info, Registry>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DeleteSubid<'info> {
    /// The parent's controller; receives the rent
    #[account(mut)]
    pub controller: Signer<'info>,

    #[account(mut)]
    pub parent: Account<'info, Registry>,

    /// CHECK: the parent's Asset, checked in the handler
    pub parent_asset: UncheckedAccount<'info>,

    /// CHECK: see CreateSubid
    pub holder: Option<UncheckedAccount<'info>>,

    #[account(
        mut,
        close = controller,
        constraint = registry.kind == MemoIdKind::Record @ ErrorCode::NotRecord,
        constraint = registry.parent == parent.key() @ ErrorCode::WrongParent
    )]
    pub registry: Account<'info, Registry>,
}

#[derive(Accounts)]
#[instruction(index: u32)]
pub struct IssueSubid<'info> {
    /// Pays the rent and burns the MEMO
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The parent's controller
    pub controller: Signer<'info>,

    #[account(mut)]
    pub parent: Account<'info, Registry>,

    /// CHECK: the parent's Asset, checked in the handler
    pub parent_asset: UncheckedAccount<'info>,

    /// CHECK: see CreateSubid
    pub holder: Option<UncheckedAccount<'info>>,

    #[account(
        mut,
        constraint = registry.kind == MemoIdKind::Record @ ErrorCode::NotRecord,
        constraint = registry.parent == parent.key() @ ErrorCode::WrongParent
    )]
    pub registry: Account<'info, Registry>,

    #[account(
        init,
        payer = payer,
        space = SubidIndex::SPACE,
        seeds = [SUBID_SEED, parent.key().as_ref(), &index.to_le_bytes()],
        bump
    )]
    pub subid_index: Account<'info, SubidIndex>,

    /// CHECK: any address except Core's may own the new Asset (checked by memo-home)
    pub new_owner: UncheckedAccount<'info>,

    pub home: HomeAccounts<'info>,
    pub burn: BurnAccounts<'info>,

    /// CHECK: instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetTarget<'info> {
    /// The MEMO ID's controller
    pub controller: Signer<'info>,

    #[account(mut)]
    pub registry: Account<'info, Registry>,

    /// The parent, when `registry` is a Record SubId
    pub parent: Option<Account<'info, Registry>>,

    /// CHECK: the Asset that decides the controller (the ID's own, or its parent's for a Record
    /// SubId), checked in the handler
    pub asset: UncheckedAccount<'info>,

    /// CHECK: see CreateSubid
    pub holder: Option<UncheckedAccount<'info>>,
}

#[derive(Accounts)]
#[instruction(target: Target)]
pub struct SetPrimary<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The Target's controller
    pub signer: Signer<'info>,

    /// The MEMO ID to show for the Target
    pub registry: Account<'info, Registry>,

    #[account(
        init_if_needed,
        payer = payer,
        space = PrimaryId::SPACE,
        seeds = [PRIMARY_ID_SEED, target.key().as_ref()],
        bump
    )]
    pub primary: Account<'info, PrimaryId>,

    /// CHECK: the Target Asset, for an `Asset` target (checked in the handler)
    pub target_asset: Option<UncheckedAccount<'info>>,

    /// CHECK: the memo-home Asset whose Vault holds the Target Asset, if it is held
    pub holder: Option<UncheckedAccount<'info>>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClearPrimary<'info> {
    /// The Target's controller; receives the rent
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(mut, close = signer)]
    pub primary: Account<'info, PrimaryId>,

    /// CHECK: see SetPrimary
    pub target_asset: Option<UncheckedAccount<'info>>,

    /// CHECK: see SetPrimary
    pub holder: Option<UncheckedAccount<'info>>,
}

#[derive(Accounts)]
pub struct UpdateUri<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The MEMO ID's controller
    pub controller: Signer<'info>,

    pub registry: Account<'info, Registry>,

    /// CHECK: the MEMO ID's Asset (checked in the handler and by memo-home)
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: see CreateSubid
    pub holder: Option<UncheckedAccount<'info>>,

    /// CHECK: memo-id PDA, Authority of the MEMO ID Collection
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub config: UncheckedAccount<'info>,

    /// CHECK: the MEMO ID Collection
    #[account(address = memo_id_collection() @ ErrorCode::WrongCollection)]
    pub collection: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub program_authority: UncheckedAccount<'info>,

    /// CHECK: checked by memo-home
    pub core_program: UncheckedAccount<'info>,

    pub memo_home_program: Program<'info, memo_home::program::MemoHome>,
    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------------------------
// Account data
// ---------------------------------------------------------------------------------------------

/// One per full MEMO ID, at [MEMO_ID_SEED, sha256(memo_id)], sized for its own `memo_id`.
/// Fixed-size fields come first, so readers can filter by `kind` (offset 8), `asset` (9) and
/// `parent` (41) with memcmp. An all-zero `asset` or `parent` means none.
#[account]
pub struct Registry {
    pub kind: MemoIdKind,
    /// The Asset (Nft only)
    pub asset: Pubkey,
    /// The parent's Registry (SubIds only)
    pub parent: Pubkey,
    pub created_at: i64,
    /// NFT SubIds issued under this ID, ever (they can never be revoked); entry i is the
    /// SubidIndex at [SUBID_SEED, this registry, i as u32 LE]
    pub nft_subids_issued: u32,
    /// Record SubIds that currently exist under this ID
    pub record_subids: u32,
    pub bump: u8,
    /// The controller that set `target`; the record is valid only while it is still the controller
    pub target_set_by: Pubkey,
    pub target: Option<Target>,
    /// The full MEMO ID, e.g. `blog.xen-artist.memo`
    pub memo_id: String,
}

impl Registry {
    /// Size of every field except the `memo_id` bytes
    pub const BASE_SPACE: usize = 8 + // discriminator
        1 +  // kind
        32 + // asset
        32 + // parent
        8 +  // created_at
        4 +  // nft_subids_issued
        4 +  // record_subids
        1 +  // bump
        32 + // target_set_by
        1 + 33 + // target (Option<Target>)
        4; // memo_id length prefix

    /// Space for a Registry whose `memo_id` is `memo_id_len` bytes (the target may be set later)
    pub const fn space_for(memo_id_len: usize) -> usize {
        Self::BASE_SPACE + memo_id_len
    }

    /// The forward record, if it is valid for the current controller
    pub fn forward(&self, controller: &Pubkey) -> Option<Target> {
        self.target.filter(|_| self.target_set_by == *controller)
    }
}

/// Entry `i` of a parent's issued NFT SubIds, at [SUBID_SEED, parent registry, i as u32 LE].
/// NFT SubIds are never deleted, so entries 0..nft_subids_issued always exist.
#[account]
pub struct SubidIndex {
    /// The issued SubId's Registry
    pub registry: Pubkey,
    pub bump: u8,
}

impl SubidIndex {
    pub const SPACE: usize = 8 + 32 + 1;
}

/// The reverse record of a Target, at [PRIMARY_ID_SEED, target key]
#[account]
pub struct PrimaryId {
    pub target: Target,
    /// The Registry of the chosen MEMO ID
    pub memo_id_registry: Pubkey,
    /// The Target's controller when it was set; valid only while it still is
    pub set_by: Pubkey,
    pub bump: u8,
}

impl PrimaryId {
    pub const SPACE: usize = 8 + 33 + 32 + 32 + 1;

    pub fn is_valid(&self, target_controller: &Pubkey) -> bool {
        self.set_by == *target_controller
    }
}

// ---------------------------------------------------------------------------------------------
// Events (program logs: hints for indexers, not proof)
// ---------------------------------------------------------------------------------------------

#[event]
pub struct Initialized {
    pub collection: Pubkey,
}

#[event]
pub struct Registered {
    pub memo_id: String,
    pub registry: Pubkey,
    pub asset: Pubkey,
    pub owner: Pubkey,
    pub burned: u64,
}

#[event]
pub struct SubidCreated {
    pub memo_id: String,
    pub registry: Pubkey,
    pub parent: Pubkey,
}

#[event]
pub struct SubidDeleted {
    pub memo_id: String,
    pub registry: Pubkey,
}

#[event]
pub struct SubidIssued {
    pub memo_id: String,
    pub registry: Pubkey,
    pub parent: Pubkey,
    pub index: u32,
    pub asset: Pubkey,
    pub owner: Pubkey,
    pub burned: u64,
}

#[event]
pub struct TargetSet {
    pub registry: Pubkey,
    pub target: Target,
    pub set_by: Pubkey,
}

#[event]
pub struct TargetCleared {
    pub registry: Pubkey,
}

#[event]
pub struct PrimarySet {
    pub target: Target,
    pub registry: Pubkey,
    pub set_by: Pubkey,
}

#[event]
pub struct PrimaryCleared {
    pub target: Target,
}

#[event]
pub struct UriUpdated {
    pub registry: Pubkey,
    pub asset: Pubkey,
}

// ---------------------------------------------------------------------------------------------
// Helpers (the pure ones are also used by apps through the `cpi` feature)
// ---------------------------------------------------------------------------------------------

/// A DNS hostname label: 1-63 bytes of `a-z`, `0-9`, `-`, no `-` at either end, no `xn--`
pub fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && !label.starts_with("xn--")
        && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A full MEMO ID in normal form: labels as in `valid_label`, ending in `.memo`, at most 253 bytes
pub fn valid_memo_id(memo_id: &str) -> bool {
    match memo_id.strip_suffix(SUFFIX).and_then(|s| s.strip_suffix('.')) {
        Some(labels) => memo_id.len() <= MAX_MEMO_ID_LEN && labels.split('.').all(valid_label),
        None => false,
    }
}

/// Price of a second-level ID by label length, in centi-mints
pub fn price_centi_mints(label_len: usize) -> u64 {
    match label_len {
        1 => PRICE_CENTI_MINTS_1,
        2 => PRICE_CENTI_MINTS_2,
        3 => PRICE_CENTI_MINTS_3,
        4 => PRICE_CENTI_MINTS_4,
        _ => PRICE_CENTI_MINTS_5_PLUS,
    }
}

/// `ceil(reward × centi_mints / 100)` in token units, where reward is memo-mint's current mint
/// reward at `supply`
pub fn burn_price(centi_mints: u64, supply: u64) -> Result<u64> {
    let reward = memo_mint::mint_amount_at_supply(supply) as u128;
    let units = reward
        .checked_mul(centi_mints as u128)
        .ok_or(ErrorCode::ArithmeticOverflow)?
        .div_ceil(100);
    u64::try_from(units).map_err(|_| error!(ErrorCode::ArithmeticOverflow))
}

/// Seed of the MEMO ID Collection: sha256 of the suffix
pub fn collection_seed() -> [u8; 32] {
    hash(SUFFIX.as_bytes()).to_bytes()
}

/// The memo-id Authority PDA
pub fn authority_address() -> Pubkey {
    Pubkey::find_program_address(&[AUTHORITY_SEED], &crate::ID).0
}

/// The MEMO ID Collection: memo-home collection_address(memo-id Authority, sha256("memo"))
pub fn memo_id_collection() -> Pubkey {
    memo_home::collection_address(&authority_address(), &collection_seed()).0
}

/// Registry PDA of a full MEMO ID
pub fn registry_address(memo_id: &str) -> Pubkey {
    Pubkey::find_program_address(&[MEMO_ID_SEED, hash(memo_id.as_bytes()).as_ref()], &crate::ID).0
}

/// Asset PDA of a full MEMO ID
pub fn asset_address(memo_id: &str) -> Pubkey {
    memo_home::asset_address(&memo_id_collection(), &hash(memo_id.as_bytes()).to_bytes()).0
}

/// Entry `index` of a parent's issued NFT SubIds
pub fn subid_index_address(parent_registry: &Pubkey, index: u32) -> Pubkey {
    Pubkey::find_program_address(&[SUBID_SEED, parent_registry.as_ref(), &index.to_le_bytes()], &crate::ID).0
}

/// Primary ID PDA of a Target
pub fn primary_id_address(target: &Target) -> Pubkey {
    Pubkey::find_program_address(&[PRIMARY_ID_SEED, target.key().as_ref()], &crate::ID).0
}

/// Controller of an Asset: its Core owner, or, when `holder` is given, the owner of `holder`
/// whose memo-home Vault holds the Asset (one level only)
pub fn controller_of(asset: &AccountInfo, holder: Option<&AccountInfo>) -> Result<Pubkey> {
    let owner = memo_home::owner_of(asset)?;
    match holder {
        None => Ok(owner),
        Some(h) => {
            require_keys_eq!(owner, memo_home::vault_of(h.key).0, ErrorCode::NotInHolderVault);
            memo_home::owner_of(h)
        }
    }
}

/// True when `registry` (at `registry_key`, valid for `controller`) and `primary` (valid for
/// `target_controller`) point at each other
pub fn is_verified(
    registry_key: &Pubkey,
    registry: &Registry,
    controller: &Pubkey,
    primary: &PrimaryId,
    target_controller: &Pubkey,
) -> bool {
    registry.forward(controller) == Some(primary.target)
        && primary.memo_id_registry == *registry_key
        && primary.is_valid(target_controller)
}

/// The signer must control the NFT ID `registry` through `asset` (its own Asset)
fn require_nft_controller(registry: &Registry, asset: &AccountInfo, holder: Option<&AccountInfo>, signer: &Pubkey) -> Result<Pubkey> {
    require!(registry.kind == MemoIdKind::Nft, ErrorCode::NotNft);
    require_keys_eq!(asset.key(), registry.asset, ErrorCode::WrongAsset);
    let controller = controller_of(asset, holder)?;
    require_keys_eq!(controller, *signer, ErrorCode::NotController);
    Ok(controller)
}

/// The signer must control `registry`: through its own Asset (Nft), or its parent's (Record)
fn require_controller(
    registry: &Account<Registry>,
    parent: Option<&Account<Registry>>,
    asset: &AccountInfo,
    holder: Option<&AccountInfo>,
    signer: &Pubkey,
) -> Result<Pubkey> {
    match registry.kind {
        MemoIdKind::Nft => require_nft_controller(registry, asset, holder, signer),
        MemoIdKind::Record => {
            let parent = parent.ok_or(ErrorCode::WrongParent)?;
            require_keys_eq!(parent.key(), registry.parent, ErrorCode::WrongParent);
            require_nft_controller(parent, asset, holder, signer)
        }
    }
}

/// The signer must control `target`: the address itself, or the controller of the Asset
fn require_target_controller(target: &Target, target_asset: Option<&AccountInfo>, holder: Option<&AccountInfo>, signer: &Pubkey) -> Result<()> {
    match target {
        Target::Address(a) => {
            require_keys_eq!(*a, *signer, ErrorCode::NotController);
        }
        Target::Asset(a) => {
            let asset = target_asset.ok_or(ErrorCode::WrongAsset)?;
            require_keys_eq!(asset.key(), *a, ErrorCode::WrongAsset);
            require_keys_eq!(controller_of(asset, holder)?, *signer, ErrorCode::NotController);
        }
    }
    Ok(())
}

fn require_uri(uri: &str) -> Result<()> {
    require!(uri.len() <= MAX_URI_LEN, ErrorCode::UriTooLong);
    require!(URI_SCHEMES.iter().any(|s| uri.starts_with(s) && uri.len() > s.len()), ErrorCode::InvalidUri);
    Ok(())
}

/// The memo at index 0 must be a BurnMemo for exactly `amount`, whose payload names this
/// operation, the payer and the MEMO ID
fn require_burn_memo(instructions: &AccountInfo, amount: u64, operation: &str, payer: &Pubkey, memo_id: &str) -> Result<()> {
    let current = load_current_index_checked(instructions)?;
    require!(current >= 1, ErrorCode::MemoRequired);
    let ix = load_instruction_at_checked(0, instructions).map_err(|_| error!(ErrorCode::MemoRequired))?;
    require_keys_eq!(ix.program_id, MEMO_PROGRAM_ID, ErrorCode::MemoRequired);
    parse_burn_memo(&ix.data, amount)?.validate(operation, payer, memo_id)
}

/// Decode a Base64 BurnMemo for exactly `amount` and its memo-id payload
pub fn parse_burn_memo(memo: &[u8], amount: u64) -> Result<MemoIdPayload> {
    require!(memo.len() >= MEMO_MIN_LENGTH, ErrorCode::MemoTooShort);
    let text = std::str::from_utf8(memo).map_err(|_| error!(ErrorCode::InvalidMemoFormat))?;
    let data = general_purpose::STANDARD.decode(text).map_err(|_| error!(ErrorCode::InvalidMemoFormat))?;
    let burn_memo = BurnMemo::try_from_slice(&data).map_err(|_| error!(ErrorCode::InvalidMemoFormat))?;
    require!(burn_memo.version == BURN_MEMO_VERSION, ErrorCode::UnsupportedMemoVersion);
    require!(burn_memo.burn_amount == amount, ErrorCode::BurnAmountMismatch);
    MemoIdPayload::try_from_slice(&burn_memo.payload).map_err(|_| error!(ErrorCode::InvalidMemoFormat))
}

fn burn<'info>(b: &BurnAccounts<'info>, payer: &Signer<'info>, instructions: &UncheckedAccount<'info>, amount: u64) -> Result<()> {
    memo_burn::cpi::process_burn(
        CpiContext::new(
            b.memo_burn_program.to_account_info(),
            memo_burn::cpi::accounts::ProcessBurn {
                user: payer.to_account_info(),
                mint: b.mint.to_account_info(),
                token_account: b.payer_token_account.to_account_info(),
                user_global_burn_stats: b.payer_burn_stats.to_account_info(),
                token_program: b.token_program.to_account_info(),
                instructions: instructions.to_account_info(),
                global_burn_stats: b.global_burn_stats.to_account_info(),
            },
        ),
        amount,
    )
}

fn create_asset<'info>(
    h: &HomeAccounts<'info>,
    payer: &Signer<'info>,
    system_program: &Program<'info, System>,
    authority_bump: u8,
    owner: &AccountInfo<'info>,
    memo_id: &str,
    uri: String,
) -> Result<()> {
    memo_home::cpi::create_asset(
        CpiContext::new_with_signer(
            h.memo_home_program.to_account_info(),
            memo_home::cpi::accounts::CreateAsset {
                payer: payer.to_account_info(),
                authority: h.authority.to_account_info(),
                config: h.config.to_account_info(),
                collection: h.collection.to_account_info(),
                asset: h.asset.to_account_info(),
                vault_state: h.vault_state.to_account_info(),
                owner: owner.clone(),
                program_authority: h.program_authority.to_account_info(),
                core_program: h.core_program.to_account_info(),
                system_program: system_program.to_account_info(),
            },
            &[&[AUTHORITY_SEED, &[authority_bump]]],
        ),
        hash(memo_id.as_bytes()).to_bytes(),
        memo_id.to_string(),
        uri,
    )
}

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid label: 1-63 bytes of a-z, 0-9 and '-', no '-' at either end, no 'xn--'")]
    InvalidLabel,
    #[msg("MEMO ID longer than 253 bytes")]
    MemoIdTooLong,
    #[msg("memo_id_hash is not sha256 of the MEMO ID")]
    HashMismatch,
    #[msg("URI must start with https://, ar:// or ipfs://")]
    InvalidUri,
    #[msg("URI longer than 256 bytes")]
    UriTooLong,
    #[msg("Not the MEMO ID Collection")]
    WrongCollection,
    #[msg("Not the MEMO mint")]
    InvalidMint,
    #[msg("Not the Asset of this MEMO ID")]
    WrongAsset,
    #[msg("Signer is not the controller")]
    NotController,
    #[msg("The Asset is not in the holder's Vault")]
    NotInHolderVault,
    #[msg("The parent must be an NFT MEMO ID")]
    ParentNotNft,
    #[msg("Not an NFT MEMO ID")]
    NotNft,
    #[msg("Not a Record SubId")]
    NotRecord,
    #[msg("Not the parent of this SubId")]
    WrongParent,
    #[msg("index must equal the parent's nft_subids_issued")]
    WrongSubidIndex,
    #[msg("Transaction must start with the BurnMemo")]
    MemoRequired,
    #[msg("Memo too short (minimum 69 bytes)")]
    MemoTooShort,
    #[msg("Invalid memo format: Base64 Borsh BurnMemo with a memo-id payload expected")]
    InvalidMemoFormat,
    #[msg("Unsupported BurnMemo version")]
    UnsupportedMemoVersion,
    #[msg("The memo's burn_amount must equal the price")]
    BurnAmountMismatch,
    #[msg("Unsupported payload version")]
    UnsupportedPayloadVersion,
    #[msg("Payload category must be 'memo_id'")]
    InvalidCategory,
    #[msg("Payload operation does not match the instruction")]
    InvalidOperation,
    #[msg("Payload user_pubkey must be the payer")]
    UserPubkeyMismatch,
    #[msg("Payload memo_id does not match")]
    MemoIdMismatch,
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests;
