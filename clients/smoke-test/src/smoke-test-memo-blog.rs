use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
};
use solana_system_interface::program as system_program;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use borsh::{BorshSerialize, BorshDeserialize};
use base64::{Engine as _, engine::general_purpose};
use sha2::{Sha256, Digest};
use rand::{thread_rng, Rng};
use chrono::Utc;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;
use memo_token_client::{get_rpc_url, get_program_id, get_token_mint};

// Borsh memo structures (must match the contract)
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BurnMemo {
    pub version: u8,
    pub burn_amount: u64,
    pub payload: Vec<u8>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BlogCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub creator: String,
    pub name: String,
    pub description: String,
    pub image: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BlogUpdateData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub creator: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BlogBurnData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub burner: String,
    pub message: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct BlogMintData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub minter: String,
    pub message: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Blog {
    pub creator: Pubkey,
    pub created_at: i64,
    pub last_updated: i64,
    pub name: String,
    pub description: String,
    pub image: String,
    pub memo_count: u64,
    pub burned_amount: u64,
    pub minted_amount: u64,
    pub last_memo_time: i64,
    pub bump: u8,
}

const BURN_MEMO_VERSION: u8 = 1;
const BLOG_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "blog";
const EXPECTED_CREATE_OPERATION: &str = "create_blog";
const EXPECTED_UPDATE_OPERATION: &str = "update_blog";
const EXPECTED_BURN_OPERATION: &str = "burn_for_blog";
const EXPECTED_MINT_OPERATION: &str = "mint_for_blog";
const BURN_AMOUNT_TOKENS: u64 = 1; // Only 1 token for blog operations!
const DECIMAL_FACTOR: u64 = 1_000_000;
const REQUIRED_TOKENS_FOR_TEST: u64 = 10; // Need at least 10 tokens for full test

/// Get token balance in raw units
fn get_token_balance_raw(client: &RpcClient, token_account: &Pubkey) -> u64 {
    match client.get_account(token_account) {
        Ok(account) => {
            if account.data.len() >= 72 {
                let amount_bytes = &account.data[64..72];
                u64::from_le_bytes(amount_bytes.try_into().unwrap_or([0; 8]))
            } else {
                0
            }
        },
        Err(_) => 0,
    }
}

/// Format token amount for display
fn format_token_amount(raw_amount: u64) -> String {
    let tokens = raw_amount as f64 / DECIMAL_FACTOR as f64;
    format!("{:.6}", tokens)
}

/// Generate exactly 69-byte ASCII memo for mint operations
fn create_69_byte_ascii_memo() -> Vec<u8> {
    let mut rng = thread_rng();
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let prefix = format!("BLOG_{}_", timestamp);
    
    let target_length = 69;
    if prefix.len() >= target_length {
        let mut memo = prefix.chars().take(target_length - 4).collect::<String>();
        memo.push_str("_END");
        return memo.as_bytes().to_vec();
    }
    
    let remaining = target_length - prefix.len();
    let ascii_chars: Vec<char> = (65u8..91).chain(97..123)
        .map(|b| b as char)
        .collect();
    let random_part: String = (0..remaining)
        .map(|_| ascii_chars[rng.gen_range(0..ascii_chars.len())])
        .collect();
    
    let memo_content = format!("{}{}", prefix, random_part);
    memo_content.as_bytes().to_vec()
}

/// Execute a mint operation to get more tokens
fn execute_mint_operation(
    client: &RpcClient,
    payer: &dyn Signer,
    mint_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
) -> Result<u64, Box<dyn std::error::Error>> {
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        mint_program_id,
    );
    
    // Create memo instruction
    let memo_bytes = create_69_byte_ascii_memo();
    let memo_ix = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: memo_bytes,
    };
    
    // Create mint instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_mint");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let mint_ix = Instruction::new_with_bytes(
        *mint_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*mint, false),
            AccountMeta::new_readonly(mint_authority_pda, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );
    
    // Execute transaction
    let recent_blockhash = client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_ix,
            mint_ix,
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    client.send_and_confirm_transaction(&transaction)?;
    
    // Return the new balance
    Ok(get_token_balance_raw(client, token_account))
}

/// Ensure user has enough tokens for the test
fn ensure_sufficient_balance(
    client: &RpcClient,
    payer: &dyn Signer,
    mint_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    required_units: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let initial_balance = get_token_balance_raw(client, token_account);
    
    println!("─────────────────────────────────────────────────────────────────");
    println!("💰 Token Balance Check");
    println!("─────────────────────────────────────────────────────────────────");
    println!("Current Balance:  {} units ({} tokens)", initial_balance, format_token_amount(initial_balance));
    println!("Required Balance: {} units ({} tokens)", required_units, format_token_amount(required_units));
    
    if initial_balance >= required_units {
        println!("✅ Sufficient balance available");
        println!();
        return Ok(());
    }
    
    println!("⚠️  Insufficient balance - minting more tokens...");
    println!();
    
    let mut current_balance = initial_balance;
    let mut mint_count = 0;
    
    while current_balance < required_units {
        mint_count += 1;
        println!("🔨 Mint operation #{}", mint_count);
        
        match execute_mint_operation(client, payer, mint_program_id, mint, token_account) {
            Ok(new_balance) => {
                let minted = if new_balance > current_balance {
                    new_balance - current_balance
                } else {
                    0
                };
                
                println!("   ✅ Minted: {} units ({} tokens)", minted, format_token_amount(minted));
                println!("   New Balance: {} units ({} tokens)", new_balance, format_token_amount(new_balance));
                
                if minted == 0 {
                    return Err("Mint operation succeeded but no tokens were minted (supply limit reached?)".into());
                }
                
                current_balance = new_balance;
                
                if current_balance >= required_units {
                    println!();
                    println!("✅ Sufficient balance achieved after {} mint operations", mint_count);
                    println!("   Final Balance: {} units ({} tokens)", current_balance, format_token_amount(current_balance));
                    println!();
                    break;
                }
            },
            Err(e) => {
                return Err(format!("Mint operation #{} failed: {}", mint_count, e).into());
            }
        }
    }
    
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     MEMO-BLOG SMOKE TEST (Full Lifecycle Test)               ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    
    let burn_amount = BURN_AMOUNT_TOKENS * DECIMAL_FACTOR;
    
    // Connect to network
    let rpc_url = get_rpc_url();
    println!("─────────────────────────────────────────────────────────────────");
    println!("📋 Configuration");
    println!("─────────────────────────────────────────────────────────────────");
    println!("RPC URL:        {}", rpc_url);
    
    let client = RpcClient::new(rpc_url);

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    
    println!("Payer:          {}", payer.pubkey());

    // Program and token addresses
    let blog_program_id = get_program_id("memo_blog")
        .expect("Failed to get memo_blog program ID");
    let burn_program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    let mint_program_id = get_program_id("memo_mint")
        .expect("Failed to get memo_mint program ID");
    let mint = get_token_mint("memo_token")
        .expect("Failed to get memo_token mint address");
    
    println!("Blog Program:   {}", blog_program_id);
    println!("Burn Program:   {}", burn_program_id);
    println!("Mint Program:   {}", mint_program_id);
    println!("Mint Address:   {}", mint);

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );
    
    println!("Token Account:  {}", token_account);

    // Calculate blog PDA (using user's pubkey as seed - each user can only have one blog)
    let (blog_pda, blog_bump) = Pubkey::find_program_address(
        &[b"blog", payer.pubkey().as_ref()],
        &blog_program_id,
    );
    
    println!("Blog PDA:       {}", blog_pda);
    println!("Blog Bump:      {}", blog_bump);
    
    // Check if blog already exists
    let blog_exists = client.get_account(&blog_pda).is_ok();

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &burn_program_id,
    );
    
    println!("Burn Stats PDA: {}", user_global_burn_stats_pda);

    // Calculate mint authority PDA
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &mint_program_id,
    );
    
    println!("Mint Auth PDA:  {}", mint_authority_pda);
    println!();

    // Calculate required balance (3 burns + buffer)
    let required_balance = REQUIRED_TOKENS_FOR_TEST * DECIMAL_FACTOR;
    
    // Ensure sufficient balance before starting tests
    ensure_sufficient_balance(
        &client,
        &payer,
        &mint_program_id,
        &mint,
        &token_account,
        required_balance,
    )?;

    // =========================================================================
    // Step 1: Create Blog (skip if already exists)
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Step 1: Create Blog");
    println!("─────────────────────────────────────────────────────────────────");
    
    let blog_name = "Smoke Test Blog";
    let blog_description = "A comprehensive test blog for smoke testing";
    let blog_image = "https://example.com/blog-cover.png";
    
    println!("Creator:        {}", payer.pubkey());
    println!("Name:           {}", blog_name);
    println!("Description:    {}", blog_description);
    println!("Image:          {}", blog_image);
    println!("Burning:        {} token(s) ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    
    if blog_exists {
        println!("⚠️  Blog already exists for this user, skipping creation");
    } else {
        create_blog(
            &client,
            &payer,
            &blog_program_id,
            &burn_program_id,
            &mint,
            &token_account,
            &blog_pda,
            &user_global_burn_stats_pda,
            burn_amount,
            blog_name,
            blog_description,
            blog_image,
        )?;
        
        println!("✅ Blog created successfully");
    }
    println!();

    // =========================================================================
    // Step 2: Verify Blog Creation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 2: Verify Blog Creation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let blog = verify_blog(&client, &blog_pda, &payer.pubkey())?;
    
    println!("Creator:        {}", blog.creator);
    println!("Name:           {}", blog.name);
    println!("Description:    {}", blog.description);
    println!("Image:          {}", blog.image);
    println!("Created At:     {}", blog.created_at);
    println!("Last Updated:   {}", blog.last_updated);
    println!("Memo Count:     {}", blog.memo_count);
    println!("Burned Amount:  {} units ({} tokens)", blog.burned_amount, format_token_amount(blog.burned_amount));
    println!("Minted Amount:  {}", blog.minted_amount);
    println!("Last Memo Time: {}", blog.last_memo_time);
    println!("Bump:           {}", blog.bump);
    
    // Verify creator matches
    assert_eq!(blog.creator, payer.pubkey(), "Creator pubkey mismatch");
    
    // Store initial burned amount for later comparisons
    let initial_burned_amount = blog.burned_amount;
    
    println!("✅ Blog verification passed");
    println!();

    // =========================================================================
    // Step 3: Update Blog
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Step 3: Update Blog");
    println!("─────────────────────────────────────────────────────────────────");
    
    let updated_name = "Updated Smoke Test Blog";
    let updated_description = "This blog has been updated";
    
    println!("Updating blog with:");
    println!("  New Name:        {}", updated_name);
    println!("  New Description: {}", updated_description);
    println!("  Burning:         {} token(s)", BURN_AMOUNT_TOKENS);
    
    update_blog(
        &client,
        &payer,
        &blog_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &blog_pda,
        &user_global_burn_stats_pda,
        burn_amount,
        Some(updated_name.to_string()),
        Some(updated_description.to_string()),
        None,
    )?;
    
    println!("✅ Blog updated successfully");
    println!();

    // =========================================================================
    // Step 4: Verify Blog Update
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 4: Verify Blog Update");
    println!("─────────────────────────────────────────────────────────────────");
    
    let blog = verify_blog(&client, &blog_pda, &payer.pubkey())?;
    
    println!("Updated Name:        {}", blog.name);
    println!("Updated Description: {}", blog.description);
    println!("Total Burned:        {} units ({} tokens)", blog.burned_amount, format_token_amount(blog.burned_amount));
    println!("Memo Count:          {}", blog.memo_count);
    
    // Verify updated fields
    assert_eq!(blog.name, updated_name, "Name should be updated");
    assert_eq!(blog.description, updated_description, "Description should be updated");
    assert_eq!(blog.burned_amount, initial_burned_amount + burn_amount, "Total burned should increase by 1 token");
    
    let burned_after_update = blog.burned_amount;
    
    println!("✅ Blog update verification passed");
    println!();

    // =========================================================================
    // Step 5: Burn for Blog
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔥 Step 5: Burn for Blog");
    println!("─────────────────────────────────────────────────────────────────");
    
    let burn_message = "Supporting this awesome blog!";
    
    println!("Burning {} token(s) for blog", BURN_AMOUNT_TOKENS);
    println!("Message: {}", burn_message);
    
    burn_for_blog(
        &client,
        &payer,
        &blog_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &blog_pda,
        &user_global_burn_stats_pda,
        burn_amount,
        burn_message,
    )?;
    
    println!("✅ Burn for blog successful");
    println!();

    // =========================================================================
    // Step 6: Verify Burn Operation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 6: Verify Burn Operation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let blog = verify_blog(&client, &blog_pda, &payer.pubkey())?;
    
    println!("Total Burned:   {} units ({} tokens)", blog.burned_amount, format_token_amount(blog.burned_amount));
    println!("Memo Count:     {}", blog.memo_count);
    println!("Last Memo Time: {}", blog.last_memo_time);
    
    // Verify burn tracking
    assert_eq!(blog.burned_amount, burned_after_update + burn_amount, "Total burned should increase by 1 token");
    assert!(blog.last_memo_time > 0, "Last memo time should be set");
    
    let memo_count_after_burn = blog.memo_count;
    
    println!("✅ Burn operation verification passed");
    println!();

    // =========================================================================
    // Step 7: Mint for Blog
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🪙 Step 7: Mint for Blog");
    println!("─────────────────────────────────────────────────────────────────");
    
    let mint_message = "Rewarding blog creator!";
    
    println!("Minting tokens for blog");
    println!("Message: {}", mint_message);
    println!("Note: Actual mint amount depends on supply tier");
    
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    mint_for_blog(
        &client,
        &payer,
        &blog_program_id,
        &mint_program_id,
        &mint,
        &token_account,
        &blog_pda,
        &mint_authority_pda,
        mint_message,
    )?;
    
    let balance_after = get_token_balance_raw(&client, &token_account);
    let minted = if balance_after > balance_before {
        balance_after - balance_before
    } else {
        0
    };
    
    println!("✅ Mint for blog successful");
    println!("   Tokens minted: {} units ({} tokens)", minted, format_token_amount(minted));
    println!();

    // =========================================================================
    // Step 8: Verify Mint Operation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 8: Verify Mint Operation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let blog = verify_blog(&client, &blog_pda, &payer.pubkey())?;
    
    println!("Minted Count:   {}", blog.minted_amount);
    println!("Memo Count:     {}", blog.memo_count);
    println!("Last Memo Time: {}", blog.last_memo_time);
    
    // Verify mint tracking
    assert!(blog.minted_amount >= 1, "Minted amount should be at least 1");
    assert!(blog.memo_count > memo_count_after_burn, "Memo count should increase after mint");
    assert!(blog.last_memo_time > 0, "Last memo time should be updated");
    
    println!("✅ Mint operation verification passed");
    println!();

    // =========================================================================
    // Final Summary
    // =========================================================================
    println!("═════════════════════════════════════════════════════════════════");
    println!("✅ ALL SMOKE TESTS PASSED");
    println!("═════════════════════════════════════════════════════════════════");
    println!("✓ Blog creation/verification");
    println!("✓ Blog update");
    println!("✓ Blog update verification");
    println!("✓ Burn for blog");
    println!("✓ Burn operation verification");
    println!("✓ Mint for blog");
    println!("✓ Mint operation verification");
    println!();
    println!("Final blog state:");
    println!("  - Creator:         {}", blog.creator);
    println!("  - Name:            {}", blog.name);
    println!("  - Total Burned:    {} tokens", format_token_amount(blog.burned_amount));
    println!("  - Mint Count:      {}", blog.minted_amount);
    println!("  - Memo Count:      {}", blog.memo_count);
    println!();

    Ok(())
}

// ============================================================================
// Helper Functions for Blog Operations
// ============================================================================

fn create_blog(
    client: &RpcClient,
    payer: &dyn Signer,
    blog_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    blog_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    burn_amount: u64,
    name: &str,
    description: &str,
    image: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create BlogCreationData
    let blog_data = BlogCreationData {
        version: BLOG_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_CREATE_OPERATION.to_string(),
        creator: payer.pubkey().to_string(),
        name: name.to_string(),
        description: description.to_string(),
        image: image.to_string(),
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&blog_data)?;
    
    // Create BurnMemo
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };
    
    // Serialize and encode
    let borsh_data = borsh::to_vec(&burn_memo)?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    
    // Create memo instruction
    let memo_instruction = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: base64_encoded.into_bytes(),
    };
    
    // Create blog instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_blog");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let create_blog_instruction = Instruction::new_with_bytes(
        *blog_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*blog_pda, false),
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(*burn_program_id, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            create_blog_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction:    {}", signature);
    
    Ok(())
}

fn update_blog(
    client: &RpcClient,
    payer: &dyn Signer,
    blog_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    blog_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    burn_amount: u64,
    name: Option<String>,
    description: Option<String>,
    image: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create BlogUpdateData
    let update_data = BlogUpdateData {
        version: BLOG_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_UPDATE_OPERATION.to_string(),
        creator: payer.pubkey().to_string(),
        name,
        description,
        image,
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&update_data)?;
    
    // Create BurnMemo
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };
    
    // Serialize and encode
    let borsh_data = borsh::to_vec(&burn_memo)?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    
    // Create memo instruction
    let memo_instruction = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: base64_encoded.into_bytes(),
    };
    
    // Create update blog instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:update_blog");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let update_blog_instruction = Instruction::new_with_bytes(
        *blog_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*blog_pda, false),
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(*burn_program_id, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            update_blog_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction:    {}", signature);
    
    Ok(())
}

fn burn_for_blog(
    client: &RpcClient,
    payer: &dyn Signer,
    blog_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    blog_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    amount: u64,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create BlogBurnData
    let burn_data = BlogBurnData {
        version: BLOG_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_BURN_OPERATION.to_string(),
        burner: payer.pubkey().to_string(),
        message: message.to_string(),
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&burn_data)?;
    
    // Create BurnMemo
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: amount,
        payload,
    };
    
    // Serialize and encode
    let borsh_data = borsh::to_vec(&burn_memo)?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    
    // Create memo instruction
    let memo_instruction = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: base64_encoded.into_bytes(),
    };
    
    // Create burn_for_blog instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_for_blog");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&amount.to_le_bytes());
    
    let burn_for_blog_instruction = Instruction::new_with_bytes(
        *blog_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*blog_pda, false),
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(*burn_program_id, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            burn_for_blog_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction:    {}", signature);
    
    Ok(())
}

fn mint_for_blog(
    client: &RpcClient,
    payer: &dyn Signer,
    blog_program_id: &Pubkey,
    mint_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    blog_pda: &Pubkey,
    mint_authority_pda: &Pubkey,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create BlogMintData
    let mint_data = BlogMintData {
        version: BLOG_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_MINT_OPERATION.to_string(),
        minter: payer.pubkey().to_string(),
        message: message.to_string(),
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&mint_data)?;
    
    // Create BurnMemo with burn_amount = 0 for mint operations
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: 0, // Mint operations use 0
        payload,
    };
    
    // Serialize and encode
    let borsh_data = borsh::to_vec(&burn_memo)?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    
    // Create memo instruction
    let memo_instruction = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: base64_encoded.into_bytes(),
    };
    
    // Create mint_for_blog instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:mint_for_blog");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let mint_for_blog_instruction = Instruction::new_with_bytes(
        *blog_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*blog_pda, false),
            AccountMeta::new(*mint, false),
            AccountMeta::new_readonly(*mint_authority_pda, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(*mint_program_id, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            mint_for_blog_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction:    {}", signature);
    
    Ok(())
}

fn verify_blog(
    client: &RpcClient,
    blog_pda: &Pubkey,
    expected_creator: &Pubkey,
) -> Result<Blog, Box<dyn std::error::Error>> {
    let account = client.get_account(blog_pda)?;
    
    // Skip 8-byte discriminator
    if account.data.len() <= 8 {
        return Err("Account data too small".into());
    }
    
    let blog_data = &account.data[8..];
    
    // Borsh deserialize
    let mut data_slice = blog_data;
    let blog = Blog::deserialize(&mut data_slice)?;
    
    // Verify creator matches
    if blog.creator != *expected_creator {
        return Err(format!("Creator mismatch: expected {}, got {}", expected_creator, blog.creator).into());
    }
    
    Ok(blog)
}
