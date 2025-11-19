#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;

#[cfg(test)]
mod tests;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::Token2022;
use memo_burn::program::MemoBurn;
use memo_burn::cpi::accounts::ProcessBurn;
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use spl_memo::ID as MEMO_PROGRAM_ID;
use base64::{Engine as _, engine::general_purpose};
use std::str::FromStr;

// Program ID - different for testnet and mainnet
#[cfg(feature = "mainnet")]
declare_id!("6Vavot6ybhWBG3rjNXnLfNRPVTz7Garf6E4EZk3byp3a");

#[cfg(not(feature = "mainnet"))]
declare_id!("ENVapgjzzMjbRhLJ279yNsSgaQtDYYVgWq98j54yYnyx");

// Authorized mint address - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Authorized admin key - different for testnet and mainnet
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");


// ===== BUSINESS LOGIC CONSTANTS =====

// Token economics
pub const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)
pub const MIN_PROJECT_CREATION_BURN_TOKENS: u64 = 42069; // Minimum tokens to burn for project creation
pub const MIN_PROJECT_CREATION_BURN_AMOUNT: u64 = MIN_PROJECT_CREATION_BURN_TOKENS * DECIMAL_FACTOR;

// Project burn constants
pub const MIN_PROJECT_BURN_TOKENS: u64 = 420; // Minimum tokens to burn for project
pub const MIN_PROJECT_BURN_AMOUNT: u64 = MIN_PROJECT_BURN_TOKENS * DECIMAL_FACTOR;

// Project update constants  
pub const MIN_PROJECT_UPDATE_BURN_TOKENS: u64 = 42069; // Minimum tokens to burn for project update
pub const MIN_PROJECT_UPDATE_BURN_AMOUNT: u64 = MIN_PROJECT_UPDATE_BURN_TOKENS * DECIMAL_FACTOR;

// Maximum burn per transaction (consistent with memo-burn)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR; // 1 trillion tokens

// ===== STRING LENGTH CONSTRAINTS =====

// Project metadata limits
pub const MAX_PROJECT_NAME_LENGTH: usize = 64;
pub const MAX_PROJECT_DESCRIPTION_LENGTH: usize = 256; 
pub const MAX_PROJECT_IMAGE_LENGTH: usize = 256;        
pub const MAX_PROJECT_WEBSITE_LENGTH: usize = 128;      
pub const MAX_TAGS_COUNT: usize = 4;
pub const MAX_TAG_LENGTH: usize = 32;

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

// Current version of ProjectCreationData structure
pub const PROJECT_CREATION_DATA_VERSION: u8 = 1;

// Current version of ProjectUpdateData structure  
pub const PROJECT_UPDATE_DATA_VERSION: u8 = 1;

// Expected category for memo-project contract
pub const EXPECTED_CATEGORY: &str = "project";

// Expected operation for project creation
pub const EXPECTED_OPERATION: &str = "create_project";

// Expected operation for project update
pub const EXPECTED_UPDATE_OPERATION: &str = "update_project";

// maximum burn message length
pub const MAX_BURN_MESSAGE_LENGTH: usize = 696;

// expected operation for project burn
pub const EXPECTED_BURN_FOR_PROJECT_OPERATION: &str = "burn_for_project";

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

/// Project creation data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ProjectCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "project" for memo-project contract)
    pub category: String,
    
    /// Operation type (must be "create_project" for project creation)
    pub operation: String,
    
    /// Project ID (must match expected_project_id)
    pub project_id: u64,
    
    /// Project name (required, 1-64 characters)
    pub name: String,
    
    /// Project description (optional, max 256 characters)
    pub description: String,
    
    /// Project image info (optional, max 256 characters)
    pub image: String,
    
    /// Project website URL (optional, max 128 characters)
    pub website: String,
    
    /// Tags (optional, max 4 tags, each max 32 characters)
    pub tags: Vec<String>,
}

impl ProjectCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_project_id: u64) -> Result<()> {
        // Validate version
        if self.version != PROJECT_CREATION_DATA_VERSION {
            msg!("Unsupported project creation data version: {} (expected: {})", 
                 self.version, PROJECT_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedProjectDataVersion.into());
        }
        
        // Validate category (must be exactly "project")
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
        
        // Validate operation (must be exactly "create_project")
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
        
        // Validate project_id
        if self.project_id != expected_project_id {
            msg!("Project ID mismatch: data contains {}, expected {}", 
                 self.project_id, expected_project_id);
            return Err(ErrorCode::ProjectIdMismatch.into());
        }
        
        // Validate name (required, 1-64 characters)
        if self.name.is_empty() || self.name.len() > MAX_PROJECT_NAME_LENGTH {
            msg!("Invalid project name: '{}' (must be 1-{} characters)", self.name, MAX_PROJECT_NAME_LENGTH);
            return Err(ErrorCode::InvalidProjectName.into());
        }
        
        // Validate description (optional, max 256 characters)
        if self.description.len() > MAX_PROJECT_DESCRIPTION_LENGTH {
            msg!("Invalid project description: {} characters (max: {})", 
                 self.description.len(), MAX_PROJECT_DESCRIPTION_LENGTH);
            return Err(ErrorCode::InvalidProjectDescription.into());
        }
        
        // Validate image (optional, max 256 characters)
        if self.image.len() > MAX_PROJECT_IMAGE_LENGTH {
            msg!("Invalid project image: {} characters (max: {})", 
                 self.image.len(), MAX_PROJECT_IMAGE_LENGTH);
            return Err(ErrorCode::InvalidProjectImage.into());
        }
        
        // Validate website (optional, max 128 characters)
        if self.website.len() > MAX_PROJECT_WEBSITE_LENGTH {
            msg!("Invalid project website: {} characters (max: {})", 
                 self.website.len(), MAX_PROJECT_WEBSITE_LENGTH);
            return Err(ErrorCode::InvalidProjectWebsite.into());
        }
        
        // Validate tags (optional, max 4 tags, each max 32 characters)
        if self.tags.len() > MAX_TAGS_COUNT {
            msg!("Too many tags: {} (max: {})", self.tags.len(), MAX_TAGS_COUNT);
            return Err(ErrorCode::TooManyTags.into());
        }
        
        for (i, tag) in self.tags.iter().enumerate() {
            if tag.is_empty() || tag.len() > MAX_TAG_LENGTH {
                msg!("Invalid tag {}: '{}' (must be 1-{} characters)", i, tag, MAX_TAG_LENGTH);
                return Err(ErrorCode::InvalidTag.into());
            }
        }
        
        msg!("Project creation data validation passed: category={}, operation={}, project_id={}, name={}, tags_count={}", 
             self.category, self.operation, self.project_id, self.name, self.tags.len());
        
        Ok(())
    }
}

/// Project update data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ProjectUpdateData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "project" for memo-project contract)
    pub category: String,
    
    /// Operation type (must be "update_project" for project update)
    pub operation: String,
    
    /// Project ID (must match the target project)
    pub project_id: u64,
    
    /// Updated fields (all optional)
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub website: Option<String>,
    pub tags: Option<Vec<String>>,
}

impl ProjectUpdateData {
    /// Validate the structure fields
    pub fn validate(&self, expected_project_id: u64) -> Result<()> {
        // Validate version
        if self.version != PROJECT_UPDATE_DATA_VERSION {
            msg!("Unsupported project update data version: {} (expected: {})", 
                 self.version, PROJECT_UPDATE_DATA_VERSION);
            return Err(ErrorCode::UnsupportedProjectDataVersion.into());
        }
        
        // Validate category (must be exactly "project")
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
        
        // Validate operation (must be exactly "update_project")
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
        
        // Validate project_id
        if self.project_id != expected_project_id {
            msg!("Project ID mismatch: data contains {}, expected {}", 
                 self.project_id, expected_project_id);
            return Err(ErrorCode::ProjectIdMismatch.into());
        }
        
        // Validate name (optional, 1-64 characters)
        if let Some(ref new_name) = self.name {
            if new_name.is_empty() || new_name.len() > MAX_PROJECT_NAME_LENGTH {
                msg!("Invalid project name: '{}' (must be 1-{} characters)", new_name, MAX_PROJECT_NAME_LENGTH);
                return Err(ErrorCode::InvalidProjectName.into());
            }
        }
        
        // Validate description (optional, max 256 characters)
        if let Some(ref new_description) = self.description {
            if new_description.len() > MAX_PROJECT_DESCRIPTION_LENGTH {
                msg!("Invalid project description: {} characters (max: {})", 
                     new_description.len(), MAX_PROJECT_DESCRIPTION_LENGTH);
                return Err(ErrorCode::InvalidProjectDescription.into());
            }
        }
        
        // Validate image (optional, max 256 characters)
        if let Some(ref new_image) = self.image {
            if new_image.len() > MAX_PROJECT_IMAGE_LENGTH {
                msg!("Invalid project image: {} characters (max: {})", 
                     new_image.len(), MAX_PROJECT_IMAGE_LENGTH);
                return Err(ErrorCode::InvalidProjectImage.into());
            }
        }
        
        // Validate website (optional, max 128 characters)
        if let Some(ref new_website) = self.website {
            if new_website.len() > MAX_PROJECT_WEBSITE_LENGTH {
                msg!("Invalid project website: {} characters (max: {})", 
                     new_website.len(), MAX_PROJECT_WEBSITE_LENGTH);
                return Err(ErrorCode::InvalidProjectWebsite.into());
            }
        }
        
        // Validate tags (optional, max 4 tags, each max 32 characters)
        if let Some(ref new_tags) = self.tags {
            if new_tags.len() > MAX_TAGS_COUNT {
                msg!("Too many tags: {} (max: {})", new_tags.len(), MAX_TAGS_COUNT);
                return Err(ErrorCode::TooManyTags.into());
            }
            
            for (i, tag) in new_tags.iter().enumerate() {
                if tag.is_empty() || tag.len() > MAX_TAG_LENGTH {
                    msg!("Invalid tag {}: '{}' (must be 1-{} characters)", i, tag, MAX_TAG_LENGTH);
                    return Err(ErrorCode::InvalidTag.into());
                }
            }
        }
        
        msg!("Project update data validation passed: category={}, operation={}, project_id={}", 
             self.category, self.operation, self.project_id);
        
        Ok(())
    }
}

/// Project burn data structure (stored in BurnMemo.payload for burn_for_project)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ProjectBurnData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "project" for memo-project contract)
    pub category: String,
    
    /// Operation type (must be "burn_for_project" for burning tokens)
    pub operation: String,
    
    /// Project ID (must match the target project)
    pub project_id: u64,
    
    /// Burner pubkey as string (must match the transaction signer)
    pub burner: String,
    
    /// Burn message (optional, max 696 characters)
    pub message: String,
}

impl ProjectBurnData {
    /// Validate the structure fields
    pub fn validate(&self, expected_project_id: u64, expected_burner: Pubkey) -> Result<()> {
        // Validate version
        if self.version != PROJECT_CREATION_DATA_VERSION {
            msg!("Unsupported project burn data version: {} (expected: {})", 
                 self.version, PROJECT_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedProjectBurnDataVersion.into());
        }
        
        // Validate category (must be exactly "project")
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
        
        // Validate operation (must be exactly "burn_for_project")
        if self.operation != EXPECTED_BURN_FOR_PROJECT_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_BURN_FOR_PROJECT_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_BURN_FOR_PROJECT_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_BURN_FOR_PROJECT_OPERATION.len(), EXPECTED_BURN_FOR_PROJECT_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate project_id matches
        if self.project_id != expected_project_id {
            msg!("Project ID mismatch: memo {} vs expected {}", self.project_id, expected_project_id);
            return Err(ErrorCode::ProjectIdMismatch.into());
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
        if self.message.len() > MAX_BURN_MESSAGE_LENGTH {
            msg!("Burn message too long: {} characters (max: {})", 
                 self.message.len(), MAX_BURN_MESSAGE_LENGTH);
            return Err(ErrorCode::BurnMessageTooLong.into());
        }
        
        msg!("Project burn data validation passed: category={}, operation={}, project_id={}, burner={}", 
             self.category, self.operation, self.project_id, self.burner);
        
        Ok(())
    }
}

#[program]
pub mod memo_project {
    use super::*;

    /// Initialize the global project counter (one-time setup, admin only)
    pub fn initialize_global_counter(ctx: Context<InitializeGlobalCounter>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let counter = &mut ctx.accounts.global_counter;
        counter.total_projects = 0;
        
        msg!("Global project counter initialized by admin {} with total_projects: {}", 
             ctx.accounts.admin.key(), counter.total_projects);
        Ok(())
    }

    /// Create a new project (requires burning tokens)
    /// Note: project_id will be automatically assigned by the contract
    pub fn create_project(
        ctx: Context<CreateProject>,
        expected_project_id: u64, // The project_id that client expects to create
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 69420 tokens for project creation
        if burn_amount < MIN_PROJECT_CREATION_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
        if burn_amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if burn_amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Get the next project_id from global counter
        let global_counter = &mut ctx.accounts.global_counter;
        let actual_project_id = global_counter.total_projects;

        // Verify that the expected project_id matches the actual next project_id
        if expected_project_id != actual_project_id {
            msg!("Project ID mismatch: expected {}, but next available ID is {}", 
                 expected_project_id, actual_project_id);
            return Err(ErrorCode::ProjectIdMismatch.into());
        }

        // Check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo data for project creation
        let project_data = parse_project_creation_borsh_memo(&memo_data, actual_project_id, burn_amount)?;
        
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
        
        // Initialize project data after successful burn
        let project = &mut ctx.accounts.project;
        project.project_id = actual_project_id;
        project.creator = ctx.accounts.creator.key();
        project.created_at = timestamp;
        project.last_updated = timestamp;
        project.name = project_data.name.clone();
        project.description = project_data.description.clone();
        project.image = project_data.image.clone();
        project.website = project_data.website.clone();
        project.tags = project_data.tags.clone();
        project.memo_count = 0; // Initialize memo_count (only tracks burn_for_project operations)
        project.burned_amount = burn_amount;
        project.last_memo_time = 0; // Set to 0 initially (no burn_for_project memos yet)
        project.bump = ctx.bumps.project;

        // Increment global counter AFTER successful project creation
        global_counter.total_projects = global_counter.total_projects.checked_add(1)
            .ok_or(ErrorCode::ProjectCounterOverflow)?;

        // Emit project creation event
        emit!(ProjectCreatedEvent {
            project_id: actual_project_id,
            creator: ctx.accounts.creator.key(),
            name: project_data.name,
            description: project_data.description,
            image: project_data.image,
            website: project_data.website,
            tags: project_data.tags,
            burn_amount,
            timestamp,
        });

        // Update burn leaderboard after successful project creation
        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        let entered_leaderboard = leaderboard.update_leaderboard(actual_project_id, burn_amount)?;

        if entered_leaderboard {
            msg!("Project {} entered burn leaderboard", actual_project_id);
        } else {
            msg!("Project {} burn amount {} not sufficient for leaderboard", 
                 actual_project_id, burn_amount / DECIMAL_FACTOR);
        }

        msg!("Project {} created successfully by {} with {} tokens burned", 
             actual_project_id, ctx.accounts.creator.key(), burn_amount / DECIMAL_FACTOR);
        Ok(())
    }

    /// Update an existing project (requires burning tokens)
    pub fn update_project(
        ctx: Context<UpdateProject>,
        project_id: u64,
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 42069 tokens for project update
        if burn_amount < MIN_PROJECT_UPDATE_BURN_AMOUNT {
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

        // Parse and validate Borsh memo data for project update
        let update_data = parse_project_update_borsh_memo(&memo_data, project_id, burn_amount)?;
        
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

        let project = &mut ctx.accounts.project;
        
        // Update fields if provided in memo data
        if let Some(new_name) = update_data.name {
            project.name = new_name;
        }
        
        if let Some(new_description) = update_data.description {
            project.description = new_description;
        }
        
        if let Some(new_image) = update_data.image {
            project.image = new_image;
        }
        
        if let Some(new_website) = update_data.website {
            project.website = new_website;
        }
        
        if let Some(new_tags) = update_data.tags {
            project.tags = new_tags;
        }
        
        // Update burn amount and timestamp
        project.burned_amount = project.burned_amount.saturating_add(burn_amount);
        project.last_updated = timestamp;
        // Note: last_memo_time is NOT updated here - only tracks burn_for_project operations

        // Emit project update event
        emit!(ProjectUpdatedEvent {
            project_id,
            updater: ctx.accounts.updater.key(),
            name: project.name.clone(),
            description: project.description.clone(),
            image: project.image.clone(),
            website: project.website.clone(),
            tags: project.tags.clone(), // Emit all tags
            burn_amount,
            total_burned: project.burned_amount,
            timestamp,
        });

        // Update burn leaderboard after successful project update
        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        let total_burned = project.burned_amount;
        let entered_leaderboard = leaderboard.update_leaderboard(project_id, total_burned)?;

        if entered_leaderboard {
            msg!("Project {} updated in burn leaderboard with total {} tokens", 
                 project_id, total_burned / DECIMAL_FACTOR);
        } else {
            msg!("Project {} total burn amount {} not sufficient for leaderboard", 
                 project_id, total_burned / DECIMAL_FACTOR);
        }

        msg!("Project {} updated successfully by {} with {} tokens burned (total: {})", 
             project_id, ctx.accounts.updater.key(), burn_amount / DECIMAL_FACTOR, 
             project.burned_amount / DECIMAL_FACTOR);
        Ok(())
    }

    /// Initialize the burn leaderboard (one-time setup, admin only)
    pub fn initialize_burn_leaderboard(ctx: Context<InitializeBurnLeaderboard>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        leaderboard.initialize(); // Use the initialize method
        
        msg!("Burn leaderboard initialized by admin {}", ctx.accounts.admin.key());
        Ok(())
    }

    /// Burn tokens for a project (only project creator can burn)
    pub fn burn_for_project(
        ctx: Context<BurnForProject>,
        project_id: u64,
        amount: u64,
    ) -> Result<()> {
        // Validate burn amount
        if amount < MIN_PROJECT_BURN_AMOUNT {
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
        parse_project_burn_borsh_memo(&memo_data, project_id, amount, ctx.accounts.burner.key())?;

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
        
        // Update project burned amount tracking
        let project = &mut ctx.accounts.project;
        let old_amount = project.burned_amount;
        project.burned_amount = project.burned_amount.saturating_add(amount);
        
        // Update memo count (only burn_for_project operations count as memos)
        project.memo_count = project.memo_count.saturating_add(1);
        
        // Update last memo time (only tracks burn_for_project operations)
        project.last_memo_time = timestamp;
        
        if project.burned_amount == u64::MAX && old_amount < u64::MAX {
            msg!("Warning: burned_amount overflow detected for project {}", project_id);
        }
        
        msg!("Successfully burned {} tokens for project {}", amount / DECIMAL_FACTOR, project_id);
        
        // Update burn leaderboard after successful burn
        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        let total_burned = project.burned_amount;
        let entered_leaderboard = leaderboard.update_leaderboard(project_id, total_burned)?;

        if entered_leaderboard {
            msg!("Project {} updated in burn leaderboard with total {} tokens", 
                 project_id, total_burned / DECIMAL_FACTOR);
        } else {
            msg!("Project {} total burn amount {} not sufficient for leaderboard", 
                 project_id, total_burned / DECIMAL_FACTOR);
        }

        // Emit burn event
        emit!(TokensBurnedForProjectEvent {
            project_id,
            burner: ctx.accounts.burner.key(),
            amount,
            total_burned: project.burned_amount,
            timestamp,
        });

        Ok(())
    }
}

/// Parse and validate Borsh-formatted memo data for project creation (with Base64 decoding)
fn parse_project_creation_borsh_memo(memo_data: &[u8], expected_project_id: u64, expected_amount: u64) -> Result<ProjectCreationData> {
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
    
    // Deserialize ProjectCreationData from payload
    let project_data = ProjectCreationData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid project creation data format in payload");
            ErrorCode::InvalidProjectDataFormat
        })?;
    
    // Validate the project creation data
    project_data.validate(expected_project_id)?;
    
    msg!("Project creation data parsed successfully: project_id={}, name={}, description_len={}, website_len={}, tags_count={}", 
         project_data.project_id, project_data.name, project_data.description.len(), 
         project_data.website.len(), project_data.tags.len());

    Ok(project_data)
}

/// Parse and validate Borsh-formatted memo data for project update (with Base64 decoding)
fn parse_project_update_borsh_memo(memo_data: &[u8], expected_project_id: u64, expected_amount: u64) -> Result<ProjectUpdateData> {
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
    
    // Deserialize ProjectUpdateData from payload
    let update_data = ProjectUpdateData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid project update data format in payload");
            ErrorCode::InvalidProjectDataFormat
        })?;
    
    // Validate the project update data
    update_data.validate(expected_project_id)?;
    
    msg!("Project update data parsed successfully: project_id={}, has updates: name={}, description={}, image={}, website={}, tag={}", 
         update_data.project_id, 
         update_data.name.is_some(),
         update_data.description.is_some(),
         update_data.image.is_some(),
         update_data.website.is_some(),
         update_data.tags.is_some());

    Ok(update_data)
}

/// Parse and validate Borsh-formatted memo data for project burn (with Base64 decoding)
fn parse_project_burn_borsh_memo(memo_data: &[u8], expected_project_id: u64, expected_amount: u64, expected_burner: Pubkey) -> Result<()> {
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
    
    // Deserialize project burn data from payload
    let burn_data = ProjectBurnData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid project burn data format in payload");
            ErrorCode::InvalidProjectBurnDataFormat
        })?;
    
    // Validate project burn data
    burn_data.validate(expected_project_id, expected_burner)?;
    
    Ok(())
}

/// Check for memo instruction at REQUIRED index 0
/// 
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-project instructions (create_project, update_project, etc.)
/// 
/// Compute budget instructions can be placed anywhere in the transaction
/// as they are processed by Solana runtime before instruction execution.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // Current instruction must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        msg!("memo-project instruction must be at index 1 or later, but current instruction is at index {}", current_index);
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

/// Burn leaderboard entry
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct LeaderboardEntry {
    pub project_id: u64,
    pub burned_amount: u64,
}

/// Burn leaderboard account (stores top 100 projects by burn amount)
#[account]
pub struct BurnLeaderboard {
    /// Array of leaderboard entries (unsorted for performance - sort off-chain for display)
    /// Maximum 100 entries
    pub entries: Vec<LeaderboardEntry>,
}

impl BurnLeaderboard {
    pub const SPACE: usize = 8 + // discriminator
        4 + // Vec length prefix
        100 * 16 + // max entries (100 * (8 + 8) bytes each)
        64; // safety buffer
    
    /// Initialize with empty entries
    pub fn initialize(&mut self) {
        self.entries = Vec::with_capacity(100);
    }
    
    /// find project position and min burned_amount position (core optimization)
    pub fn find_project_position_and_min(&self, project_id: u64) -> (Option<usize>, Option<usize>) {
        if self.entries.is_empty() {
            return (None, None);
        }
        
        let mut min_pos = None;
        let mut min_amount = u64::MAX;
        let mut found_project_pos = None;
        
        // loop all elements
        for (i, entry) in self.entries.iter().enumerate() {
            // record target project position
            if entry.project_id == project_id {
                found_project_pos = Some(i);
            }
            
            // always record min position
            if entry.burned_amount < min_amount {
                min_amount = entry.burned_amount;
                min_pos = Some(i);
            }
        }
        
        (found_project_pos, min_pos)
    }
    
    /// update leaderboard - zero array move version
    pub fn update_leaderboard(&mut self, project_id: u64, new_burned_amount: u64) -> Result<bool> {
        // 1. one loop to get project position and min position
        let (existing_pos, min_pos) = self.find_project_position_and_min(project_id);
        
        // 2. if project exists, update burned_amount (zero move)
        if let Some(pos) = existing_pos {
            self.entries[pos].burned_amount = new_burned_amount;
            return Ok(true);
        }
        
        // 3. new project and leaderboard not full, add directly (no sort)
        if self.entries.len() < 100 {
            let new_entry = LeaderboardEntry {
                project_id,
                burned_amount: new_burned_amount,
            };
            self.entries.push(new_entry);
            return Ok(true);
        }
        
        // 4. new project and leaderboard full, check if can replace min value
        if let Some(min_position) = min_pos {
            let min_amount = self.entries[min_position].burned_amount;
            if new_burned_amount > min_amount {
                // replace min value entry (zero move)
                self.entries[min_position] = LeaderboardEntry {
                    project_id,
                    burned_amount: new_burned_amount,
                };
                return Ok(true);
            } else {
                // new value not big enough, cannot enter leaderboard
                return Ok(false);
            }
        }
        
        Ok(false)
    }
}

/// Global project counter account
#[account]
pub struct GlobalProjectCounter {
    pub total_projects: u64,          // Total number of projects created (starts at 0)
}

impl GlobalProjectCounter {
    pub const SPACE: usize = 8 + // discriminator
        8; // total_projects (u64)
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
        space = GlobalProjectCounter::SPACE,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalProjectCounter>,
    
    pub system_program: Program<'info, System>,
}

/// Account structure for creating a project
#[derive(Accounts)]
#[instruction(expected_project_id: u64, burn_amount: u64)]
pub struct CreateProject<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalProjectCounter>,
    
    #[account(
        init,
        payer = creator,
        space = Project::calculate_space_max(),
        seeds = [b"project", expected_project_id.to_le_bytes().as_ref()],
        bump
    )]
    pub project: Account<'info, Project>,
    
    #[account(
        mut,
        seeds = [b"burn_leaderboard"],
        bump
    )]
    pub burn_leaderboard: Account<'info, BurnLeaderboard>,
    
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

/// Account structure for updating a project
#[derive(Accounts)]
#[instruction(project_id: u64, burn_amount: u64)]
pub struct UpdateProject<'info> {
    #[account(
        mut,
        constraint = updater.key() == project.creator @ ErrorCode::UnauthorizedProjectAccess
    )]
    pub updater: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"project", project_id.to_le_bytes().as_ref()],
        bump = project.bump
    )]
    pub project: Account<'info, Project>,
    
    #[account(
        mut,
        seeds = [b"burn_leaderboard"],
        bump
    )]
    pub burn_leaderboard: Account<'info, BurnLeaderboard>,
    
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

/// Account structure for initializing burn leaderboard (admin only)
#[derive(Accounts)]
pub struct InitializeBurnLeaderboard<'info> {
    #[account(
        mut,
        constraint = admin.key() == AUTHORIZED_ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin
    )]
    pub admin: Signer<'info>,
    
    #[account(
        init,
        payer = admin,
        space = BurnLeaderboard::SPACE,
        seeds = [b"burn_leaderboard"],
        bump
    )]
    pub burn_leaderboard: Account<'info, BurnLeaderboard>,
    
    pub system_program: Program<'info, System>,
}

/// Account structure for burning tokens for a project
#[derive(Accounts)]
#[instruction(project_id: u64, amount: u64)]
pub struct BurnForProject<'info> {
    #[account(
        mut,
        constraint = burner.key() == project.creator @ ErrorCode::UnauthorizedProjectAccess
    )]
    pub burner: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"project", project_id.to_le_bytes().as_ref()],
        bump = project.bump
    )]
    pub project: Account<'info, Project>,
    
    #[account(
        mut,
        seeds = [b"burn_leaderboard"],
        bump
    )]
    pub burn_leaderboard: Account<'info, BurnLeaderboard>,
    
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

/// Project data structure
#[account]
pub struct Project {
    pub project_id: u64,              // Sequential project ID (0, 1, 2, ...)
    pub creator: Pubkey,              // Creator
    pub created_at: i64,              // Creation timestamp
    pub last_updated: i64,            // Last updated timestamp (updated on project updates)
    pub name: String,                 // Project name
    pub description: String,          // Project description
    pub image: String,                // Project image info (max 256 chars)
    pub website: String,              // Project website URL (max 128 chars)
    pub tags: Vec<String>,            // Tags (max 4 tags, each max 32 chars)
    pub memo_count: u64,              // Number of burn_for_project operations (not create/update)
    pub burned_amount: u64,           // Total burned tokens for this project
    pub last_memo_time: i64,          // Last burn_for_project operation timestamp (0 if never burned)
    pub bump: u8,                     // PDA bump
}

impl Project {
    /// Calculate maximum space for the account (conservative estimate)
    pub fn calculate_space_max() -> usize {
        8 + // discriminator
        8 + // project_id (u64)
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
        4 + 128 + // website (max 128 chars)
        4 + (4 + 32) * 4 + // tags (max 4 tags, 32 chars each)
        128 // safety buffer
    }
}

/// Event emitted when a project is created
#[event]
pub struct ProjectCreatedEvent {
    pub project_id: u64,
    pub creator: Pubkey,
    pub name: String,
    pub description: String,
    pub image: String,
    pub website: String,
    pub tags: Vec<String>,
    pub burn_amount: u64,
    pub timestamp: i64,
}

/// Event emitted when a project is updated
#[event]
pub struct ProjectUpdatedEvent {
    pub project_id: u64,
    pub updater: Pubkey,
    pub name: String,
    pub description: String,
    pub image: String,
    pub website: String,
    pub tags: Vec<String>,
    pub burn_amount: u64,
    pub total_burned: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are burned for a project
#[event]
pub struct TokensBurnedForProjectEvent {
    pub project_id: u64,
    pub burner: Pubkey,
    pub amount: u64,
    pub total_burned: u64,
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
    
    #[msg("Unauthorized project access: Only the project creator can perform this operation.")]
    UnauthorizedProjectAccess,
    
    #[msg("Memo required: SPL Memo instruction must be present with valid memo content.")]
    MemoRequired,

    #[msg("Invalid memo format: Memo must contain valid Borsh-formatted data.")]
    InvalidMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,

    #[msg("Unsupported project creation data version. Please use the correct structure version.")]
    UnsupportedProjectDataVersion,

    #[msg("Invalid project creation data format. Must be valid Borsh-serialized data.")]
    InvalidProjectDataFormat,

    #[msg("Invalid category: Must be 'project' for project operations.")]
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
    
    #[msg("Empty project name: Project name field cannot be empty.")]
    EmptyProjectName,
    
    #[msg("Project name too long: Project name must be at most 64 characters.")]
    ProjectNameTooLong,

    #[msg("Invalid project name: Project name contains invalid characters or format.")]
    InvalidProjectName,
    
    #[msg("Project description too long: Description must be at most 128 characters.")]
    ProjectDescriptionTooLong,

    #[msg("Invalid project description: Description contains invalid characters or format.")]
    InvalidProjectDescription,
    
    #[msg("Project image too long: Image info must be at most 256 characters.")]
    ProjectImageTooLong,

    #[msg("Invalid project image: Image info contains invalid characters or format.")]
    InvalidProjectImage,
    
    #[msg("Project website too long: Website URL must be at most 128 characters.")]
    ProjectWebsiteTooLong,

    #[msg("Invalid project website: Website URL contains invalid characters or format.")]
    InvalidProjectWebsite,

    #[msg("Invalid tag: Tag must be at most 32 characters.")]
    InvalidTag,

    #[msg("Too many tags: Maximum 4 tags allowed.")]
    TooManyTags,
    
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

    #[msg("Unauthorized admin: Only the authorized admin can perform this operation.")]
    UnauthorizedAdmin,

    #[msg("Global project counter already initialized.")]
    GlobalProjectCounterAlreadyInitialized,

    #[msg("Burn leaderboard already initialized.")]
    BurnLeaderboardAlreadyInitialized,

    #[msg("Project counter overflow: Cannot create more projects due to counter overflow.")]
    ProjectCounterOverflow,

    #[msg("Project ID mismatch: The project_id in memo must match the target project.")]
    ProjectIdMismatch,

    #[msg("Unsupported project burn data version. Please use the correct structure version.")]
    UnsupportedProjectBurnDataVersion,

    #[msg("Invalid project burn data format. Must be valid Borsh-serialized data.")]
    InvalidProjectBurnDataFormat,

    #[msg("Invalid burner pubkey format in memo. Must be a valid Pubkey string.")]
    InvalidBurnerPubkeyFormat,
    
    #[msg("Burner pubkey mismatch: The burner pubkey in memo must match the transaction signer.")]
    BurnerPubkeyMismatch,
    
    #[msg("Burn message too long: Message must be at most 696 characters.")]
    BurnMessageTooLong,
}
