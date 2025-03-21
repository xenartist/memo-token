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

// single max burn shard
#[account]
#[derive(Default)]
pub struct SingleMaxBurnShard {
    pub records: Vec<BurnRecord>, // burn records sorted by amount (descending)
}

impl SingleMaxBurnShard {
    pub const MAX_RECORDS: usize = 69;
    
    pub fn add_record_if_qualified(&mut self, record: BurnRecord) -> bool {
        // First, check if this record qualifies to be added
        if self.records.len() < Self::MAX_RECORDS {
            // If we have less than max records, always add the record
            // Insert into sorted position
            self.insert_sorted(record);
            return true;
        } else {
            // Check if the new record has a higher amount than the smallest record
            let smallest_record = &self.records[self.records.len() - 1];
            if record.amount >= smallest_record.amount {
                // Remove the smallest record (last in our sorted array)
                self.records.pop();
                // Insert the new record in sorted position
                self.insert_sorted(record);
                return true;
            }
        }
        
        // Not qualified to be added
        false
    }
    
    // Helper function to insert a record in sorted position (by amount, descending)
    fn insert_sorted(&mut self, record: BurnRecord) {
        let mut insert_pos = self.records.len();
        
        // Find the position to insert (descending order)
        for (i, existing) in self.records.iter().enumerate() {
            if record.amount >= existing.amount {
                insert_pos = i;
                break;
            }
        }
        
        // Insert at the found position
        self.records.insert(insert_pos, record);
    }
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
        
        // check memo instruction
        let (memo_found, memo_data) = check_memo_instruction(ctx.accounts.instructions.as_ref(), 69)?;
        if !memo_found {
            msg!("No memo instruction found");
            return Err(ErrorCode::MemoRequired.into());
        }
        
        // get current clock information
        let clock = Clock::get()?;
        
        // Try to convert to string and parse JSON
        let signature = if let Ok(memo_str) = String::from_utf8(memo_data.clone()) {
            let clean_str = memo_str
                .trim_matches('"')
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            
            // Try to parse JSON
            let json_data = serde_json::from_str::<serde_json::Value>(&clean_str)
                .map_err(|_| {
                    msg!("Failed to parse JSON after cleaning");
                    ErrorCode::InvalidMemoFormat
                })?;

            msg!("Successfully parsed JSON: {}", json_data);
            
            // Extract signature as a String to avoid borrowing issues
            json_data["signature"]
                .as_str()
                .ok_or(ErrorCode::MissingSignature)?
                .to_string()
        } else {
            return Err(ErrorCode::InvalidMemoFormat.into());
        };

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
        
        // Create the burn record
        let record = BurnRecord {
            pubkey: ctx.accounts.user.key(),
            signature,
            slot: clock.slot,
            blocktime: clock.unix_timestamp,
            amount,
        };
        
        // update latest burn shard
        if let Some(latest_burn_shard) = &mut ctx.accounts.latest_burn_shard {
            latest_burn_shard.add_record(record.clone());
            msg!("Added new burn record to latest burn shard");
        }
        
        // update single max burn shard
        if let Some(single_max_burn_shard) = &mut ctx.accounts.single_max_burn_shard {
            if single_max_burn_shard.add_record_if_qualified(record) {
                msg!("Added new burn record to single max burn shard");
            } else {
                msg!("Burn amount not high enough for single max burn shard");
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

    // initialize single max burn shard
    pub fn initialize_single_max_burn_shard(ctx: Context<InitializeSingleMaxBurnShard>) -> Result<()> {
        // check if caller is admin
        if ctx.accounts.payer.key().to_string() != ADMIN_PUBKEY {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        // initialize shard
        let burn_shard = &mut ctx.accounts.single_max_burn_shard;
        burn_shard.records = Vec::new();

        // update global burn index
        let burn_index = &mut ctx.accounts.global_burn_index;
        burn_index.shard_count += 1;
        burn_index.shards.push(ShardInfo {
            pubkey: ctx.accounts.single_max_burn_shard.key(),
        });

        msg!("Single max burn shard initialized");
        Ok(())
    }

    // Close single max burn shard account
    pub fn close_single_max_burn_shard(ctx: Context<CloseSingleMaxBurnShard>) -> Result<()> {
        // Authority check is handled in the account validation
        // Remove shard info from index
        let burn_index = &mut ctx.accounts.global_burn_index;
        if let Some(pos) = burn_index.shards.iter().position(|x| x.pubkey == ctx.accounts.single_max_burn_shard.key()) {
            burn_index.shards.remove(pos);
            burn_index.shard_count -= 1;
        }
        
        msg!("Closing single max burn shard account");
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
    
    /// Single max burn shard (optional)
    #[account(mut)]
    pub single_max_burn_shard: Option<Account<'info, SingleMaxBurnShard>>,
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
pub struct InitializeSingleMaxBurnShard<'info> {
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
        seeds = [b"single_max_burn_shard"],
        bump
    )]
    pub single_max_burn_shard: Account<'info, SingleMaxBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseSingleMaxBurnShard<'info> {
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
    pub single_max_burn_shard: Account<'info, SingleMaxBurnShard>,
    
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
}