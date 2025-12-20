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
pub struct ProjectCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub project_id: u64,
    pub name: String,
    pub description: String,
    pub image: String,
    pub website: String,
    pub tags: Vec<String>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Project {
    pub project_id: u64,
    pub creator: Pubkey,
    pub created_at: i64,
    pub last_updated: i64,
    pub memo_count: u64,
    pub burned_amount: u64,
    pub last_memo_time: i64,
    pub bump: u8,
    pub name: String,
    pub description: String,
    pub image: String,
    pub website: String,
    pub tags: Vec<String>,
}

const BURN_MEMO_VERSION: u8 = 1;
const PROJECT_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "project";
const EXPECTED_OPERATION: &str = "create_project";
const BURN_AMOUNT_TOKENS: u64 = 42069; // Minimum burn for project creation
const DECIMAL_FACTOR: u64 = 1_000_000;
const REQUIRED_TOKENS_FOR_TEST: u64 = 50000; // Need at least 50000 tokens for test

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
    let prefix = format!("PROJECT_{}_", timestamp);
    
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
    println!("║       MEMO-PROJECT SMOKE TEST (Create + Verify)              ║");
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
    let project_program_id = get_program_id("memo_project")
        .expect("Failed to get memo_project program ID");
    let burn_program_id = get_program_id("memo_burn")
        .expect("Failed to get memo_burn program ID");
    let mint = get_token_mint("memo_token")
        .expect("Failed to get memo_token mint address");
    
    println!("Project Program: {}", project_program_id);
    println!("Burn Program:    {}", burn_program_id);
    println!("Mint Address:    {}", mint);

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );
    
    println!("Token Account:   {}", token_account);

    // Get global counter to determine next project ID
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &project_program_id,
    );
    
    println!("Global Counter:  {}", global_counter_pda);
    
    let project_id = match client.get_account(&global_counter_pda) {
        Ok(account) => {
            if account.data.len() >= 16 {
                let total_projects = u64::from_le_bytes(account.data[8..16].try_into().unwrap_or([0; 8]));
                println!("Current Projects: {}", total_projects);
                total_projects // Next project ID will be total_projects
            } else {
                println!("⚠️  Global counter data too small, using 0");
                0
            }
        },
        Err(_) => {
            println!("⚠️  Global counter not found, using 0");
            0
        }
    };
    
    println!("Next Project ID: {}", project_id);

    // Calculate project PDA
    let (project_pda, project_bump) = Pubkey::find_program_address(
        &[b"project", project_id.to_le_bytes().as_ref()],
        &project_program_id,
    );
    
    println!("Project PDA:     {}", project_pda);
    println!("Project Bump:    {}", project_bump);

    // Calculate user global burn statistics PDA
    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &burn_program_id,
    );
    
    println!("Burn Stats PDA:  {}", user_global_burn_stats_pda);

    // Calculate burn leaderboard PDA
    let (burn_leaderboard_pda, _) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &project_program_id,
    );
    
    println!("Leaderboard PDA: {}", burn_leaderboard_pda);
    println!();

    // Get mint program ID for minting operations
    let mint_program_id = get_program_id("memo_mint")
        .expect("Failed to get memo_mint program ID");
    
    // Calculate required balance
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

    // Step 1: Create Project
    println!("─────────────────────────────────────────────────────────────────");
    println!("📝 Step 1: Create Project");
    println!("─────────────────────────────────────────────────────────────────");
    
    let project_name = "Smoke Test Project";
    let project_description = "A test project for smoke testing";
    let project_image = "https://example.com/image.png";
    let project_website = "https://example.com";
    let project_tags = vec!["test".to_string(), "smoke".to_string()];
    
    println!("Project ID:    {}", project_id);
    println!("Name:          {}", project_name);
    println!("Description:   {}", project_description);
    println!("Image:         {}", project_image);
    println!("Website:       {}", project_website);
    println!("Tags:          {:?}", project_tags);
    println!("Burning:       {} tokens ({} units)", BURN_AMOUNT_TOKENS, burn_amount);
    
    create_project(
        &client,
        &payer,
        &project_program_id,
        &burn_program_id,
        &mint,
        &token_account,
        &project_pda,
        &global_counter_pda,
        &user_global_burn_stats_pda,
        &burn_leaderboard_pda,
        burn_amount,
        project_id,
        project_name,
        project_description,
        project_image,
        project_website,
        project_tags.clone(),
    )?;
    
    println!("✅ Project created successfully");
    println!();

    // Step 2: Verify Project Creation
    println!("─────────────────────────────────────────────────────────────────");
    println!("🔍 Step 2: Verify Project Creation");
    println!("─────────────────────────────────────────────────────────────────");
    
    let project = verify_project(&client, &project_pda, project_id, &payer.pubkey())?;
    
    println!("Project ID:      {}", project.project_id);
    println!("Creator:         {}", project.creator);
    println!("Name:            {}", project.name);
    println!("Description:     {}", project.description);
    println!("Image:           {}", project.image);
    println!("Website:         {}", project.website);
    println!("Tags:            {:?}", project.tags);
    println!("Created At:      {}", project.created_at);
    println!("Last Updated:    {}", project.last_updated);
    println!("Memo Count:      {}", project.memo_count);
    println!("Burned Amount:   {} units ({} tokens)", project.burned_amount, format_token_amount(project.burned_amount));
    println!("Last Memo Time:  {}", project.last_memo_time);
    println!("Bump:            {}", project.bump);
    
    // Verify fields match
    assert_eq!(project.project_id, project_id, "Project ID mismatch");
    assert_eq!(project.creator, payer.pubkey(), "Creator pubkey mismatch");
    assert_eq!(project.name, project_name, "Name mismatch");
    assert_eq!(project.description, project_description, "Description mismatch");
    assert_eq!(project.image, project_image, "Image mismatch");
    assert_eq!(project.website, project_website, "Website mismatch");
    assert_eq!(project.tags, project_tags, "Tags mismatch");
    assert_eq!(project.burned_amount, burn_amount, "Burned amount mismatch");
    assert_eq!(project.memo_count, 0, "Initial memo count should be 0");
    assert_eq!(project.last_memo_time, 0, "Initial last memo time should be 0");
    
    println!("✅ Project verification passed");
    println!();

    // Final Summary
    println!("═════════════════════════════════════════════════════════════════");
    println!("✅ ALL SMOKE TESTS PASSED");
    println!("═════════════════════════════════════════════════════════════════");
    println!("✓ Project creation");
    println!("✓ Project creation verification");
    println!();
    println!("Total project operations: 2 (create + verify)");
    println!("Total tokens burned: {} tokens", BURN_AMOUNT_TOKENS);
    println!();

    Ok(())
}

fn create_project(
    client: &RpcClient,
    payer: &dyn Signer,
    project_program_id: &Pubkey,
    burn_program_id: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    project_pda: &Pubkey,
    global_counter_pda: &Pubkey,
    user_global_burn_stats_pda: &Pubkey,
    burn_leaderboard_pda: &Pubkey,
    burn_amount: u64,
    project_id: u64,
    name: &str,
    description: &str,
    image: &str,
    website: &str,
    tags: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create ProjectCreationData
    let project_data = ProjectCreationData {
        version: PROJECT_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        project_id,
        name: name.to_string(),
        description: description.to_string(),
        image: image.to_string(),
        website: website.to_string(),
        tags,
    };
    
    // Serialize to payload
    let payload = borsh::to_vec(&project_data)?;
    
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
    
    // Create project instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_project");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&project_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());
    
    // Account order must match CreateProject struct in lib.rs:
    // 1. creator, 2. global_counter, 3. project, 4. burn_leaderboard,
    // 5. mint, 6. creator_token_account, 7. user_global_burn_stats,
    // 8. token_program, 9. memo_burn_program, 10. system_program, 11. instructions
    let create_project_instruction = Instruction::new_with_bytes(
        *project_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),              // 1. creator
            AccountMeta::new(*global_counter_pda, false),        // 2. global_counter
            AccountMeta::new(*project_pda, false),               // 3. project
            AccountMeta::new(*burn_leaderboard_pda, false),      // 4. burn_leaderboard
            AccountMeta::new(*mint, false),                      // 5. mint
            AccountMeta::new(*token_account, false),             // 6. creator_token_account
            AccountMeta::new(*user_global_burn_stats_pda, false),// 7. user_global_burn_stats
            AccountMeta::new_readonly(token_2022_id(), false),   // 8. token_program
            AccountMeta::new_readonly(*burn_program_id, false),  // 9. memo_burn_program
            AccountMeta::new_readonly(system_program::id(), false), // 10. system_program
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false), // 11. instructions
        ],
    );
    
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create and send transaction
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_instruction,
            create_project_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    let signature = client.send_and_confirm_transaction(&transaction)?;
    println!("Transaction: {}", signature);
    
    Ok(())
}

fn verify_project(
    client: &RpcClient,
    project_pda: &Pubkey,
    expected_project_id: u64,
    expected_creator: &Pubkey,
) -> Result<Project, Box<dyn std::error::Error>> {
    let account = client.get_account(project_pda)?;
    
    // Skip 8-byte discriminator
    if account.data.len() <= 8 {
        return Err("Account data too small".into());
    }
    
    let project_data = &account.data[8..];
    
    // Borsh deserialize expects &mut &[u8], which allows reading from a slice
    // This will only consume the bytes needed and ignore any trailing padding
    let mut data_slice = project_data;
    let project = Project::deserialize(&mut data_slice)?;
    
    // Verify project ID matches
    if project.project_id != expected_project_id {
        return Err(format!("Project ID mismatch: expected {}, got {}", expected_project_id, project.project_id).into());
    }
    
    // Verify creator matches
    if project.creator != *expected_creator {
        return Err(format!("Creator mismatch: expected {}, got {}", expected_creator, project.creator).into());
    }
    
    Ok(project)
}

