#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;

#[cfg(test)]
mod tests;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::Token2022;
use memo_burn::program::MemoBurn;
use memo_burn::cpi::accounts::ProcessBurn;
use memo_mint::program::MemoMint;
use memo_mint::cpi::accounts::ProcessMint;
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use spl_memo::ID as MEMO_PROGRAM_ID;
use base64::{Engine as _, engine::general_purpose};
use std::str::FromStr;

// Program ID - different for testnet and mainnet
// Note: These are placeholder IDs, should be replaced after deployment
#[cfg(feature = "mainnet")]
declare_id!("3EKdp88FgyPC41bxRDzFAtCDUMV2g9SVt5UiytE8wdzM");

#[cfg(not(feature = "mainnet"))]
declare_id!("HPvqPUneCLwb8YYoYTrWmy6o7viRKsnLTgxwkg7CCpfB");

// Authorized mint address - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// ===== BUSINESS LOGIC CONSTANTS =====

// Token economics
pub const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)

// Blog creation/update/burn constants - all require at least 1 MEMO token
pub const MIN_BLOG_BURN_TOKENS: u64 = 1; // Minimum tokens to burn for any blog operation
pub const MIN_BLOG_BURN_AMOUNT: u64 = MIN_BLOG_BURN_TOKENS * DECIMAL_FACTOR;

// Maximum burn per transaction (consistent with memo-burn)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR; // 1 trillion tokens

// ===== STRING LENGTH CONSTRAINTS =====

// Blog metadata limits (no website, no tags - simpler than project)
pub const MAX_BLOG_NAME_LENGTH: usize = 64;
pub const MAX_BLOG_DESCRIPTION_LENGTH: usize = 256; 
pub const MAX_BLOG_IMAGE_LENGTH: usize = 256;        

// Memo length constraints (consistent with memo-mint and memo-burn)
pub const MEMO_MIN_LENGTH: usize = 69;
pub const MEMO_MAX_LENGTH: usize = 800;

// Borsh serialization constants (from memo-burn)
const BORSH_U8_SIZE: usize = 1;         // version (u8)
const BORSH_U64_SIZE: usize = 8;        // burn_amount (u64)
const BORSH_VEC_LENGTH_SIZE: usize = 4; // user_data.len() (u32)
const BORSH_FIXED_OVERHEAD: usize = BORSH_U8_SIZE + BORSH_U64_SIZE + BORSH_VEC_LENGTH_SIZE;

// maximum payload length = memo maximum length - borsh fixed overhead
pub const MAX_PAYLOAD_LENGTH: usize = MEMO_MAX_LENGTH - BORSH_FIXED_OVERHEAD; // 800 - 13 = 787

// Maximum allowed Borsh data size after Base64 decoding (security limit)
pub const MAX_BORSH_DATA_SIZE: usize = MEMO_MAX_LENGTH;

// Current version of BurnMemo structure (consistent with memo-burn)
pub const BURN_MEMO_VERSION: u8 = 1;

// Current version of BlogCreationData structure
pub const BLOG_CREATION_DATA_VERSION: u8 = 1;

// Current version of BlogUpdateData structure  
pub const BLOG_UPDATE_DATA_VERSION: u8 = 1;

// Current version of BlogBurnData structure
pub const BLOG_BURN_DATA_VERSION: u8 = 1;

// Current version of BlogMintData structure
pub const BLOG_MINT_DATA_VERSION: u8 = 1;

// Expected category for memo-blog contract
pub const EXPECTED_CATEGORY: &str = "blog";

// Expected operation for blog creation
pub const EXPECTED_OPERATION: &str = "create_blog";

// Expected operation for blog update
pub const EXPECTED_UPDATE_OPERATION: &str = "update_blog";

// maximum burn/mint message length
pub const MAX_MESSAGE_LENGTH: usize = 696;

// expected operation for blog burn
pub const EXPECTED_BURN_FOR_BLOG_OPERATION: &str = "burn_for_blog";

// expected operation for blog mint
pub const EXPECTED_MINT_FOR_BLOG_OPERATION: &str = "mint_for_blog";

/// BurnMemo structure (compatible with memo-burn contract)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BurnMemo {
    /// version of the BurnMemo structure (for future compatibility)
    pub version: u8,
    
    /// burn amount (must match actual burn amount)
    pub burn_amount: u64,
    
    /// application payload (variable length, max 787 bytes)
    pub payload: Vec<u8>,
}

/// Blog creation data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BlogCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "blog" for memo-blog contract)
    pub category: String,
    
    /// Operation type (must be "create_blog" for blog creation)
    pub operation: String,
    
    /// Creator pubkey as string (must match the transaction signer)
    pub creator: String,
    
    /// Blog name (required, 1-64 characters)
    pub name: String,
    
    /// Blog description (optional, max 256 characters)
    pub description: String,
    
    /// Blog image info (optional, max 256 characters)
    pub image: String,
}

impl BlogCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_creator: Pubkey) -> Result<()> {
        // Validate version
        if self.version != BLOG_CREATION_DATA_VERSION {
            msg!("Unsupported blog creation data version: {} (expected: {})", 
                 self.version, BLOG_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedBlogDataVersion.into());
        }
        
        // Validate category (must be exactly "blog")
        if self.category != EXPECTED_CATEGORY {
            msg!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategory.into());
        }
        
        // Validate category length
        if self.category.len() != EXPECTED_CATEGORY.len() {
            msg!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategoryLength.into());
        }
        
        // Validate operation (must be exactly "create_blog")
        if self.operation != EXPECTED_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_OPERATION.len(), EXPECTED_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate creator pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.creator)
            .map_err(|_| {
                msg!("Invalid creator pubkey format: {}", self.creator);
                ErrorCode::InvalidCreatorPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_creator {
            msg!("Creator pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_creator);
            return Err(ErrorCode::CreatorPubkeyMismatch.into());
        }
        
        // Validate name (required, 1-64 characters)
        if self.name.is_empty() || self.name.len() > MAX_BLOG_NAME_LENGTH {
            msg!("Invalid blog name: '{}' (must be 1-{} characters)", self.name, MAX_BLOG_NAME_LENGTH);
            return Err(ErrorCode::InvalidBlogName.into());
        }
        
        // Validate description (optional, max 256 characters)
        if self.description.len() > MAX_BLOG_DESCRIPTION_LENGTH {
            msg!("Invalid blog description: {} characters (max: {})", 
                 self.description.len(), MAX_BLOG_DESCRIPTION_LENGTH);
            return Err(ErrorCode::InvalidBlogDescription.into());
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > MAX_BLOG_IMAGE_LENGTH {
            msg!("Invalid blog image: {} characters (max: {})", 
                 self.image.len(), MAX_BLOG_IMAGE_LENGTH);
            return Err(ErrorCode::InvalidBlogImage.into());
        }
        
        msg!("Blog creation data validation passed: category={}, operation={}, creator={}, name={}", 
             self.category, self.operation, self.creator, self.name);
        
        Ok(())
    }
}

/// Blog update data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BlogUpdateData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "blog" for memo-blog contract)
    pub category: String,
    
    /// Operation type (must be "update_blog" for blog update)
    pub operation: String,
    
    /// Creator pubkey as string (must match the transaction signer / blog owner)
    pub creator: String,
    
    /// Updated fields (all optional)
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
}

impl BlogUpdateData {
    /// Validate the structure fields
    pub fn validate(&self, expected_creator: Pubkey) -> Result<()> {
        // Validate version
        if self.version != BLOG_UPDATE_DATA_VERSION {
            msg!("Unsupported blog update data version: {} (expected: {})", 
                 self.version, BLOG_UPDATE_DATA_VERSION);
            return Err(ErrorCode::UnsupportedBlogDataVersion.into());
        }
        
        // Validate category (must be exactly "blog")
        if self.category != EXPECTED_CATEGORY {
            msg!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategory.into());
        }
        
        // Validate category length
        if self.category.len() != EXPECTED_CATEGORY.len() {
            msg!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategoryLength.into());
        }
        
        // Validate operation (must be exactly "update_blog")
        if self.operation != EXPECTED_UPDATE_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_UPDATE_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_UPDATE_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_UPDATE_OPERATION.len(), EXPECTED_UPDATE_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate creator pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.creator)
            .map_err(|_| {
                msg!("Invalid creator pubkey format: {}", self.creator);
                ErrorCode::InvalidCreatorPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_creator {
            msg!("Creator pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_creator);
            return Err(ErrorCode::CreatorPubkeyMismatch.into());
        }
        
        // Validate name (optional, 1-64 characters)
        if let Some(ref new_name) = self.name {
            if new_name.is_empty() || new_name.len() > MAX_BLOG_NAME_LENGTH {
                msg!("Invalid blog name: '{}' (must be 1-{} characters)", new_name, MAX_BLOG_NAME_LENGTH);
                return Err(ErrorCode::InvalidBlogName.into());
            }
        }
        
        // Validate description (optional, max 256 characters)
        if let Some(ref new_description) = self.description {
            if new_description.len() > MAX_BLOG_DESCRIPTION_LENGTH {
                msg!("Invalid blog description: {} characters (max: {})", 
                     new_description.len(), MAX_BLOG_DESCRIPTION_LENGTH);
                return Err(ErrorCode::InvalidBlogDescription.into());
            }
        }
        
        // Validate image (optional, max 256 characters)
        if let Some(ref new_image) = self.image {
            if new_image.len() > MAX_BLOG_IMAGE_LENGTH {
                msg!("Invalid blog image: {} characters (max: {})", 
                     new_image.len(), MAX_BLOG_IMAGE_LENGTH);
                return Err(ErrorCode::InvalidBlogImage.into());
            }
        }
        
        msg!("Blog update data validation passed: category={}, operation={}, creator={}", 
             self.category, self.operation, self.creator);
        
        Ok(())
    }
}

/// Blog burn data structure (stored in BurnMemo.payload for burn_for_blog)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BlogBurnData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "blog" for memo-blog contract)
    pub category: String,
    
    /// Operation type (must be "burn_for_blog" for burning tokens)
    pub operation: String,
    
    /// Burner pubkey as string (must match the transaction signer / blog creator)
    pub burner: String,
    
    /// Burn message (optional, max 696 characters)
    pub message: String,
}

impl BlogBurnData {
    /// Validate the structure fields
    pub fn validate(&self, expected_burner: Pubkey) -> Result<()> {
        // Validate version
        if self.version != BLOG_BURN_DATA_VERSION {
            msg!("Unsupported blog burn data version: {} (expected: {})", 
                 self.version, BLOG_BURN_DATA_VERSION);
            return Err(ErrorCode::UnsupportedBlogBurnDataVersion.into());
        }
        
        // Validate category (must be exactly "blog")
        if self.category != EXPECTED_CATEGORY {
            msg!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategory.into());
        }
        
        // Validate category length
        if self.category.len() != EXPECTED_CATEGORY.len() {
            msg!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategoryLength.into());
        }
        
        // Validate operation (must be exactly "burn_for_blog")
        if self.operation != EXPECTED_BURN_FOR_BLOG_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_BURN_FOR_BLOG_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_BURN_FOR_BLOG_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_BURN_FOR_BLOG_OPERATION.len(), EXPECTED_BURN_FOR_BLOG_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate burner pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.burner)
            .map_err(|_| {
                msg!("Invalid burner pubkey format: {}", self.burner);
                ErrorCode::InvalidBurnerPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_burner {
            msg!("Burner pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_burner);
            return Err(ErrorCode::BurnerPubkeyMismatch.into());
        }
        
        // Validate message length (optional, max 696 characters)
        if self.message.len() > MAX_MESSAGE_LENGTH {
            msg!("Burn message too long: {} characters (max: {})", 
                 self.message.len(), MAX_MESSAGE_LENGTH);
            return Err(ErrorCode::MessageTooLong.into());
        }
        
        msg!("Blog burn data validation passed: category={}, operation={}, burner={}", 
             self.category, self.operation, self.burner);
        
        Ok(())
    }
}

/// Blog mint data structure (stored in BurnMemo.payload for mint_for_blog)
/// Note: For mint operations, the burn_amount in BurnMemo should be 0
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct BlogMintData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "blog" for memo-blog contract)
    pub category: String,
    
    /// Operation type (must be "mint_for_blog" for minting tokens)
    pub operation: String,
    
    /// Minter pubkey as string (must match the transaction signer / blog creator)
    pub minter: String,
    
    /// Mint message (optional, max 696 characters)
    pub message: String,
}

impl BlogMintData {
    /// Validate the structure fields
    pub fn validate(&self, expected_minter: Pubkey) -> Result<()> {
        // Validate version
        if self.version != BLOG_MINT_DATA_VERSION {
            msg!("Unsupported blog mint data version: {} (expected: {})", 
                 self.version, BLOG_MINT_DATA_VERSION);
            return Err(ErrorCode::UnsupportedBlogMintDataVersion.into());
        }
        
        // Validate category (must be exactly "blog")
        if self.category != EXPECTED_CATEGORY {
            msg!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategory.into());
        }
        
        // Validate category length
        if self.category.len() != EXPECTED_CATEGORY.len() {
            msg!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategoryLength.into());
        }
        
        // Validate operation (must be exactly "mint_for_blog")
        if self.operation != EXPECTED_MINT_FOR_BLOG_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_MINT_FOR_BLOG_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_MINT_FOR_BLOG_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_MINT_FOR_BLOG_OPERATION.len(), EXPECTED_MINT_FOR_BLOG_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate minter pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.minter)
            .map_err(|_| {
                msg!("Invalid minter pubkey format: {}", self.minter);
                ErrorCode::InvalidMinterPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_minter {
            msg!("Minter pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_minter);
            return Err(ErrorCode::MinterPubkeyMismatch.into());
        }
        
        // Validate message length (optional, max 696 characters)
        if self.message.len() > MAX_MESSAGE_LENGTH {
            msg!("Mint message too long: {} characters (max: {})", 
                 self.message.len(), MAX_MESSAGE_LENGTH);
            return Err(ErrorCode::MessageTooLong.into());
        }
        
        msg!("Blog mint data validation passed: category={}, operation={}, minter={}", 
             self.category, self.operation, self.minter);
        
        Ok(())
    }
}

#[program]
pub mod memo_blog {
    use super::*;

    /// Create a new blog (requires burning at least 1 MEMO token)
    /// Each user can only create one unique blog, bound to their pubkey
    pub fn create_blog(
        ctx: Context<CreateBlog>,
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 1 token for blog creation
        if burn_amount < MIN_BLOG_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
        if burn_amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if burn_amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo data for blog creation
        let blog_data = parse_blog_creation_borsh_memo(&memo_data, ctx.accounts.creator.key(), burn_amount)?;
        
        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.creator.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.creator_token_account.to_account_info(),
            user_global_burn_stats: ctx.accounts.user_global_burn_stats.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        memo_burn::cpi::process_burn(cpi_ctx, burn_amount)?;
        
        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;
        
        // Initialize blog data after successful burn
        let blog = &mut ctx.accounts.blog;
        blog.creator = ctx.accounts.creator.key();
        blog.created_at = timestamp;
        blog.last_updated = timestamp;
        blog.name = blog_data.name.clone();
        blog.description = blog_data.description.clone();
        blog.image = blog_data.image.clone();
        blog.memo_count = 0; // Initialize memo_count (tracks burn_for_blog and mint_for_blog operations)
        blog.burned_amount = burn_amount;
        blog.last_memo_time = 0; // Set to 0 initially (no burn/mint_for_blog memos yet)
        blog.bump = ctx.bumps.blog;

        // Emit blog creation event
        emit!(BlogCreatedEvent {
            creator: ctx.accounts.creator.key(),
            name: blog_data.name,
            description: blog_data.description,
            image: blog_data.image,
            burn_amount,
            timestamp,
        });

        msg!("Blog created successfully by {} with {} tokens burned", 
             ctx.accounts.creator.key(), burn_amount / DECIMAL_FACTOR);
        Ok(())
    }

    /// Update an existing blog (requires burning at least 1 MEMO token)
    pub fn update_blog(
        ctx: Context<UpdateBlog>,
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 1 token for blog update
        if burn_amount < MIN_BLOG_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
        if burn_amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if burn_amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo data for blog update
        let update_data = parse_blog_update_borsh_memo(&memo_data, ctx.accounts.updater.key(), burn_amount)?;
        
        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.updater.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.updater_token_account.to_account_info(),
            user_global_burn_stats: ctx.accounts.user_global_burn_stats.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        memo_burn::cpi::process_burn(cpi_ctx, burn_amount)?;

        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;

        let blog = &mut ctx.accounts.blog;
        
        // Update fields if provided in memo data
        if let Some(new_name) = update_data.name {
            blog.name = new_name;
        }
        
        if let Some(new_description) = update_data.description {
            blog.description = new_description;
        }
        
        if let Some(new_image) = update_data.image {
            blog.image = new_image;
        }
        
        // Update burn amount and timestamp
        blog.burned_amount = blog.burned_amount.saturating_add(burn_amount);
        blog.last_updated = timestamp;
        // Note: last_memo_time is NOT updated here - only tracks burn_for_blog/mint_for_blog operations

        // Emit blog update event
        emit!(BlogUpdatedEvent {
            creator: ctx.accounts.updater.key(),
            name: blog.name.clone(),
            description: blog.description.clone(),
            image: blog.image.clone(),
            burn_amount,
            total_burned: blog.burned_amount,
            timestamp,
        });

        msg!("Blog updated successfully by {} with {} tokens burned (total: {})", 
             ctx.accounts.updater.key(), burn_amount / DECIMAL_FACTOR, 
             blog.burned_amount / DECIMAL_FACTOR);
        Ok(())
    }

    /// Burn tokens for a blog (only blog creator can burn)
    pub fn burn_for_blog(
        ctx: Context<BurnForBlog>,
        amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 1 token
        if amount < MIN_BLOG_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount limit
        if amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with enhanced validation
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo content for burn operation
        parse_blog_burn_borsh_memo(&memo_data, amount, ctx.accounts.burner.key())?;

        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.burner.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.burner_token_account.to_account_info(),
            user_global_burn_stats: ctx.accounts.user_global_burn_stats.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        // Call memo-burn's process_burn instruction
        memo_burn::cpi::process_burn(cpi_ctx, amount)?;
        
        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;
        
        // Update blog burned amount tracking
        let blog = &mut ctx.accounts.blog;
        let old_amount = blog.burned_amount;
        blog.burned_amount = blog.burned_amount.saturating_add(amount);
        
        // Update memo count (burn_for_blog and mint_for_blog operations count as memos)
        blog.memo_count = blog.memo_count.saturating_add(1);
        
        // Update last memo time
        blog.last_memo_time = timestamp;
        
        if blog.burned_amount == u64::MAX && old_amount < u64::MAX {
            msg!("Warning: burned_amount overflow detected for blog creator {}", ctx.accounts.burner.key());
        }
        
        msg!("Successfully burned {} tokens for blog (creator: {})", amount / DECIMAL_FACTOR, ctx.accounts.burner.key());
        
        // Emit burn event
        emit!(TokensBurnedForBlogEvent {
            creator: ctx.accounts.burner.key(),
            amount,
            total_burned: blog.burned_amount,
            timestamp,
        });

        Ok(())
    }

    /// Mint tokens for a blog (only blog creator can mint)
    pub fn mint_for_blog(
        ctx: Context<MintForBlog>,
    ) -> Result<()> {
        // Check memo instruction with enhanced validation
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo content for mint operation
        parse_blog_mint_borsh_memo(&memo_data, ctx.accounts.minter.key())?;

        // Call memo-mint contract to mint tokens
        // Using process_mint which mints to the caller's own account
        let cpi_program = ctx.accounts.memo_mint_program.to_account_info();
        let cpi_accounts = ProcessMint {
            user: ctx.accounts.minter.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            mint_authority: ctx.accounts.mint_authority.to_account_info(),
            token_account: ctx.accounts.minter_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        // Call memo-mint's process_mint instruction
        memo_mint::cpi::process_mint(cpi_ctx)?;
        
        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;
        
        // Update blog tracking
        let blog = &mut ctx.accounts.blog;
        
        // Update memo count (burn_for_blog and mint_for_blog operations count as memos)
        blog.memo_count = blog.memo_count.saturating_add(1);
        
        // Update last memo time
        blog.last_memo_time = timestamp;
        
        msg!("Successfully minted tokens for blog (creator: {})", ctx.accounts.minter.key());
        
        // Emit mint event
        emit!(TokensMintedForBlogEvent {
            creator: ctx.accounts.minter.key(),
            timestamp,
        });

        Ok(())
    }
}

/// Parse and validate Borsh-formatted memo data for blog creation (with Base64 decoding)
fn parse_blog_creation_borsh_memo(memo_data: &[u8], expected_creator: Pubkey, expected_amount: u64) -> Result<BlogCreationData> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes (following memo-burn pattern)
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // Validate version compatibility
    if burn_memo.version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", 
             burn_memo.version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // Validate burn amount matches
    if burn_memo.burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", 
             burn_memo.burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    // Validate payload length does not exceed maximum allowed value
    if burn_memo.payload.len() > MAX_PAYLOAD_LENGTH {
        msg!("Payload too long: {} bytes (max: {})", 
             burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
        return Err(ErrorCode::PayloadTooLong.into());
    }
    
    msg!("Borsh+Base64 memo validation passed: version {}, {} units, payload: {} bytes", 
         burn_memo.version, expected_amount, burn_memo.payload.len());
    
    // Deserialize BlogCreationData from payload
    let blog_data = BlogCreationData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid blog creation data format in payload");
            ErrorCode::InvalidBlogDataFormat
        })?;
    
    // Validate the blog creation data
    blog_data.validate(expected_creator)?;
    
    msg!("Blog creation data parsed successfully: creator={}, name={}, description_len={}", 
         blog_data.creator, blog_data.name, blog_data.description.len());

    Ok(blog_data)
}

/// Parse and validate Borsh-formatted memo data for blog update (with Base64 decoding)
fn parse_blog_update_borsh_memo(memo_data: &[u8], expected_creator: Pubkey, expected_amount: u64) -> Result<BlogUpdateData> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes (following memo-burn pattern)
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // Validate version compatibility
    if burn_memo.version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", 
             burn_memo.version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // Validate burn amount matches
    if burn_memo.burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", 
             burn_memo.burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    // Validate payload length does not exceed maximum allowed value
    if burn_memo.payload.len() > MAX_PAYLOAD_LENGTH {
        msg!("Payload too long: {} bytes (max: {})", 
             burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
        return Err(ErrorCode::PayloadTooLong.into());
    }
    
    msg!("Borsh+Base64 update memo validation passed: version {}, {} units, payload: {} bytes", 
         burn_memo.version, expected_amount, burn_memo.payload.len());
    
    // Deserialize BlogUpdateData from payload
    let update_data = BlogUpdateData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid blog update data format in payload");
            ErrorCode::InvalidBlogDataFormat
        })?;
    
    // Validate the blog update data
    update_data.validate(expected_creator)?;
    
    msg!("Blog update data parsed successfully: creator={}, has updates: name={}, description={}, image={}", 
         update_data.creator, 
         update_data.name.is_some(),
         update_data.description.is_some(),
         update_data.image.is_some());

    Ok(update_data)
}

/// Parse and validate Borsh-formatted memo data for blog burn (with Base64 decoding)
fn parse_blog_burn_borsh_memo(memo_data: &[u8], expected_amount: u64, expected_burner: Pubkey) -> Result<()> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;

    // Check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes (following memo-burn pattern)
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // Validate version compatibility
    if burn_memo.version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", 
             burn_memo.version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // Validate burn amount matches
    if burn_memo.burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", 
             burn_memo.burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }
    
    // Validate payload length does not exceed maximum allowed value
    if burn_memo.payload.len() > MAX_PAYLOAD_LENGTH {
        msg!("Payload too long: {} bytes (max: {})", 
             burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
        return Err(ErrorCode::PayloadTooLong.into());
    }
    
    msg!("Borsh+Base64 burn memo validation passed: version {}, {} units, payload: {} bytes", 
         burn_memo.version, expected_amount, burn_memo.payload.len());
    
    // Deserialize blog burn data from payload
    let burn_data = BlogBurnData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid blog burn data format in payload");
            ErrorCode::InvalidBlogBurnDataFormat
        })?;
    
    // Validate blog burn data
    burn_data.validate(expected_burner)?;
    
    Ok(())
}

/// Parse and validate Borsh-formatted memo data for blog mint (with Base64 decoding)
/// Note: For mint operations, the burn_amount in BurnMemo should be 0
fn parse_blog_mint_borsh_memo(memo_data: &[u8], expected_minter: Pubkey) -> Result<()> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;

    // Check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidMemoFormat
        })?;
    
    // Validate version compatibility
    if burn_memo.version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", 
             burn_memo.version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }
    
    // For mint operations, burn_amount should be 0
    if burn_memo.burn_amount != 0 {
        msg!("Mint operation should have burn_amount=0, got {}", burn_memo.burn_amount);
        return Err(ErrorCode::InvalidMintMemoFormat.into());
    }
    
    // Validate payload length does not exceed maximum allowed value
    if burn_memo.payload.len() > MAX_PAYLOAD_LENGTH {
        msg!("Payload too long: {} bytes (max: {})", 
             burn_memo.payload.len(), MAX_PAYLOAD_LENGTH);
        return Err(ErrorCode::PayloadTooLong.into());
    }
    
    msg!("Borsh+Base64 mint memo validation passed: version {}, payload: {} bytes", 
         burn_memo.version, burn_memo.payload.len());
    
    // Deserialize blog mint data from payload
    let mint_data = BlogMintData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid blog mint data format in payload");
            ErrorCode::InvalidBlogMintDataFormat
        })?;
    
    // Validate blog mint data
    mint_data.validate(expected_minter)?;
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-blog instructions (create_blog, update_blog, etc.)
/// 
/// Compute budget instructions can be placed anywhere in the transaction
/// as they are processed by Solana runtime before instruction execution.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("memo-blog instruction must be at index 1 or later, but current instruction is at index {}", current_index);
        return Ok((false, vec![]));
    }
    
    // Check that index 0 contains the memo instruction
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at required index 0");
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH)
            } else {
                msg!("Instruction at index 0 is not a memo (program_id: {})", ix.program_id);
                Ok((false, vec![]))
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 0: {:?}", e);
            Ok((false, vec![]))
        }
    }
}

/// Validate memo data length and return result
fn validate_memo_length(memo_data: &[u8], min_length: usize, max_length: usize) -> Result<(bool, Vec<u8>)> {
    let memo_length = memo_data.len();
    
    // Ensure data is not empty
    if memo_data.is_empty() {
        msg!("Memo data is empty");
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    // Check minimum length requirement
    if memo_length < min_length {
        msg!("Memo too short: {} bytes (minimum: {})", memo_length, min_length);
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    // Check maximum length requirement
    if memo_length > max_length {
        msg!("Memo too long: {} bytes (maximum: {})", memo_length, max_length);
        return Err(ErrorCode::MemoTooLong.into());
    }
    
    // Length is valid, return memo data
    msg!("Memo length validation passed: {} bytes (range: {}-{})", memo_length, min_length, max_length);
    Ok((true, memo_data.to_vec()))
}

/// Account structure for creating a blog
#[derive(Accounts)]
#[instruction(burn_amount: u64)]
pub struct CreateBlog<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        init,
        payer = creator,
        space = Blog::calculate_space_max(),
        seeds = [b"blog", creator.key().as_ref()],
        bump
    )]
    pub blog: Account<'info, Blog>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = creator_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = creator_token_account.owner == creator.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub creator_token_account: InterfaceAccount<'info, TokenAccount>,

    /// User global burn statistics tracking account (now required)
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", creator.key().as_ref()],
        bump,
        seeds::program = memo_burn_program.key()
    )]
    pub user_global_burn_stats: Account<'info, memo_burn::UserGlobalBurnStats>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-burn program
    pub memo_burn_program: Program<'info, MemoBurn>,
    
    pub system_program: Program<'info, System>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for updating a blog
#[derive(Accounts)]
#[instruction(burn_amount: u64)]
pub struct UpdateBlog<'info> {
    #[account(mut)]
    pub updater: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"blog", updater.key().as_ref()],
        bump = blog.bump,
        constraint = blog.creator == updater.key() @ ErrorCode::UnauthorizedBlogAccess
    )]
    pub blog: Account<'info, Blog>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = updater_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = updater_token_account.owner == updater.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub updater_token_account: InterfaceAccount<'info, TokenAccount>,

    /// User global burn statistics tracking account (now required)
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", updater.key().as_ref()],
        bump,
        seeds::program = memo_burn_program.key()
    )]
    pub user_global_burn_stats: Account<'info, memo_burn::UserGlobalBurnStats>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-burn program
    pub memo_burn_program: Program<'info, MemoBurn>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for burning tokens for a blog
#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct BurnForBlog<'info> {
    #[account(mut)]
    pub burner: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"blog", burner.key().as_ref()],
        bump = blog.bump,
        constraint = blog.creator == burner.key() @ ErrorCode::UnauthorizedBlogAccess
    )]
    pub blog: Account<'info, Blog>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = burner_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = burner_token_account.owner == burner.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub burner_token_account: InterfaceAccount<'info, TokenAccount>,

    /// User global burn statistics tracking account (now required)
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", burner.key().as_ref()],
        bump,
        seeds::program = memo_burn_program.key()
    )]
    pub user_global_burn_stats: Account<'info, memo_burn::UserGlobalBurnStats>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-burn program
    pub memo_burn_program: Program<'info, MemoBurn>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for minting tokens for a blog
#[derive(Accounts)]
pub struct MintForBlog<'info> {
    #[account(mut)]
    pub minter: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"blog", minter.key().as_ref()],
        bump = blog.bump,
        constraint = blog.creator == minter.key() @ ErrorCode::UnauthorizedBlogAccess
    )]
    pub blog: Account<'info, Blog>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA serving as mint authority (from memo-mint program)
    #[account(
        seeds = [b"mint_authority"],
        bump,
        seeds::program = memo_mint_program.key()
    )]
    pub mint_authority: AccountInfo<'info>,
    
    #[account(
        mut,
        constraint = minter_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = minter_token_account.owner == minter.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub minter_token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-mint program
    pub memo_mint_program: Program<'info, MemoMint>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Blog data structure (simpler than Project - no website, no tags)
/// Each user can only have one blog, bound to their pubkey
#[account]
pub struct Blog {
    pub creator: Pubkey,              // Creator (unique identifier for the blog)
    pub created_at: i64,              // Creation timestamp
    pub last_updated: i64,            // Last updated timestamp (updated on blog updates)
    pub name: String,                 // Blog name
    pub description: String,          // Blog description
    pub image: String,                // Blog image info (max 256 chars)
    pub memo_count: u64,              // Number of burn_for_blog + mint_for_blog operations
    pub burned_amount: u64,           // Total burned tokens for this blog
    pub last_memo_time: i64,          // Last burn/mint_for_blog operation timestamp (0 if never)
    pub bump: u8,                     // PDA bump
}

impl Blog {
    /// Calculate maximum space for the account (conservative estimate)
    pub fn calculate_space_max() -> usize {
        8 + // discriminator
        32 + // creator
        8 + // created_at
        8 + // last_updated
        8 + // memo_count
        8 + // burned_amount
        8 + // last_memo_time
        1 + // bump
        4 + 64 + // name (max 64 chars)
        4 + 256 + // description (max 256 chars)
        4 + 256 + // image (max 256 chars)
        128 // safety buffer
    }
}

/// Event emitted when a blog is created
#[event]
pub struct BlogCreatedEvent {
    pub creator: Pubkey,
    pub name: String,
    pub description: String,
    pub image: String,
    pub burn_amount: u64,
    pub timestamp: i64,
}

/// Event emitted when a blog is updated
#[event]
pub struct BlogUpdatedEvent {
    pub creator: Pubkey,
    pub name: String,
    pub description: String,
    pub image: String,
    pub burn_amount: u64,
    pub total_burned: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are burned for a blog
#[event]
pub struct TokensBurnedForBlogEvent {
    pub creator: Pubkey,
    pub amount: u64,
    pub total_burned: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are minted for a blog
#[event]
pub struct TokensMintedForBlogEvent {
    pub creator: Pubkey,
    pub timestamp: i64,
}

/// Error code definitions
#[error_code]
pub enum ErrorCode {
    #[msg("Memo too short. Must be at least 69 bytes to meet memo requirements.")]
    MemoTooShort,
    
    #[msg("Memo too long. Must be at most 800 bytes.")]
    MemoTooLong,
    
    #[msg("Invalid token account: Account must belong to the correct mint.")]
    InvalidTokenAccount,

    #[msg("Unauthorized mint: Only the specified mint address can be used.")]
    UnauthorizedMint,

    #[msg("Unauthorized token account: User must own the token account.")]
    UnauthorizedTokenAccount,
    
    #[msg("Unauthorized blog access: Only the blog creator can perform this operation.")]
    UnauthorizedBlogAccess,
    
    #[msg("Memo required: SPL Memo instruction must be present with valid memo content.")]
    MemoRequired,

    #[msg("Invalid memo format: Memo must contain valid Borsh-formatted data.")]
    InvalidMemoFormat,

    #[msg("Invalid mint memo format: For mint operations, burn_amount must be 0.")]
    InvalidMintMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,

    #[msg("Unsupported blog creation data version. Please use the correct structure version.")]
    UnsupportedBlogDataVersion,

    #[msg("Invalid blog creation data format. Must be valid Borsh-serialized data.")]
    InvalidBlogDataFormat,

    #[msg("Invalid category: Must be 'blog' for blog operations.")]
    InvalidCategory,
    
    #[msg("Invalid category length. Category must be exactly the expected length.")]
    InvalidCategoryLength,
    
    #[msg("Invalid operation: Operation does not match the expected operation for this instruction.")]
    InvalidOperation,

    #[msg("Invalid operation length. Operation must be exactly the expected length.")]
    InvalidOperationLength,

    #[msg("Invalid user pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidUserPubkeyFormat,
    
    #[msg("User pubkey mismatch: The user pubkey in memo must match the transaction signer.")]
    UserPubkeyMismatch,
    
    #[msg("Empty blog name: Blog name field cannot be empty.")]
    EmptyBlogName,
    
    #[msg("Blog name too long: Blog name must be at most 64 characters.")]
    BlogNameTooLong,

    #[msg("Invalid blog name: Blog name contains invalid characters or format.")]
    InvalidBlogName,
    
    #[msg("Blog description too long: Description must be at most 256 characters.")]
    BlogDescriptionTooLong,

    #[msg("Invalid blog description: Description contains invalid characters or format.")]
    InvalidBlogDescription,
    
    #[msg("Blog image too long: Image info must be at most 256 characters.")]
    BlogImageTooLong,

    #[msg("Invalid blog image: Image info contains invalid characters or format.")]
    InvalidBlogImage,
    
    #[msg("Burn amount too small. Must burn at least 1 token (1,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Burn amount too large. Maximum allowed: 1,000,000,000,000 tokens per transaction.")]
    BurnAmountTooLarge,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Burn amount mismatch. The burn_amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Payload too long. (maximum 787 bytes).")]
    PayloadTooLong,

    #[msg("Invalid creator pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidCreatorPubkeyFormat,
    
    #[msg("Creator pubkey mismatch: The creator pubkey in memo must match the transaction signer.")]
    CreatorPubkeyMismatch,

    #[msg("Unsupported blog burn data version. Please use the correct structure version.")]
    UnsupportedBlogBurnDataVersion,

    #[msg("Invalid blog burn data format. Must be valid Borsh-serialized data.")]
    InvalidBlogBurnDataFormat,

    #[msg("Unsupported blog mint data version. Please use the correct structure version.")]
    UnsupportedBlogMintDataVersion,

    #[msg("Invalid blog mint data format. Must be valid Borsh-serialized data.")]
    InvalidBlogMintDataFormat,

    #[msg("Invalid burner pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidBurnerPubkeyFormat,
    
    #[msg("Burner pubkey mismatch: The burner pubkey in memo must match the transaction signer.")]
    BurnerPubkeyMismatch,

    #[msg("Invalid minter pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidMinterPubkeyFormat,
    
    #[msg("Minter pubkey mismatch: The minter pubkey in memo must match the transaction signer.")]
    MinterPubkeyMismatch,
    
    #[msg("Message too long: Message must be at most 696 characters.")]
    MessageTooLong,
}
