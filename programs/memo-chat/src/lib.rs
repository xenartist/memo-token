#![allow(deprecated)]
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::Token2022;
use memo_mint::cpi::accounts::ProcessMintTo;
use memo_mint::program::MemoMint;
use memo_burn::cpi::accounts::ProcessBurn;
use memo_burn::program::MemoBurn;
use anchor_lang::solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use serde_json::Value;
use serde::{Deserialize, Serialize};
use anchor_lang::solana_program::{instruction::Instruction, program::invoke_signed};
use sha2::{Sha256, Digest};

declare_id!("54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF");

// Authorized mint address
pub const AUTHORIZED_MINT: &str = "HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1";

// Authorized admin key (only this address can initialize the global counter)
pub const AUTHORIZED_ADMIN: &str = "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn";

#[program]
pub mod memo_chat {
    use super::*;

    /// Initialize the global group counter (one-time setup, admin only)
    pub fn initialize_global_counter(ctx: Context<InitializeGlobalCounter>) -> Result<()> {
        // Verify admin authorization
        if ctx.accounts.admin.key().to_string() != AUTHORIZED_ADMIN {
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
        // Validate burn amount
        if burn_amount < 1_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        if burn_amount % 1_000_000 != 0 {
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

        // Parse and validate memo data
        let group_data = parse_group_creation_memo(&memo_data, actual_group_id, burn_amount)?;
        
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
        chat_group.min_memo_interval = group_data.min_memo_interval.unwrap_or(60);
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

        msg!("Chat group {} created successfully by {} with {} tokens burned", 
             actual_group_id, ctx.accounts.creator.key(), burn_amount / 1_000_000);
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
        
        // Parse and validate memo content with required fields
        let memo_content = parse_and_validate_memo_for_send(&memo_data, group_id, ctx.accounts.sender.key())?;
        
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

        // Call memo-mint contract through chat group PDA to mint tokens to the sender
        let group_id_bytes = group_id.to_le_bytes();
        let bump = chat_group.bump;
        let group_seeds: &[&[u8]] = &[
            b"chat_group",
            group_id_bytes.as_ref(),
            &[bump]
        ];
        let signer_seeds = [group_seeds];

        // Manual instruction construction for process_mint_to (similar to successful test client)
        // use anchor_lang::solana_program::{instruction::Instruction, program::invoke_signed};
        // use sha2::{Sha256, Digest};
        
        let recipient = ctx.accounts.sender.key();
        
        // Build instruction data manually
        let mut hasher = Sha256::new();
        hasher.update(b"global:process_mint_to");
        let result = hasher.finalize();
        let mut instruction_data = result[..8].to_vec();
        
        // Add recipient parameter (required by ProcessMintTo)
        instruction_data.extend_from_slice(&recipient.to_bytes());
        
        let accounts = vec![
            AccountMeta::new(chat_group.key(), true),  // caller (chat group PDA as signer)
            AccountMeta::new(ctx.accounts.mint.key(), false),   // mint
            AccountMeta::new_readonly(ctx.accounts.mint_authority.key(), false), // mint_authority
            AccountMeta::new(ctx.accounts.sender_token_account.key(), false),    // recipient_token_account
            AccountMeta::new_readonly(anchor_spl::token_2022::ID, false),  // token_program
            AccountMeta::new_readonly(
                Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap(),
                false
            ), // instructions
        ];
        
        let mint_ix = Instruction::new_with_bytes(
            ctx.accounts.memo_mint_program.key(),
            &instruction_data,
            accounts,
        );
        
        invoke_signed(
            &mint_ix,
            &[
                chat_group.to_account_info(),
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.mint_authority.to_account_info(),
                ctx.accounts.sender_token_account.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.instructions.to_account_info(),
            ],
            &signer_seeds,
        )?;

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
        if amount < 1_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }
        
        if amount % 1_000_000 != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }

        // Check memo instruction with enhanced validation
        let (memo_found, memo_data) = check_memo_instruction(&ctx.accounts.instructions)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }

        // Validate memo contains correct amount and group_id
        validate_memo_for_burn(&memo_data, group_id, amount)?;

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
        chat_group.burned_amount = chat_group.burned_amount.saturating_add(amount);
        
        msg!("Successfully burned {} tokens for group {}", amount / 1_000_000, group_id);
        
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
}

/// Parse and validate group creation memo data
fn parse_group_creation_memo(memo_data: &[u8], expected_group_id: u64, expected_amount: u64) -> Result<GroupCreationData> {
    // Enhanced UTF-8 validation (following memo-burn pattern)
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check (prevent malicious input)
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Clean the string (handle JSON escaping)
    let clean_str = memo_str
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    
    // Parse as JSON
    let json_data: Value = serde_json::from_str(&clean_str)
        .map_err(|e| {
            msg!("JSON parsing failed: {}", e);
            ErrorCode::InvalidMemoFormat
        })?;

    // Extract and validate operation field (must match expected operation)
    let operation = json_data["operation"]
        .as_str()
        .ok_or(ErrorCode::MissingOperationField)?;
    
    if operation != "create_group" {
        msg!("Invalid operation: expected 'create_group', got '{}'", operation);
        return Err(ErrorCode::InvalidOperation.into());
    }

    // Extract and validate category field (must be "chat")
    let category = json_data["category"]
        .as_str()
        .unwrap_or("");
    
    if category != "chat" {
        msg!("Invalid category: expected 'chat', got '{}'", category);
        return Err(ErrorCode::InvalidCategory.into());
    }

    // Extract and validate burn_amount field (must match burn amount)
    let memo_burn_amount = match &json_data["burn_amount"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        Value::String(s) => {
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingBurnAmountField.into()),
    };

    // Check if memo burn amount matches expected burn amount
    if memo_burn_amount != expected_amount {
        let memo_tokens = memo_burn_amount / 1_000_000;
        let expected_tokens = expected_amount / 1_000_000;
        msg!("Burn amount mismatch: memo contains {} tokens ({} units), but burning {} tokens ({} units)", 
             memo_tokens, memo_burn_amount, expected_tokens, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }

    // Extract and validate group_id field
    let memo_group_id = match &json_data["group_id"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        Value::String(s) => {
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingGroupIdField.into()),
    };
    
    if memo_group_id != expected_group_id {
        msg!("Group ID mismatch: memo contains {}, expected {}", memo_group_id, expected_group_id);
        return Err(ErrorCode::GroupIdMismatch.into());
    }

    let name = json_data["name"]
        .as_str()
        .ok_or(ErrorCode::MissingNameField)?
        .to_string();
    
    if name.is_empty() || name.len() > 64 {
        return Err(ErrorCode::InvalidGroupName.into());
    }

    let description = json_data["description"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    if description.len() > 128 {
        return Err(ErrorCode::InvalidGroupDescription.into());
    }

    // Extract and validate image field
    let image = json_data["image"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    if image.len() > 256 {
        return Err(ErrorCode::InvalidGroupImage.into());
    }

    let tags = match &json_data["tags"] {
        Value::Array(arr) => {
            let mut tags = Vec::new();
            for tag_value in arr {
                if let Some(tag_str) = tag_value.as_str() {
                    if tag_str.is_empty() || tag_str.len() > 32 {
                        return Err(ErrorCode::InvalidTag.into());
                    }
                    tags.push(tag_str.to_string());
                } else {
                    return Err(ErrorCode::InvalidTag.into());
                }
            }
            if tags.len() > 4 {
                return Err(ErrorCode::TooManyTags.into());
            }
            tags
        },
        Value::Null => Vec::new(),
        _ => return Err(ErrorCode::InvalidTagsFormat.into()),
    };

    let min_memo_interval = json_data["min_memo_interval"]
        .as_i64();

    msg!("Group creation memo parsed successfully: category=chat, group_id={}, name={}, image_len={}, amount={} tokens", 
         expected_group_id, name, image.len(), expected_amount / 1_000_000);

    Ok(GroupCreationData {
        group_id: expected_group_id,
        name,
        description,
        image,
        tags,
        min_memo_interval,
    })
}

/// Validate memo for burn operation (enhanced with operation and group_id validation)
fn validate_memo_for_burn(memo_data: &[u8], expected_group_id: u64, expected_amount: u64) -> Result<()> {
    // Enhanced UTF-8 validation
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Clean the string
    let clean_str = memo_str
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    
    // Parse as JSON
    let json_data: Value = serde_json::from_str(&clean_str)
        .map_err(|_| ErrorCode::InvalidMemoFormat)?;

    // Extract and validate operation field (must match expected operation)
    let operation = json_data["operation"]
        .as_str()
        .ok_or(ErrorCode::MissingOperationField)?;
    
    if operation != "like_group" {
        msg!("Invalid operation: expected 'like_group', got '{}'", operation);
        return Err(ErrorCode::InvalidOperation.into());
    }

    // Validate category field (required) - must be "chat"
    let category = json_data["category"]
        .as_str()
        .unwrap_or("");
    
    if category != "chat" {
        msg!("Invalid category: expected 'chat', got '{}'", category);
        return Err(ErrorCode::InvalidCategory.into());
    }

    // Extract and validate group_id field (required for burn operation)
    let memo_group_id = match &json_data["group_id"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        Value::String(s) => {
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingGroupIdField.into()),
    };
    
    if memo_group_id != expected_group_id {
        msg!("Group ID mismatch: memo contains {}, expected {}", memo_group_id, expected_group_id);
        return Err(ErrorCode::GroupIdMismatch.into());
    }

    // Extract burn_amount from memo
    let memo_burn_amount = match &json_data["burn_amount"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        Value::String(s) => {
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidBurnAmountFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingBurnAmountField.into()),
    };

    // Check if memo burn amount matches expected burn amount
    if memo_burn_amount != expected_amount {
        let memo_tokens = memo_burn_amount / 1_000_000;
        let expected_tokens = expected_amount / 1_000_000;
        msg!("Burn amount mismatch: memo contains {} tokens ({} units), but burning {} tokens ({} units)", 
             memo_tokens, memo_burn_amount, expected_tokens, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }

    let token_count = expected_amount / 1_000_000;
    msg!("Burn memo validation passed: operation=like_group, category=chat, group_id={}, {} tokens ({} units)", 
         expected_group_id, token_count, expected_amount);
    Ok(())
}

/// Parse and validate memo content for send memo (with required field validation)
fn parse_and_validate_memo_for_send(memo_data: &[u8], expected_group_id: u64, expected_sender: Pubkey) -> Result<String> {
    // Enhanced UTF-8 validation
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // Clean the string (handle JSON escaping)
    let clean_str = memo_str
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    
    // Parse as JSON
    let json_data: Value = serde_json::from_str(&clean_str)
        .map_err(|e| {
            msg!("JSON parsing failed: {}", e);
            ErrorCode::InvalidMemoFormat
        })?;

    // 0. Validate operation field (required) - must be "send_message"
    let operation = json_data["operation"]
        .as_str()
        .ok_or(ErrorCode::MissingOperationField)?;
    
    if operation != "send_message" {
        msg!("Invalid operation: expected 'send_message', got '{}'", operation);
        return Err(ErrorCode::InvalidOperation.into());
    }

    // 0. Validate category field (required) - must be "chat"
    let category = json_data["category"]
        .as_str()
        .unwrap_or("");
    
    if category != "chat" {
        msg!("Invalid category: expected 'chat', got '{}'", category);
        return Err(ErrorCode::InvalidCategory.into());
    }

    // 1. Validate group_id field (required)
    let memo_group_id = match &json_data["group_id"] {
        Value::Number(n) => {
            if let Some(int_val) = n.as_u64() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        Value::String(s) => {
            if let Ok(int_val) = s.parse::<u64>() {
                int_val
            } else {
                return Err(ErrorCode::InvalidGroupIdFormat.into());
            }
        },
        _ => return Err(ErrorCode::MissingGroupIdField.into()),
    };
    
    if memo_group_id != expected_group_id {
        msg!("Group ID mismatch: memo contains {}, expected {}", memo_group_id, expected_group_id);
        return Err(ErrorCode::GroupIdMismatch.into());
    }

    // 2. Validate sender field (required)
    let memo_sender_str = json_data["sender"]
        .as_str()
        .ok_or(ErrorCode::MissingSenderField)?;
    
    let memo_sender = Pubkey::from_str(memo_sender_str)
        .map_err(|_| ErrorCode::InvalidSenderFormat)?;
    
    if memo_sender != expected_sender {
        msg!("Sender mismatch: memo contains {}, expected {}", memo_sender, expected_sender);
        return Err(ErrorCode::SenderMismatch.into());
    }

    // 3. Validate message field (required)
    let message = json_data["message"]
        .as_str()
        .ok_or(ErrorCode::MissingMessageField)?
        .to_string();
    
    if message.is_empty() {
        return Err(ErrorCode::EmptyMessage.into());
    }
    
    if message.len() > 512 {
        return Err(ErrorCode::MessageTooLong.into());
    }

    // 4. Validate receiver field (optional)
    let receiver_info = match &json_data["receiver"] {
        Value::String(s) => {
            if s.is_empty() {
                // Empty string is treated as no receiver
                None
            } else {
                // Validate that it's a valid Pubkey
                match Pubkey::from_str(s) {
                    Ok(pubkey) => {
                        msg!("Receiver field validated: {}", pubkey);
                        Some(pubkey)
                    },
                    Err(_) => {
                        msg!("Invalid receiver format: {}", s);
                        return Err(ErrorCode::InvalidReceiverFormat.into());
                    }
                }
            }
        },
        Value::Null => None, // Explicitly null is OK
        _ => {
            // Field exists but is not a string or null
            if json_data.get("receiver").is_some() {
                return Err(ErrorCode::InvalidReceiverFormat.into());
            } else {
                None // Field doesn't exist, which is OK
            }
        }
    };

    // 5. Validate reply_to_sig field (optional)
    let reply_to_sig_info = match &json_data["reply_to_sig"] {
        Value::String(s) => {
            if s.is_empty() {
                // Empty string is treated as no reply
                None
            } else {
                // Validate that it's a valid signature format (base58 encoded, 64 bytes when decoded)
                match bs58::decode(s).into_vec() {
                    Ok(decoded) => {
                        if decoded.len() == 64 {
                            msg!("Reply signature field validated: {}", s);
                            Some(s.clone())
                        } else {
                            msg!("Invalid reply signature length: {} bytes (expected 64)", decoded.len());
                            return Err(ErrorCode::InvalidReplySignatureFormat.into());
                        }
                    },
                    Err(_) => {
                        msg!("Invalid reply signature encoding: {}", s);
                        return Err(ErrorCode::InvalidReplySignatureFormat.into());
                    }
                }
            }
        },
        Value::Null => None, // Explicitly null is OK
        _ => {
            // Field exists but is not a string or null
            if json_data.get("reply_to_sig").is_some() {
                return Err(ErrorCode::InvalidReplySignatureFormat.into());
            } else {
                None // Field doesn't exist, which is OK
            }
        }
    };

    // Log validation results
    let mut log_parts = vec![
        format!("category=chat"),
        format!("group_id={}", expected_group_id),
        format!("sender={}", expected_sender),
        format!("message_len={}", message.len()),
    ];
    
    if let Some(receiver) = receiver_info {
        log_parts.push(format!("receiver={}", receiver));
    }
    
    if let Some(ref reply_sig) = reply_to_sig_info {
        log_parts.push(format!("reply_to_sig={}...", &reply_sig[..16]));
    }

    msg!("Memo validation passed: {}", log_parts.join(", "));

    Ok(message)
}

/// Parse memo content for display (simpler parsing for send memo)
fn parse_memo_content(memo_data: &[u8]) -> Result<String> {
    // Enhanced UTF-8 validation
    let memo_str = match std::str::from_utf8(memo_data) {
        Ok(s) => s,
        Err(e) => {
            msg!("Invalid UTF-8 sequence at byte position: {}", e.valid_up_to());
            return Err(ErrorCode::InvalidMemoFormat.into());
        }
    };
    
    // Basic security check
    if memo_str.contains('\0') {
        msg!("Memo contains null characters");
        return Err(ErrorCode::InvalidMemoFormat.into());
    }
    
    // For display, just return cleaned string
    let clean_str = memo_str
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\");
    
    Ok(clean_str)
}

/// Check for memo instruction with enhanced validation (following memo-burn pattern)
fn check_memo_instruction(instructions: &AccountInfo) -> Result<(bool, Vec<u8>)> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // Get current instruction index
    let current_index = anchor_lang::solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // First check the most likely position (index 1)
    if current_index > 1 {
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    // Validate memo length (69-800 bytes)
                    let memo_length = ix.data.len();
                    if memo_length < 69 {
                        msg!("Memo too short: {} bytes (minimum: 69)", memo_length);
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                    if memo_length > 800 {
                        msg!("Memo too long: {} bytes (maximum: 800)", memo_length);
                        return Err(ErrorCode::MemoTooLong.into());
                    }
                    
                    msg!("Memo length validation passed: {} bytes (range: 69-800)", memo_length);
                    return Ok((true, ix.data.to_vec()));
                }
            },
            Err(_) => {}
        }
    }
    
    // If not found at index 1, check other positions as fallback
    for i in 0..current_index {
        if i == 1 { continue; } // Skip index 1 as we already checked it
        
        match anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked(i.into(), instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    // Validate memo length (69-800 bytes)
                    let memo_length = ix.data.len();
                    if memo_length < 69 {
                        msg!("Memo too short: {} bytes (minimum: 69)", memo_length);
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                    if memo_length > 800 {
                        msg!("Memo too long: {} bytes (maximum: 800)", memo_length);
                        return Err(ErrorCode::MemoTooLong.into());
                    }
                    
                    msg!("Memo length validation passed: {} bytes (range: 69-800)", memo_length);
                    return Ok((true, ix.data.to_vec()));
                }
            },
            Err(_) => { continue; }
        }
    }
    
    // No valid memo found
    Ok((false, vec![]))
}

/// Data structure for group creation memo
#[derive(Debug, Serialize, Deserialize)]
struct GroupCreationData {
    group_id: u64,
    name: String,
    description: String,
    image: String,
    tags: Vec<String>,
    min_memo_interval: Option<i64>,
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
        constraint = admin.key().to_string() == AUTHORIZED_ADMIN @ ErrorCode::UnauthorizedAdmin
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
        constraint = mint.key().to_string() == AUTHORIZED_MINT @ ErrorCode::UnauthorizedMint
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
        constraint = mint.key().to_string() == AUTHORIZED_MINT @ ErrorCode::UnauthorizedMint
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA serving as mint authority (from memo-mint program)
    #[account(
        mut,
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
        constraint = mint.key().to_string() == AUTHORIZED_MINT @ ErrorCode::UnauthorizedMint
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

    #[msg("Invalid memo format: Memo must contain valid UTF-8 and properly formatted data.")]
    InvalidMemoFormat,

    #[msg("Group ID mismatch: Group ID from memo does not match instruction parameter.")]
    GroupIdMismatch,

    #[msg("Missing group_id field in memo JSON.")]
    MissingGroupIdField,

    #[msg("Missing name field in memo JSON.")]
    MissingNameField,

    #[msg("Invalid tags format: Tags must be an array of strings.")]
    InvalidTagsFormat,

    #[msg("Invalid group ID format: Group ID must be a valid u64 number.")]
    InvalidGroupIdFormat,

    #[msg("Burn amount too small. Must burn at least 1 token (1,000,000 units for decimal=6).")]
    BurnAmountTooSmall,

    #[msg("Invalid burn amount. Amount must be a multiple of 1,000,000 units (whole tokens only).")]
    InvalidBurnAmount,

    #[msg("Missing burn_amount field in memo JSON.")]
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
    
    #[msg("Missing sender field in memo JSON.")]
    MissingSenderField,
    
    #[msg("Invalid sender format in memo. Must be a valid Pubkey string.")]
    InvalidSenderFormat,
    
    #[msg("Sender mismatch: The sender in memo must match the transaction signer.")]
    SenderMismatch,
    
    #[msg("Missing message field in memo JSON.")]
    MissingMessageField,
    
    #[msg("Empty message: Message field cannot be empty.")]
    EmptyMessage,
    
    #[msg("Message too long: Message must be at most 512 characters.")]
    MessageTooLong,
    
    #[msg("Invalid receiver format in memo. Must be a valid Pubkey string.")]
    InvalidReceiverFormat,
    
    #[msg("Invalid reply signature format in memo. Must be a valid base58-encoded signature string.")]
    InvalidReplySignatureFormat,
    
    #[msg("Missing operation field in memo JSON.")]
    MissingOperationField,
    
    #[msg("Invalid operation: Operation does not match the expected operation for this instruction.")]
    InvalidOperation,
}
