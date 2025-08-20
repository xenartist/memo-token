#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::Token2022;
use memo_burn::program::MemoBurn;
use memo_burn::cpi::accounts::ProcessBurn;
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use spl_memo::ID as MEMO_PROGRAM_ID;
use base64::{Engine as _, engine::general_purpose};

// ===== BUSINESS LOGIC CONSTANTS =====

// Token economics
pub const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)
pub const MIN_PROFILE_CREATION_BURN_TOKENS: u64 = 420; // Minimum tokens to burn for profile creation
pub const MIN_PROFILE_CREATION_BURN_AMOUNT: u64 = MIN_PROFILE_CREATION_BURN_TOKENS * DECIMAL_FACTOR;

// Maximum burn per transaction (consistent with memo-burn)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR; // 1 trillion tokens

// ===== STRING LENGTH CONSTRAINTS =====

// Profile metadata limits
pub const MAX_USERNAME_LENGTH: usize = 32;
pub const MAX_PROFILE_IMAGE_LENGTH: usize = 256;
pub const MAX_ABOUT_ME_LENGTH: usize = 128;
pub const MAX_URL_LENGTH: usize = 128;

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
pub const MAX_BORSH_DATA_SIZE: usize = 1024;

// Current version of BurnMemo structure (consistent with memo-burn)
pub const BURN_MEMO_VERSION: u8 = 1;

// Current version of ProfileCreationData structure
pub const PROFILE_CREATION_DATA_VERSION: u8 = 1;

// Expected category for memo-profile contract
pub const EXPECTED_CATEGORY: &str = "profile";

// Expected operation for profile creation
pub const EXPECTED_OPERATION: &str = "create_profile";

declare_id!("BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US");

// Authorized mint address
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

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

/// Profile creation data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ProfileCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "profile" for memo-profile contract)
    pub category: String,
    
    /// Operation type (must be "create_profile" for profile creation)
    pub operation: String,
    
    /// User pubkey as string (must match the transaction signer)
    pub user_pubkey: String,
    
    /// Username (required, 1-32 characters)
    pub username: String,
    
    /// Profile image info (optional, max 256 characters)
    pub image: String,
    
    /// About me description (optional, max 128 characters)
    pub about_me: Option<String>,
    
    /// URL (optional, max 128 characters)
    pub url: Option<String>,
}

impl ProfileCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_user: Pubkey) -> Result<()> {
        // Validate version
        if self.version != PROFILE_CREATION_DATA_VERSION {
            msg!("Unsupported profile creation data version: {} (expected: {})", 
                 self.version, PROFILE_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedProfileDataVersion.into());
        }
        
        // Validate category (must be exactly "profile")
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
        
        // Validate operation (must be exactly "create_profile")
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
        
        // Validate user_pubkey matches transaction signer
        let parsed_pubkey = Pubkey::from_str(&self.user_pubkey)
            .map_err(|_| {
                msg!("Invalid user_pubkey format: {}", self.user_pubkey);
                ErrorCode::InvalidUserPubkeyFormat
            })?;
        
        if parsed_pubkey != expected_user {
            msg!("User pubkey mismatch: memo {} vs expected {}", parsed_pubkey, expected_user);
            return Err(ErrorCode::UserPubkeyMismatch.into());
        }
        
        // Validate username (required, 1-32 characters)
        if self.username.is_empty() {
            msg!("Username cannot be empty");
            return Err(ErrorCode::EmptyUsername.into());
        }
        
        if self.username.len() > MAX_USERNAME_LENGTH {
            msg!("Username too long: {} characters (max: {})", 
                 self.username.len(), MAX_USERNAME_LENGTH);
            return Err(ErrorCode::UsernameTooLong.into());
        }
        
        // Validate image length (optional, max 256 characters)
        if self.image.len() > MAX_PROFILE_IMAGE_LENGTH {
            msg!("Profile image too long: {} characters (max: {})", 
                 self.image.len(), MAX_PROFILE_IMAGE_LENGTH);
            return Err(ErrorCode::ProfileImageTooLong.into());
        }
        
        // Validate about_me length (optional, max 128 characters)
        if let Some(ref about_me) = self.about_me {
            if about_me.len() > MAX_ABOUT_ME_LENGTH {
                msg!("About me too long: {} characters (max: {})", 
                     about_me.len(), MAX_ABOUT_ME_LENGTH);
                return Err(ErrorCode::AboutMeTooLong.into());
            }
        }
        
        // Validate url length (optional, max 128 characters)
        if let Some(ref url) = self.url {
            if url.len() > MAX_URL_LENGTH {
                msg!("URL too long: {} characters (max: {})", 
                     url.len(), MAX_URL_LENGTH);
                return Err(ErrorCode::UrlTooLong.into());
            }
        }
        
        msg!("Profile creation data validation passed: category={}, operation={}, user={}, username={}", 
             self.category, self.operation, self.user_pubkey, self.username);
        
        Ok(())
    }
}

#[program]
pub mod memo_profile {
    use super::*;

    /// Create a user profile (requires burning tokens)
    pub fn create_profile(
        ctx: Context<CreateProfile>,
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 420 tokens for profile creation
        if burn_amount < MIN_PROFILE_CREATION_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // Check burn amount limit
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

        // Parse and validate Borsh memo data for profile creation
        let profile_data = parse_profile_creation_borsh_memo(&memo_data, ctx.accounts.user.key(), burn_amount)?;
        
        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.user.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.user_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        memo_burn::cpi::process_burn(cpi_ctx, burn_amount)?;
        
        // Initialize profile data after successful burn
        let profile = &mut ctx.accounts.profile;
        profile.pubkey = ctx.accounts.user.key();
        profile.username = profile_data.username.clone();
        profile.image = profile_data.image.clone();
        profile.burned_amount = burn_amount;
        profile.created_at = Clock::get()?.unix_timestamp;
        profile.last_updated = Clock::get()?.unix_timestamp;
        profile.about_me = profile_data.about_me.clone();
        profile.url = profile_data.url.clone();
        profile.bump = ctx.bumps.profile;

        // Emit profile creation event
        emit!(ProfileCreatedEvent {
            user: ctx.accounts.user.key(),
            username: profile_data.username,
            image: profile_data.image,
            about_me: profile_data.about_me,
            url: profile_data.url,
            burn_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Profile created successfully for user {} with {} tokens burned", 
             ctx.accounts.user.key(), burn_amount / DECIMAL_FACTOR);

        Ok(())
    }

    /// Update an existing profile
    pub fn update_profile(
        ctx: Context<UpdateProfile>,
        username: Option<String>,
        image: Option<String>,
        about_me: Option<Option<String>>,
        url: Option<Option<String>>,
    ) -> Result<()> {
        let profile = &mut ctx.accounts.profile;
        
        // Validate username if provided
        if let Some(ref new_username) = username {
            if new_username.is_empty() {
                return Err(ErrorCode::EmptyUsername.into());
            }
            if new_username.len() > MAX_USERNAME_LENGTH {
                return Err(ErrorCode::UsernameTooLong.into());
            }
            profile.username = new_username.clone();
        }
        
        // Validate image if provided
        if let Some(ref new_image) = image {
            if new_image.len() > MAX_PROFILE_IMAGE_LENGTH {
                return Err(ErrorCode::ProfileImageTooLong.into());
            }
            profile.image = new_image.clone();
        }
        
        // Update about_me if provided
        if let Some(new_about_me) = about_me {
            if let Some(ref about_me_text) = new_about_me {
                if about_me_text.len() > MAX_ABOUT_ME_LENGTH {
                    return Err(ErrorCode::AboutMeTooLong.into());
                }
            }
            profile.about_me = new_about_me;
        }
        
        // Update url if provided
        if let Some(new_url) = url {
            if let Some(ref url_text) = new_url {
                if url_text.len() > MAX_URL_LENGTH {
                    return Err(ErrorCode::UrlTooLong.into());
                }
            }
            profile.url = new_url;
        }
        
        // Update timestamp
        profile.last_updated = Clock::get()?.unix_timestamp;

        // Emit profile update event
        emit!(ProfileUpdatedEvent {
            user: ctx.accounts.user.key(),
            username: profile.username.clone(),
            image: profile.image.clone(),
            about_me: profile.about_me.clone(),
            url: profile.url.clone(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Profile updated successfully for user {}", ctx.accounts.user.key());

        Ok(())
    }

    /// Delete a user profile (user can only delete their own profile)
    pub fn delete_profile(ctx: Context<DeleteProfile>) -> Result<()> {
        let profile = &ctx.accounts.profile;
        
        // Store profile info for event before deletion
        let user_pubkey = profile.pubkey;
        let username = profile.username.clone();

        // Emit profile deletion event
        emit!(ProfileDeletedEvent {
            user: user_pubkey,
            username,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Profile deleted successfully for user {}", user_pubkey);

        // Account closure is handled automatically by Anchor through close constraint
        Ok(())
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

/// Check for memo instruction at REQUIRED index 1
/// 
/// IMPORTANT: This contract enforces a strict instruction ordering:
/// - Index 0: Compute budget instruction (optional)
/// - Index 1: SPL Memo instruction (REQUIRED)
/// - Index 2+: memo-profile::create_profile (other instructions)
///
/// Any deviation from this pattern will result in transaction failure.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Ensure there are enough instructions (at least index 1 must exist)
    if current_index <= 1 {
        msg!("Memo instruction must be at index 1, but transaction only has {} instructions", current_index);
        return Ok((false, vec![]));
    }
    
    // Check fixed position: index 1
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at required index 1");
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH)
            } else {
                msg!("Instruction at index 1 is not a memo (program_id: {})", ix.program_id);
                Ok((false, vec![]))
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at required index 1: {:?}", e);
            Ok((false, vec![]))
        }
    }
}

/// Parse and validate Borsh-formatted memo data for profile creation (with Base64 decoding)
fn parse_profile_creation_borsh_memo(memo_data: &[u8], expected_user: Pubkey, expected_amount: u64) -> Result<ProfileCreationData> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidProfileDataFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidProfileDataFormat
        })?;

    // check decoded borsh data size
    if decoded_data.len() > MAX_BORSH_DATA_SIZE {
        msg!("Decoded data too large: {} bytes (max: {})", decoded_data.len(), MAX_BORSH_DATA_SIZE);
        return Err(ErrorCode::InvalidProfileDataFormat.into());
    }
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes (following memo-burn pattern)
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidProfileDataFormat
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
    
    // Deserialize profile creation data from payload
    let profile_data = ProfileCreationData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid profile creation data format in payload");
            ErrorCode::InvalidProfileDataFormat
        })?;
    
    // Validate profile creation data
    profile_data.validate(expected_user)?;
    
    Ok(profile_data)
}

/// Account structure for creating a profile
#[derive(Accounts)]
pub struct CreateProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        init,
        payer = user,
        space = Profile::calculate_space_max(),
        seeds = [b"profile", user.key().as_ref()],
        bump
    )]
    pub profile: Account<'info, Profile>,
    
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
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-burn program
    pub memo_burn_program: Program<'info, MemoBurn>,
    
    pub system_program: Program<'info, System>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for updating a profile
#[derive(Accounts)]
pub struct UpdateProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"profile", user.key().as_ref()],
        bump = profile.bump,
        constraint = profile.pubkey == user.key() @ ErrorCode::UnauthorizedProfileAccess
    )]
    pub profile: Account<'info, Profile>,
}

/// Account structure for deleting a profile
#[derive(Accounts)]
pub struct DeleteProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"profile", user.key().as_ref()],
        bump = profile.bump,
        constraint = profile.pubkey == user.key() @ ErrorCode::UnauthorizedProfileAccess,
        close = user // Close account and send rent back to user
    )]
    pub profile: Account<'info, Profile>,
}

/// Profile data structure
#[account]
pub struct Profile {
    pub pubkey: Pubkey,           // 32 bytes - user pubkey (natural ID)
    pub username: String,         // 4 + 32 bytes - username, max 32 characters
    pub image: String,            // 4 + 256 bytes - profile image, hex string
    pub burned_amount: u64,       // 8 bytes - amount burned to create profile
    pub created_at: i64,          // 8 bytes - created timestamp
    pub last_updated: i64,        // 8 bytes - last updated timestamp
    pub about_me: Option<String>, // 1 + 4 + 128 bytes - about me, max 128 characters, optional
    pub url: Option<String>,      // 1 + 4 + 128 bytes - url, max 128 characters, optional
    pub bump: u8,                 // 1 byte - PDA bump
}

impl Profile {
    /// Calculate maximum space for the account (conservative estimate)
    pub fn calculate_space_max() -> usize {
        8 + // discriminator
        32 + // pubkey
        8 + // burned_amount
        8 + // created_at
        8 + // last_updated
        1 + // bump
        4 + 32 + // username
        4 + 256 + // image
        1 + 4 + 128 + // about_me (Option<String>)
        1 + 4 + 128 + // url (Option<String>)
        128 // safety buffer
    }
}

/// Event emitted when a profile is created
#[event]
pub struct ProfileCreatedEvent {
    pub user: Pubkey,
    pub username: String,
    pub image: String,
    pub about_me: Option<String>,
    pub url: Option<String>,
    pub burn_amount: u64,
    pub timestamp: i64,
}

/// Event emitted when a profile is updated
#[event]
pub struct ProfileUpdatedEvent {
    pub user: Pubkey,
    pub username: String,
    pub image: String,
    pub about_me: Option<String>,
    pub url: Option<String>,
    pub timestamp: i64,
}

/// Event emitted when a profile is deleted
#[event]
pub struct ProfileDeletedEvent {
    pub user: Pubkey,
    pub username: String,
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
    
    #[msg("Unauthorized profile access: User can only access their own profile.")]
    UnauthorizedProfileAccess,
    
    #[msg("Memo required: SPL Memo instruction must be present with valid memo content.")]
    MemoRequired,

    #[msg("Invalid memo format: Memo must contain valid Borsh-formatted data.")]
    InvalidMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,

    #[msg("Unsupported profile creation data version. Please use the correct structure version.")]
    UnsupportedProfileDataVersion,

    #[msg("Invalid profile creation data format. Must be valid Borsh-serialized data.")]
    InvalidProfileDataFormat,

    #[msg("Invalid category: Must be 'profile' for profile operations.")]
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
    
    #[msg("Empty username: Username field cannot be empty.")]
    EmptyUsername,
    
    #[msg("Username too long: Username must be at most 32 characters.")]
    UsernameTooLong,
    
    #[msg("Profile image too long: Image info must be at most 256 characters.")]
    ProfileImageTooLong,
    
    #[msg("About me too long: About me must be at most 128 characters.")]
    AboutMeTooLong,
    
    #[msg("URL too long: URL must be at most 128 characters.")]
    UrlTooLong,

    #[msg("Burn amount too small. Must burn at least 420 tokens (420,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Burn amount too large. Maximum allowed: 1,000,000,000,000 tokens per transaction.")]
    BurnAmountTooLarge,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Burn amount mismatch. The burn_amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Payload too long. (maximum 787 bytes).")]
    PayloadTooLong,
}
