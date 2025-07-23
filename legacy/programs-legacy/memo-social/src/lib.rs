use anchor_lang::prelude::*;

// define program id - need to replace after deployment
declare_id!("CamUGqtEX8knHJ9a4jBeo3hBmdE2pWonbiFjBgyEG92q");

// admin pubkey - keep consistent with original contract
pub const ADMIN_PUBKEY: &str = "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn";

// user social profile
#[account]
#[derive(Default)]
pub struct SocialProfile {
    pub pubkey: Pubkey,           // 32 bytes - user pubkey
    pub username: String,         // 4 + 32 bytes - username, max 32 characters
    pub profile_image: String,    // 4 + 256 bytes - profile image, hex string
    pub about_me: Option<String>, // 1 + 4 + 128 bytes - about me, max 128 characters, optional
    pub created_at: i64,          // 8 bytes - created timestamp
    pub last_updated: i64,        // 8 bytes - last updated timestamp
}

#[program]
pub mod memo_social {
    use super::*;

    // initialize user social profile
    pub fn initialize_social_profile(
        ctx: Context<InitializeSocialProfile>, 
        username: String, 
        profile_image: String,
        about_me: Option<String>,
    ) -> Result<()> {
        // check username length
        if username.len() > 32 {
            return Err(ErrorCode::UsernameTooLong.into());
        }
        
        // check profile image length
        if profile_image.len() > 256 {
            return Err(ErrorCode::ProfileImageTooLong.into());
        }
        
        // check about me length (if provided)
        if let Some(about_text) = &about_me {
            if about_text.len() > 128 {
                return Err(ErrorCode::AboutMeTooLong.into());
            }
        }
        
        let social_profile = &mut ctx.accounts.social_profile;
        let clock = Clock::get()?;
        
        social_profile.pubkey = ctx.accounts.user.key();
        social_profile.username = username;
        social_profile.profile_image = profile_image;
        social_profile.about_me = about_me;
        social_profile.created_at = clock.unix_timestamp;
        social_profile.last_updated = clock.unix_timestamp;
        
        msg!("User social profile initialized for: {}", social_profile.username);
        Ok(())
    }
    
    // update user social profile
    pub fn update_social_profile(
        ctx: Context<UpdateSocialProfile>, 
        username: Option<String>, 
        profile_image: Option<String>,
        about_me: Option<String>,
    ) -> Result<()> {
        let social_profile = &mut ctx.accounts.social_profile;
        let clock = Clock::get()?;
        
        // update username (if provided)
        if let Some(new_username) = username {
            if new_username.len() > 32 {
                return Err(ErrorCode::UsernameTooLong.into());
            }
            social_profile.username = new_username;
        }
        
        // update profile image (if provided)
        if let Some(new_profile_image) = profile_image {
            if new_profile_image.len() > 256 {
                return Err(ErrorCode::ProfileImageTooLong.into());
            }
            social_profile.profile_image = new_profile_image;
        }
        
        // update about me (if provided)
        if let Some(new_about_me) = about_me {
            if new_about_me.len() > 128 {
                return Err(ErrorCode::AboutMeTooLong.into());
            }
            social_profile.about_me = Some(new_about_me);
        }
        
        // update last updated timestamp
        social_profile.last_updated = clock.unix_timestamp;
        
        msg!("User social profile updated for: {}", social_profile.username);
        Ok(())
    }
    
    // close user social profile
    pub fn close_social_profile(ctx: Context<CloseSocialProfile>) -> Result<()> {
        // ensure only user can close their own profile
        if ctx.accounts.user.key() != ctx.accounts.social_profile.pubkey {
            return Err(ErrorCode::UnauthorizedUser.into());
        }
        
        msg!("Closing social profile for: {}", ctx.accounts.social_profile.username);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeSocialProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        init,
        payer = user,
        space = 8 +    // discriminator
               32 +    // pubkey
               4 + 32 + // username (String)
               4 + 256 + // profile_image (Hex String)
               1 + 4 + 128 + // about_me (Option<String>)
               8 +     // created_at
               8,      // last_updated
        seeds = [b"social_profile", user.key().as_ref()],
        bump
    )]
    pub social_profile: Account<'info, SocialProfile>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateSocialProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"social_profile", user.key().as_ref()],
        bump,
        constraint = social_profile.pubkey == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub social_profile: Account<'info, SocialProfile>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseSocialProfile<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"social_profile", user.key().as_ref()],
        bump,
        close = user // close account and return SOL to user
    )]
    pub social_profile: Account<'info, SocialProfile>,
    
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Username too long. Maximum length is 32 characters.")]
    UsernameTooLong,
    
    #[msg("Profile image too long. Maximum length is 256 characters.")]
    ProfileImageTooLong,
    
    #[msg("About me too long. Maximum length is 128 characters.")]
    AboutMeTooLong,
    
    #[msg("Unauthorized: Only the user can update their own profile")]
    UnauthorizedUser,
}
