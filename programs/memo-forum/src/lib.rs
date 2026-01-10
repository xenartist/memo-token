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
#[cfg(feature = "mainnet")]
declare_id!("6gzhG5BveTkJfTi466toX4qmN3BtU9qp1Grnk61GvmXD");

#[cfg(not(feature = "mainnet"))]
declare_id!("9kwS5nSidmoHq84TyNzqFrtD29odp4sdRxm97tCbdpbS");

// Authorized mint address - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Authorized admin for initializing global counter - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");

// ===== BUSINESS LOGIC CONSTANTS =====

// Token economics
pub const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)

// Post creation/update/burn constants - all require at least 1 MEMO token
pub const MIN_POST_BURN_TOKENS: u64 = 1; // Minimum tokens to burn for any post operation
pub const MIN_POST_BURN_AMOUNT: u64 = MIN_POST_BURN_TOKENS * DECIMAL_FACTOR;

// Maximum burn per transaction (consistent with memo-burn)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR; // 1 trillion tokens

// ===== STRING LENGTH CONSTRAINTS =====

// Post metadata limits
pub const MAX_POST_TITLE_LENGTH: usize = 128;     // Post title (required)
pub const MAX_POST_CONTENT_LENGTH: usize = 512;   // Post content (required)
pub const MAX_POST_IMAGE_LENGTH: usize = 256;     // Post image (optional)

// Reply message length for burn_for_post and mint_for_post
pub const MAX_REPLY_MESSAGE_LENGTH: usize = 512;

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

// Current version of data structures
pub const POST_CREATION_DATA_VERSION: u8 = 1;
pub const POST_BURN_DATA_VERSION: u8 = 1;
pub const POST_MINT_DATA_VERSION: u8 = 1;

// Expected category for memo-forum contract
pub const EXPECTED_CATEGORY: &str = "forum";

// Expected operations
pub const EXPECTED_CREATE_POST_OPERATION: &str = "create_post";
pub const EXPECTED_BURN_FOR_POST_OPERATION: &str = "burn_for_post";
pub const EXPECTED_MINT_FOR_POST_OPERATION: &str = "mint_for_post";

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

/// Post creation data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct PostCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "forum" for memo-forum contract)
    pub category: String,
    
    /// Operation type (must be "create_post" for post creation)
    pub operation: String,
    
    /// Creator pubkey as string (must match the transaction signer)
    pub creator: String,
    
    /// Post ID (provided by client, used as part of PDA seed)
    pub post_id: u64,
    
    /// Post title (required, 1-128 characters)
    pub title: String,
    
    /// Post content (required, 1-512 characters)
    pub content: String,
    
    /// Post image (optional, max 256 characters)
    pub image: String,
}

impl PostCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_creator: Pubkey, expected_post_id: u64) -> Result<()> {
        // Validate version
        if self.version != POST_CREATION_DATA_VERSION {
            msg!("Unsupported post creation data version: {} (expected: {})", 
                 self.version, POST_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedPostDataVersion.into());
        }
        
        // Validate category (must be exactly "forum")
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
        
        // Validate operation (must be exactly "create_post")
        if self.operation != EXPECTED_CREATE_POST_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_CREATE_POST_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_CREATE_POST_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_CREATE_POST_OPERATION.len(), EXPECTED_CREATE_POST_OPERATION);
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
        
        // Validate post_id matches expected
        if self.post_id != expected_post_id {
            msg!("Post ID mismatch: memo {} vs expected {}", self.post_id, expected_post_id);
            return Err(ErrorCode::PostIdMismatch.into());
        }
        
        // Validate title (required, 1-128 characters)
        if self.title.is_empty() || self.title.len() > MAX_POST_TITLE_LENGTH {
            msg!("Invalid post title: '{}' (must be 1-{} characters)", self.title, MAX_POST_TITLE_LENGTH);
            return Err(ErrorCode::InvalidPostTitle.into());
        }
        
        // Validate content (required, 1-512 characters)
        if self.content.is_empty() || self.content.len() > MAX_POST_CONTENT_LENGTH {
            msg!("Invalid post content: {} characters (must be 1-{})", 
                 self.content.len(), MAX_POST_CONTENT_LENGTH);
            return Err(ErrorCode::InvalidPostContent.into());
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > MAX_POST_IMAGE_LENGTH {
            msg!("Invalid post image: {} characters (max: {})", 
                 self.image.len(), MAX_POST_IMAGE_LENGTH);
            return Err(ErrorCode::InvalidPostImage.into());
        }
        
        msg!("Post creation data validation passed: category={}, operation={}, creator={}, post_id={}", 
             self.category, self.operation, self.creator, self.post_id);
        
        Ok(())
    }
}

/// Post burn data structure (stored in BurnMemo.payload for burn_for_post)
/// Note: Anyone can burn for a post (not just the creator)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct PostBurnData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "forum" for memo-forum contract)
    pub category: String,
    
    /// Operation type (must be "burn_for_post" for burning tokens)
    pub operation: String,
    
    /// User pubkey as string (must match the transaction signer)
    pub user: String,
    
    /// Post ID being replied to
    pub post_id: u64,
    
    /// Reply message (optional, max 512 characters)
    pub message: String,
}

impl PostBurnData {
    /// Validate the structure fields
    pub fn validate(&self, expected_user: Pubkey, expected_post_id: u64) -> Result<()> {
        // Validate version
        if self.version != POST_BURN_DATA_VERSION {
            msg!("Unsupported post burn data version: {} (expected: {})", 
                 self.version, POST_BURN_DATA_VERSION);
            return Err(ErrorCode::UnsupportedPostBurnDataVersion.into());
        }
        
        // Validate category (must be exactly "forum")
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
        
        // Validate operation (must be exactly "burn_for_post")
        if self.operation != EXPECTED_BURN_FOR_POST_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_BURN_FOR_POST_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_BURN_FOR_POST_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_BURN_FOR_POST_OPERATION.len(), EXPECTED_BURN_FOR_POST_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate user pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.user)
            .map_err(|_| {
                msg!("Invalid user pubkey format: {}", self.user);
                ErrorCode::InvalidUserPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_user {
            msg!("User pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_user);
            return Err(ErrorCode::UserPubkeyMismatch.into());
        }
        
        // Validate post_id matches expected
        if self.post_id != expected_post_id {
            msg!("Post ID mismatch: memo {} vs expected {}", self.post_id, expected_post_id);
            return Err(ErrorCode::PostIdMismatch.into());
        }
        
        // Validate message length (optional, max 512 characters)
        if self.message.len() > MAX_REPLY_MESSAGE_LENGTH {
            msg!("Reply message too long: {} characters (max: {})", 
                 self.message.len(), MAX_REPLY_MESSAGE_LENGTH);
            return Err(ErrorCode::ReplyMessageTooLong.into());
        }
        
        msg!("Post burn data validation passed: category={}, operation={}, user={}, post_id={}", 
             self.category, self.operation, self.user, self.post_id);
        
        Ok(())
    }
}

/// Post mint data structure (stored in BurnMemo.payload for mint_for_post)
/// Note: Anyone can mint for a post (not just the creator)
/// For mint operations, the burn_amount in BurnMemo should be 0
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct PostMintData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "forum" for memo-forum contract)
    pub category: String,
    
    /// Operation type (must be "mint_for_post" for minting tokens)
    pub operation: String,
    
    /// User pubkey as string (must match the transaction signer)
    pub user: String,
    
    /// Post ID being replied to
    pub post_id: u64,
    
    /// Reply message (optional, max 512 characters)
    pub message: String,
}

impl PostMintData {
    /// Validate the structure fields
    pub fn validate(&self, expected_user: Pubkey, expected_post_id: u64) -> Result<()> {
        // Validate version
        if self.version != POST_MINT_DATA_VERSION {
            msg!("Unsupported post mint data version: {} (expected: {})", 
                 self.version, POST_MINT_DATA_VERSION);
            return Err(ErrorCode::UnsupportedPostMintDataVersion.into());
        }
        
        // Validate category (must be exactly "forum")
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
        
        // Validate operation (must be exactly "mint_for_post")
        if self.operation != EXPECTED_MINT_FOR_POST_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_MINT_FOR_POST_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_MINT_FOR_POST_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_MINT_FOR_POST_OPERATION.len(), EXPECTED_MINT_FOR_POST_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate user pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.user)
            .map_err(|_| {
                msg!("Invalid user pubkey format: {}", self.user);
                ErrorCode::InvalidUserPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_user {
            msg!("User pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_user);
            return Err(ErrorCode::UserPubkeyMismatch.into());
        }
        
        // Validate post_id matches expected
        if self.post_id != expected_post_id {
            msg!("Post ID mismatch: memo {} vs expected {}", self.post_id, expected_post_id);
            return Err(ErrorCode::PostIdMismatch.into());
        }
        
        // Validate message length (optional, max 512 characters)
        if self.message.len() > MAX_REPLY_MESSAGE_LENGTH {
            msg!("Reply message too long: {} characters (max: {})", 
                 self.message.len(), MAX_REPLY_MESSAGE_LENGTH);
            return Err(ErrorCode::ReplyMessageTooLong.into());
        }
        
        msg!("Post mint data validation passed: category={}, operation={}, user={}, post_id={}", 
             self.category, self.operation, self.user, self.post_id);
        
        Ok(())
    }
}

#[program]
pub mod memo_forum {
    use super::*;

    /// Initialize the global post counter (one-time setup, admin only)
    pub fn initialize_global_counter(ctx: Context<InitializeGlobalCounter>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let counter = &mut ctx.accounts.global_counter;
        counter.total_posts = 0;
        
        msg!("Global post counter initialized by admin {} with total_posts: {}", 
             ctx.accounts.admin.key(), counter.total_posts);
        Ok(())
    }

    /// Create a new forum post (requires burning at least 1 MEMO token)
    /// Post ID is automatically assigned from the global counter
    pub fn create_post(
        ctx: Context<CreatePost>,
        expected_post_id: u64,
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 1 token for post creation
        if burn_amount < MIN_POST_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
        if burn_amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if burn_amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Get the next post_id from global counter
        let global_counter = &mut ctx.accounts.global_counter;
        let actual_post_id = global_counter.total_posts;

        // Verify that the expected post_id matches the actual next post_id
        if expected_post_id != actual_post_id {
            msg!("Post ID mismatch: expected {}, but next available ID is {}", 
                 expected_post_id, actual_post_id);
            return Err(ErrorCode::PostIdMismatch.into());
        }

        // Check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo data for post creation
        let post_data = parse_post_creation_borsh_memo(&memo_data, ctx.accounts.creator.key(), actual_post_id, burn_amount)?;
        
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
        
        // Initialize post data after successful burn
        let post = &mut ctx.accounts.post;
        post.post_id = actual_post_id;
        post.creator = ctx.accounts.creator.key();
        post.created_at = timestamp;
        post.last_updated = timestamp;
        post.title = post_data.title.clone();
        post.content = post_data.content.clone();
        post.image = post_data.image.clone();
        post.reply_count = 0; // Initialize reply count (tracks burn_for_post and mint_for_post operations)
        post.burned_amount = burn_amount;
        post.last_reply_time = 0; // Set to 0 initially (no replies yet)
        post.bump = ctx.bumps.post;

        // Increment global counter AFTER successful post creation
        // Using checked_add - if overflow, creation fails (post limit reached)
        global_counter.total_posts = global_counter.total_posts.checked_add(1)
            .ok_or(ErrorCode::PostCounterOverflow)?;

        // Emit post creation event
        emit!(PostCreatedEvent {
            post_id: actual_post_id,
            creator: ctx.accounts.creator.key(),
            title: post_data.title,
            content: post_data.content,
            image: post_data.image,
            burn_amount,
            timestamp,
        });

        msg!("Post {} created successfully by {} with {} tokens burned (total posts: {})", 
             actual_post_id, ctx.accounts.creator.key(), burn_amount / DECIMAL_FACTOR, 
             global_counter.total_posts);
        Ok(())
    }

    /// Burn tokens for a post (ANY USER can reply with burn)
    /// This is a key difference from memo-blog: anyone can burn for any post
    pub fn burn_for_post(
        ctx: Context<BurnForPost>,
        post_id: u64,
        amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 1 token
        if amount < MIN_POST_BURN_AMOUNT {
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
        // Note: user can be any user, not just the post creator
        parse_post_burn_borsh_memo(&memo_data, amount, ctx.accounts.user.key(), post_id)?;

        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.user.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.user_token_account.to_account_info(),
            user_global_burn_stats: ctx.accounts.user_global_burn_stats.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        // Call memo-burn's process_burn instruction
        memo_burn::cpi::process_burn(cpi_ctx, amount)?;
        
        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;
        
        // Update post statistics
        let post = &mut ctx.accounts.post;
        let old_amount = post.burned_amount;
        post.burned_amount = post.burned_amount.saturating_add(amount);
        
        // Update reply count
        post.reply_count = post.reply_count.saturating_add(1);
        
        // Update last reply time
        post.last_reply_time = timestamp;
        
        if post.burned_amount == u64::MAX && old_amount < u64::MAX {
            msg!("Warning: burned_amount overflow detected for post {}", post_id);
        }
        
        msg!("Successfully burned {} tokens for post {} by user {}", 
             amount / DECIMAL_FACTOR, post_id, ctx.accounts.user.key());
        
        // Emit burn event
        emit!(TokensBurnedForPostEvent {
            post_id,
            user: ctx.accounts.user.key(),
            amount,
            total_burned: post.burned_amount,
            reply_count: post.reply_count,
            timestamp,
        });

        Ok(())
    }

    /// Mint tokens for a post (ANY USER can reply with mint)
    /// This is a key difference from memo-blog: anyone can mint for any post
    pub fn mint_for_post(
        ctx: Context<MintForPost>,
        post_id: u64,
    ) -> Result<()> {
        // Check memo instruction with enhanced validation
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo content for mint operation
        // Note: user can be any user, not just the post creator
        parse_post_mint_borsh_memo(&memo_data, ctx.accounts.user.key(), post_id)?;

        // Call memo-mint contract to mint tokens
        let cpi_program = ctx.accounts.memo_mint_program.to_account_info();
        let cpi_accounts = ProcessMint {
            user: ctx.accounts.user.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            mint_authority: ctx.accounts.mint_authority.to_account_info(),
            token_account: ctx.accounts.user_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        // Call memo-mint's process_mint instruction
        memo_mint::cpi::process_mint(cpi_ctx)?;
        
        // Get current timestamp once for consistency and efficiency
        let timestamp = Clock::get()?.unix_timestamp;
        
        // Update post statistics
        let post = &mut ctx.accounts.post;
        
        // Update reply count
        post.reply_count = post.reply_count.saturating_add(1);
        
        // Update last reply time
        post.last_reply_time = timestamp;
        
        msg!("Successfully minted tokens for post {} by user {}", 
             post_id, ctx.accounts.user.key());
        
        // Emit mint event
        emit!(TokensMintedForPostEvent {
            post_id,
            user: ctx.accounts.user.key(),
            reply_count: post.reply_count,
            timestamp,
        });

        Ok(())
    }
}

/// Parse and validate Borsh-formatted memo data for post creation (with Base64 decoding)
fn parse_post_creation_borsh_memo(memo_data: &[u8], expected_creator: Pubkey, expected_post_id: u64, expected_amount: u64) -> Result<PostCreationData> {
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
    
    // Deserialize PostCreationData from payload
    let post_data = PostCreationData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid post creation data format in payload");
            ErrorCode::InvalidPostDataFormat
        })?;
    
    // Validate the post creation data
    post_data.validate(expected_creator, expected_post_id)?;
    
    msg!("Post creation data parsed successfully: creator={}, post_id={}, title={}", 
         post_data.creator, post_data.post_id, post_data.title);

    Ok(post_data)
}

/// Parse and validate Borsh-formatted memo data for post burn (with Base64 decoding)
fn parse_post_burn_borsh_memo(memo_data: &[u8], expected_amount: u64, expected_user: Pubkey, expected_post_id: u64) -> Result<()> {
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
    
    // Deserialize post burn data from payload
    let burn_data = PostBurnData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid post burn data format in payload");
            ErrorCode::InvalidPostBurnDataFormat
        })?;
    
    // Validate post burn data
    burn_data.validate(expected_user, expected_post_id)?;
    
    Ok(())
}

/// Parse and validate Borsh-formatted memo data for post mint (with Base64 decoding)
/// Note: For mint operations, the burn_amount in BurnMemo should be 0
fn parse_post_mint_borsh_memo(memo_data: &[u8], expected_user: Pubkey, expected_post_id: u64) -> Result<()> {
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
    
    // Deserialize post mint data from payload
    let mint_data = PostMintData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid post mint data format in payload");
            ErrorCode::InvalidPostMintDataFormat
        })?;
    
    // Validate post mint data
    mint_data.validate(expected_user, expected_post_id)?;
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-forum instructions (create_post, update_post, etc.)
/// 
/// Compute budget instructions can be placed anywhere in the transaction
/// as they are processed by Solana runtime before instruction execution.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("memo-forum instruction must be at index 1 or later, but current instruction is at index {}", current_index);
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

/// Global post counter account
#[account]
pub struct GlobalPostCounter {
    pub total_posts: u64,  // Total number of posts created (starts at 0)
}

impl GlobalPostCounter {
    pub const SPACE: usize = 8 + // discriminator
        8; // total_posts (u64)
}

/// Account structure for initializing global counter (admin only)
#[derive(Accounts)]
pub struct InitializeGlobalCounter<'info> {
    #[account(
        mut,
        constraint = admin.key() == AUTHORIZED_ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin
    )]
    pub admin: Signer<'info>,
    
    #[account(
        init,
        payer = admin,
        space = GlobalPostCounter::SPACE,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalPostCounter>,
    
    pub system_program: Program<'info, System>,
}

/// Account structure for creating a post
#[derive(Accounts)]
#[instruction(expected_post_id: u64, burn_amount: u64)]
pub struct CreatePost<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalPostCounter>,
    
    /// Post account - PDA derived from post_id (from global counter)
    #[account(
        init,
        payer = creator,
        space = Post::calculate_space_max(),
        seeds = [b"post", expected_post_id.to_le_bytes().as_ref()],
        bump
    )]
    pub post: Account<'info, Post>,
    
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

    /// User global burn statistics tracking account
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

/// Account structure for burning tokens for a post (ANY USER)
#[derive(Accounts)]
#[instruction(post_id: u64, amount: u64)]
pub struct BurnForPost<'info> {
    /// Any user can burn for a post (not restricted to creator)
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"post", post_id.to_le_bytes().as_ref()],
        bump = post.bump,
        // Note: NO creator constraint here - any user can burn for any post
    )]
    pub post: Account<'info, Post>,
    
    #[account(
        mut,
        constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = user_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = user_token_account.owner == user.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    /// User global burn statistics tracking account
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", user.key().as_ref()],
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

/// Account structure for minting tokens for a post (ANY USER)
#[derive(Accounts)]
#[instruction(post_id: u64)]
pub struct MintForPost<'info> {
    /// Any user can mint for a post (not restricted to creator)
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"post", post_id.to_le_bytes().as_ref()],
        bump = post.bump,
        // Note: NO creator constraint here - any user can mint for any post
    )]
    pub post: Account<'info, Post>,
    
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
        constraint = user_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = user_token_account.owner == user.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-mint program
    pub memo_mint_program: Program<'info, MemoMint>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Post data structure
/// Each post is a unique PDA derived from post_id
/// Users can create multiple posts
#[account]
pub struct Post {
    pub post_id: u64,                 // Unique post identifier (part of PDA seed)
    pub creator: Pubkey,              // Post creator
    pub created_at: i64,              // Creation timestamp
    pub last_updated: i64,            // Last updated timestamp (updated on post updates)
    pub title: String,                // Post title (1-128 chars)
    pub content: String,              // Post content (1-512 chars)
    pub image: String,                // Post image (optional, max 256 chars)
    pub reply_count: u64,             // Number of burn_for_post + mint_for_post operations
    pub burned_amount: u64,           // Total burned tokens for this post
    pub last_reply_time: i64,         // Last burn/mint_for_post operation timestamp (0 if never)
    pub bump: u8,                     // PDA bump
}

impl Post {
    /// Calculate maximum space for the account (conservative estimate)
    pub fn calculate_space_max() -> usize {
        8 + // discriminator
        8 + // post_id
        32 + // creator
        8 + // created_at
        8 + // last_updated
        8 + // reply_count
        8 + // burned_amount
        8 + // last_reply_time
        1 + // bump
        4 + 128 + // title (max 128 chars)
        4 + 512 + // content (max 512 chars)
        4 + 256 + // image (max 256 chars)
        128 // safety buffer
    }
}

/// Event emitted when a post is created
#[event]
pub struct PostCreatedEvent {
    pub post_id: u64,
    pub creator: Pubkey,
    pub title: String,
    pub content: String,
    pub image: String,
    pub burn_amount: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are burned for a post
#[event]
pub struct TokensBurnedForPostEvent {
    pub post_id: u64,
    pub user: Pubkey,
    pub amount: u64,
    pub total_burned: u64,
    pub reply_count: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are minted for a post
#[event]
pub struct TokensMintedForPostEvent {
    pub post_id: u64,
    pub user: Pubkey,
    pub reply_count: u64,
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
    
    #[msg("Unauthorized post access: Only the post creator can perform this operation.")]
    UnauthorizedPostAccess,

    #[msg("Unauthorized admin: Only the designated admin can perform this operation.")]
    UnauthorizedAdmin,

    #[msg("Post counter overflow: Maximum number of posts (u64::MAX) reached. No more posts can be created.")]
    PostCounterOverflow,
    
    #[msg("Memo required: SPL Memo instruction must be present with valid memo content.")]
    MemoRequired,

    #[msg("Invalid memo format: Memo must contain valid Borsh-formatted data.")]
    InvalidMemoFormat,

    #[msg("Invalid mint memo format: For mint operations, burn_amount must be 0.")]
    InvalidMintMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,

    #[msg("Unsupported post creation/update data version. Please use the correct structure version.")]
    UnsupportedPostDataVersion,

    #[msg("Invalid post creation/update data format. Must be valid Borsh-serialized data.")]
    InvalidPostDataFormat,

    #[msg("Invalid category: Must be 'forum' for forum operations.")]
    InvalidCategory,
    
    #[msg("Invalid category length. Category must be exactly the expected length.")]
    InvalidCategoryLength,
    
    #[msg("Invalid operation: Operation does not match the expected operation for this instruction.")]
    InvalidOperation,

    #[msg("Invalid operation length. Operation must be exactly the expected length.")]
    InvalidOperationLength,

    #[msg("Invalid creator pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidCreatorPubkeyFormat,
    
    #[msg("Creator pubkey mismatch: The creator pubkey in memo must match the transaction signer.")]
    CreatorPubkeyMismatch,

    #[msg("Invalid user pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidUserPubkeyFormat,
    
    #[msg("User pubkey mismatch: The user pubkey in memo must match the transaction signer.")]
    UserPubkeyMismatch,

    #[msg("Post ID mismatch: The post_id in memo must match the instruction parameter.")]
    PostIdMismatch,
    
    #[msg("Invalid post title: Title must be 1-128 characters.")]
    InvalidPostTitle,
    
    #[msg("Invalid post content: Content must be 1-512 characters.")]
    InvalidPostContent,

    #[msg("Invalid post image: Image must be at most 256 characters.")]
    InvalidPostImage,
    
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

    #[msg("Unsupported post burn data version. Please use the correct structure version.")]
    UnsupportedPostBurnDataVersion,

    #[msg("Invalid post burn data format. Must be valid Borsh-serialized data.")]
    InvalidPostBurnDataFormat,

    #[msg("Unsupported post mint data version. Please use the correct structure version.")]
    UnsupportedPostMintDataVersion,

    #[msg("Invalid post mint data format. Must be valid Borsh-serialized data.")]
    InvalidPostMintDataFormat,
    
    #[msg("Reply message too long: Message must be at most 512 characters.")]
    ReplyMessageTooLong,
}
