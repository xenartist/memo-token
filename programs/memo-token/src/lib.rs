use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use solana_program::sysvar::instructions::{ID as INSTRUCTIONS_ID};
use std::str::FromStr;
use serde_json::Value;

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

// global burn index
#[account]
#[derive(Default)]
pub struct GlobalBurnIndex {
    pub shard_count: u8,    // current shard count
    pub shards: Vec<ShardInfo>, // shard info list
}

// shard info
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ShardInfo {
    pub pubkey: Pubkey,   // shard account address
}

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
    pub current_index: u8,    // current index
    pub records: Vec<BurnRecord>, // burn records
}

impl TopBurnShard {
    pub const MAX_RECORDS: usize = 69;
    pub const MIN_BURN_AMOUNT: u64 = 42069 * 1_000_000_000; // 42069 tokens threshold
    
    pub fn add_record(&mut self, record: BurnRecord) -> bool {
        // Check if the burn amount meets the minimum threshold
        if record.amount < Self::MIN_BURN_AMOUNT {
            msg!("Burn amount {} is below threshold {}", 
                record.amount / 1_000_000_000, 
                Self::MIN_BURN_AMOUNT / 1_000_000_000);
            return false;
        }

        // Add record using circular buffer logic
        if self.records.len() < Self::MAX_RECORDS {
            self.records.push(record);
        } else {
            self.records[self.current_index as usize] = record;
        }
        
        // Update current_index
        self.current_index = ((self.current_index as usize + 1) % Self::MAX_RECORDS) as u8;
        
        msg!("Added record to top burn shard at index {}", self.current_index);
        true
    }
}

// user profile
#[account]
#[derive(Default)]
pub struct UserProfile {
    pub pubkey: Pubkey,           // 32 bytes - user pubkey
    pub username: String,         // 4 + 32 bytes - max 32 characters username
    pub total_minted: u64,        // 8 bytes - total minted tokens
    pub total_burned: u64,        // 8 bytes - total burned tokens
    pub mint_count: u64,          // 8 bytes - mint count
    pub burn_count: u64,          // 8 bytes - burn count
    pub profile_image: String,    // 4 + 256 bytes - hex string of the profile image
    pub created_at: i64,          // 8 bytes - create timestamp
    pub last_updated: i64,        // 8 bytes - last updated timestamp
    pub latest_burn_history_index: Option<u64>, // 9 bytes (1 byte for Option + 8 bytes for u64)
}

#[account]
#[derive(Default)]
pub struct UserBurnHistory {
    pub owner: Pubkey,           // 32 bytes - user pubkey
    pub index: u64,              // 8 bytes - history index
    pub signatures: Vec<String>, // 4 + (92 * 100) bytes - max 100 signatures
}

#[program]
pub mod memo_token {
    use super::*;

    // initialize global burn index
    pub fn initialize_global_burn_index(ctx: Context<InitializeGlobalBurnIndex>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.payer.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }
        
        let burn_index = &mut ctx.accounts.global_burn_index;
        burn_index.shard_count = 0;
        burn_index.shards = Vec::new();
        msg!("Global burn index initialized");
        Ok(())
    }

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

        // update global burn index
        let burn_index = &mut ctx.accounts.global_burn_index;
        burn_index.shard_count += 1;
        burn_index.shards.push(ShardInfo {
            pubkey: ctx.accounts.latest_burn_shard.key(),
        });

        msg!("Latest burn shard initialized");
        Ok(())
    }

    // initialize user profile
    pub fn initialize_user_profile(
        ctx: Context<InitializeUserProfile>, 
        username: String, 
        profile_image: String
    ) -> Result<()> {
        // check username length
        if username.len() > 32 {
            return Err(ErrorCode::UsernameTooLong.into());
        }
        
        // check profile image length
        if profile_image.len() > 256 {
            return Err(ErrorCode::ProfileImageTooLong.into());
        }
        
        let user_profile = &mut ctx.accounts.user_profile;
        let clock = Clock::get()?;
        
        user_profile.pubkey = ctx.accounts.user.key();
        user_profile.username = username;
        user_profile.total_minted = 0;
        user_profile.total_burned = 0;
        user_profile.mint_count = 0;
        user_profile.burn_count = 0;
        user_profile.profile_image = profile_image;
        user_profile.created_at = clock.unix_timestamp;
        user_profile.last_updated = clock.unix_timestamp;
        user_profile.latest_burn_history_index = None;
        
        msg!("User profile initialized for: {}", user_profile.username);
        Ok(())
    }
    
    // update user profile
    pub fn update_user_profile(
        ctx: Context<UpdateUserProfile>, 
        username: Option<String>, 
        profile_image: Option<String>
    ) -> Result<()> {
        let user_profile = &mut ctx.accounts.user_profile;
        let clock = Clock::get()?;
        
        // update username (if provided)
        if let Some(new_username) = username {
            if new_username.len() > 32 {
                return Err(ErrorCode::UsernameTooLong.into());
            }
            user_profile.username = new_username;
        }
        
        // update profile image (if provided)
        if let Some(new_profile_image) = profile_image {
            if new_profile_image.len() > 256 {
                return Err(ErrorCode::ProfileImageTooLong.into());
            }
            user_profile.profile_image = new_profile_image;
        }
        
        // update last updated time
        user_profile.last_updated = clock.unix_timestamp;
        
        msg!("User profile updated for: {}", user_profile.username);
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
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
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

        // check if should record burn history
        let should_record_history = json_data["burn_history"]
            .as_str()
            .map(|v| v == "Y")
            .unwrap_or(false);

        // burn tokens
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Burn {
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
        
        // update top burn shard
        if let Some(top_burn_shard) = &mut ctx.accounts.top_burn_shard {
            if top_burn_shard.add_record(record) {
                msg!("Added new burn record to top burn shard");
            } else {
                msg!("Burn amount not high enough for top burn shard (minimum 42069 tokens)");
            }
        }

        // check if should record history
        if should_record_history {
            if let Some(user_profile) = &mut ctx.accounts.user_profile {
                // check user authority
                if user_profile.pubkey != ctx.accounts.user.key() {
                    return Err(ErrorCode::UnauthorizedUser.into());
                }

                // get current burn history account
                if let Some(burn_history) = &mut ctx.accounts.burn_history {
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
                } else {
                    // if no burn history account provided, return error
                    return Err(ErrorCode::BurnHistoryRequired.into());
                }
            }
        }

        Ok(())
    }

    // Close global burn index account
    pub fn close_global_burn_index(ctx: Context<CloseGlobalBurnIndex>) -> Result<()> {
        // Authority check is handled in the account validation
        msg!("Closing global burn index account");
        Ok(())
    }

    // Close latest burn shard account
    pub fn close_latest_burn_shard(ctx: Context<CloseLatestBurnShard>) -> Result<()> {
        // Authority check is handled in the account validation
        // Remove shard info from index
        let burn_index = &mut ctx.accounts.global_burn_index;
        if let Some(pos) = burn_index.shards.iter().position(|x| x.pubkey == ctx.accounts.latest_burn_shard.key()) {
            burn_index.shards.remove(pos);
            burn_index.shard_count -= 1;
        }
        
        msg!("Closing latest burn shard account");
        Ok(())
    }

    // initialize top burn shard
    pub fn initialize_top_burn_shard(ctx: Context<InitializeTopBurnShard>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.payer.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        // initialize shard
        let burn_shard = &mut ctx.accounts.top_burn_shard;
        burn_shard.current_index = 0;
        burn_shard.records = Vec::new();

        // update global burn index
        let burn_index = &mut ctx.accounts.global_burn_index;
        burn_index.shard_count += 1;
        burn_index.shards.push(ShardInfo {
            pubkey: ctx.accounts.top_burn_shard.key(),
        });

        msg!("Top burn shard initialized");
        Ok(())
    }

    // Close top burn shard account
    pub fn close_top_burn_shard(ctx: Context<CloseTopBurnShard>) -> Result<()> {
        // Authority check is handled in the account validation
        // Remove shard info from index
        let burn_index = &mut ctx.accounts.global_burn_index;
        if let Some(pos) = burn_index.shards.iter().position(|x| x.pubkey == ctx.accounts.top_burn_shard.key()) {
            burn_index.shards.remove(pos);
            burn_index.shard_count -= 1;
        }
        
        msg!("Closing top burn shard account");
        Ok(())
    }

    // Close user profile
    pub fn close_user_profile(ctx: Context<CloseUserProfile>) -> Result<()> {
        // Ensure only the user can close their own profile
        if ctx.accounts.user.key() != ctx.accounts.user_profile.pubkey {
            return Err(ErrorCode::UnauthorizedUser.into());
        }
        
        msg!("Closing user profile for: {}", ctx.accounts.user_profile.username);
        Ok(())
    }

    pub fn initialize_burn_history(
        ctx: Context<InitializeUserBurnHistory>,
        index: u64
    ) -> Result<()> {
        let burn_history = &mut ctx.accounts.burn_history;
        let user_profile = &mut ctx.accounts.user_profile;
        
        // check index
        match user_profile.latest_burn_history_index {
            None => {
                // first create, index must be 0
                if index != 0 {
                    return Err(ErrorCode::InvalidBurnHistoryIndex.into());
                }
            },
            Some(latest_index) => {
                // ensure new index is consecutive
                if index != latest_index + 1 {
                    return Err(ErrorCode::InvalidBurnHistoryIndex.into());
                }
            }
        }
        
        // initialize burn history
        burn_history.owner = ctx.accounts.user.key();
        burn_history.index = index;
        burn_history.signatures = Vec::new();
        
        // update user profile
        user_profile.latest_burn_history_index = Some(index);
        
        msg!("Initialized burn history account with index: {}", index);
        Ok(())
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

// modify ProcessTransfer structure, add optional storage account
#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    /// CHECK: PDA as mint authority
    pub mint_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    
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
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = user
    )]
    pub token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    
    /// CHECK: Instructions sysvar
    #[account(address = INSTRUCTIONS_ID)]
    pub instructions: AccountInfo<'info>,
    
    /// Latest burn shard (optional)
    #[account(mut)]
    pub latest_burn_shard: Option<Account<'info, LatestBurnShard>>,
    
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

    // burn history (optional)
    #[account(
        mut,
        seeds = [
            b"burn_history",
            user.key().as_ref(),
            user_profile.as_ref().map(|p| p.latest_burn_history_index.unwrap_or(0)).unwrap_or(0).to_le_bytes().as_ref()
        ],
        bump
    )]
    pub burn_history: Option<Account<'info, UserBurnHistory>>,
}

#[derive(Accounts)]
pub struct InitializeGlobalBurnIndex<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + // discriminator
               1 + // shard_count
               4 + // vec len
               (128 * 32), // 128 shards
        seeds = [b"global_burn_index"],
        bump
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeLatestBurnShard<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"global_burn_index"],
        bump
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
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
pub struct CloseGlobalBurnIndex<'info> {
    #[account(mut, constraint = recipient.key().to_string() == ADMIN_PUBKEY)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_burn_index"],
        bump,
        close = recipient
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseLatestBurnShard<'info> {
    #[account(mut, constraint = recipient.key().to_string() == ADMIN_PUBKEY)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_burn_index"],
        bump
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
    #[account(
        mut,
        close = recipient
    )]
    pub latest_burn_shard: Account<'info, LatestBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeTopBurnShard<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"global_burn_index"],
        bump
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + // discriminator
               4 + // vec len
               (69 * (32 + 88 + 8 + 8 + 8)), // 69 records
        seeds = [b"top_burn_shard"],
        bump
    )]
    pub top_burn_shard: Account<'info, TopBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseTopBurnShard<'info> {
    #[account(mut, constraint = recipient.key().to_string() == ADMIN_PUBKEY)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"global_burn_index"],
        bump
    )]
    pub global_burn_index: Account<'info, GlobalBurnIndex>,
    
    #[account(
        mut,
        close = recipient
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
               4 + 32 + // username (String)
               8 +     // total_minted
               8 +     // total_burned
               8 +     // mint_count
               8 +     // burn_count
               4 + 256 + // profile_image (Hex String)
               8 +     // created_at
               8,      // last_updated
        seeds = [b"user_profile", user.key().as_ref()],
        bump
    )]
    pub user_profile: Account<'info, UserProfile>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateUserProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"user_profile", user.key().as_ref()],
        bump,
        constraint = user_profile.pubkey == user.key() @ ErrorCode::UnauthorizedUser
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
#[instruction(new_index: u64)]
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
            new_index.to_le_bytes().as_ref()
        ],
        bump
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
    
    #[msg("Unauthorized: Only the authority can perform this action")]
    UnauthorizedAuthority,
    
    #[msg("Unauthorized: Only the admin can perform this action")]
    UnauthorizedAdmin,
    
    #[msg("Burn amount too small. Must burn at least 1 token.")]
    BurnAmountTooSmall,
    
    #[msg("Username too long. Maximum length is 32 characters.")]
    UsernameTooLong,
    
    #[msg("Profile image too long. Maximum length is 256 characters.")]
    ProfileImageTooLong,
    
    #[msg("Unauthorized: Only the user can update their own profile")]
    UnauthorizedUser,

    #[msg("Invalid burn amount. Must be an integer multiple of 1 token (1,000,000,000 units).")]
    InvalidBurnAmount,

    #[msg("Invalid burn history index")]
    InvalidBurnHistoryIndex,
    
    #[msg("Burn history account is full")]
    BurnHistoryFull,
    
    #[msg("Invalid signature length")]
    InvalidSignatureLength,

    #[msg("Burn history account is required for recording burn history")]
    BurnHistoryRequired,
}