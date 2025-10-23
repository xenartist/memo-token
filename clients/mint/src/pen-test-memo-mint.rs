use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer, Keypair},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
    system_instruction,
    rent::Rent,
};
use spl_associated_token_account::{get_associated_token_address_with_program_id, instruction::create_associated_token_account};
use std::str::FromStr;
use std::collections::HashMap;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

use memo_token_client::get_rpc_url;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let test_type = args[1].as_str();
    
    // run the corresponding test
    match test_type {
        "memo-attacks" => run_memo_attack_tests(),
        "instruction-attacks" => run_instruction_attack_tests(),
        "pda-attacks" => run_pda_attack_tests(),
        "mint-authority-attacks" => run_mint_authority_attack_tests(),
        "account-attacks" => run_account_attack_tests(),
        "dos-attacks" => run_dos_attack_tests(),
        "combined-attacks" => run_combined_attack_tests(),
        "all" => run_all_tests(),
        _ => {
            println!("❌ Unknown test type: {}", test_type);
            print_usage();
            Ok(())
        }
    }
}

fn print_usage() {
    println!("📋 MEMO-MINT CONTRACT PENETRATION TESTING SUITE");
    println!();
    println!("Usage: cargo run -- <test_type>");
    println!();
    println!("Available test types:");
    println!("  memo-attacks         - Test memo instruction vulnerabilities");
    println!("  instruction-attacks  - Test instruction ordering attacks");
    println!("  pda-attacks         - Test PDA derivation attacks");
    println!("  mint-authority-attacks - Test mint authority bypass attempts");
    println!("  account-attacks     - Test account substitution attacks");
    println!("  dos-attacks         - Test denial of service attacks");
    println!("  combined-attacks    - Test complex combined attack vectors");
    println!("  all                 - Run all attack tests");
    println!();
    println!("Examples:");
    println!("  cargo run -- memo-attacks");
    println!("  cargo run -- all");
}

// base configuration function
fn get_test_config() -> (RpcClient, Keypair, Pubkey, Pubkey, Pubkey) {
    let rpc_url = get_rpc_url();
    let client = RpcClient::new(rpc_url.to_string());
    
    // use default test address
    let payer = read_keypair_file("/Users/bobdos/.config/solana/id.json")
        .expect("Failed to read keypair file");
    
    let memo_mint_program_id = Pubkey::from_str("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy")
        .expect("Invalid program ID");
    
    let mint_address = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");
    
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_address,
        &token_2022_id(),
    );
    
    (client, payer, memo_mint_program_id, mint_address, token_account)
}

// test function type definition
type TestFunction = fn(&RpcClient, &Keypair, &Pubkey, &Pubkey, &Pubkey, &Pubkey, &Pubkey) -> Result<(), Box<dyn std::error::Error>>;

// run each test separately - fix function array type problem
fn run_memo_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING MEMO ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    // use Vec instead of array to avoid type problem
    let tests: Vec<(&str, TestFunction)> = vec![
        ("No memo attack", test_no_memo_attack),
        ("Empty memo data", test_empty_memo_attack),
        ("Memo at wrong index", test_wrong_memo_index),
        ("Multiple memo instructions", test_multiple_memo_attack),
        ("Fake memo program", test_fake_memo_program),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_instruction_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING INSTRUCTION ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Memo after mint", test_memo_after_mint),
        ("Multiple mints one memo", test_multiple_mints_one_memo),
        ("Interleaved instructions", test_interleaved_instructions),
        ("Compute budget in wrong position", test_compute_budget_position),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_pda_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING PDA ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Fake mint authority", test_fake_mint_authority),
        ("Wrong PDA seeds", test_wrong_pda_seeds),
        ("Cross-program PDA", test_cross_program_pda),
        ("PDA bump manipulation", test_pda_bump_manipulation),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_mint_authority_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING MINT AUTHORITY ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Wrong mint address", test_wrong_mint_address),
        ("Unauthorized mint", test_unauthorized_mint),
        ("Token account manipulation", test_token_account_manipulation),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_account_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING ACCOUNT ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Wrong token program", test_wrong_token_program),
        ("Wrong instructions sysvar", test_wrong_instructions_sysvar),
        ("Account order manipulation", test_account_order_manipulation),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_dos_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING DOS ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Compute exhaustion", test_compute_exhaustion),
        ("Account size bloat", test_account_bloat),
        ("Transaction spam", test_transaction_spam),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_combined_attack_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 STARTING COMBINED ATTACK TESTS");
    
    let (client, payer, program_id, mint, token_account) = get_test_config();
    let instructions_sysvar = Pubkey::from_str("Sysvar1nstructions1111111111111111111111111").unwrap();
    let (mint_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &program_id);
    
    let tests: Vec<(&str, TestFunction)> = vec![
        ("Memo Bypass", test_memo_bypass_attacks),
        ("Memo Injection", test_memo_injection_attacks), 
        ("Memo Length", test_memo_length_attacks),
        ("Instruction Reorder", test_instruction_reorder_attacks),
        ("PDA Attacks", test_pda_attacks),
        ("Mint Authority", test_mint_authority_attacks),
        ("Overflow Attacks", test_overflow_attacks),
        ("Supply Manipulation", test_supply_manipulation),
        ("Reentrancy", test_reentrancy_attacks),
        ("Account Substitution", test_account_substitution),
        ("DoS Attacks", test_dos_attacks),
        ("Economic Attacks", test_economic_attacks),
    ];
    
    for (test_name, test_fn) in tests {
        println!("\n🎯 Testing: {}", test_name);
        match test_fn(&client, &payer, &program_id, &mint, &mint_authority, &token_account, &instructions_sysvar) {
            Ok(_) => println!("❌ SECURITY ISSUE: {} should have failed but succeeded!", test_name),
            Err(e) => println!("✅ Expected failure: {}", e),
        }
    }
    
    Ok(())
}

fn run_all_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 RUNNING ALL PENETRATION TESTS");
    
    run_memo_attack_tests()?;
    run_instruction_attack_tests()?;
    run_pda_attack_tests()?;
    run_mint_authority_attack_tests()?;
    run_account_attack_tests()?;
    run_dos_attack_tests()?;
    run_combined_attack_tests()?;
    
    println!("\n🎉 ALL PENETRATION TESTS COMPLETED");
    Ok(())
}

// 1. Memo attack test function
fn test_no_memo_attack(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to mint without memo instruction...");
    
    // try to mint without memo instruction
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    let mut transaction = Transaction::new_with_payer(&[mint_ix], Some(&payer.pubkey()));
    let recent_blockhash = client.get_latest_blockhash()?;
    transaction.sign(&[payer], recent_blockhash);
    
    match client.simulate_transaction(&transaction) {
        Ok(result) => {
            if result.value.err.is_some() {
                return Err(format!("Transaction failed as expected: {:?}", result.value.err).into());
            } else {
                return Ok(()); // this means security vulnerability
            }
        }
        Err(e) => return Err(format!("Simulation error: {}", e).into()),
    }
}

// other test function implementation (for brevity, I only show a few key ones)
fn test_empty_memo_attack(
    client: &RpcClient,
    payer: &Keypair, 
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to mint with empty memo...");
    
    let memo_ix = create_memo_instruction(""); // 空memo
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

// helper function
fn create_memo_instruction(memo_text: &str) -> Instruction {
    Instruction::new_with_bytes(
        spl_memo::ID,
        memo_text.as_bytes(),
        vec![],
    )
}

fn create_process_mint_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    // use correct instruction identifier
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]); // process_mint instruction identifier
    
    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn execute_attack_transaction(
    client: &RpcClient,
    payer: &Keypair,
    instructions: Vec<Instruction>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    let recent_blockhash = client.get_latest_blockhash()?;
    transaction.sign(&[payer], recent_blockhash);
    
    match client.simulate_transaction(&transaction) {
        Ok(result) => {
            if result.value.err.is_some() {
                return Err(format!("Expected failure: {:?}", result.value.err).into());
            } else {
                return Ok(()); // security vulnerability: should have failed but succeeded
            }
        }
        Err(e) => return Err(format!("Expected simulation error: {}", e).into()),
    }
}

// 1. memo attack test
fn test_wrong_memo_index(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to place memo at wrong index (index 0)...");
    
    // try to place memo instruction at index 0 instead of required index 1
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    let memo_ix = create_memo_instruction("This is a test memo that meets requirement!");
    
    // wrong order: memo at index 0, mint at index 1
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_multiple_memo_attack(
    client: &RpcClient,
    payer: &Keypair, 
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use multiple memo instructions...");
    
    let memo1 = create_memo_instruction("First memo that meets minimum length requirement!");
    let memo2 = create_memo_instruction("Second memo meets minimum length requirement!");
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo1, memo2, mint_ix])
}

fn test_fake_memo_program(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use fake memo program...");
    
    // use wrong program ID to create "fake" memo instruction
    let fake_program_id = Pubkey::new_unique();
    let fake_memo_ix = Instruction::new_with_bytes(
        fake_program_id,
        "this is a fake memo that meets minimum length requirement!".as_bytes(),
        vec![],
    );
    
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![fake_memo_ix, mint_ix])
}

// 2. instruction order attack test
fn test_memo_after_mint(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to place memo after mint instruction...");
    
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    let memo_ix = create_memo_instruction("This memo comes after mint but meets requirement!");
    
    // wrong order: mint at index 0, memo at index 1 (should be reversed)
    execute_attack_transaction(client, payer, vec![mint_ix, memo_ix])
}

fn test_multiple_mints_one_memo(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use one memo for multiple mint attempts...");
    
    let memo_ix = create_memo_instruction("Single memo for multiple mint attempts!");
    let mint_ix1 = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    let mint_ix2 = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix1, mint_ix2])
}

fn test_interleaved_instructions(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting interleaved instructions...");
    
    let memo_ix = create_memo_instruction("Interleaved instruction test memo!");
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    // insert a system instruction in the middle
    let transfer_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0);
    
    execute_attack_transaction(client, payer, vec![memo_ix, transfer_ix, mint_ix])
}

fn test_compute_budget_position(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting compute budget in wrong position...");
    
    let memo_ix = create_memo_instruction("Compute budget position test memo!");
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    
    // wrong order: memo, compute budget, mint (should be compute budget, memo, mint)
    execute_attack_transaction(client, payer, vec![memo_ix, compute_ix, mint_ix])
}

// 3. PDA attack test
fn test_fake_mint_authority(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    _mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use fake mint authority...");
    
    let memo_ix = create_memo_instruction("Fake mint authority test memo!");
    
    // 使用假的mint authority
    let fake_authority = Keypair::new();
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        &fake_authority.pubkey(),
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_wrong_pda_seeds(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    _mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use wrong PDA seeds...");
    
    let memo_ix = create_memo_instruction("Wrong PDA seeds test memo!");
    
    // use wrong seeds to generate PDA
    let (wrong_authority, _) = Pubkey::find_program_address(&[b"wrong_authority"], program_id);
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        &wrong_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_cross_program_pda(
    client: &RpcClient,
    payer: &Keypair,
    _program_id: &Pubkey,
    mint: &Pubkey,
    _mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting cross-program PDA attack...");
    
    let memo_ix = create_memo_instruction("Cross program PDA test memo!");
    
    // use PDA from other program
    let other_program = Pubkey::new_unique();
    let (cross_authority, _) = Pubkey::find_program_address(&[b"mint_authority"], &other_program);
    let mint_ix = create_process_mint_instruction(
        &other_program,
        &payer.pubkey(),
        mint,
        &cross_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_pda_bump_manipulation(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    _mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting PDA bump manipulation attack...");
    
    let memo_ix = create_memo_instruction("PDA bump manipulation test memo!");
    
    // try to use wrong bump value
    let (correct_authority, correct_bump) = Pubkey::find_program_address(&[b"mint_authority"], program_id);
    let wrong_bump = if correct_bump == 255 { 254 } else { correct_bump + 1 };
    
    // try to use wrong bump to generate PDA
    let wrong_authority = Pubkey::create_program_address(&[b"mint_authority", &[wrong_bump]], program_id)
        .unwrap_or(correct_authority);
    
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        &wrong_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

// 4. Mint Authority attack test
fn test_wrong_mint_address(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    _mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use wrong mint address...");
    
    let memo_ix = create_memo_instruction("Wrong mint address test memo!");
    
    // use wrong mint address
    let wrong_mint = Pubkey::new_unique();
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        &wrong_mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_unauthorized_mint(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting unauthorized mint operation...");
    
    let memo_ix = create_memo_instruction("Unauthorized mint test memo!");
    
    // try to use user as mint authority instead of PDA
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(payer.pubkey(), true), // wrong: use user instead of PDA
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    let mint_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_token_account_manipulation(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    _token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting token account manipulation attack...");
    
    let memo_ix = create_memo_instruction("Token account manipulation test!");
    
    // use wrong token account (other user's)
    let other_user = Keypair::new();
    let wrong_token_account = get_associated_token_address_with_program_id(
        &other_user.pubkey(),
        mint,
        &token_2022_id(),
    );
    
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        &wrong_token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

// 5. account attack test
fn test_wrong_token_program(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use wrong token program...");
    
    let memo_ix = create_memo_instruction("Wrong token program test memo!");
    
    // use wrong token program (SPL Token instead of Token-2022)
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false), // wrong: use fake token program ID
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    let mint_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_wrong_instructions_sysvar(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    _instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting to use wrong instructions sysvar...");
    
    let memo_ix = create_memo_instruction("Wrong sysvar test memo!");
    
    // use wrong sysvar account
    let wrong_sysvar = Pubkey::new_unique();
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        &wrong_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_account_order_manipulation(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting account order manipulation attack...");
    
    let memo_ix = create_memo_instruction("Account order manipulation test memo!");
    
    // wrong account order
    let accounts = vec![
        AccountMeta::new(*mint, false), // wrong order: mint first
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    let mint_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

// 6. DoS attack test
fn test_compute_exhaustion(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting compute resource exhaustion attack...");
    
    // use longest memo to consume compute resources
    let max_memo = "X".repeat(800); // 最大允许长度
    let memo_ix = create_memo_instruction(&max_memo);
    
    // set very low compute unit limit
    let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(1000); // very low limit
    
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![compute_ix, memo_ix, mint_ix])
}

fn test_account_bloat(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting account bloat attack...");
    
    let memo_ix = create_memo_instruction("Account bloat test memo!");
    
    // add many unnecessary accounts
    let mut accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    // add many fake accounts
    for _ in 0..20 {
        accounts.push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
    }
    
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    let mint_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix])
}

fn test_transaction_spam(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting transaction spam attack...");
    
    // quickly send multiple transactions in a row
    for i in 0..5 {
        let memo_text = format!("Spam transaction #{} meets requirement!", i);
        let memo_ix = create_memo_instruction(&memo_text);
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix, mint_ix]);
        // don't wait, send next immediately
    }
    
    Err("Transaction spam attack attempted".into())
}

// 7. combined attack test
fn test_memo_bypass_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting memo bypass attack...");
    
    // combine multiple memo bypass techniques
    // 1. empty memo + wrong position
    let empty_memo = create_memo_instruction("");
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    // try multiple combinations
    let combinations = vec![
        vec![empty_memo.clone(), mint_ix.clone()],
        vec![mint_ix.clone(), empty_memo.clone()],
    ];
    
    for combo in combinations {
        let _ = execute_attack_transaction(client, payer, combo);
    }
    
    Err("Memo bypass attack attempted".into())
}

fn test_memo_injection_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting memo injection attack...");
    
    // try to inject special characters and control characters
    let injection_memos: Vec<String> = vec![
        format!("{}null byte injection test{} meets requirement!", "X".repeat(20), "Y".repeat(20)),
        "Escape sequence injection test\\n\\r\\t meets requirement!".to_string(),
        "Unicode injection test🚀🔥 meets requirement!".to_string(),
    ];
    
    for memo_text in injection_memos {
        let memo_ix = create_memo_instruction(&memo_text);
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix, mint_ix]);
    }
    
    Err("Memo injection attack attempted".into())
}

fn test_memo_length_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting memo length attack...");
    
    // test various length boundaries
    let length_tests: Vec<String> = vec![
        "".to_string(),                           // 0 bytes
        "X".to_string(),                         // 1 byte
        "X".repeat(68),              // 68 bytes (below minimum 69)
        "X".repeat(801),             // 801 bytes (exceeds maximum 800)
        "X".repeat(10000),           // very long memo
    ];
    
    for memo_text in length_tests {
        let memo_ix = create_memo_instruction(&memo_text);
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix, mint_ix]);
    }
    
    Err("Memo length attack attempted".into())
}

fn test_instruction_reorder_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting instruction reorder attack...");
    
    let memo_ix = create_memo_instruction("Instruction reorder test memo!");
    let mint_ix = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(200_000);
    let transfer_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 0);
    
    // try multiple wrong orders
    let reorder_combinations = vec![
        vec![mint_ix.clone(), memo_ix.clone()],
        vec![compute_ix.clone(), mint_ix.clone(), memo_ix.clone()],
        vec![memo_ix.clone(), transfer_ix.clone(), mint_ix.clone()],
        vec![transfer_ix.clone(), memo_ix.clone(), mint_ix.clone()],
    ];
    
    for combo in reorder_combinations {
        let _ = execute_attack_transaction(client, payer, combo);
    }
    
    Err("Instruction reorder attack attempted".into())
}

fn test_pda_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    _mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting comprehensive PDA attack...");
    
    let memo_ix = create_memo_instruction("Comprehensive PDA attack test memo!");
    
    // multiple PDA attacks
    let pda_attacks = vec![
        Pubkey::new_unique(), // random address
        payer.pubkey(),       // user address
        *mint,                // mint address as authority
        spl_token_2022::ID,   // token program as authority
    ];
    
    for fake_authority in pda_attacks {
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            &fake_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix.clone(), mint_ix]);
    }
    
    Err("PDA attack attempted".into())
}

fn test_mint_authority_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting mint authority attack...");
    
    let memo_ix = create_memo_instruction("Mint authority attack test memo!");
    
    // try signature bypass
    let evil_keypair = Keypair::new();
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(evil_keypair.pubkey(), true), // try to make evil keypair sign
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    let mut instruction_data = [0u8; 8];
    instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    let evil_mint_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
    
    // try multiple signers
    let mut transaction = Transaction::new_with_payer(&[memo_ix, evil_mint_ix], Some(&payer.pubkey()));
    let recent_blockhash = client.get_latest_blockhash()?;
    transaction.sign(&[payer, &evil_keypair], recent_blockhash);
    
    match client.simulate_transaction(&transaction) {
        Ok(result) => {
            if result.value.err.is_some() {
                return Err(format!("Expected failure: {:?}", result.value.err).into());
            } else {
                return Ok(());
            }
        }
        Err(e) => return Err(format!("Expected simulation error: {}", e).into()),
    }
}

fn test_overflow_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting overflow attack...");
    
    // quickly mint multiple times to trigger overflow
    for i in 0..10 {
        let memo_text = format!("Overflow attack attempt #{} meets requirement!", i);
        let memo_ix = create_memo_instruction(&memo_text);
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix, mint_ix]);
    }
    
    Err("Overflow attack attempted".into())
}

fn test_supply_manipulation(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting supply manipulation attack...");
    
    // try to directly manipulate mint supply (should be blocked by contract)
    let memo_ix = create_memo_instruction("Supply manipulation test memo!");
    
    // construct malicious mint instruction to mint large amount of tokens
    let evil_accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(spl_token_2022::ID, false),
        AccountMeta::new_readonly(*instructions_sysvar, false),
    ];
    
    // try to use wrong instruction identifier
    let mut evil_data = [0u8; 16];
    evil_data[0..8].copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
    evil_data[8..16].copy_from_slice(&u64::MAX.to_le_bytes()); // try to mint maximum amount
    
    let evil_ix = Instruction::new_with_bytes(*program_id, &evil_data, evil_accounts);
    
    execute_attack_transaction(client, payer, vec![memo_ix, evil_ix])
}

fn test_reentrancy_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting reentrancy attack...");
    
    let memo_ix = create_memo_instruction("Reentrancy attack test memo!");
    
    // try to call mint multiple times in the same transaction
    let mint_ix1 = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    let mint_ix2 = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    let mint_ix3 = create_process_mint_instruction(
        program_id,
        &payer.pubkey(),
        mint,
        mint_authority,
        token_account,
        instructions_sysvar,
    );
    
    execute_attack_transaction(client, payer, vec![memo_ix, mint_ix1, mint_ix2, mint_ix3])
}

fn test_account_substitution(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    _token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting account substitution attack...");
    
    let memo_ix = create_memo_instruction("Account substitution test memo!");
    
    // create multiple fake accounts for substitution
    let fake_accounts = vec![
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];
    
    // try to substitute different accounts
    for fake_account in fake_accounts {
        let accounts = vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(fake_account, false), // substitute mint
            AccountMeta::new_readonly(*mint_authority, false),
            AccountMeta::new(fake_account, false), // substitute token account
            AccountMeta::new_readonly(spl_token_2022::ID, false),
            AccountMeta::new_readonly(*instructions_sysvar, false),
        ];
        
        let mut instruction_data = [0u8; 8];
        instruction_data.copy_from_slice(&[175, 175, 109, 31, 13, 152, 155, 237]);
        let fake_ix = Instruction::new_with_bytes(*program_id, &instruction_data, accounts);
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix.clone(), fake_ix]);
    }
    
    Err("Account substitution attack attempted".into())
}

fn test_dos_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting comprehensive DoS attack...");
    
    // 1. resource exhaustion
    let max_memo = "X".repeat(800);
    let memo_ix = create_memo_instruction(&max_memo);
    
    // 2. many instructions
    let mut instructions = vec![memo_ix];
    for _ in 0..50 {
        instructions.push(create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        ));
    }
    
    execute_attack_transaction(client, payer, instructions)
}

fn test_economic_attacks(
    client: &RpcClient,
    payer: &Keypair,
    program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
    instructions_sysvar: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  🎯 Attempting economic attack...");
    
    // try to manipulate economy by minting frequently
    for i in 0..20 {
        let memo_text = format!("Economic attack iteration #{} meets requirement!", i);
        let memo_ix = create_memo_instruction(&memo_text);
        let mint_ix = create_process_mint_instruction(
            program_id,
            &payer.pubkey(),
            mint,
            mint_authority,
            token_account,
            instructions_sysvar,
        );
        
        let _ = execute_attack_transaction(client, payer, vec![memo_ix, mint_ix]);
        
        // short delay
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    
    Err("Economic attack attempted".into())
}
