#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022,
    token_interface::{Mint, TokenAccount, Token2022}
};

declare_id!("68ASgTRCbbwsfgvpkfp3LvdXbpn33QbxbV64jXVaW8Ap");

#[program]
pub mod memo_token {
    use super::*;
    
    pub fn process_transfer(ctx: Context<ProcessTransfer>) -> Result<()> {
        // Get PDA and bump
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            ctx.program_id
        );
        
        // Verify PDA
        if mint_authority != ctx.accounts.mint_authority.key() {
            return Err(ProgramError::InvalidSeeds.into());
        }

        // Mint tokens (fixed amount of 1)
        let amount = 1_000_000_000; // 1 token (9 decimals)
        let mint_authority_seeds = &[
            b"mint_authority".as_ref(),
            &[bump],
        ];
        
        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token_2022::MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                &[mint_authority_seeds]
            ),
            amount
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    
    /// CHECK: This is the token mint account
    #[account(mut)]
    pub mint: InterfaceAccount<'info, Mint>,
    
    /// CHECK: PDA as mint authority
    pub mint_authority: AccountInfo<'info>,
    
    /// CHECK: This is the token account
    #[account(mut)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token2022>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::{
        solana_program::{
            instruction::Instruction,
            pubkey::Pubkey,
        },
        InstructionData,
    };
    use anchor_spl::token_2022::ID as TOKEN_2022_ID;

    #[test]
    fn test_instruction_creation() {
        let program_id = crate::ID;
        
        // 创建所需的账户
        let user = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        
        // 计算 PDA
        let (mint_authority, _bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            &program_id,
        );

        // 创建指令数据
        let ix_data = crate::instruction::ProcessTransfer {}.data();

        // 创建账户元数据
        let accounts = vec![
            AccountMeta::new(user, true),                    // user (signer, writable)
            AccountMeta::new(mint, true),                    // mint (writable)
            AccountMeta::new_readonly(mint_authority, false), // mint_authority (readonly)
            AccountMeta::new(token_account, true),           // token_account (writable)
            AccountMeta::new_readonly(TOKEN_2022_ID, false), // token_program (readonly)
        ];

        // 构建指令
        let ix = Instruction {
            program_id,
            accounts,
            data: ix_data,
        };

        // 验证指令
        assert_eq!(ix.program_id, program_id);
        
        // 验证账户数量
        assert_eq!(ix.accounts.len(), 5);
        
        // 验证每个账户的属性
        let accounts = &ix.accounts;
        
        // 验证用户账户
        assert_eq!(accounts[0].pubkey, user);
        assert!(accounts[0].is_signer);
        assert!(accounts[0].is_writable);
        
        // 验证 mint 账户
        assert_eq!(accounts[1].pubkey, mint);
        assert!(accounts[1].is_writable);
        
        // 验证 mint authority
        assert_eq!(accounts[2].pubkey, mint_authority);
        assert!(!accounts[2].is_writable);
        
        // 验证 token 账户
        assert_eq!(accounts[3].pubkey, token_account);
        assert!(accounts[3].is_writable);
        
        // 验证 token program
        assert_eq!(accounts[4].pubkey, TOKEN_2022_ID);
        assert!(!accounts[4].is_writable);
        assert!(!accounts[4].is_signer);
    }

    #[test]
    fn test_pda_derivation() {
        let program_id = crate::ID;
        
        // 测试 PDA 导出
        let (mint_authority, bump) = Pubkey::find_program_address(
            &[b"mint_authority"],
            &program_id,
        );

        // 验证 PDA
        assert!(mint_authority != Pubkey::default());
        // 验证 bump 在合理范围内（通常会小于 255）
        assert!(bump > 0, "Bump seed should be positive");
        
        // 验证 PDA 一致性
        let (recalculated_authority, recalculated_bump) = 
            Pubkey::find_program_address(&[b"mint_authority"], &program_id);
            
        assert_eq!(mint_authority, recalculated_authority);
        assert_eq!(bump, recalculated_bump);
    }

    #[test]
    fn test_instruction_data() {
        // 测试指令数据序列化
        let ix_data = crate::instruction::ProcessTransfer {}.data();
        
        // 验证指令数据不为空
        assert!(!ix_data.is_empty());
        
        // 如果您的指令有特定的格式，可以在这里添加更多验证
    }
}