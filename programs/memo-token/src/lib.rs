use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount};
use anchor_spl::token_2022::{self, Token2022};
use solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use serde_json::Value;
use borsh::BorshDeserialize;

declare_id!("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw");

// individual burn record
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct BurnRecord {
    pub pubkey: Pubkey,      // 32 bytes
    pub signature: String,    // 88 bytes (base58 encoded signature)
    pub slot: u64,           // 8 bytes
    pub blocktime: i64,      // 8 bytes
    pub amount: u64,         // 8 bytes - token burn amount
}

// admin public key
pub const ADMIN_PUBKEY: &str = "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn"; // replace with your admin public key

// latest burn shard
#[account]
#[derive(Default)]
pub struct LatestBurnShard {
    pub current_index: u8,    // current index
    pub records: Vec<BurnRecord>, // burn records
}

impl LatestBurnShard {
    pub const MAX_RECORDS: usize = 69;
    
    pub fn add_record(&mut self, record: BurnRecord) {
        if self.records.len() < Self::MAX_RECORDS {
            self.records.push(record);
        } else {
            self.records[self.current_index as usize] = record;
        }
        self.current_index = ((self.current_index as usize + 1) % Self::MAX_RECORDS) as u8;
    }
}

// top burn shard
#[account]
#[derive(Default)]
pub struct TopBurnShard {
    pub index: u128,           // Index of this shard in the global index
    pub creator: Pubkey,      // Creator's public key
    pub records: Vec<BurnRecord>, // Burn records
}

impl TopBurnShard {
    pub const MAX_RECORDS: usize = 69;
    pub const MIN_BURN_AMOUNT: u64 = 420 * 1_000_000_000; // 420 tokens threshold
    
    pub fn add_record(&mut self, record: BurnRecord) -> bool {
        // Check if the burn amount meets the minimum threshold
        if record.amount < Self::MIN_BURN_AMOUNT {
            msg!("Burn amount {} is below threshold {}", 
                record.amount / 1_000_000_000, 
                Self::MIN_BURN_AMOUNT / 1_000_000_000);
            return false;
        }

        // Only add if there's still space
        if self.records.len() < Self::MAX_RECORDS {
            self.records.push(record);
            msg!("Added record to top burn shard at index {}", self.index);
            true
        } else {
            msg!("Top burn shard at index {} is full", self.index);
            false
        }
    }
    
    pub fn is_full(&self) -> bool {
        self.records.len() >= Self::MAX_RECORDS
    }
}

// user profile
#[account]
#[derive(Default)]
pub struct UserProfile {
    pub pubkey: Pubkey,           // 32 bytes - user pubkey
    pub total_minted: u64,        // 8 bytes - total minted tokens
    pub total_burned: u64,        // 8 bytes - total burned tokens
    pub mint_count: u64,          // 8 bytes - mint count
    pub burn_count: u64,          // 8 bytes - burn count
    pub created_at: i64,          // 8 bytes - create timestamp
    pub last_updated: i64,        // 8 bytes - last updated timestamp
    pub burn_history_index: Option<u64>, // 9 bytes (1 byte for Option + 8 bytes for u64)
}

#[account]
#[derive(Default)]
pub struct UserBurnHistory {
    pub owner: Pubkey,           // 32 bytes - user pubkey
    pub index: u64,              // 8 bytes - history index
    pub signatures: Vec<String>, // 4 + (92 * 100) bytes - max 100 signatures
}

// First, add the new GlobalTopBurnIndex structure
#[account]
#[derive(Default)]
pub struct GlobalTopBurnIndex {
    pub top_burn_shard_total_count: u128,       // Total count of allocated shards
    pub top_burn_shard_current_index: Option<u128>,  // Current index with available space, None if no shards exist
}

#[program]
pub mod memo_token {
    use super::*;

    // initialize latest burn shard
    pub fn initialize_latest_burn_shard(ctx: Context<InitializeLatestBurnShard>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.payer.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        // initialize shard
        let burn_shard = &mut ctx.accounts.latest_burn_shard;
        burn_shard.current_index = 0;
        burn_shard.records = Vec::new();

        msg!("Latest burn shard initialized");
        Ok(())
    }

    // initialize user profile
    pub fn initialize_user_profile(ctx: Context<InitializeUserProfile>) -> Result<()> {
        let user_profile = &mut ctx.accounts.user_profile;
        let clock = Clock::get()?;
        
        user_profile.pubkey = ctx.accounts.user.key();
        user_profile.total_minted = 0;
        user_profile.total_burned = 0;
        user_profile.mint_count = 0;
        user_profile.burn_count = 0;
        user_profile.created_at = clock.unix_timestamp;
        user_profile.last_updated = clock.unix_timestamp;
        user_profile.burn_history_index = None;
        
        msg!("User profile initialized for user: {}", ctx.accounts.user.key());
        Ok(())
    }

    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // calculate memo length
        let memo_length = memo_data.len();
        
        // check memo length is not too long
        if memo_length > 700 {
            return Err(ErrorCode::MemoTooLong.into());
        }
        
        // determine the possible token count range based on length
        let max_tokens = if memo_length <= 100 {
            1
        } else if memo_length <= 200 {
            2
        } else if memo_length <= 300 {
            3
        } else if memo_length <= 400 {
            4
        } else if memo_length <= 500 {
            5
        } else if memo_length <= 600 {
            6
        } else {
            7 // max 700 bytes
        };
        
        // get PDA and bump
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            ctx.program_id
        );
        
        // verify PDA
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }
        
        // generate random number
        let clock = Clock::get()?;
        let mut hasher = solana_program::hash::Hasher::default();
        hasher.hash(&memo_data);
        hasher.hash(&clock.slot.to_le_bytes());
        hasher.hash(&clock.unix_timestamp.to_le_bytes());
        hasher.hash(ctx.accounts.user.key().as_ref());
        let hash = hasher.result();
        
        // generate random number between 1 and max_tokens
        let random_bytes = &hash.to_bytes()[0..8];
        let random_value = u64::from_le_bytes(random_bytes.try_into().unwrap());
        let token_count = (random_value % max_tokens as u64) + 1;
        
        // calculate mint amount (1 token = 10^9 units)
        let amount = token_count * 1_000_000_000;
        
        // mint tokens
        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token_2022::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[&[b"mint_authority".as_ref(), &[bump]]]
            ),
            amount
        )?;
        
        // update user profile stats (if user profile account exists)
        if let Some(user_profile) = &mut ctx.accounts.user_profile {
            // Check if user_profile.pubkey matches the signer's key
            if user_profile.pubkey != ctx.accounts.user.key() {
                return Err(ErrorCode::UnauthorizedUser.into());
            }
            
            // Check if total_minted would overflow
            let tokens_to_add = token_count;
            if let Some(new_total) = user_profile.total_minted.checked_add(tokens_to_add) {
                user_profile.total_minted = new_total;
            } else {
                msg!("Warning: Total minted would overflow, keeping at max value");
                user_profile.total_minted = u64::MAX;
            }
            
            // Check if mint_count would overflow
            if let Some(new_count) = user_profile.mint_count.checked_add(1) {
                user_profile.mint_count = new_count;
            } else {
                msg!("Warning: Mint count would overflow, keeping at max value");
                user_profile.mint_count = u64::MAX;
            }
            
            // Update last_updated timestamp
            user_profile.last_updated = clock.unix_timestamp;
            
            msg!("Updated user profile stats for mint operation");
        }
        
        // record the number of tokens minted
        msg!("Minted {} tokens", token_count);
        
        Ok(())
    }

    // modify process_burn function
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64) -> Result<()> {
        // check burn amount is at least 1 token (10^9 units)
        if amount < 1_000_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }

        // check burn amount is an integer multiple of 1 token (10^9 units)
        if amount % 1_000_000_000 != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }
        
        // check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            msg!("No memo instruction found");
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // get current clock information
        let clock = Clock::get()?;
        
        // parse memo JSON
        let memo_str = String::from_utf8(memo_data.clone())
            .map_err(|_| ErrorCode::InvalidMemoFormat)?;
        let clean_str = memo_str
            .trim_matches('"')
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        
        // parse JSON
        let json_data: Value = serde_json::from_str(&clean_str)
            .map_err(|_| ErrorCode::InvalidMemoFormat)?;

        // get signature
        let signature = json_data["signature"]
            .as_str()
            .ok_or(ErrorCode::MissingSignature)?
            .to_string();

        // burn tokens
        token_2022::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token_2022::Burn {
                    mint: ctx.accounts.mint.to_account_info(),
                    from: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        msg!("Burned {} tokens", amount / 1_000_000_000);
        
        // update user profile stats (if user profile account exists)
        if let Some(user_profile) = &mut ctx.accounts.user_profile {
            // Check if user_profile.pubkey matches the signer's key
            if user_profile.pubkey != ctx.accounts.user.key() {
                return Err(ErrorCode::UnauthorizedUser.into());
            }
            
            // Calculate tokens to add
            let tokens_to_add = amount / 1_000_000_000;
            
            // Check if total_burned would overflow
            if let Some(new_total) = user_profile.total_burned.checked_add(tokens_to_add) {
                user_profile.total_burned = new_total;
            } else {
                msg!("Warning: Total burned would overflow, keeping at max value");
                user_profile.total_burned = u64::MAX;
            }
            
            // Check if burn_count would overflow
            if let Some(new_count) = user_profile.burn_count.checked_add(1) {
                user_profile.burn_count = new_count;
            } else {
                msg!("Warning: Burn count would overflow, keeping at max value");
                user_profile.burn_count = u64::MAX;
            }
            
            // Update last_updated timestamp
            user_profile.last_updated = clock.unix_timestamp;
            
            msg!("Updated user profile stats for burn operation");
        }
        
        // Create the burn record
        let record = BurnRecord {
            pubkey: ctx.accounts.user.key(),
            signature: signature.clone(),
            slot: clock.slot,
            blocktime: clock.unix_timestamp,
            amount,
        };
        
        // update latest burn shard
        if let Some(latest_burn_shard) = &mut ctx.accounts.latest_burn_shard {
            latest_burn_shard.add_record(record.clone());
            msg!("Added new burn record to latest burn shard");
        }
        
        // if burn amount is enough
        if record.amount >= TopBurnShard::MIN_BURN_AMOUNT {
            // if there is current top burn shard
            if let Some(top_burn_shard) = &mut ctx.accounts.top_burn_shard {
                // check if it is full
                if top_burn_shard.is_full() {
                    msg!("Current top burn shard is full. Please create more shards with init-top-burn-shard.");
                    return Err(ErrorCode::TopBurnShardFull.into());
                }
                
                // current shard has space, add the record
                top_burn_shard.add_record(record.clone());
                msg!("Added burn record to top burn shard with index {}", top_burn_shard.index);
                
                // check if this is the last empty shard
                if top_burn_shard.is_full() {
                    // add this record makes the shard full, update the global index to point to the next shard
                    if let Some(global_index) = &mut ctx.accounts.global_top_burn_index {
                        if let Some(current_index) = global_index.top_burn_shard_current_index {
                            // ensure there is a next available shard
                            if current_index + 1 < global_index.top_burn_shard_total_count {
                                // update the global index to point to the next shard
                                global_index.top_burn_shard_current_index = Some(current_index + 1);
                                msg!("Current shard is now full. Updated global index to point to next shard with index {}", current_index + 1);
                            } else {
                                msg!("Warning: Current shard is now full and no more pre-allocated shards available");
                                msg!("Please create a new shard using init-top-burn-shard before the next high-value burn");
                            }
                        }
                    }
                }
            } else {
                // no top burn shard provided
                msg!("No top burn shard provided. This burn exceeds threshold but can't be recorded in top burns");
            }
        }

        Ok(())
    }

    // Close latest burn shard account
    pub fn close_latest_burn_shard(ctx: Context<CloseLatestBurnShard>) -> Result<()> {
        // Authority check is handled in the account validation
        msg!("Closing latest burn shard account");
        Ok(())
    }

    // initialize top burn shard
    pub fn initialize_top_burn_shard(ctx: Context<InitializeTopBurnShard>) -> Result<()> {
        // initialize shard
        let top_burn_shard = &mut ctx.accounts.top_burn_shard;
        let global_top_burn_index = &mut ctx.accounts.global_top_burn_index;
        
        // basic settings
        top_burn_shard.index = global_top_burn_index.top_burn_shard_total_count;
        top_burn_shard.creator = ctx.accounts.user.key();
        top_burn_shard.records = Vec::new();
        
        // update global index count
        if let Some(new_count) = global_top_burn_index.top_burn_shard_total_count.checked_add(1) {
            global_top_burn_index.top_burn_shard_total_count = new_count;
        } else {
            return Err(ErrorCode::CounterOverflow.into());
        }
        
        // handle current_index logic
        if global_top_burn_index.top_burn_shard_current_index.is_none() {
            // first shard
            global_top_burn_index.top_burn_shard_current_index = Some(top_burn_shard.index);
            msg!("Set initial current index to {}", top_burn_shard.index);
        } else if let Some(current_shard) = &ctx.accounts.current_top_burn_shard {
            // has current shard, check if it is full
            if current_shard.key() == Pubkey::default() {
                // the account is default pubkey (client placeholder)
                global_top_burn_index.top_burn_shard_current_index = Some(top_burn_shard.index);
                msg!("Using placeholder account, updated to new shard {}", top_burn_shard.index);
            } else {
                // check if current_shard is full (check if the owner is program)
                if current_shard.owner == &crate::ID {
                    // get account data, avoid parsing the whole structure
                    if let Ok(data) = current_shard.try_borrow_data() {
                        if data.len() >= 68 { // 8 + 16 + 32 + 4 + 8 = 68
                            // read the records.len field (64-68 bytes)
                            let records_len = u32::from_le_bytes([data[64], data[65], data[66], data[67]]) as usize;
                            
                            if records_len >= TopBurnShard::MAX_RECORDS {
                                // shard is full, update index
                                if let Some(current_index) = global_top_burn_index.top_burn_shard_current_index {
                                    let next_index = current_index + 1;
                                    if next_index < global_top_burn_index.top_burn_shard_total_count {
                                        global_top_burn_index.top_burn_shard_current_index = Some(next_index);
                                        msg!("Current shard full, updated index to {}", next_index);
                                    } else {
                                        global_top_burn_index.top_burn_shard_current_index = Some(top_burn_shard.index);
                                        msg!("No more shards, updated to new one {}", top_burn_shard.index);
                                    }
                                }
                            } else {
                                msg!("Current shard has space, keeping index");
                            }
                        }
                    }
                } else {
                    // non-program account, update to new shard
                    global_top_burn_index.top_burn_shard_current_index = Some(top_burn_shard.index);
                    msg!("Invalid current shard, updated to new shard {}", top_burn_shard.index);
                }
            }
        } else {
            // no current shard provided, update to new shard
            global_top_burn_index.top_burn_shard_current_index = Some(top_burn_shard.index);
            msg!("No current shard provided, updated to new shard {}", top_burn_shard.index);
        }
        
        msg!("Initialized top burn shard with index {}", top_burn_shard.index);
        Ok(())
    }

    // Close top burn shard account
    pub fn close_top_burn_shard(ctx: Context<CloseTopBurnShard>) -> Result<()> {
        let global_top_burn_index = &mut ctx.accounts.global_top_burn_index;
        let top_burn_shard = &ctx.accounts.top_burn_shard;
        
        // If the current index points to this shard, we need to update it
        if let Some(current_index) = global_top_burn_index.top_burn_shard_current_index {
            if current_index == top_burn_shard.index {
                // If this is the only shard, set to None
                if global_top_burn_index.top_burn_shard_total_count <= 1 {
                    global_top_burn_index.top_burn_shard_current_index = None;
                } else {
                    // Otherwise, simply set to 0 (should be more intelligently finding the next available shard in a real implementation)
                    global_top_burn_index.top_burn_shard_current_index = Some(0);
                }
            }
        }
        
        msg!("Closing top burn shard with index {}", top_burn_shard.index);
        Ok(())
    }

    // Close user profile
    pub fn close_user_profile(ctx: Context<CloseUserProfile>) -> Result<()> {
        // Ensure only the user can close their own profile
        if ctx.accounts.user.key() != ctx.accounts.user_profile.pubkey {
            return Err(ErrorCode::UnauthorizedUser.into());
        }
        
        msg!("Closing user profile for: {}", ctx.accounts.user_profile.pubkey);
        Ok(())
    }

    pub fn initialize_burn_history(ctx: Context<InitializeUserBurnHistory>) -> Result<()> {
        // get burn history and user profile
        let burn_history = &mut ctx.accounts.burn_history;
        let user_profile = &mut ctx.accounts.user_profile;
        
        // automatically calculate new index
        let new_index = match user_profile.burn_history_index {
            None => {
                0
            },
            Some(latest_index) => {
                latest_index + 1
                }
        };
        
        // initialize burn history
        burn_history.owner = ctx.accounts.user.key();
        burn_history.index = new_index;
        burn_history.signatures = Vec::new();
        
        // update user profile
        user_profile.burn_history_index = Some(new_index);
        
        msg!("Initialized burn history account with index: {}", new_index);
        Ok(())
    }

    // close user burn history
    pub fn close_user_burn_history(ctx: Context<CloseUserBurnHistory>) -> Result<()> {
        let user_profile = &mut ctx.accounts.user_profile;
        let burn_history = &ctx.accounts.burn_history;

        // ensure closing the current index burn history
        if let Some(current_index) = user_profile.burn_history_index {
            // verify current burn history index
            if burn_history.index != current_index {
                return Err(ErrorCode::InvalidBurnHistoryIndex.into());
            }

            // if current index is 0, set burn_history_index to None
            if current_index == 0 {
                user_profile.burn_history_index = None;
                msg!("Closed last burn history account, burn_history_index set to None");
            } else {
                // otherwise, reduce index by 1
                user_profile.burn_history_index = Some(current_index - 1);
                msg!("Reduced burn_history_index to {}", current_index - 1);
            }
        } else {
            return Err(ErrorCode::InvalidBurnHistoryIndex.into());
        }

        msg!("Closing burn history with index {}", burn_history.index);
        Ok(())
    }

    // 2. process burn with history
    pub fn process_burn_with_history(ctx: Context<ProcessBurnWithHistory>, amount: u64) -> Result<()> {
        // check burn amount is at least 1 token (10^9 units)
        if amount < 1_000_000_000 {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }

        // check burn amount is an integer multiple of 1 token (10^9 units)
        if amount % 1_000_000_000 != 0 {
            return Err(ErrorCode::InvalidBurnAmount.into());
        }
        
        // check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            msg!("No memo instruction found");
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // get current clock information
        let clock = Clock::get()?;
        
        // parse memo JSON
        let memo_str = String::from_utf8(memo_data.clone())
            .map_err(|_| ErrorCode::InvalidMemoFormat)?;
        let clean_str = memo_str
            .trim_matches('"')
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        
        // parse JSON
        let json_data: Value = serde_json::from_str(&clean_str)
            .map_err(|_| ErrorCode::InvalidMemoFormat)?;

        // get signature
        let signature = json_data["signature"]
            .as_str()
            .ok_or(ErrorCode::MissingSignature)?
            .to_string();

        // burn tokens
        token_2022::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token_2022::Burn {
                    mint: ctx.accounts.mint.to_account_info(),
                    from: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        msg!("Burned {} tokens", amount / 1_000_000_000);
        
        // update user profile stats (if user profile account exists)
        if let Some(user_profile) = &mut ctx.accounts.user_profile {
            // Check if user_profile.pubkey matches the signer's key
            if user_profile.pubkey != ctx.accounts.user.key() {
                return Err(ErrorCode::UnauthorizedUser.into());
            }
            
            // Calculate tokens to add
            let tokens_to_add = amount / 1_000_000_000;
            
            // Check if total_burned would overflow
            if let Some(new_total) = user_profile.total_burned.checked_add(tokens_to_add) {
                user_profile.total_burned = new_total;
            } else {
                msg!("Warning: Total burned would overflow, keeping at max value");
                user_profile.total_burned = u64::MAX;
            }
            
            // Check if burn_count would overflow
            if let Some(new_count) = user_profile.burn_count.checked_add(1) {
                user_profile.burn_count = new_count;
            } else {
                msg!("Warning: Burn count would overflow, keeping at max value");
                user_profile.burn_count = u64::MAX;
            }
            
            // Update last_updated timestamp
            user_profile.last_updated = clock.unix_timestamp;
            
            msg!("Updated user profile stats for burn operation");
        }
        
        // Create the burn record
        let record = BurnRecord {
            pubkey: ctx.accounts.user.key(),
            signature: signature.clone(),
            slot: clock.slot,
            blocktime: clock.unix_timestamp,
            amount,
        };
        
        // update latest burn shard
        if let Some(latest_burn_shard) = &mut ctx.accounts.latest_burn_shard {
            latest_burn_shard.add_record(record.clone());
            msg!("Added new burn record to latest burn shard");
        }
        
        // if burn amount is enough
        if record.amount >= TopBurnShard::MIN_BURN_AMOUNT {
            // if there is current top burn shard
            if let Some(top_burn_shard) = &mut ctx.accounts.top_burn_shard {
                // check if it is full
                if top_burn_shard.is_full() {
                    msg!("Current top burn shard is full. Please create more shards with init-top-burn-shard.");
                    return Err(ErrorCode::TopBurnShardFull.into());
                }
                
                // current shard has space, add the record
                top_burn_shard.add_record(record.clone());
                msg!("Added burn record to top burn shard with index {}", top_burn_shard.index);
                
                // check if this is the last empty shard
                if top_burn_shard.is_full() {
                    // add this record makes the shard full, update the global index to point to the next shard
                    if let Some(global_index) = &mut ctx.accounts.global_top_burn_index {
                        if let Some(current_index) = global_index.top_burn_shard_current_index {
                            // ensure there is a next available shard
                            if current_index + 1 < global_index.top_burn_shard_total_count {
                                // update the global index to point to the next shard
                                global_index.top_burn_shard_current_index = Some(current_index + 1);
                                msg!("Current shard is now full. Updated global index to point to next shard with index {}", current_index + 1);
                            } else {
                                msg!("Warning: Current shard is now full and no more pre-allocated shards available");
                                msg!("Please create a new shard using init-top-burn-shard before the next high-value burn");
                            }
                        }
                    }
                }
            } else {
                // no top burn shard provided
                msg!("No top burn shard provided. This burn exceeds threshold but can't be recorded in top burns");
            }
        }

        // process burn history
        let burn_history = &mut ctx.accounts.burn_history;
        
        // check burn history account owner
        if burn_history.owner != ctx.accounts.user.key() {
            return Err(ErrorCode::UnauthorizedUser.into());
        }

        // check if burn history account is full
        if burn_history.signatures.len() >= 100 {
            // if full, return error, client needs to create new burn history account
            return Err(ErrorCode::BurnHistoryFull.into());
        }

        // add signature to history
        burn_history.signatures.push(signature);
        msg!("Added burn signature to history index: {}", burn_history.index);

        Ok(())
    }

    // InitializeGlobalBurnIndex
    #[derive(Accounts)]
    pub struct InitializeGlobalTopBurnIndex<'info> {
        #[account(mut, constraint = payer.key().to_string() == ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin)]
        pub payer: Signer<'info>,
        
        #[account(
            init,
            payer = payer,
            space = 8 + // discriminator
                   16 + // top_burn_shard_total_count (u128 needs 16 bytes)
                   17,  // top_burn_shard_current_index (Option<u128>: 1 byte for Option tag + 16 bytes for u128)
            seeds = [b"global_top_burn_index"],
            bump
        )]
        pub global_top_burn_index: Account<'info, GlobalTopBurnIndex>,
        
        pub system_program: Program<'info, System>,
    }

    // initialize global top burn index
    pub fn initialize_global_top_burn_index(ctx: Context<InitializeGlobalTopBurnIndex>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.payer.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }
        
        let global_top_burn_index = &mut ctx.accounts.global_top_burn_index;
        global_top_burn_index.top_burn_shard_total_count = 0;
        global_top_burn_index.top_burn_shard_current_index = None; // initialize to None
        
        msg!("Global top burn index initialized");
        Ok(())
    }

    // close global top burn index
    pub fn close_global_top_burn_index(ctx: Context<CloseGlobalTopBurnIndex>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.recipient.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }
        
        msg!("Closing global top burn index account");
        Ok(())
    }

    // close global top burn index
    #[derive(Accounts)]
    pub struct CloseGlobalTopBurnIndex<'info> {
        #[account(mut, constraint = recipient.key().to_string() == ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin)]
        pub recipient: Signer<'info>,
        
        #[account(
            mut,
            seeds = [b"global_top_burn_index"],
            bump,
            close = recipient
        )]
        pub global_top_burn_index: Account<'info, GlobalTopBurnIndex>,
        
        pub system_program: Program<'info, System>,
    }
}

// Optimized but still somewhat flexible approach
fn check_memo_instruction(instructions: &AccountInfo, min_length: usize) -> Result<(bool, Vec<u8>)> {
    // SPL Memo program ID
    let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
        .expect("Failed to parse memo program ID");
    
    // get current instruction index
    let current_index = solana_program::sysvar::instructions::load_current_index_checked(instructions)?;
    
    // First check the most likely position (index 1)
    if current_index > 1 {
        match solana_program::sysvar::instructions::load_instruction_at_checked(1_usize, instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    if ix.data.len() >= min_length {
                        return Ok((true, ix.data.to_vec()));
                    } else {
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                }
            },
            Err(_) => {}
        }
    }
    
    // If not found at index 1, check other positions as fallback
    for i in 0..current_index {
        if i == 1 { continue; } // Skip index 1 as we already checked it
        
        match solana_program::sysvar::instructions::load_instruction_at_checked(i.into(), instructions) {
            Ok(ix) => {
                if ix.program_id == memo_program_id {
                    if ix.data.len() >= min_length {
                        return Ok((true, ix.data.to_vec()));
                    } else {
                        return Err(ErrorCode::MemoTooShort.into());
                    }
                }
            },
            Err(_) => { continue; }
        }
    }
    
    // No valid memo found
    Ok((false, vec![]))
}

// modify ProcessTransfer structure
#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA as mint authority
    pub mint_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
    
    // user profile (optional)
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
    )]
    pub user_profile: Option<Account<'info, UserProfile>>,
}

// add account structure for burning instruction
#[derive(Accounts)]
pub struct ProcessBurn<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = token_account.mint == mint.key() && token_account.owner == user.key()
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
    
    /// Latest burn shard (optional)
    #[account(mut)]
    pub latest_burn_shard: Option<Account<'info, LatestBurnShard>>,
    
    /// Global top burn index (optional)
    #[account(mut)]
    pub global_top_burn_index: Option<Account<'info, GlobalTopBurnIndex>>,

    // only need the current top burn shard
    /// Current top burn shard (optional)
    #[account(mut)]
    pub top_burn_shard: Option<Account<'info, TopBurnShard>>,
    
    // user profile (optional)
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
    )]
    pub user_profile: Option<Account<'info, UserProfile>>,
}

#[derive(Accounts)]
pub struct ProcessBurnWithHistory<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        constraint = token_account.mint == mint.key() && token_account.owner == user.key()
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
    
    /// Latest burn shard (optional)
    #[account(mut)]
    pub latest_burn_shard: Option<Account<'info, LatestBurnShard>>,
    
    /// Global top burn index (optional)
    #[account(mut)]
    pub global_top_burn_index: Option<Account<'info, GlobalTopBurnIndex>>,

    /// Top burn shard (optional)
    #[account(mut)]
    pub top_burn_shard: Option<Account<'info, TopBurnShard>>,
    
    // user profile (optional)
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
    )]
    pub user_profile: Option<Account<'info, UserProfile>>,

    // burn history (required for this instruction)
    #[account(
        mut,
        seeds = [
            b"burn_history",
            user.key().as_ref(),
            user_profile.as_ref().map(|p| p.burn_history_index.unwrap_or(0)).unwrap_or(0).to_le_bytes().as_ref()
        ],
        bump
    )]
    pub burn_history: Account<'info, UserBurnHistory>,
}

#[derive(Accounts)]
pub struct InitializeLatestBurnShard<'info> {
    #[account(mut, constraint = payer.key().to_string() == ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin)]
    pub payer: Signer<'info>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + // discriminator
               1 + // current_index
               4 + // vec len
               (69 * (32 + 88 + 8 + 8 + 8)), // 69 records
        seeds = [b"latest_burn_shard"],
        bump
    )]
    pub latest_burn_shard: Account<'info, LatestBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseLatestBurnShard<'info> {
    #[account(mut, constraint = recipient.key().to_string() == ADMIN_PUBKEY @ ErrorCode::UnauthorizedAdmin)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"latest_burn_shard"],
        bump,
        close = recipient
    )]
    pub latest_burn_shard: Account<'info, LatestBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeTopBurnShard<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"global_top_burn_index"],
        bump
    )]
    pub global_top_burn_index: Account<'info, GlobalTopBurnIndex>,
    
    #[account(
        init,
        payer = user,
        space = 8 + 16 + 32 + 4 + (69 * (32 + 88 + 8 + 8 + 8)), // 16 bytes for u128 index
        seeds = [
            b"top_burn_shard", 
            &global_top_burn_index.top_burn_shard_total_count.to_le_bytes()[..] // 16 bytes for u128 index
        ],
        bump
    )]
    pub top_burn_shard: Account<'info, TopBurnShard>,
    
    /// CHECK: Validated in initialize_top_burn_shard function
    pub current_top_burn_shard: Option<UncheckedAccount<'info>>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseTopBurnShard<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_top_burn_index"],
        bump
    )]
    pub global_top_burn_index: Account<'info, GlobalTopBurnIndex>,
    
    #[account(
        mut,
        seeds = [
            b"top_burn_shard", 
            top_burn_shard.index.to_le_bytes().as_ref()
        ],
        bump,
        close = user
    )]
    pub top_burn_shard: Account<'info, TopBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeUserProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        init,
        payer = user,
        space = 8 +    // discriminator
               32 +    // pubkey
               8 +     // total_minted
               8 +     // total_burned
               8 +     // mint_count
               8 +     // burn_count
               8 +     // created_at
               8 +     // last_updated
               9,      // burn_history_index (Option<u64>)
        seeds = [b"user_profile", user.key().as_ref()],
        bump
    )]
    pub user_profile: Account<'info, UserProfile>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseUserProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
        close = user // Close account and return SOL to user
    )]
    pub user_profile: Account<'info, UserProfile>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeUserBurnHistory<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
        constraint = user_profile.pubkey == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub user_profile: Account<'info, UserProfile>,
    
    #[account(
        init,
        payer = user,
        space = 8 +    // discriminator
               32 +    // owner
               8 +     // index
               4 + (92 * 100), // Vec<String> for signatures (100 signatures max)
        seeds = [
            b"burn_history",
            user.key().as_ref(),
            &user_profile.burn_history_index.map_or(0, |i| i + 1).to_le_bytes()
        ],
        bump
    )]
    pub burn_history: Account<'info, UserBurnHistory>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseUserBurnHistory<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
        constraint = user_profile.pubkey == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub user_profile: Account<'info, UserProfile>,
    
    #[account(
        mut,
        seeds = [
            b"burn_history",
            user.key().as_ref(),
            &user_profile.burn_history_index.unwrap_or(0).to_le_bytes()
        ],
        bump,
        constraint = burn_history.owner == user.key() @ ErrorCode::UnauthorizedUser,
        close = user  // close account and return SOL to user
    )]
    pub burn_history: Account<'info, UserBurnHistory>,
    
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Memo is too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Memo is too long. Must be at most 700 bytes.")]
    MemoTooLong,
    
    #[msg("Transaction must include a memo.")]
    MemoRequired,

    #[msg("Invalid memo format. Expected JSON format.")]
    InvalidMemoFormat,

    #[msg("Missing signature field in memo JSON.")]
    MissingSignature,
    
    #[msg("Unauthorized: Only the admin can perform this action")]
    UnauthorizedAdmin,
    
    #[msg("Burn amount too small. Must burn at least 1 token.")]
    BurnAmountTooSmall,
    
    #[msg("Unauthorized: Only the user can update their own profile")]
    UnauthorizedUser,

    #[msg("Invalid burn amount. Must be an integer multiple of 1 token (1,000,000,000 units).")]
    InvalidBurnAmount,

    #[msg("Invalid burn history index")]
    InvalidBurnHistoryIndex,
    
    #[msg("Burn history account is full")]
    BurnHistoryFull,

    #[msg("Counter overflow: maximum number of shards reached")]
    CounterOverflow,

    #[msg("Top burn shard is full")]
    TopBurnShardFull,
}