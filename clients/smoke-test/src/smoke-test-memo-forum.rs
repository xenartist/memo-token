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
pub struct PostCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub creator: String,
    pub post_id: u64,
    pub title: String,
    pub content: String,
    pub image: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct PostBurnData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub user: String,
    pub post_id: u64,
    pub message: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct PostMintData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub user: String,
    pub post_id: u64,
    pub message: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Post {
    pub post_id: u64,
    pub creator: Pubkey,
    pub created_at: i64,
    pub last_updated: i64,
    pub title: String,
    pub content: String,
    pub image: String,
    pub reply_count: u64,
    pub burned_amount: u64,
    pub last_reply_time: i64,
    pub bump: u8,
}

const BURN_MEMO_VERSION: u8 = 1;
const POST_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "forum";
const EXPECTED_CREATE_OPERATION: &str = "create_post";
const EXPECTED_BURN_OPERATION: &str = "burn_for_post";
const EXPECTED_MINT_OPERATION: &str = "mint_for_post";
const BURN_AMOUNT_TOKENS: u64 = 1;
const DECIMAL_FACTOR: u64 = 1_000_000;
const REQUIRED_TOKENS_FOR_TEST: u64 = 10;

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
    let prefix = format!("FORUM_{}_", timestamp);
    
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

/// Get the next post ID from global counter
fn get_next_post_id(client: &RpcClient, forum_program_id: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        forum_program_id,
    );

    match client.get_account(&global_counter_pda) {
        Ok(account) => {
            if account.data.len() >= 16 { // 8 bytes discriminator + 8 bytes u64
                let total_posts_bytes = &account.data[8..16];
                let total_posts = u64::from_le_bytes(total_posts_bytes.try_into().unwrap());
                Ok(total_posts) // Next post ID is current total_posts (0-indexed)
            } else {
                Err("Invalid global counter data (too short)".into())
            }
        },
        Err(e) => {
            Err(format!("Global counter not found: {}. The admin needs to initialize it first. Run: cargo run --bin admin-init-global-post-counter", e).into())
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     MEMO-FORUM SMOKE TEST (Full Lifecycle Test)              ║");
    println!("║     (create_post, burn_for_post, mint_for_post)              ║");
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
    let forum_program_id = get_program_id("memo_forum")
        .expect("Failed to get memo_forum program ID");
    let burn_program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    let mint_program_id = get_program_id("memo_mint")
        .expect("Failed to get memo_mint program ID");
    let mint = get_token_mint("memo_token")
        .expect("Failed to get memo_token mint address");
    
    println!("Forum Program:  {}", forum_program_id);
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

    // Calculate global counter PDA
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &forum_program_id,
    );
    
    println!("Global Counter: {}", global_counter_pda);

    // Get next available post_id from global counter
    let post_id = get_next_post_id(&client, &forum_program_id)?;
    
    println!("Next Post ID:   {} (auto-assigned from global counter)", post_id);
    
    // Calculate post PDA
    let (post_pda, post_bump) = Pubkey::find_program_address(
        &[b"post", post_id.to_le_bytes().as_ref()],
        &forum_program_id,
    );
    
    println!("Post PDA:       {}", post_pda);
    println!("Post Bump:      {}", post_bump);

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

    // Calculate required balance (2 burns + buffer)
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
    // Step 1: Create Post
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Step 1: Create Post");
    println!("─────────────────────────────────────────────────────────────────");
    
    let post_title = "Smoke Test Forum Post";
    let post_content = "This is a comprehensive test post for forum smoke testing. Anyone can reply to this post by burning or minting tokens.";
    let post_image = "https://example.com/forum-post.png";
    
    println!("Creator:        {}", payer.pubkey());
    println!("Post ID:        {}", post_id);
    println!("Title:          {}", post_title);
    println!("Content:        {} chars", post_content.len());
    println!("Burning:        {} token(s) ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    
    create_post(
        &client,
        &payer,
        &forum_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &global_counter_pda,
        &post_pda,
        &user_global_burn_stats_pda,
        post_id,
        burn_amount,
        post_title,
        post_content,
        post_image,
    )?;
    
    println!("✅ Post created successfully");
    println!();

    // =========================================================================
    // Step 2: Verify Post Creation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 2: Verify Post Creation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let post = verify_post(&client, &post_pda, &payer.pubkey(), post_id)?;
    
    println!("Post ID:        {}", post.post_id);
    println!("Creator:        {}", post.creator);
    println!("Title:          {}", post.title);
    println!("Content:        {} chars", post.content.len());
    println!("Image:          {}", post.image);
    println!("Created At:     {}", post.created_at);
    println!("Reply Count:    {}", post.reply_count);
    println!("Burned Amount:  {} units ({} tokens)", post.burned_amount, format_token_amount(post.burned_amount));
    println!("Bump:           {}", post.bump);
    
    // Verify fields
    assert_eq!(post.post_id, post_id, "Post ID mismatch");
    assert_eq!(post.creator, payer.pubkey(), "Creator pubkey mismatch");
    
    let initial_burned_amount = post.burned_amount;
    let initial_reply_count = post.reply_count;
    
    println!("✅ Post creation verification passed");
    println!();

    // =========================================================================
    // Step 3: Burn for Post (reply with burn)
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔥 Step 3: Burn for Post (Reply with Burn)");
    println!("─────────────────────────────────────────────────────────────────");
    
    let burn_message = "This is a burn reply - supporting this post!";
    
    println!("Burning {} token(s) to reply to post", BURN_AMOUNT_TOKENS);
    println!("Message: {}", burn_message);
    println!("Note: Anyone can burn for any post (not just creator)");
    
    burn_for_post(
        &client,
        &payer,
        &forum_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &post_pda,
        &user_global_burn_stats_pda,
        post_id,
        burn_amount,
        burn_message,
    )?;
    
    println!("✅ Burn for post successful");
    println!();

    // =========================================================================
    // Step 4: Verify Burn Operation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 4: Verify Burn Operation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let post = verify_post(&client, &post_pda, &payer.pubkey(), post_id)?;
    
    println!("Total Burned:   {} units ({} tokens)", post.burned_amount, format_token_amount(post.burned_amount));
    println!("Reply Count:    {}", post.reply_count);
    println!("Last Reply:     {}", post.last_reply_time);
    
    // Verify burn tracking
    assert_eq!(post.burned_amount, initial_burned_amount + burn_amount, "Total burned should increase by 1 token");
    assert_eq!(post.reply_count, initial_reply_count + 1, "Reply count should increase by 1");
    assert!(post.last_reply_time > 0, "Last reply time should be set");
    
    let reply_count_after_burn = post.reply_count;
    
    println!("✅ Burn operation verification passed");
    println!();

    // =========================================================================
    // Step 5: Mint for Post (reply with mint)
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🪙 Step 5: Mint for Post (Reply with Mint)");
    println!("─────────────────────────────────────────────────────────────────");
    
    let mint_message = "This is a mint reply - rewarding the post creator!";
    
    println!("Minting tokens to reply to post");
    println!("Message: {}", mint_message);
    println!("Note: Anyone can mint for any post (not just creator)");
    println!("Note: Actual mint amount depends on supply tier");
    
    let balance_before = get_token_balance_raw(&client, &token_account);
    
    mint_for_post(
        &client,
        &payer,
        &forum_program_id,
        &mint_program_id,
        &mint,
        &token_account,
        &post_pda,
        &mint_authority_pda,
        post_id,
        mint_message,
    )?;
    
    let balance_after = get_token_balance_raw(&client, &token_account);
    let minted = if balance_after > balance_before {
        balance_after - balance_before
    } else {
        0
    };
    
    println!("✅ Mint for post successful");
    println!("   Tokens minted: {} units ({} tokens)", minted, format_token_amount(minted));
    println!();

    // =========================================================================
    // Step 6: Verify Mint Operation
    // =========================================================================
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 6: Verify Mint Operation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let post = verify_post(&client, &post_pda, &payer.pubkey(), post_id)?;
    
    println!("Reply Count:    {}", post.reply_count);
    println!("Last Reply:     {}", post.last_reply_time);
    
    // Verify mint tracking
    assert!(post.reply_count > reply_count_after_burn, "Reply count should increase after mint");
    assert!(post.last_reply_time > 0, "Last reply time should be updated");
    
    println!("✅ Mint operation verification passed");
    println!();

    // =========================================================================
    // Final Summary
    // =========================================================================
    println!("═════════════════════════════════════════════════════════════════");
    println!("✅ ALL SMOKE TESTS PASSED");
    println!("═════════════════════════════════════════════════════════════════");
    println!("✓ Post creation");
    println!("✓ Post creation verification");
    println!("✓ Burn for post (reply)");
    println!("✓ Burn operation verification");
    println!("✓ Mint for post (reply)");
    println!("✓ Mint operation verification");
    println!();
    println!("Final post state:");
    println!("  - Post ID:         {}", post.post_id);
    println!("  - Creator:         {}", post.creator);
    println!("  - Title:           {}", post.title);
    println!("  - Total Burned:    {} tokens", format_token_amount(post.burned_amount));
    println!("  - Reply Count:     {}", post.reply_count);
    println!();
    println!("Key differences from memo-blog:");
    println!("  - Users can create multiple posts (post_id auto-assigned from global counter)");
    println!("  - Anyone can reply via burn_for_post or mint_for_post");
    println!("  - No update_post functionality");
    println!("  - Has admin-managed global counter for post ID assignment");
    println!("  - No leaderboard");
    println!();

    Ok(())
}

// ============================================================================
// Helper Functions for Post Operations
// ============================================================================

fn create_post(
    client: &RpcClient,
    payer: &dyn Signer,
    forum_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    global_counter_pda: &Pubkey,
    post_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    expected_post_id: u64,
    burn_amount: u64,
    title: &str,
    content: &str,
    image: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create PostCreationData
    let post_data = PostCreationData {
        version: POST_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_CREATE_OPERATION.to_string(),
        creator: payer.pubkey().to_string(),
        post_id: expected_post_id,
        title: title.to_string(),
        content: content.to_string(),
        image: image.to_string(),
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&post_data)?;
    
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
    
    // Create post instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_post");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&expected_post_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let create_post_instruction = Instruction::new_with_bytes(
        *forum_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*global_counter_pda, false),   // global_counter PDA
            AccountMeta::new(*post_pda, false),
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
            create_post_instruction,
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

fn burn_for_post(
    client: &RpcClient,
    payer: &dyn Signer,
    forum_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    post_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    post_id: u64,
    amount: u64,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create PostBurnData
    let burn_data = PostBurnData {
        version: POST_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_BURN_OPERATION.to_string(),
        user: payer.pubkey().to_string(),
        post_id,
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
    
    // Create burn_for_post instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_for_post");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&post_id.to_le_bytes());
    instruction_data.extend_from_slice(&amount.to_le_bytes());
    
    let burn_for_post_instruction = Instruction::new_with_bytes(
        *forum_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*post_pda, false),
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
            burn_for_post_instruction,
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

fn mint_for_post(
    client: &RpcClient,
    payer: &dyn Signer,
    forum_program_id: &Pubkey,
    mint_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    post_pda: &Pubkey,
    mint_authority_pda: &Pubkey,
    post_id: u64,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create PostMintData
    let mint_data = PostMintData {
        version: POST_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_MINT_OPERATION.to_string(),
        user: payer.pubkey().to_string(),
        post_id,
        message: message.to_string(),
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&mint_data)?;
    
    // Create BurnMemo with burn_amount = 0 for mint operations
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: 0,
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
    
    // Create mint_for_post instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:mint_for_post");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&post_id.to_le_bytes());
    
    let mint_for_post_instruction = Instruction::new_with_bytes(
        *forum_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*post_pda, false),
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
            mint_for_post_instruction,
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

fn verify_post(
    client: &RpcClient,
    post_pda: &Pubkey,
    expected_creator: &Pubkey,
    expected_post_id: u64,
) -> Result<Post, Box<dyn std::error::Error>> {
    let account = client.get_account(post_pda)?;
    
    // Skip 8-byte discriminator
    if account.data.len() <= 8 {
        return Err("Account data too small".into());
    }
    
    let post_data = &account.data[8..];
    
    // Borsh deserialize
    let mut data_slice = post_data;
    let post = Post::deserialize(&mut data_slice)?;
    
    // Verify creator matches
    if post.creator != *expected_creator {
        return Err(format!("Creator mismatch: expected {}, got {}", expected_creator, post.creator).into());
    }
    
    // Verify post_id matches
    if post.post_id != expected_post_id {
        return Err(format!("Post ID mismatch: expected {}, got {}", expected_post_id, post.post_id).into());
    }
    
    Ok(post)
}
