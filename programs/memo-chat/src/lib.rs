#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::Token2022;
use memo_mint::program::MemoMint;
use memo_mint::cpi::accounts::ProcessMint;
use memo_burn::cpi::accounts::ProcessBurn;
use memo_burn::program::MemoBurn;
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use spl_memo::ID as MEMO_PROGRAM_ID;
use base64::{Engine as _, engine::general_purpose};

// ===== BUSINESS LOGIC CONSTANTS =====

// Token economics
pub const DECIMAL_FACTOR: u64 = 1_000_000; // Token decimals (6)
pub const MIN_GROUP_CREATION_BURN_TOKENS: u64 = 42_069; // Minimum tokens to burn for group creation
pub const MIN_GROUP_CREATION_BURN_AMOUNT: u64 = MIN_GROUP_CREATION_BURN_TOKENS * DECIMAL_FACTOR;
pub const MIN_BURN_AMOUNT: u64 = 1 * DECIMAL_FACTOR; // Minimum burn amount (1 token)

// Maximum burn per transaction (consistent with memo-burn)
pub const MAX_BURN_PER_TX: u64 = 1_000_000_000_000 * DECIMAL_FACTOR; // 1 trillion tokens

// Time limits  
pub const DEFAULT_MEMO_INTERVAL_SECONDS: i64 = 60; // Default memo interval (1 minute)
pub const MAX_MEMO_INTERVAL_SECONDS: i64 = 86400; // Maximum memo interval (24 hours)

// ===== STRING LENGTH CONSTRAINTS =====

// Group metadata limits
pub const MAX_GROUP_NAME_LENGTH: usize = 64;
pub const MAX_GROUP_DESCRIPTION_LENGTH: usize = 128;
pub const MAX_GROUP_IMAGE_LENGTH: usize = 256;
pub const MAX_TAGS_COUNT: usize = 4;
pub const MAX_TAG_LENGTH: usize = 32;

// Message limits
pub const MAX_MESSAGE_LENGTH: usize = 512;
pub const MAX_BURN_MESSAGE_LENGTH: usize = 512;

// Signature format
pub const SIGNATURE_LENGTH_BYTES: usize = 64;

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

// Current version of ChatGroupCreationData structure
pub const CHAT_GROUP_CREATION_DATA_VERSION: u8 = 1;

// Expected category for memo-chat contract
pub const EXPECTED_CATEGORY: &str = "chat";

// Expected operation for group creation
pub const EXPECTED_OPERATION: &str = "create_group";

// Expected operation for sending messages
pub const EXPECTED_SEND_MESSAGE_OPERATION: &str = "send_message";

// Expected operation for burning tokens for group
pub const EXPECTED_BURN_FOR_GROUP_OPERATION: &str = "burn_for_group";

declare_id!("54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF");

// Authorized mint address
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// Authorized admin key (only this address can initialize the global counter)
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");

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

/// Chat group creation data structure (stored in BurnMemo.payload)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ChatGroupCreationData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "chat" for memo-chat contract)
    pub category: String,
    
    /// Operation type (must be "create_group" for group creation)
    pub operation: String,
    
    /// Group ID (must match expected_group_id)
    pub group_id: u64,
    
    /// Group name (required, 1-64 characters)
    pub name: String,
    
    /// Group description (optional, max 128 characters)  
    pub description: String,
    
    /// Group image info (optional, max 256 characters)
    pub image: String,
    
    /// Tags (optional, max 4 tags, each max 32 characters)
    pub tags: Vec<String>,
    
    /// Minimum memo interval in seconds (optional, defaults to 60)
    pub min_memo_interval: Option<i64>,
}

impl ChatGroupCreationData {
    /// Validate the structure fields
    pub fn validate(&self, expected_group_id: u64) -> Result<()> {
        // Validate version
        if self.version != CHAT_GROUP_CREATION_DATA_VERSION {
            msg!("Unsupported chat group creation data version: {} (expected: {})", 
                 self.version, CHAT_GROUP_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedChatGroupDataVersion.into());
        }
        
        // Validate category (must be exactly "chat")
        if self.category != EXPECTED_CATEGORY {
            msg!("Invalid category: '{}' (expected: '{}')", self.category, EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategory.into());
        }
        
        // Validate category length (must be exactly the expected length)
        if self.category.len() != EXPECTED_CATEGORY.len() {
            msg!("Invalid category length: {} bytes (expected: {} bytes for '{}')", 
                 self.category.len(), EXPECTED_CATEGORY.len(), EXPECTED_CATEGORY);
            return Err(ErrorCode::InvalidCategoryLength.into());
        }
        
        // Validate operation (must be exactly "create_group")
        if self.operation != EXPECTED_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length (must be exactly the expected length)
        if self.operation.len() != EXPECTED_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_OPERATION.len(), EXPECTED_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate group_id
        if self.group_id != expected_group_id {
            msg!("Group ID mismatch: data contains {}, expected {}", 
                 self.group_id, expected_group_id);
            return Err(ErrorCode::GroupIdMismatch.into());
        }
        
        // Validate name (required, 1-MAX_GROUP_NAME_LENGTH characters)
        if self.name.is_empty() || self.name.len() > MAX_GROUP_NAME_LENGTH {
            msg!("Invalid group name: '{}' (must be 1-{} characters)", self.name, MAX_GROUP_NAME_LENGTH);
            return Err(ErrorCode::InvalidGroupName.into());
        }
        
        // Validate description (optional, max MAX_GROUP_DESCRIPTION_LENGTH characters)
        if self.description.len() > MAX_GROUP_DESCRIPTION_LENGTH {
            msg!("Invalid group description: {} characters (max: {})", self.description.len(), MAX_GROUP_DESCRIPTION_LENGTH);
            return Err(ErrorCode::InvalidGroupDescription.into());
        }
        
        // Validate image (optional, max MAX_GROUP_IMAGE_LENGTH characters)
        if self.image.len() > MAX_GROUP_IMAGE_LENGTH {
            msg!("Invalid group image: {} characters (max: {})", self.image.len(), MAX_GROUP_IMAGE_LENGTH);
            return Err(ErrorCode::InvalidGroupImage.into());
        }
        
        // Validate tags (optional, max MAX_TAGS_COUNT tags, each max MAX_TAG_LENGTH characters)
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
        
        // Validate min_memo_interval (optional, should be reasonable if provided)
        if let Some(interval) = self.min_memo_interval {
            if interval < 0 || interval > MAX_MEMO_INTERVAL_SECONDS {  // Max 24 hours
                msg!("Invalid min_memo_interval: {} (must be 0-{} seconds)", interval, MAX_MEMO_INTERVAL_SECONDS);
                return Err(ErrorCode::InvalidMemoInterval.into());
            }
        }
        
        msg!("Chat group creation data validation passed: category={}, operation={}, group_id={}, name={}, tags_count={}", 
             self.category, self.operation, self.group_id, self.name, self.tags.len());
        
        Ok(())
    }
}

/// Chat message data structure (stored in BurnMemo.payload for send_memo_to_group)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ChatMessageData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "chat" for memo-chat contract)
    pub category: String,
    
    /// Operation type (must be "send_message" for sending messages)
    pub operation: String,
    
    /// Group ID (must match the target group)
    pub group_id: u64,
    
    /// Sender pubkey as string (must match the transaction signer)
    pub sender: String,
    
    /// Message content (required, 1-512 characters)
    pub message: String,
    
    /// Optional receiver pubkey as string (for direct messages within group)
    pub receiver: Option<String>,
    
    /// Optional reply to signature (for message threading)
    pub reply_to_sig: Option<String>,
}

impl ChatMessageData {
    /// Validate the structure fields
    pub fn validate(&self, expected_group_id: u64, expected_sender: Pubkey) -> Result<()> {
        // Validate version
        if self.version != CHAT_GROUP_CREATION_DATA_VERSION {
            msg!("Unsupported chat message data version: {} (expected: {})", 
                 self.version, CHAT_GROUP_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedChatMessageDataVersion.into());
        }
        
        // Validate category (must be exactly "chat")
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
        
        // Validate operation (must be exactly "send_message")
        if self.operation != EXPECTED_SEND_MESSAGE_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_SEND_MESSAGE_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_SEND_MESSAGE_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_SEND_MESSAGE_OPERATION.len(), EXPECTED_SEND_MESSAGE_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate group_id
        if self.group_id != expected_group_id {
            msg!("Group ID mismatch: data contains {}, expected {}", 
                 self.group_id, expected_group_id);
            return Err(ErrorCode::GroupIdMismatch.into());
        }
        
        // Validate sender (convert string to Pubkey and compare)
        let sender_pubkey = Pubkey::from_str(&self.sender)
            .map_err(|_| {
                msg!("Invalid sender format: {}", self.sender);
                ErrorCode::InvalidSenderFormat
            })?;
            
        if sender_pubkey != expected_sender {
            msg!("Sender mismatch: data contains {}, expected {}", 
                 sender_pubkey, expected_sender);
            return Err(ErrorCode::SenderMismatch.into());
        }
        
        // Validate message (required, 1-512 characters)
        if self.message.is_empty() {
            return Err(ErrorCode::EmptyMessage.into());
        }
        
        if self.message.len() > MAX_MESSAGE_LENGTH {
            return Err(ErrorCode::MessageTooLong.into());
        }
        
        // Validate receiver format if provided
        if let Some(ref receiver_str) = self.receiver {
            if !receiver_str.is_empty() {
                Pubkey::from_str(receiver_str)
                    .map_err(|_| {
                        msg!("Invalid receiver format: {}", receiver_str);
                        ErrorCode::InvalidReceiverFormat
                    })?;
            }
        }
        
        // Validate reply_to_sig format if provided
        if let Some(ref reply_sig) = self.reply_to_sig {
            if !reply_sig.is_empty() {
                // Validate signature format (base58 encoded, 64 bytes when decoded)
                match bs58::decode(reply_sig).into_vec() {
                    Ok(decoded) => {
                        if decoded.len() != SIGNATURE_LENGTH_BYTES {
                            msg!("Invalid reply signature length: {} bytes (expected {})", decoded.len(), SIGNATURE_LENGTH_BYTES);
                            return Err(ErrorCode::InvalidReplySignatureFormat.into());
                        }
                    },
                    Err(_) => {
                        msg!("Invalid reply signature encoding: {}", reply_sig);
                        return Err(ErrorCode::InvalidReplySignatureFormat.into());
                    }
                }
            }
        }
        
        msg!("Chat message data validation passed: category={}, operation={}, group_id={}, sender={}, message_len={}", 
             self.category, self.operation, self.group_id, self.sender, self.message.len());
        
        Ok(())
    }
}

/// Chat group burn data structure (stored in BurnMemo.payload for burn_tokens_for_group)
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ChatGroupBurnData {
    /// Version of this structure (for future compatibility)
    pub version: u8,
    
    /// Category of the request (must be "chat" for memo-chat contract)
    pub category: String,
    
    /// Operation type (must be "burn_for_group" for burning tokens)
    pub operation: String,
    
    /// Group ID (must match the target group)
    pub group_id: u64,
    
    /// Burner pubkey as string (must match the transaction signer)
    pub burner: String,
    
    /// Burn message (optional, max 512 characters)
    pub message: String,
}

impl ChatGroupBurnData {
    /// Validate the structure fields
    pub fn validate(&self, expected_group_id: u64, expected_burner: Pubkey) -> Result<()> {
        // Validate version
        if self.version != CHAT_GROUP_CREATION_DATA_VERSION {
            msg!("Unsupported chat group burn data version: {} (expected: {})", 
                 self.version, CHAT_GROUP_CREATION_DATA_VERSION);
            return Err(ErrorCode::UnsupportedChatGroupBurnDataVersion.into());
        }
        
        // Validate category (must be exactly "chat")
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
        
        // Validate operation (must be exactly "burn_for_group")
        if self.operation != EXPECTED_BURN_FOR_GROUP_OPERATION {
            msg!("Invalid operation: '{}' (expected: '{}')", self.operation, EXPECTED_BURN_FOR_GROUP_OPERATION);
            return Err(ErrorCode::InvalidOperation.into());
        }
        
        // Validate operation length
        if self.operation.len() != EXPECTED_BURN_FOR_GROUP_OPERATION.len() {
            msg!("Invalid operation length: {} bytes (expected: {} bytes for '{}')", 
                 self.operation.len(), EXPECTED_BURN_FOR_GROUP_OPERATION.len(), EXPECTED_BURN_FOR_GROUP_OPERATION);
            return Err(ErrorCode::InvalidOperationLength.into());
        }
        
        // Validate group_id
        if self.group_id != expected_group_id {
            msg!("Group ID mismatch: data contains {}, expected {}", 
                 self.group_id, expected_group_id);
            return Err(ErrorCode::GroupIdMismatch.into());
        }
        
        // Validate burner (convert string to Pubkey and compare)
        let burner_pubkey = Pubkey::from_str(&self.burner)
            .map_err(|_| {
                msg!("Invalid burner format: {}", self.burner);
                ErrorCode::InvalidBurnerFormat
            })?;
            
        if burner_pubkey != expected_burner {
            msg!("Burner mismatch: data contains {}, expected {}", 
                 burner_pubkey, expected_burner);
            return Err(ErrorCode::BurnerMismatch.into());
        }
        
        // Validate message (optional, max MAX_BURN_MESSAGE_LENGTH characters)
        if self.message.len() > MAX_BURN_MESSAGE_LENGTH {
            msg!("Burn message too long: {} characters (max: {})", self.message.len(), MAX_BURN_MESSAGE_LENGTH);
            return Err(ErrorCode::BurnMessageTooLong.into());
        }
        
        msg!("Chat group burn data validation passed: category={}, operation={}, group_id={}, burner={}, message_len={}", 
             self.category, self.operation, self.group_id, self.burner, self.message.len());
        
        Ok(())
    }
}

#[program]
pub mod memo_chat {
    use super::*;

    /// Initialize the global group counter (one-time setup, admin only)
    pub fn initialize_global_counter(ctx: Context<InitializeGlobalCounter>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let counter = &mut ctx.accounts.global_counter;
        counter.total_groups = 0;
        
        msg!("Global group counter initialized by admin {} with total_groups: {}", 
             ctx.accounts.admin.key(), counter.total_groups);
        Ok(())
    }

    /// Create a new chat group (requires burning tokens)
    /// Note: group_id will be automatically assigned by the contract
    pub fn create_chat_group(
        ctx: Context<CreateChatGroup>,
        expected_group_id: u64, // The group_id that client expects to create
        burn_amount: u64,
    ) -> Result<()> {
        // Validate burn amount - require at least 42069 tokens for group creation
        if burn_amount < MIN_GROUP_CREATION_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
        if burn_amount > MAX_BURN_PER_TX {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
        
        if burn_amount % DECIMAL_FACTOR != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Get the next group_id from global counter
        let global_counter = &mut ctx.accounts.global_counter;
        let actual_group_id = global_counter.total_groups;

        // Verify that the expected group_id matches the actual next group_id
        if expected_group_id != actual_group_id {
            msg!("Group ID mismatch: expected {}, but next available ID is {}", 
                 expected_group_id, actual_group_id);
            return Err(ErrorCode::GroupIdMismatch.into());
        }

        // Check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Parse and validate Borsh memo data for group creation
        let group_data = parse_group_creation_borsh_memo(&memo_data, actual_group_id, burn_amount)?;
        
        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.creator.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.creator_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        memo_burn::cpi::process_burn(cpi_ctx, burn_amount)?;
        
        // Initialize chat group data after successful burn
        let chat_group = &mut ctx.accounts.chat_group;
        chat_group.group_id = actual_group_id;
        chat_group.creator = ctx.accounts.creator.key();
        chat_group.created_at = Clock::get()?.unix_timestamp;
        chat_group.name = group_data.name.clone();
        chat_group.description = group_data.description.clone();
        chat_group.image = group_data.image.clone();
        chat_group.tags = group_data.tags.clone();
        chat_group.memo_count = 0;
        chat_group.burned_amount = burn_amount;
        chat_group.min_memo_interval = group_data.min_memo_interval.unwrap_or(DEFAULT_MEMO_INTERVAL_SECONDS);
        chat_group.last_memo_time = 0;
        chat_group.bump = ctx.bumps.chat_group;

        // Increment global counter AFTER successful group creation
        global_counter.total_groups = global_counter.total_groups.checked_add(1)
            .ok_or(ErrorCode::GroupCounterOverflow)?;

        // Emit group creation event
        emit!(ChatGroupCreatedEvent {
            group_id: actual_group_id,
            creator: ctx.accounts.creator.key(),
            name: group_data.name,
            description: group_data.description,
            image: group_data.image,
            tags: group_data.tags,
            burn_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        // Update burn leaderboard after successful group creation
        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        let entered_leaderboard = leaderboard.update_leaderboard(actual_group_id, burn_amount)?;

        if entered_leaderboard {
            msg!("Group {} entered burn leaderboard", actual_group_id);
        } else {
            msg!("Group {} burn amount {} not sufficient for leaderboard", 
                 actual_group_id, burn_amount / DECIMAL_FACTOR);
        }

        msg!("Chat group {} created successfully by {} with {} tokens burned", 
             actual_group_id, ctx.accounts.creator.key(), burn_amount / DECIMAL_FACTOR);
        Ok(())
    }

    /// Send memo to group (only group_id needed, content from memo)
    pub fn send_memo_to_group(
        ctx: Context<SendMemoToGroup>,
        group_id: u64,
    ) -> Result<()> {
        // Check memo instruction with enhanced validation
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // Parse and validate Borsh memo content
        let memo_content = parse_message_borsh_memo(&memo_data, group_id, ctx.accounts.sender.key())?;
        
        let chat_group = &mut ctx.accounts.chat_group;
        let current_time = Clock::get()?.unix_timestamp;

        // Check memo frequency limit
        if chat_group.last_memo_time > 0 {
            let time_since_last = current_time - chat_group.last_memo_time;
            if time_since_last < chat_group.min_memo_interval {
                return Err(ErrorCode::MemoTooFrequent.into());
            }
        }

        // Update chat group statistics
        chat_group.memo_count = chat_group.memo_count.saturating_add(1);
        chat_group.last_memo_time = current_time;
        let memo_count = chat_group.memo_count;

        // Log the memo
        msg!("Memo from {} to group {}: {}", 
             ctx.accounts.sender.key(), 
             group_id, 
             memo_content);

        // Call memo-mint contract using CPI to process_mint (user as direct signer)
        // This allows sender to directly mint tokens without using chat group PDA
        let cpi_program = ctx.accounts.memo_mint_program.to_account_info();
        let cpi_accounts = ProcessMint {
            user: ctx.accounts.sender.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            mint_authority: ctx.accounts.mint_authority.to_account_info(),
            token_account: ctx.accounts.sender_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        memo_mint::cpi::process_mint(cpi_ctx)?;

        // Emit memo event
        emit!(MemoSentEvent {
            group_id,
            sender: ctx.accounts.sender.key(),
            memo: memo_content,
            memo_count,
            timestamp: current_time,
        });

        Ok(())
    }

    /// Burn tokens for a chat group
    pub fn burn_tokens_for_group(
        ctx: Context<BurnTokensForGroup>,
        group_id: u64,
        amount: u64,
    ) -> Result<()> {
        // Validate burn amount
        if amount < MIN_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        // check burn amount limit
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
        parse_burn_borsh_memo(&memo_data, group_id, amount, ctx.accounts.burner.key())?;

        // Call memo-burn contract to burn tokens
        let cpi_program = ctx.accounts.memo_burn_program.to_account_info();
        let cpi_accounts = ProcessBurn {
            user: ctx.accounts.burner.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            token_account: ctx.accounts.burner_token_account.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            instructions: ctx.accounts.instructions.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        
        // Call memo-burn's process_burn instruction
        memo_burn::cpi::process_burn(cpi_ctx, amount)?;
        
        // Update chat group burned amount tracking
        let chat_group = &mut ctx.accounts.chat_group;
        let old_amount = chat_group.burned_amount;
        chat_group.burned_amount = chat_group.burned_amount.saturating_add(amount);
        if chat_group.burned_amount == u64::MAX && old_amount < u64::MAX {
            msg!("Warning: burned_amount overflow detected for group {}", group_id);
        }
        
        msg!("Successfully burned {} tokens for group {}", amount / DECIMAL_FACTOR, group_id);
        
        // Update burn leaderboard after successful burn
        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        let total_burned = chat_group.burned_amount;
        let entered_leaderboard = leaderboard.update_leaderboard(group_id, total_burned)?;

        if entered_leaderboard {
            msg!("Group {} updated in burn leaderboard with total {} tokens", 
                 group_id, total_burned / DECIMAL_FACTOR);
        } else {
            msg!("Group {} total burn amount {} not sufficient for leaderboard", 
                 group_id, total_burned / DECIMAL_FACTOR);
        }

        // Emit burn event
        emit!(TokensBurnedForGroupEvent {
            group_id,
            burner: ctx.accounts.burner.key(),
            amount,
            total_burned: chat_group.burned_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Initialize the burn leaderboard (one-time setup, admin only)
    pub fn initialize_burn_leaderboard(ctx: Context<InitializeBurnLeaderboard>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        leaderboard.initialize(); // Use the new initialize method
        
        msg!("Burn leaderboard initialized by admin {}", ctx.accounts.admin.key());
        Ok(())
    }

    /// Clear the burn leaderboard (admin only, for cleaning up duplicate data)
    pub fn clear_burn_leaderboard(ctx: Context<ClearBurnLeaderboard>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key() != AUTHORIZED_ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        let leaderboard = &mut ctx.accounts.burn_leaderboard;
        
        // Record current state for logging
        let old_size = leaderboard.current_size;
        let old_entries_count = leaderboard.entries.len();
        
        // Clear all entries
        leaderboard.current_size = 0;
        leaderboard.entries.clear();
        
        msg!("Burn leaderboard cleared by admin {}", ctx.accounts.admin.key());
        msg!("Removed {} entries (current_size was {})", old_entries_count, old_size);
        
        Ok(())
    }
}

/// Parse and validate Borsh-formatted memo data for group creation (with Base64 decoding)
fn parse_group_creation_borsh_memo(memo_data: &[u8], expected_group_id: u64, expected_amount: u64) -> Result<ChatGroupCreationData> {
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
    
    // Record payload preview for debugging
    if !burn_memo.payload.is_empty() {
        msg!("Payload: [binary data, {} bytes]", burn_memo.payload.len());
    }
    
    // Deserialize ChatGroupCreationData from payload
    let group_data = ChatGroupCreationData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid chat group creation data format in payload");
            ErrorCode::InvalidChatGroupDataFormat
        })?;
    
    // Validate the group creation data
    group_data.validate(expected_group_id)?;
    
    msg!("Chat group creation data parsed successfully: group_id={}, name={}, description_len={}, image_len={}, tags_count={}", 
         group_data.group_id, group_data.name, group_data.description.len(), 
         group_data.image.len(), group_data.tags.len());

    Ok(group_data)
}

/// Parse and validate Borsh-formatted memo data for burn operation (with Base64 decoding)
fn parse_burn_borsh_memo(memo_data: &[u8], expected_group_id: u64, expected_amount: u64, expected_burner: Pubkey) -> Result<()> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidChatGroupBurnDataFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidChatGroupBurnDataFormat
        })?;
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize Borsh data from decoded bytes (following memo-burn pattern)
    let burn_memo = BurnMemo::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidChatGroupBurnDataFormat
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
    
    // Record payload preview for debugging
    if !burn_memo.payload.is_empty() {
        msg!("Payload: [binary data, {} bytes]", burn_memo.payload.len());
    }
    
    // Deserialize ChatGroupBurnData from payload
    let burn_data = ChatGroupBurnData::try_from_slice(&burn_memo.payload)
        .map_err(|_| {
            msg!("Invalid chat group burn data format in payload");
            ErrorCode::InvalidChatGroupBurnDataFormat
        })?;
    
    // Validate the burn data
    burn_data.validate(expected_group_id, expected_burner)?;
    
    msg!("Chat group burn data parsed successfully: group_id={}, category={}, operation={}, burner={}, message={}", 
         burn_data.group_id, burn_data.category, burn_data.operation, burn_data.burner, 
         burn_data.message.chars().take(50).collect::<String>());

    Ok(())
}

/// Parse and validate Borsh-formatted memo data for sending messages (with Base64 decoding)
fn parse_message_borsh_memo(memo_data: &[u8], expected_group_id: u64, expected_sender: Pubkey) -> Result<String> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidChatMessageDataFormat
        })?;
    
    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidChatMessageDataFormat
        })?;
    
    msg!("Base64 decoded: {} bytes -> {} bytes", memo_data.len(), decoded_data.len());
    
    // Deserialize ChatMessageData from decoded bytes
    let message_data = ChatMessageData::try_from_slice(&decoded_data)
        .map_err(|_| {
            msg!("Invalid Borsh format after Base64 decoding");
            ErrorCode::InvalidChatMessageDataFormat
        })?;
    
    // Validate message data
    message_data.validate(expected_group_id, expected_sender)?;
    
    msg!("Chat message data parsed successfully: group_id={}, sender={}, message_len={}, receiver={:?}, reply_to={:?}", 
         message_data.group_id, message_data.sender, message_data.message.len(), 
         message_data.receiver, message_data.reply_to_sig.as_ref().map(|s| &s[..16.min(s.len())]));

    Ok(message_data.message)
}

/// Check for memo instruction at REQUIRED index 1
/// 
/// IMPORTANT: This contract enforces a strict instruction ordering:
/// - Index 0: Compute budget instruction (optional)
/// - Index 1: SPL Memo instruction (REQUIRED)
/// - Index 2+: memo-chat instructions (create_chat_group, send_memo_to_group, etc.)
///
/// This function searches both positions to accommodate different transaction structures.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // Try index 0 first (no compute budget case)
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at index 0");
                return validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
            }
        },
        Err(_) => {
            // Index 0 doesn't exist or failed to load, continue to check index 1
        }
    }
    
    // Try index 1 (with compute budget case)
    match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                msg!("Found memo instruction at index 1");
                return validate_memo_length(&ix.data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
            } else {
                msg!("Instruction at index 1 is not a memo (program_id: {})", ix.program_id);
            }
        },
        Err(e) => {
            msg!("Failed to load instruction at index 1: {:?}", e);
        }
    }
    
    // If we reach here, no memo instruction was found at either position
    msg!("No memo instruction found at index 0 or 1");
    Ok((false, vec![]))
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
    pub group_id: u64,
    pub burned_amount: u64,
}

/// Burn leaderboard account (stores top 100 groups by burn amount)
#[account]
pub struct BurnLeaderboard {
    /// Current number of entries in the leaderboard (0-100)
    pub current_size: u8,
    /// Array of leaderboard entries, sorted by burned_amount in descending order
    pub entries: Vec<LeaderboardEntry>,
}

impl BurnLeaderboard {
    pub const SPACE: usize = 8 + // discriminator
        1 + // current_size
        4 + // Vec length prefix
        100 * 16 + // max entries (100 * (8 + 8) bytes each)
        64; // safety buffer
    
    /// Initialize with empty entries
    pub fn initialize(&mut self) {
        self.current_size = 0;
        self.entries = Vec::with_capacity(100);
    }
    
    ///  find group position and min burned_amount position (core optimization)
    pub fn find_group_position_and_min(&self, group_id: u64) -> (Option<usize>, Option<usize>) {
        if self.entries.is_empty() {
            return (None, None);
        }
        
        let mut min_pos = None;
        let mut min_amount = u64::MAX;
        let mut found_group_pos = None;
        
        // loop all elements
        for (i, entry) in self.entries.iter().enumerate() {
            // record target group position
            if entry.group_id == group_id {
                found_group_pos = Some(i);
            }
            
            // always record min position
            if entry.burned_amount < min_amount {
                min_amount = entry.burned_amount;
                min_pos = Some(i);
            }
        }
        
        (found_group_pos, min_pos)
    }
    
    /// update leaderboard - zero array move version
    pub fn update_leaderboard(&mut self, group_id: u64, new_burned_amount: u64) -> Result<bool> {
        // 1. one loop to get group position and min position
        let (existing_pos, min_pos) = self.find_group_position_and_min(group_id);
        
        // 2. if group exists, update burned_amount (zero move)
        if let Some(pos) = existing_pos {
            self.entries[pos].burned_amount = new_burned_amount;
            return Ok(true);
        }
        
        // 3. new group and leaderboard not full, add directly (no sort)
        if self.entries.len() < 100 {
            let new_entry = LeaderboardEntry {
                group_id,
                burned_amount: new_burned_amount,
            };
            self.entries.push(new_entry);
            self.current_size = self.entries.len() as u8;
            return Ok(true);
        }
        
        // 4. new group and leaderboard full, check if can replace min value
        if let Some(min_position) = min_pos {
            let min_amount = self.entries[min_position].burned_amount;
            if new_burned_amount > min_amount {
                // replace min value entry (zero move)
                self.entries[min_position] = LeaderboardEntry {
                    group_id,
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

/// Global group counter account
#[account]
pub struct GlobalGroupCounter {
    pub total_groups: u64,          // Total number of groups created (starts at 0)
}

impl GlobalGroupCounter {
    pub const SPACE: usize = 8 + // discriminator
        8; // total_groups (u64)
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
        space = GlobalGroupCounter::SPACE,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalGroupCounter>,
    
    pub system_program: Program<'info, System>,
}

/// Account structure for creating a chat group
#[derive(Accounts)]
#[instruction(expected_group_id: u64, burn_amount: u64)]
pub struct CreateChatGroup<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_counter"],
        bump
    )]
    pub global_counter: Account<'info, GlobalGroupCounter>,
    
    #[account(
        init,
        payer = creator,
        space = ChatGroup::calculate_space_max(),
        seeds = [b"chat_group", expected_group_id.to_le_bytes().as_ref()],
        bump
    )]
    pub chat_group: Account<'info, ChatGroup>,
    
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
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-burn program
    pub memo_burn_program: Program<'info, MemoBurn>,
    
    pub system_program: Program<'info, System>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for sending memo to a chat group
#[derive(Accounts)]
#[instruction(group_id: u64)]
pub struct SendMemoToGroup<'info> {
    #[account(mut)]
    pub sender: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"chat_group", group_id.to_le_bytes().as_ref()],
        bump = chat_group.bump
    )]
    pub chat_group: Account<'info, ChatGroup>,
    
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
        constraint = sender_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
        constraint = sender_token_account.owner == sender.key() @ ErrorCode::UnauthorizedTokenAccount
    )]
    pub sender_token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// The memo-mint program
    pub memo_mint_program: Program<'info, MemoMint>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
}

/// Account structure for burning tokens for a chat group
#[derive(Accounts)]
#[instruction(group_id: u64, amount: u64)]
pub struct BurnTokensForGroup<'info> {
    #[account(mut)]
    pub burner: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"chat_group", group_id.to_le_bytes().as_ref()],
        bump = chat_group.bump
    )]
    pub chat_group: Account<'info, ChatGroup>,
    
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

/// Account structure for clearing burn leaderboard (admin only)
#[derive(Accounts)]
pub struct ClearBurnLeaderboard<'info> {
    #[account(
        mut,
        constraint = admin.key() == AUTHORIZED_ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin
    )]
    pub admin: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"burn_leaderboard"],
        bump
    )]
    pub burn_leaderboard: Account<'info, BurnLeaderboard>,
}

/// Chat group data structure
#[account]
pub struct ChatGroup {
    pub group_id: u64,              // Sequential group ID (0, 1, 2, ...)
    pub creator: Pubkey,            // Creator
    pub created_at: i64,            // Creation timestamp
    pub name: String,               // Group name
    pub description: String,        // Group description
    pub image: String,              // Group image info (max 256 chars)
    pub tags: Vec<String>,          // Tags
    pub memo_count: u64,            // Memo count
    pub burned_amount: u64,         // Total burned tokens for this group
    pub min_memo_interval: i64,     // Minimum memo interval in seconds
    pub last_memo_time: i64,        // Last memo timestamp
    pub bump: u8,                   // PDA bump
}

impl ChatGroup {
    /// Calculate maximum space for the account (conservative estimate)
    pub fn calculate_space_max() -> usize {
        8 + // discriminator
        8 + // group_id (u64)
        32 + // creator
        8 + // created_at
        8 + // memo_count
        8 + // burned_amount
        8 + // min_memo_interval
        8 + // last_memo_time
        1 + // bump
        4 + 64 + // name (max 64 chars)
        4 + 128 + // description (max 128 chars)
        4 + 256 + // image (max 256 chars)
        4 + (4 + 32) * 4 + // tags (max 4 tags, 32 chars each)
        128 // safety buffer
    }
}

/// Event emitted when a chat group is created
#[event]
pub struct ChatGroupCreatedEvent {
    pub group_id: u64,
    pub creator: Pubkey,
    pub name: String,
    pub description: String,
    pub image: String,
    pub tags: Vec<String>,
    pub burn_amount: u64,
    pub timestamp: i64,
}

/// Event emitted when a memo is sent to a group
#[event]
pub struct MemoSentEvent {
    pub group_id: u64,
    pub sender: Pubkey,
    pub memo: String,
    pub memo_count: u64,
    pub timestamp: i64,
}

/// Event emitted when tokens are burned for a group
#[event]
pub struct TokensBurnedForGroupEvent {
    pub group_id: u64,
    pub burner: Pubkey,
    pub amount: u64,
    pub total_burned: u64,
    pub timestamp: i64,
}

/// Event emitted when leaderboard is updated
#[event]
pub struct LeaderboardUpdatedEvent {
    pub group_id: u64,
    pub new_rank: u8,
    pub burned_amount: u64,
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
    
    #[msg("Invalid group name: Name must be 1-64 characters.")]
    InvalidGroupName,
    
    #[msg("Invalid group description: Description must be at most 128 characters.")]
    InvalidGroupDescription,
    
    #[msg("Too many tags: Maximum 4 tags allowed.")]
    TooManyTags,
    
    #[msg("Invalid tag: Tag must be 1-32 characters.")]
    InvalidTag,
    
    #[msg("Memo sent too frequently: Please wait before sending another memo.")]
    MemoTooFrequent,
    
    #[msg("Chat group not found.")]
    GroupNotFound,

    #[msg("Memo required: SPL Memo instruction must be present with valid memo content.")]
    MemoRequired,

    #[msg("Invalid memo format: Memo must contain valid Borsh-formatted data.")]
    InvalidMemoFormat,

    #[msg("Unsupported memo version. Please use the correct memo structure version.")]
    UnsupportedMemoVersion,

    #[msg("Unsupported chat group creation data version. Please use the correct structure version.")]
    UnsupportedChatGroupDataVersion,

    #[msg("Invalid chat group creation data format in user_data. Must be valid Borsh-serialized data.")]
    InvalidChatGroupDataFormat,

    #[msg("Group ID mismatch: Group ID from memo does not match instruction parameter.")]
    GroupIdMismatch,

    #[msg("Missing group_id field in memo.")]
    MissingGroupIdField,

    #[msg("Missing name field in memo.")]
    MissingNameField,

    #[msg("Invalid tags format: Tags must be an array of strings.")]
    InvalidTagsFormat,

    #[msg("Invalid group ID format: Group ID must be a valid u64 number.")]
    InvalidGroupIdFormat,

    #[msg("Burn amount too small. Must burn at least 42069 tokens (42,069,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Missing burn_amount field in memo.")]
    MissingBurnAmountField,

    #[msg("Invalid burn_amount format in memo. Must be a positive integer in units.")]
    InvalidBurnAmountFormat,

    #[msg("Burn amount mismatch. The burn_amount in memo must match the burn amount (in units).")]
    BurnAmountMismatch,

    #[msg("Group counter overflow. Maximum number of groups reached.")]
    GroupCounterOverflow,

    #[msg("Invalid chat group PDA. The provided account does not match the expected PDA for this group ID.")]
    InvalidChatGroupPDA,

    #[msg("Unauthorized admin. Only the authorized admin can perform this operation.")]
    UnauthorizedAdmin,

    #[msg("Invalid group image: Image info must be at most 256 characters.")]
    InvalidGroupImage,

    #[msg("Invalid category: Must be 'chat' for chat group operations.")]
    InvalidCategory,
    
    #[msg("Invalid category length. Category must be exactly the expected length.")]
    InvalidCategoryLength,
    
    #[msg("Missing sender field in memo.")]
    MissingSenderField,
    
    #[msg("Invalid sender format in memo. Must be a valid Pubkey string.")]
    InvalidSenderFormat,
    
    #[msg("Sender mismatch: The sender in memo must match the transaction signer.")]
    SenderMismatch,
    
    #[msg("Missing message field in memo.")]
    MissingMessageField,
    
    #[msg("Empty message: Message field cannot be empty.")]
    EmptyMessage,
    
    #[msg("Message too long: Message must be at most 512 characters.")]
    MessageTooLong,
    
    #[msg("Invalid receiver format in memo. Must be a valid Pubkey string.")]
    InvalidReceiverFormat,
    
    #[msg("Invalid reply signature format in memo. Must be a valid base58-encoded signature string.")]
    InvalidReplySignatureFormat,
    
    #[msg("Missing operation field in memo.")]
    MissingOperationField,
    
    #[msg("Invalid operation: Operation does not match the expected operation for this instruction.")]
    InvalidOperation,

    #[msg("Invalid operation length. Operation must be exactly the expected length.")]
    InvalidOperationLength,

    #[msg("User data too long. (maximum 787 bytes).")]
    UserDataTooLong,

    #[msg("Invalid memo interval: Must be between 0 and 86400 seconds (24 hours).")]
    InvalidMemoInterval,

    #[msg("Payload too long. (maximum 787 bytes).")]
    PayloadTooLong,

    #[msg("Unsupported chat message data version. Please use the correct structure version.")]
    UnsupportedChatMessageDataVersion,
    
    #[msg("Invalid chat message data format in payload. Must be valid Borsh-serialized data.")]
    InvalidChatMessageDataFormat,

    #[msg("Unsupported chat group burn data version. Please use the correct structure version.")]
    UnsupportedChatGroupBurnDataVersion,
    
    #[msg("Invalid chat group burn data format in payload. Must be valid Borsh-serialized data.")]
    InvalidChatGroupBurnDataFormat,

    #[msg("Invalid burner format in memo. Must be a valid Pubkey string.")]
    InvalidBurnerFormat,
    
    #[msg("Burner mismatch: The burner in memo must match the transaction signer.")]
    BurnerMismatch,
    
    #[msg("Burn message too long: Message must be at most 512 characters.")]
    BurnMessageTooLong,

    #[msg("Leaderboard update failed. Unable to process leaderboard entry.")]
    LeaderboardUpdateFailed,

    #[msg("Leaderboard full. Entry does not qualify for top 100.")]
    LeaderboardFull,

    #[msg("Burn amount too large. Maximum allowed: 1,000,000,000,000 tokens per transaction.")]
    BurnAmountTooLarge,
}
