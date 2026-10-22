#![allow(unexpected_cfgs)]

//! Throwaway prototype behind the home:// browser extension (clients/home-extension), for the
//! local validator only and never deployed. It stands in for memo-id and memo-file, which do not
//! exist yet, with the smallest layout the extension needs:
//!
//! - Name  `[b"name", sha256(name)]`: a MEMO ID such as `xen-artist.memo` → a Core Asset
//! - File  `[b"file", asset, sha256(path)]`: one file of that Asset, written in chunks by the
//!   Asset's owner, with the SHA-256 of the full content for the reader to check
//!
//! No verification records, prices, versions, blocks or session keys: those belong to the real
//! programs (MEMO-ID-DESIGN.md, MEMO-HOME-DESIGN.md §6.6).

use anchor_lang::prelude::*;
use solana_sha256_hasher::hash;

declare_id!("6uknhjicEvg7B7oAvZ3y7TWojo8YfvKicn7HFVDPtiip");

pub const CORE_PROGRAM: Pubkey = pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
pub const NAME_SEED: &[u8] = b"name";
pub const FILE_SEED: &[u8] = b"file";
pub const SUFFIX: &str = ".memo";
pub const MAX_NAME_LEN: usize = 253;
pub const MAX_LABEL_LEN: usize = 63;
pub const CONTENT_TYPE_LEN: usize = 48;
/// Accounts created by CPI can be at most 10 KiB, so a file is at most 9 KB here
pub const MAX_FILE_SIZE: u32 = 9 * 1024;

#[program]
pub mod home_web_test {
    use super::*;

    /// Point a MEMO ID at an Asset; the Asset's owner signs. First come, first served.
    pub fn register_name(ctx: Context<RegisterName>, name_hash: [u8; 32], name: String) -> Result<()> {
        require!(valid_name(&name), WebError::InvalidName);
        require!(hash(name.as_bytes()).to_bytes() == name_hash, WebError::HashMismatch);
        require_asset_owner(&ctx.accounts.asset, &ctx.accounts.owner.key())?;
        let record = &mut ctx.accounts.name_record;
        record.asset = ctx.accounts.asset.key();
        record.name = name;
        msg!("name {} -> asset {}", record.name, record.asset);
        Ok(())
    }

    /// Create an empty file of `size` bytes for an Asset; the Asset's owner signs.
    pub fn init_file(
        ctx: Context<InitFile>,
        path_hash: [u8; 32],
        size: u32,
        content_hash: [u8; 32],
        content_type: String,
    ) -> Result<()> {
        require!(size <= MAX_FILE_SIZE, WebError::FileTooLarge);
        require!(content_type.len() <= CONTENT_TYPE_LEN, WebError::ContentTypeTooLong);
        require_asset_owner(&ctx.accounts.asset, &ctx.accounts.owner.key())?;
        let file = &mut ctx.accounts.file;
        file.asset = ctx.accounts.asset.key();
        file.path_hash = path_hash;
        file.content_hash = content_hash;
        file.size = size;
        file.content_type[..content_type.len()].copy_from_slice(content_type.as_bytes());
        Ok(())
    }

    /// Write `data` at `offset` into a file; the Asset's owner signs.
    pub fn write_file(ctx: Context<WriteFile>, offset: u32, data: Vec<u8>) -> Result<()> {
        require_asset_owner(&ctx.accounts.asset, &ctx.accounts.owner.key())?;
        let end = (offset as usize).checked_add(data.len()).ok_or(WebError::OutOfRange)?;
        require!(end <= ctx.accounts.file.size as usize, WebError::OutOfRange);
        let info = ctx.accounts.file.to_account_info();
        let mut raw = info.try_borrow_mut_data()?;
        raw[WebFile::HEADER + offset as usize..WebFile::HEADER + end].copy_from_slice(&data);
        Ok(())
    }

    /// Close a file and refund its rent to the owner, so it can be uploaded again
    pub fn close_file(ctx: Context<CloseFile>) -> Result<()> {
        require_asset_owner(&ctx.accounts.asset, &ctx.accounts.owner.key())?;
        Ok(())
    }
}

/// `label(.label)*.memo`, each label a DNS hostname label (a-z, 0-9, '-', no '-' at either end,
/// no `xn--`), at most 63 bytes per label and 253 in total
pub fn valid_name(name: &str) -> bool {
    if name.len() > MAX_NAME_LEN || !name.ends_with(SUFFIX) || name.len() == SUFFIX.len() {
        return false;
    }
    name[..name.len() - SUFFIX.len()].split('.').all(|l| {
        !l.is_empty()
            && l.len() <= MAX_LABEL_LEN
            && !l.starts_with('-')
            && !l.ends_with('-')
            && !l.starts_with("xn--")
            && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// A Core AssetV1 (key 1) whose owner (bytes 1..33) is `owner`
fn require_asset_owner(asset: &UncheckedAccount, owner: &Pubkey) -> Result<()> {
    require_keys_eq!(*asset.owner, CORE_PROGRAM, WebError::NotAnAsset);
    let data = asset.try_borrow_data()?;
    require!(data.len() >= 33 && data[0] == 1, WebError::NotAnAsset);
    require!(data[1..33] == owner.to_bytes(), WebError::NotAssetOwner);
    Ok(())
}

#[derive(Accounts)]
#[instruction(name_hash: [u8; 32])]
pub struct RegisterName<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub owner: Signer<'info>,
    /// CHECK: a Core Asset owned by `owner`, checked in the handler
    pub asset: UncheckedAccount<'info>,
    #[account(init, payer = payer, space = NameRecord::SPACE, seeds = [NAME_SEED, name_hash.as_ref()], bump)]
    pub name_record: Account<'info, NameRecord>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(path_hash: [u8; 32], size: u32)]
pub struct InitFile<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub owner: Signer<'info>,
    /// CHECK: a Core Asset owned by `owner`, checked in the handler
    pub asset: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        space = WebFile::HEADER + size as usize,
        seeds = [FILE_SEED, asset.key().as_ref(), path_hash.as_ref()],
        bump
    )]
    pub file: Account<'info, WebFile>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WriteFile<'info> {
    pub owner: Signer<'info>,
    /// CHECK: a Core Asset owned by `owner`, checked in the handler
    pub asset: UncheckedAccount<'info>,
    #[account(mut, has_one = asset)]
    pub file: Account<'info, WebFile>,
}

#[derive(Accounts)]
pub struct CloseFile<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: a Core Asset owned by `owner`, checked in the handler
    pub asset: UncheckedAccount<'info>,
    #[account(mut, has_one = asset, close = owner)]
    pub file: Account<'info, WebFile>,
}

#[account]
pub struct NameRecord {
    pub asset: Pubkey,
    pub name: String,
}

impl NameRecord {
    pub const SPACE: usize = 8 + 32 + 4 + MAX_NAME_LEN;
}

/// Fixed header, then `size` bytes of content at offset HEADER
#[account]
pub struct WebFile {
    pub asset: Pubkey,
    pub path_hash: [u8; 32],
    pub content_hash: [u8; 32],
    pub size: u32,
    /// UTF-8, zero-padded
    pub content_type: [u8; CONTENT_TYPE_LEN],
}

impl WebFile {
    pub const HEADER: usize = 8 + 32 + 32 + 32 + 4 + CONTENT_TYPE_LEN;
}

#[error_code]
pub enum WebError {
    #[msg("Invalid MEMO ID")]
    InvalidName,
    #[msg("Hash does not match")]
    HashMismatch,
    #[msg("Not a Core Asset")]
    NotAnAsset,
    #[msg("Signer is not the Asset's owner")]
    NotAssetOwner,
    #[msg("File too large")]
    FileTooLarge,
    #[msg("Content type too long")]
    ContentTypeTooLong,
    #[msg("Write out of range")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a.memo", "xen-artist.memo", "blog.xen-artist.memo", "0x1.memo"] {
            assert!(valid_name(ok), "{}", ok);
        }
        for bad in [".memo", "memo", "a.x1", "-a.memo", "a-.memo", "A.memo", "a..memo", "xn--a.memo", "a_b.memo", "a b.memo"] {
            assert!(!valid_name(bad), "{}", bad);
        }
        assert!(valid_name(&format!("{}.memo", "a".repeat(63))));
        assert!(!valid_name(&format!("{}.memo", "a".repeat(64))));
    }
}
