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
pub struct ChatGroupCreationData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub group_id: u64,
    pub name: String,
    pub description: String,
    pub image: String,
    pub tags: Vec<String>,
    pub min_memo_interval: Option<i64>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ChatMessageData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub group_id: u64,
    pub sender: String,
    pub message: String,
    pub receiver: Option<String>,
    pub reply_to_sig: Option<String>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ChatGroupBurnData {
    pub version: u8,
    pub category: String,
    pub operation: String,
    pub group_id: u64,
    pub burner: String,
    pub message: String,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ChatGroup {
    pub group_id: u64,
    pub creator: Pubkey,
    pub created_at: i64,
    pub name: String,
    pub description: String,
    pub image: String,
    pub tags: Vec<String>,
    pub memo_count: u64,
    pub burned_amount: u64,
    pub min_memo_interval: i64,
    pub last_memo_time: i64,
    pub bump: u8,
}

const BURN_MEMO_VERSION: u8 = 1;
const CHAT_GROUP_CREATION_DATA_VERSION: u8 = 1;
const EXPECTED_CATEGORY: &str = "chat";
const EXPECTED_CREATE_GROUP_OPERATION: &str = "create_group";
const EXPECTED_SEND_MESSAGE_OPERATION: &str = "send_message";
const EXPECTED_BURN_FOR_GROUP_OPERATION: &str = "burn_for_group";
const GROUP_CREATION_BURN_TOKENS: u64 = 42069; // Minimum burn for group creation
const BURN_FOR_GROUP_TOKENS: u64 = 1000; // Burn amount for group support
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
    let prefix = format!("CHAT_{}_", timestamp);
    
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
    
    let recent_blockhash = client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_ix,  // Index 0: Required by memo-mint contract
            mint_ix,  // Index 1: Main instruction
            ComputeBudgetInstruction::set_compute_unit_limit(300_000),
            ComputeBudgetInstruction::set_compute_unit_price(1_000),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("✅ Mint transaction successful: {}", signature);
            let balance_after = get_token_balance_raw(client, token_account);
            Ok(balance_after)
        }
        Err(e) => {
            eprintln!("❌ Mint transaction failed: {:?}", e);
            Err(Box::new(e))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🧪 Memo Chat Smoke Test");
    println!("=======================\n");

    // Get program IDs and RPC configuration
    let rpc_url = get_rpc_url();
    let chat_program_id = get_program_id("memo-chat")?;
    let burn_program_id = get_program_id("memo-burn")?;
    let mint_program_id = get_program_id("memo-mint")?;
    let mint = get_token_mint("memo_token")?;

    println!("Configuration:");
    println!("  RPC URL: {}", rpc_url);
    println!("  Chat Program: {}", chat_program_id);
    println!("  Burn Program: {}", burn_program_id);
    println!("  Mint Program: {}", mint_program_id);
    println!("  Token Mint: {}", mint);

    // Connect to Solana
    let client = RpcClient::new(rpc_url);

    // Load payer keypair
    let payer_path = std::env::var("PAYER_KEYPAIR_PATH")
        .unwrap_or_else(|_| shellexpand::tilde("~/.config/solana/id.json").to_string());
    let payer = read_keypair_file(&payer_path)?;
    println!("  Payer: {}", payer.pubkey());

    // Get payer's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),
    );
    println!("  Token Account: {}", token_account);

    // Check balance
    let balance_raw = get_token_balance_raw(&client, &token_account);
    let balance_tokens = balance_raw / DECIMAL_FACTOR;
    println!("\n💰 Current Balance: {} tokens ({} raw units)", 
             format_token_amount(balance_raw), balance_raw);

    // Mint tokens if needed
    if balance_tokens < REQUIRED_TOKENS_FOR_TEST {
        println!("\n⚠️  Insufficient balance for test. Minting tokens...");
        println!("  Required: {} tokens", REQUIRED_TOKENS_FOR_TEST);
        println!("  Current: {} tokens", balance_tokens);
        
        match execute_mint_operation(&client, &payer, &mint_program_id, &mint, &token_account) {
            Ok(new_balance) => {
                let new_balance_tokens = new_balance / DECIMAL_FACTOR;
                println!("✅ Tokens minted successfully!");
                println!("  New balance: {} tokens", new_balance_tokens);
            }
            Err(e) => {
                eprintln!("❌ Failed to mint tokens: {:?}", e);
                eprintln!("\nℹ️  Please mint tokens manually and run the test again.");
                return Err(e);
            }
        }
    }

    // Get next group ID from global counter
    let (global_counter_pda, _) = Pubkey::find_program_address(
        &[b"global_counter"],
        &chat_program_id,
    );

    let counter_account = client.get_account(&global_counter_pda)?;
    let next_group_id = if counter_account.data.len() >= 16 {
        let id_bytes = &counter_account.data[8..16];
        u64::from_le_bytes(id_bytes.try_into().unwrap_or([0; 8]))
    } else {
        0
    };

    println!("\n📋 Next Group ID: {}", next_group_id);

    // ============================================================
    // Step 1: Create Chat Group
    // ============================================================
    println!("\n🔥 Step 1: Creating Chat Group");
    println!("----------------------------------------");

    let burn_amount = GROUP_CREATION_BURN_TOKENS * DECIMAL_FACTOR;
    let group_name = format!("Test Group {}", next_group_id);
    let group_description = format!("Smoke test group created at {}", Utc::now());
    
    println!("  Group Name: {}", group_name);
    println!("  Burn Amount: {} tokens", GROUP_CREATION_BURN_TOKENS);

    // Create ChatGroupCreationData
    let group_data = ChatGroupCreationData {
        version: CHAT_GROUP_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_CREATE_GROUP_OPERATION.to_string(),
        group_id: next_group_id,
        name: group_name.clone(),
        description: group_description.clone(),
        image: "https://example.com/image.png".to_string(),
        tags: vec!["test".to_string(), "smoke".to_string()],
        min_memo_interval: Some(60),
    };

    // Serialize to Borsh
    let payload = group_data.try_to_vec()?;

    // Create BurnMemo structure
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };

    // Serialize BurnMemo to Borsh
    let burn_memo_bytes = burn_memo.try_to_vec()?;

    // Encode to Base64
    let memo_base64 = general_purpose::STANDARD.encode(&burn_memo_bytes);
    println!("  Memo length: {} bytes (Base64)", memo_base64.len());

    // Create memo instruction
    let memo_ix = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: memo_base64.as_bytes().to_vec(),
    };

    // Derive PDAs
    let (chat_group_pda, _) = Pubkey::find_program_address(
        &[b"chat_group", &next_group_id.to_le_bytes()],
        &chat_program_id,
    );

    let (burn_leaderboard_pda, _) = Pubkey::find_program_address(
        &[b"burn_leaderboard"],
        &chat_program_id,
    );

    let (user_global_burn_stats_pda, _) = Pubkey::find_program_address(
        &[b"user_global_burn_stats", payer.pubkey().as_ref()],
        &burn_program_id,
    );

    // Create create_chat_group instruction discriminator
    let mut hasher = Sha256::new();
    hasher.update(b"global:create_chat_group");
    let result = hasher.finalize();
    let mut instruction_data = result[..8].to_vec();
    instruction_data.extend_from_slice(&next_group_id.to_le_bytes());
    instruction_data.extend_from_slice(&burn_amount.to_le_bytes());

    let create_group_ix = Instruction::new_with_bytes(
        chat_program_id,
        &instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(global_counter_pda, false),
            AccountMeta::new(chat_group_pda, false),
            AccountMeta::new(burn_leaderboard_pda, false),
            AccountMeta::new(mint, false),
            AccountMeta::new(token_account, false),
            AccountMeta::new(user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(burn_program_id, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );

    // Create and send transaction
    // Note: Memo MUST be at index 0 for the contract's validation
    let recent_blockhash = client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[
            memo_ix,  // Index 0: Required by contract
            create_group_ix,  // Index 1: Main instruction
            ComputeBudgetInstruction::set_compute_unit_limit(500_000),
            ComputeBudgetInstruction::set_compute_unit_price(1_000),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    print!("  Sending transaction... ");
    match client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            println!("✅");
            println!("  Transaction: {}", signature);
        }
        Err(e) => {
            println!("❌");
            eprintln!("  Error: {:?}", e);
            return Err(Box::new(e));
        }
    }

    // ============================================================
    // Step 2: Verify Chat Group Creation
    // ============================================================
    println!("\n🔍 Step 2: Verifying Chat Group Creation");
    println!("----------------------------------------");

    let group_account = client.get_account(&chat_group_pda)?;
    println!("  Account exists: ✅");
    println!("  Account size: {} bytes", group_account.data.len());

    // Deserialize ChatGroup (skip discriminator)
    if group_account.data.len() > 8 {
        println!("  Raw data size: {} bytes (minus 8 discriminator = {} bytes)", 
                 group_account.data.len(), group_account.data.len() - 8);
        
        // Try partial deserialization to see what we can read
        let data_slice = &group_account.data[8..];
        println!("  Attempting to deserialize {} bytes...", data_slice.len());
        
        // Use deserialize instead of try_from_slice to allow extra bytes (for padding/realloc)
        let mut data_cursor = &data_slice[..];
        match ChatGroup::deserialize(&mut data_cursor) {
            Ok(chat_group) => {
                println!("\n  Group Details:");
                println!("    ID: {}", chat_group.group_id);
                println!("    Name: {}", chat_group.name);
                println!("    Description: {}", chat_group.description);
                println!("    Creator: {}", chat_group.creator);
                println!("    Burned Amount: {} tokens", chat_group.burned_amount / DECIMAL_FACTOR);
                println!("    Memo Count: {}", chat_group.memo_count);
                println!("    Tags: {:?}", chat_group.tags);
                println!("    Min Memo Interval: {} seconds", chat_group.min_memo_interval);
                println!("    Last Memo Time: {}", chat_group.last_memo_time);
                println!("    Bump: {}", chat_group.bump);

                // Verify data
                assert_eq!(chat_group.group_id, next_group_id, "Group ID mismatch");
                assert_eq!(chat_group.name, group_name, "Group name mismatch");
                assert_eq!(chat_group.creator, payer.pubkey(), "Creator mismatch");
                assert_eq!(chat_group.burned_amount, burn_amount, "Burned amount mismatch");
                println!("\n  ✅ All verifications passed!");
            }
            Err(e) => {
                eprintln!("  ❌ Failed to deserialize chat group: {:?}", e);
                eprintln!("  This might indicate a struct mismatch between client and contract");
                eprintln!("  First 100 bytes: {:?}", &group_account.data[..100.min(group_account.data.len())]);
                return Err(Box::new(e));
            }
        }
    } else {
        eprintln!("  ❌ Chat group account data too small");
        return Err("Chat group account data too small".into());
    }

    // ============================================================
    // Step 3: Send Memo to Group
    // ============================================================
    println!("\n💬 Step 3: Sending Memo to Group");
    println!("----------------------------------------");

    let test_message = format!("Hello from smoke test at {}", Utc::now());
    println!("  Message: {}", test_message);

    // Create ChatMessageData (NOT wrapped in BurnMemo)
    let message_data = ChatMessageData {
        version: CHAT_GROUP_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_SEND_MESSAGE_OPERATION.to_string(),
        group_id: next_group_id,
        sender: payer.pubkey().to_string(),
        message: test_message.clone(),
        receiver: None,
        reply_to_sig: None,
    };

    // Serialize to Borsh and encode to Base64
    let message_bytes = message_data.try_to_vec()?;
    let message_base64 = general_purpose::STANDARD.encode(&message_bytes);
    println!("  Memo length: {} bytes (Base64)", message_base64.len());

    // Create memo instruction for send_memo_to_group
    let send_memo_ix = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: message_base64.as_bytes().to_vec(),
    };

    // Get mint authority PDA from memo-mint program
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &mint_program_id,
    );

    // Create send_memo_to_group instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:send_memo_to_group");
    let result = hasher.finalize();
    let mut send_instruction_data = result[..8].to_vec();
    send_instruction_data.extend_from_slice(&next_group_id.to_le_bytes());

    let send_memo_instruction = Instruction::new_with_bytes(
        chat_program_id,
        &send_instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(chat_group_pda, false),
            AccountMeta::new(mint, false),
            AccountMeta::new_readonly(mint_authority_pda, false),
            AccountMeta::new(token_account, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(mint_program_id, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );

    // Send message transaction
    let recent_blockhash = client.get_latest_blockhash()?;
    let send_transaction = Transaction::new_signed_with_payer(
        &[
            send_memo_ix,
            send_memo_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(300_000),
            ComputeBudgetInstruction::set_compute_unit_price(1_000),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    print!("  Sending message... ");
    match client.send_and_confirm_transaction(&send_transaction) {
        Ok(signature) => {
            println!("✅");
            println!("  Transaction: {}", signature);
        }
        Err(e) => {
            println!("❌");
            eprintln!("  Error: {:?}", e);
            return Err(Box::new(e));
        }
    }

    // ============================================================
    // Step 4: Burn for Group
    // ============================================================
    println!("\n🔥 Step 4: Burning Tokens for Group");
    println!("----------------------------------------");

    let burn_for_group_amount = BURN_FOR_GROUP_TOKENS * DECIMAL_FACTOR;
    println!("  Burn Amount: {} tokens", BURN_FOR_GROUP_TOKENS);

    // Create ChatGroupBurnData
    let burn_data = ChatGroupBurnData {
        version: CHAT_GROUP_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_BURN_FOR_GROUP_OPERATION.to_string(),
        group_id: next_group_id,
        burner: payer.pubkey().to_string(),
        message: "Supporting this group from smoke test!".to_string(),
    };

    // Serialize to Borsh
    let burn_payload = burn_data.try_to_vec()?;

    // Wrap in BurnMemo structure
    let burn_memo_struct = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount: burn_for_group_amount,
        payload: burn_payload,
    };

    // Serialize BurnMemo to Borsh and encode to Base64
    let burn_memo_bytes = burn_memo_struct.try_to_vec()?;
    let burn_memo_base64 = general_purpose::STANDARD.encode(&burn_memo_bytes);
    println!("  Memo length: {} bytes (Base64)", burn_memo_base64.len());

    // Create memo instruction for burn_tokens_for_group
    let burn_memo_ix = Instruction {
        program_id: spl_memo::id(),
        accounts: vec![],
        data: burn_memo_base64.as_bytes().to_vec(),
    };

    // Create burn_tokens_for_group instruction
    let mut hasher = Sha256::new();
    hasher.update(b"global:burn_tokens_for_group");
    let result = hasher.finalize();
    let mut burn_instruction_data = result[..8].to_vec();
    burn_instruction_data.extend_from_slice(&next_group_id.to_le_bytes());
    burn_instruction_data.extend_from_slice(&burn_for_group_amount.to_le_bytes());

    let burn_for_group_instruction = Instruction::new_with_bytes(
        chat_program_id,
        &burn_instruction_data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(chat_group_pda, false),
            AccountMeta::new(burn_leaderboard_pda, false),
            AccountMeta::new(mint, false),
            AccountMeta::new(token_account, false),
            AccountMeta::new(user_global_burn_stats_pda, false),
            AccountMeta::new_readonly(token_2022_id(), false),
            AccountMeta::new_readonly(burn_program_id, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::instructions::id(), false),
        ],
    );

    // Burn for group transaction
    let recent_blockhash = client.get_latest_blockhash()?;
    let burn_transaction = Transaction::new_signed_with_payer(
        &[
            burn_memo_ix,
            burn_for_group_instruction,
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ComputeBudgetInstruction::set_compute_unit_price(1_000),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    print!("  Burning tokens... ");
    match client.send_and_confirm_transaction(&burn_transaction) {
        Ok(signature) => {
            println!("✅");
            println!("  Transaction: {}", signature);
        }
        Err(e) => {
            println!("❌");
            eprintln!("  Error: {:?}", e);
            return Err(Box::new(e));
        }
    }

    // ============================================================
    // Final Summary
    // ============================================================
    println!("\n{}", "=".repeat(50));
    println!("🎉 Memo Chat Smoke Test PASSED!");
    println!("{}", "=".repeat(50));
    println!("\nTest Results:");
    println!("  ✅ Chat group created successfully");
    println!("  ✅ Group data verified correctly");
    println!("  ✅ Message sent to group successfully");
    println!("  ✅ Tokens burned for group successfully");
    println!("\nChat Group:");
    println!("  ID: {}", next_group_id);
    println!("  PDA: {}", chat_group_pda);
    println!("  Name: {}", group_name);
    println!("\nOperations Tested:");
    println!("  1. create_chat_group - Burned {} tokens", GROUP_CREATION_BURN_TOKENS);
    println!("  2. send_memo_to_group - Sent message to group");
    println!("  3. burn_tokens_for_group - Burned {} tokens", BURN_FOR_GROUP_TOKENS);

    Ok(())
}

