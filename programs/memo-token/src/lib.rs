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
}

// latest burn index
#[account]
#[derive(Default)]
pub struct LatestBurnIndex {
    pub authority: Pubkey,    // creator's address
    pub shard_count: u8,    // current shard count
    pub shards: Vec<ShardInfo>, // shard info list
}

// shard info
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ShardInfo {
    pub zone: String,     // shard zone name (max 32 bytes)
    pub pubkey: Pubkey,   // shard account address
    pub record_count: u16, // current record count
}

// latest burn shard
#[account]
#[derive(Default)]
pub struct LatestBurnShard {
    pub authority: Pubkey,    // creator's address
    pub zone: String,     // shard zone
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

#[program]
pub mod memo_token {
    use super::*;

    // initialize latest burn index
    pub fn initialize_latest_burn_index(ctx: Context<InitializeLatestBurnIndex>) -> Result<()> {
        let burn_index = &mut ctx.accounts.latest_burn_index;
        burn_index.authority = ctx.accounts.payer.key();
        burn_index.shard_count = 0;
        burn_index.shards = Vec::new();
        msg!("Latest burn index initialized");
        Ok(())
    }

    // create new shard
    pub fn create_latest_burn_shard(ctx: Context<CreateLatestBurnShard>, zone: String) -> Result<()> {
        // Check if the payer is the index authority
        if ctx.accounts.latest_burn_index.authority != ctx.accounts.payer.key() {
            return Err(ErrorCode::UnauthorizedAuthority.into());
        }
        
        // validate zone name length
        if zone.len() > 32 {
            return Err(ErrorCode::ZoneNameTooLong.into());
        }

        // initialize shard
        let burn_shard = &mut ctx.accounts.latest_burn_shard;
        burn_shard.authority = ctx.accounts.payer.key();
        burn_shard.zone = zone.clone();
        burn_shard.current_index = 0;
        burn_shard.records = Vec::new();

        // update latest burn index
        let burn_index = &mut ctx.accounts.latest_burn_index;
        burn_index.shard_count += 1;
        burn_index.shards.push(ShardInfo {
            zone: zone.clone(),
            pubkey: ctx.accounts.latest_burn_shard.key(),
            record_count: 0,
        });

        msg!("Created new latest burn shard: {}", zone);
        Ok(())
    }

    // modify process_burn function
    pub fn process_burn(ctx: Context<ProcessBurn>, amount: u64, zone: String) -> Result<()> {
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
        
        // update storage
        if let Some(latest_burn_shard) = &mut ctx.accounts.latest_burn_shard {
            // validate zone
            if latest_burn_shard.zone != zone {
                return Err(ErrorCode::InvalidShardZone.into());
            }

            let record = BurnRecord {
                pubkey: ctx.accounts.user.key(),
                signature,
                slot: clock.slot,
                blocktime: clock.unix_timestamp,
            };
            
            latest_burn_shard.add_record(record);
            
            // update record count in latest burn index
            if let Some(latest_burn_index) = &mut ctx.accounts.latest_burn_index {
                if let Some(shard_info) = latest_burn_index.shards.iter_mut()
                    .find(|s| s.pubkey == latest_burn_shard.key()) {
                    shard_info.record_count = latest_burn_shard.records.len() as u16;
                }
            }
            
            msg!("Added new burn record to shard: {}", zone);
        }

        Ok(())
    }

    // Close latest burn index account
    pub fn close_latest_burn_index(ctx: Context<CloseLatestBurnIndex>) -> Result<()> {
        // Authority check is handled in the account validation
        msg!("Closing latest burn index account");
        Ok(())
    }

    // Close latest burn shard account
    pub fn close_latest_burn_shard(ctx: Context<CloseLatestBurnShard>, zone: String) -> Result<()> {
        // Authority check is handled in the account validation
        // Remove shard info from index
        let burn_index = &mut ctx.accounts.latest_burn_index;
        if let Some(pos) = burn_index.shards.iter().position(|x| x.zone == zone) {
            burn_index.shards.remove(pos);
            burn_index.shard_count -= 1;
        }
        
        msg!("Closing latest burn shard account: {}", zone);
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

// initialize storage account
#[derive(Accounts)]
pub struct InitializeLatestBurnIndex<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + // discriminator
               1 + // shard_count
               4 + // vec len
               (128 * (36 + 32 + 2)), // 128个分片的空间
        seeds = [b"latest_burn_index"],
        bump
    )]
    pub latest_burn_index: Account<'info, LatestBurnIndex>,
    
    pub system_program: Program<'info, System>,
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
#[instruction(zone: String)]
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
    
    /// Latest burn index (optional)
    #[account(
        mut,
        seeds = [b"latest_burn_index"],
        bump
    )]
    pub latest_burn_index: Option<Account<'info, LatestBurnIndex>>,
    
    /// Latest burn shard (optional)
    #[account(
        mut,
        seeds = [b"latest_burn_shard", zone.as_bytes()],
        bump
    )]
    pub latest_burn_shard: Option<Account<'info, LatestBurnShard>>,
}

#[derive(Accounts)]
#[instruction(zone: String)]
pub struct CreateLatestBurnShard<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"latest_burn_index"],
        bump
    )]
    pub latest_burn_index: Account<'info, LatestBurnIndex>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + // discriminator
               32 + // zone
               1 + // current_index
               4 + // vec len
               (69 * (32 + 88 + 8 + 8)), // 69 records
        seeds = [b"latest_burn_shard", zone.as_bytes()],
        bump
    )]
    pub latest_burn_shard: Account<'info, LatestBurnShard>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseLatestBurnIndex<'info> {
    #[account(mut)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"latest_burn_index"],
        bump,
        constraint = latest_burn_index.authority == recipient.key(),
        close = recipient
    )]
    pub latest_burn_index: Account<'info, LatestBurnIndex>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(zone: String)]
pub struct CloseLatestBurnShard<'info> {
    #[account(mut)]
    pub recipient: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"latest_burn_index"],
        bump,
        constraint = latest_burn_index.authority == recipient.key()
    )]
    pub latest_burn_index: Account<'info, LatestBurnIndex>,
    
    #[account(
        mut,
        seeds = [b"latest_burn_shard", zone.as_bytes()],
        bump,
        constraint = latest_burn_shard.authority == recipient.key(),
        close = recipient
    )]
    pub latest_burn_shard: Account<'info, LatestBurnShard>,
    
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

    #[msg("Zone name too long. Maximum 32 bytes allowed.")]
    ZoneNameTooLong,
    
    #[msg("Invalid shard zone.")]
    InvalidShardZone,

    #[msg("Unauthorized: Only the authority can perform this action")]
    UnauthorizedAuthority,
}