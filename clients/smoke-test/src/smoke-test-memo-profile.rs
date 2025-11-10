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
pub struct ProfileCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub user_pubkey: String,
    pub username: String,
    pub image: String,
    pub about_me: Option<String>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ProfileUpdateData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub user_pubkey: String,
    pub username: Option<String>,
    pub image: Option<String>,
    pub about_me: Option<Option<String>>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Profile {
    pub user: Pubkey,
    pub username: String,
    pub image: String,
    pub created_at: i64,
    pub last_updated: i64,
    pub about_me: Option<String>,
    pub bump: u8,
}

const BURN_MEMO_VERSION: u8 = 1;
const PROFILE_CREATION_DATA_VERSION: u8 = 1;
const PROFILE_UPDATE_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "profile";
const EXPECTED_OPERATION_CREATE: &str = "create_profile";
const EXPECTED_OPERATION_UPDATE: &str = "update_profile";
const BURN_AMOUNT_TOKENS: u64 = 420; // Minimum burn for profile operations
const DECIMAL_FACTOR: u64 = 1_000_000;
const REQUIRED_TOKENS_FOR_TEST: u64 = 1000; // Need at least 1000 tokens (create 420 + update 420 + buffer)

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
    let prefix = format!("PROFILE_{}_", timestamp);
    
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
    println!("║  MEMO-PROFILE SMOKE TEST (Create + Update + Delete + Verify) ║");
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
    let profile_program_id = get_program_id("memo_profile")
        .expect("Failed to get memo_profile program ID");
    let burn_program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    let mint = get_token_mint("memo_token")
        .expect("Failed to get memo_token mint address");
    
    println!("Profile Program: {}", profile_program_id);
    println!("Burn Program:    {}", burn_program_id);
    println!("Mint Address:    {}", mint);

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );
    
    println!("Token Account:   {}", token_account);

    // Calculate profile PDA
    let (profile_pda, profile_bump) = Pubkey::find_program_address(
        &[b"profile", payer.pubkey().as_ref()],
        &profile_program_id,
    );
    
    println!("Profile PDA:     {}", profile_pda);
    println!("Profile Bump:    {}", profile_bump);

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &burn_program_id,
    );
    
    println!("Burn Stats PDA:  {}", user_global_burn_stats_pda);
    println!();

    // Check if profile already exists (cleanup from previous run)
    println!("─────────────────────────────────────────────────────────────────");
    println!("🧹 Pre-Test Cleanup");
    println!("─────────────────────────────────────────────────────────────────");
    
    match client.get_account(&profile_pda) {
        Ok(_) => {
            println!("⚠️  Profile already exists, deleting for clean test...");
            delete_profile(&client, &payer, &profile_program_id, &profile_pda)?;
            println!("✅ Profile deleted");
        },
        Err(_) => {
            println!("✅ No existing profile found");
        }
    }
    println!();

    // Get mint program ID for minting operations
    let mint_program_id = get_program_id("memo_mint")
        .expect("Failed to get memo_mint program ID");
    
    // Calculate required balance (create + update + safety margin)
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

    // Step 1: Create Profile
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Step 1: Create Profile");
    println!("─────────────────────────────────────────────────────────────────");
    
    let username = "SmokeTestUser";
    let image = "c:32x32:test_image_data";
    let about_me = Some("Smoke test profile".to_string());
    
    println!("Username:  {}", username);
    println!("Image:     {}", image);
    println!("About Me:  {:?}", about_me);
    println!("Burning:   {} tokens ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    
    create_profile(
        &client,
        &payer,
        &profile_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &profile_pda,
        &user_global_burn_stats_pda,
        burn_amount,
        username,
        image,
        about_me.clone(),
    )?;
    
    println!("✅ Profile created successfully");
    println!();

    // Step 2: Verify Profile Creation
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 2: Verify Profile Creation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let profile = verify_profile(&client, &profile_pda, &payer.pubkey())?;
    
    println!("Username:     {}", profile.username);
    println!("Image:        {}", profile.image);
    println!("About Me:     {:?}", profile.about_me);
    println!("Created At:   {}", profile.created_at);
    println!("Last Updated: {}", profile.last_updated);
    println!("Bump:         {}", profile.bump);
    
    // Verify fields match
    assert_eq!(profile.username, username, "Username mismatch");
    assert_eq!(profile.image, image, "Image mismatch");
    assert_eq!(profile.about_me, about_me, "About me mismatch");
    assert_eq!(profile.user, payer.pubkey(), "User pubkey mismatch");
    
    println!("✅ Profile verification passed");
    println!();

    // Step 3: Update Profile
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔄 Step 3: Update Profile");
    println!("─────────────────────────────────────────────────────────────────");
    
    let new_username = "UpdatedUser";
    let new_image = "c:64x64:updated_image_data";
    let new_about_me = Some(Some("Updated smoke test profile".to_string()));
    
    println!("New Username:  {}", new_username);
    println!("New Image:     {}", new_image);
    println!("New About Me:  {:?}", new_about_me);
    println!("Burning:       {} tokens ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    
    update_profile(
        &client,
        &payer,
        &profile_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &profile_pda,
        &user_global_burn_stats_pda,
        burn_amount,
        Some(new_username.to_string()),
        Some(new_image.to_string()),
        new_about_me.clone(),
    )?;
    
    println!("✅ Profile updated successfully");
    println!();

    // Step 4: Verify Profile Update
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 4: Verify Profile Update");
    println!("─────────────────────────────────────────────────────────────────");
    
    let updated_profile = verify_profile(&client, &profile_pda, &payer.pubkey())?;
    
    println!("Username:     {}", updated_profile.username);
    println!("Image:        {}", updated_profile.image);
    println!("About Me:     {:?}", updated_profile.about_me);
    println!("Last Updated: {}", updated_profile.last_updated);
    
    // Verify fields match
    assert_eq!(updated_profile.username, new_username, "Updated username mismatch");
    assert_eq!(updated_profile.image, new_image, "Updated image mismatch");
    assert_eq!(updated_profile.about_me, new_about_me.unwrap(), "Updated about me mismatch");
    assert!(updated_profile.last_updated > profile.last_updated, "Last updated timestamp not increased");
    
    println!("✅ Profile update verification passed");
    println!();

    // Step 5: Delete Profile
    println!("─────────────────────────────────────────────────────────────────");
    println!("🗑️  Step 5: Delete Profile");
    println!("─────────────────────────────────────────────────────────────────");
    
    delete_profile(&client, &payer, &profile_program_id, &profile_pda)?;
    
    println!("✅ Profile deleted successfully");
    println!();

    // Step 6: Verify Profile Deletion
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 6: Verify Profile Deletion");
    println!("─────────────────────────────────────────────────────────────────");
    
    match client.get_account(&profile_pda) {
        Ok(_) => {
            return Err("Profile still exists after deletion!".into());
        },
        Err(_) => {
            println!("✅ Profile successfully deleted (account not found)");
        }
    }
    println!();

    // Final Summary
    println!("═════════════════════════════════════════════════════════════════");
    println!("✅ ALL SMOKE TESTS PASSED");
    println!("═════════════════════════════════════════════════════════════════");
    println!("✓ Profile creation");
    println!("✓ Profile creation verification");
    println!("✓ Profile update");
    println!("✓ Profile update verification");
    println!("✓ Profile deletion");
    println!("✓ Profile deletion verification");
    println!();
    println!("Total profile operations: 6 (create + verify + update + verify + delete + verify)");
    println!("Total tokens burned: {} tokens", BURN_AMOUNT_TOKENS * 2);
    println!();

    Ok(())
}

fn create_profile(
    client: &RpcClient,
    payer: &dyn Signer,
    profile_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    profile_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    burn_amount: u64,
    username: &str,
    image: &str,
    about_me: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create ProfileCreationData
    let profile_data = ProfileCreationData {
        version: PROFILE_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION_CREATE.to_string(),
        user_pubkey: payer.pubkey().to_string(),
        username: username.to_string(),
        image: image.to_string(),
        about_me,
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&profile_data)?;
    
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
    
    // Create profile instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_profile");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let create_profile_instruction = Instruction::new_with_bytes(
        *profile_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*profile_pda, false),
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
            create_profile_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction: {}", signature);
    
    Ok(())
}

fn update_profile(
    client: &RpcClient,
    payer: &dyn Signer,
    profile_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    profile_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    burn_amount: u64,
    username: Option<String>,
    image: Option<String>,
    about_me: Option<Option<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create ProfileUpdateData
    let profile_data = ProfileUpdateData {
        version: PROFILE_UPDATE_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION_UPDATE.to_string(),
        user_pubkey: payer.pubkey().to_string(),
        username,
        image,
        about_me,
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&profile_data)?;
    
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
    
    // Create update profile instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:update_profile");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    let update_profile_instruction = Instruction::new_with_bytes(
        *profile_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*mint, false),
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*profile_pda, false),
            AccountMeta::new(*user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
            AccountMeta::new_readonly(*burn_program_id, false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            update_profile_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(300_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction: {}", signature);
    
    Ok(())
}

fn delete_profile(
    client: &RpcClient,
    payer: &dyn Signer,
    profile_program_id: &Pubkey,
    profile_pda: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create delete profile instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:delete_profile");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let delete_profile_instruction = Instruction::new_with_bytes(
        *profile_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(*profile_pda, false),
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            delete_profile_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(100_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction: {}", signature);
    
    Ok(())
}

fn verify_profile(
    client: &RpcClient,
    profile_pda: &Pubkey,
    expected_user: &Pubkey,
) -> Result<Profile, Box<dyn std::error::Error>> {
    let account = client.get_account(profile_pda)?;
    
    // Skip 8-byte discriminator
    if account.data.len() <= 8 {
        return Err("Account data too small".into());
    }
    
    let profile_data = &account.data[8..];
    
    // Borsh deserialize expects &mut &[u8], which allows reading from a slice
    // This will only consume the bytes needed and ignore any trailing padding
    let mut data_slice = profile_data;
    let profile = Profile::deserialize(&mut data_slice)?;
    
    // Verify user matches
    if profile.user != *expected_user {
        return Err(format!("User mismatch: expected {}, got {}", expected_user, profile.user).into());
    }
    
    Ok(profile)
}

