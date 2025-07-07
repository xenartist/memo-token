// clients/src/test-perf-batch-mint.rs
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcSimulateTransactionConfig, RpcSendTransactionConfig},
};
use solana_sdk::{
    signature::{read_keypair_file, Signer, Keypair},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest};
use serde_json;
use rand::Rng;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Parse number of mints (default: 100)
    let mint_count = if args.len() > 1 {
        args[1].parse().unwrap_or(100)
    } else {
        100
    };
    
    // Parse number of threads (default: 16)
    let thread_count = if args.len() > 2 {
        args[2].parse().unwrap_or(16)
    } else {
        16
    };
    
    // Parse initial compute units (default: 200_000) - used as fallback
    let initial_compute_units = if args.len() > 3 {
        args[3].parse().unwrap_or(200_000)
    } else {
        200_000
    };

    // Display input information
    println!("Performance Batch Mint Configuration:");
    println!("  Number of mints: {}", mint_count);
    println!("  Thread count: {}", thread_count);
    println!("  Fallback compute units: {}", initial_compute_units);
    println!();

    // Connect to network
    let rpc_url = "https://rpc-testnet.x1.wiki";
    
    // Program and token address
    let program_id = Pubkey::from_str("TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw")
        .expect("Invalid program ID");
    let mint = Pubkey::from_str("MEM69mjnKAMxgqwosg5apfYNk2rMuV26FR9THDfT3Q7")
        .expect("Invalid mint address");

    // Calculate PDA for mint authority
    let (mint_authority_pda, _) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );

    // Load wallet
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    // Get user's token account
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint,
        &token_2022_id(),  // Use token-2022 program ID
    );
    
    // Calculate user profile PDA
    let (user_profile_pda, _) = Pubkey::find_program_address(
        &[b"user_profile", payer.pubkey().as_ref()],
        &program_id,
    );
    
    // Check if user profile exists
    let client = RpcClient::new(rpc_url);
    let user_profile_exists = match client.get_account(&user_profile_pda) {
        Ok(_) => {
            println!("User profile found at: {}", user_profile_pda);
            true
        },
        Err(_) => {
            println!("No user profile found. Performance test will continue without profile tracking.");
            false
        }
    };

    // Calculate Anchor instruction sighash for process_transfer once
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_transfer");
    let sighash_result = hasher.finalize()[..8].to_vec();

    // Shared statistics
    let stats = Arc::new(Mutex::new(PerformanceStats::new()));
    
    // Calculate mints per thread
    let mints_per_thread = mint_count / thread_count;
    let remaining_mints = mint_count % thread_count;

    println!("Starting performance batch mint test with {} mints across {} threads", mint_count, thread_count);
    println!("Mints per thread: {} (+ {} remainder for first thread)", mints_per_thread, remaining_mints);
    println!("----------------------------------------\n");

    let start_time = Instant::now();
    let mut handles = vec![];

    // Spawn worker threads
    for thread_id in 0..thread_count {
        let thread_mint_count = if thread_id == 0 {
            mints_per_thread + remaining_mints
        } else {
            mints_per_thread
        };

        let stats_clone = Arc::clone(&stats);
        let sighash_clone = sighash_result.clone();
        let rpc_url_clone = rpc_url.to_string();

        let handle = thread::spawn(move || {
            worker_thread(
                thread_id,
                thread_mint_count,
                rpc_url_clone,
                program_id,
                mint,
                mint_authority_pda,
                token_account,
                user_profile_pda,
                user_profile_exists,
                sighash_clone,
                initial_compute_units,
                stats_clone,
            )
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for (thread_id, handle) in handles.into_iter().enumerate() {
        match handle.join() {
            Ok(_) => println!("Thread {} completed", thread_id),
            Err(_) => println!("Thread {} panicked", thread_id),
        }
    }

    let total_time = start_time.elapsed();

    // Print final statistics
    let final_stats = stats.lock().unwrap();
    print_performance_summary(&final_stats, total_time, mint_count, thread_count);

    Ok(())
}

#[derive(Debug)]
struct PerformanceStats {
    successful_mints: u32,
    failed_mints: u32,
    total_compute_units: u64,
    total_simulation_time: Duration,
    total_send_time: Duration,
    fastest_mint: Option<Duration>,
    slowest_mint: Option<Duration>,
}

impl PerformanceStats {
    fn new() -> Self {
        PerformanceStats {
            successful_mints: 0,
            failed_mints: 0,
            total_compute_units: 0,
            total_simulation_time: Duration::new(0, 0),
            total_send_time: Duration::new(0, 0),
            fastest_mint: None,
            slowest_mint: None,
        }
    }

    fn update_mint_time(&mut self, duration: Duration) {
        match self.fastest_mint {
            None => self.fastest_mint = Some(duration),
            Some(fastest) if duration < fastest => self.fastest_mint = Some(duration),
            _ => {}
        }

        match self.slowest_mint {
            None => self.slowest_mint = Some(duration),
            Some(slowest) if duration > slowest => self.slowest_mint = Some(duration),
            _ => {}
        }
    }
}

fn worker_thread(
    thread_id: usize,
    mint_count: usize,
    rpc_url: String,
    program_id: Pubkey,
    mint: Pubkey,
    mint_authority_pda: Pubkey,
    token_account: Pubkey,
    user_profile_pda: Pubkey,
    user_profile_exists: bool,
    sighash_result: Vec<u8>,
    initial_compute_units: u32,
    stats: Arc<Mutex<PerformanceStats>>,
) {
    let client = RpcClient::new(rpc_url);
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");

    let mut rng = rand::thread_rng();

    println!("Thread {}: Starting {} mints", thread_id, mint_count);

    for i in 1..=mint_count {
        let mint_start = Instant::now();
        
        // Generate unique signature for this mint
        let signature = format!("PerfThread{}Mint{}", thread_id, i);
        
        // Generate a random length between 69 and 700 for the message
        let message_length: usize = rng.gen_range(69..=700);
        
        // Generate a unique message for each mint
        let base_message = format!("Perf test T{} #{}/{}", thread_id, i, mint_count);
        let padding_length = message_length.saturating_sub(base_message.len());
        let padding = "x".repeat(padding_length);
        let message = format!("{}{}", base_message, padding);
        
        // Build JSON memo
        let memo_json = serde_json::json!({
            "signature": signature,
            "message": message
        });
        
        let memo_text = serde_json::to_string(&memo_json)
            .expect("Failed to serialize JSON");

        // Create memo instruction
        let memo_ix = spl_memo::build_memo(
            memo_text.as_bytes(),
            &[&payer.pubkey()],
        );
        
        // Create mint instruction
        let mut accounts = vec![
            AccountMeta::new(payer.pubkey(), true),         // user
            AccountMeta::new(mint, false),                  // mint
            AccountMeta::new(mint_authority_pda, false),    // mint_authority (PDA)
            AccountMeta::new(token_account, false),         // token_account
            AccountMeta::new_readonly(token_2022_id(), false), // token_program
            AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false), // instructions sysvar
        ];
        
        if user_profile_exists {
            accounts.push(AccountMeta::new(user_profile_pda, false));
        }
        
        let mint_ix = Instruction::new_with_bytes(
            program_id,
            &sighash_result,
            accounts,
        );

        // Get latest blockhash
        let recent_blockhash = match client.get_latest_blockhash() {
            Ok(hash) => hash,
            Err(err) => {
                println!("Thread {}: Failed to get blockhash for mint {}: {}", thread_id, i, err);
                let mut stats = stats.lock().unwrap();
                stats.failed_mints += 1;
                continue;
            }
        };

        // Simulate transaction to get compute units
        let sim_start = Instant::now();
        let sim_transaction = Transaction::new_signed_with_payer(
            &[memo_ix.clone(), mint_ix.clone()],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let (compute_units, sim_units_consumed) = match client.simulate_transaction_with_config(
            &sim_transaction,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: false,
                commitment: Some(CommitmentConfig::confirmed()),
                encoding: None,
                accounts: None,
                min_context_slot: None,
                inner_instructions: true,
            },
        ) {
            Ok(result) => {
                if let Some(err) = result.value.err {
                    println!("Thread {}: Simulation failed for mint {}: {:?}", thread_id, i, err);
                    (initial_compute_units, None)
                } else if let Some(units_consumed) = result.value.units_consumed {
                    let required_cu = (units_consumed as f64 * 1.1) as u32;
                    (required_cu, Some(units_consumed))
                } else {
                    (initial_compute_units, None)
                }
            },
            Err(_) => (initial_compute_units, None)
        };

        let sim_time = sim_start.elapsed();

        // Create final transaction with compute budget
        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(compute_units);
        let transaction = Transaction::new_signed_with_payer(
            &[compute_budget_ix, memo_ix, mint_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        // Send transaction
        let send_start = Instant::now();
        match client.send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: None,
                encoding: None,
                max_retries: Some(1), // Reduced retries for performance
                min_context_slot: None,
            },
        ) {
            Ok(sig) => {
                let send_time = send_start.elapsed();
                let total_mint_time = mint_start.elapsed();
                
                if i % 10 == 0 || mint_count <= 10 {
                    println!("Thread {}: Mint {}/{} completed in {:.2}ms: {}", 
                        thread_id, i, mint_count, total_mint_time.as_millis(), sig);
                }
                
                // Update statistics
                let mut stats = stats.lock().unwrap();
                stats.successful_mints += 1;
                stats.total_simulation_time += sim_time;
                stats.total_send_time += send_time;
                stats.update_mint_time(total_mint_time);
                
                if let Some(units) = sim_units_consumed {
                    stats.total_compute_units += units;
                }
            }
            Err(err) => {
                let send_time = send_start.elapsed();
                println!("Thread {}: Mint {}/{} failed after {:.2}ms: {}", 
                    thread_id, i, mint_count, send_time.as_millis(), err);
                
                let mut stats = stats.lock().unwrap();
                stats.failed_mints += 1;
                stats.total_send_time += send_time;
            }
        }
    }

    println!("Thread {}: Completed {} mints", thread_id, mint_count);
}

fn print_performance_summary(
    stats: &PerformanceStats,
    total_time: Duration,
    total_mints: usize,
    thread_count: usize,
) {
    println!("\n========================================");
    println!("PERFORMANCE BATCH MINT TEST SUMMARY");
    println!("========================================");
    
    println!("\n📊 Basic Statistics:");
    println!("   Total mints attempted: {}", total_mints);
    println!("   Successful mints: {}", stats.successful_mints);
    println!("   Failed mints: {}", stats.failed_mints);
    println!("   Success rate: {:.2}%", 
        (stats.successful_mints as f64 / total_mints as f64) * 100.0);
    
    println!("\n⏱️  Performance Metrics:");
    println!("   Total execution time: {:.2}s", total_time.as_secs_f64());
    println!("   Average TPS (transactions per second): {:.2}", 
        stats.successful_mints as f64 / total_time.as_secs_f64());
    println!("   Thread count: {}", thread_count);
    println!("   TPS per thread: {:.2}", 
        (stats.successful_mints as f64 / total_time.as_secs_f64()) / thread_count as f64);
    
    if let (Some(fastest), Some(slowest)) = (stats.fastest_mint, stats.slowest_mint) {
        println!("   Fastest mint: {:.2}ms", fastest.as_millis());
        println!("   Slowest mint: {:.2}ms", slowest.as_millis());
        println!("   Average mint time: {:.2}ms", 
            (stats.total_send_time.as_millis() as f64) / (stats.successful_mints as f64));
    }
    
    println!("\n💰 Cost Analysis:");
    println!("   Total compute units consumed: {}", stats.total_compute_units);
    if stats.successful_mints > 0 {
        println!("   Average CU per mint: {:.2}", 
            stats.total_compute_units as f64 / stats.successful_mints as f64);
        
        const SOL_PER_COMPUTE_UNIT: f64 = 0.0000001;
        let total_sol_cost = stats.total_compute_units as f64 * SOL_PER_COMPUTE_UNIT;
        let avg_sol_per_mint = total_sol_cost / stats.successful_mints as f64;
        
        println!("   Total estimated cost: {:.8} SOL", total_sol_cost);
        println!("   Average cost per mint: {:.8} SOL", avg_sol_per_mint);
    }
    
    println!("\n🔧 Timing Breakdown:");
    println!("   Total simulation time: {:.2}ms", stats.total_simulation_time.as_millis());
    println!("   Total send time: {:.2}ms", stats.total_send_time.as_millis());
    if stats.successful_mints > 0 {
        println!("   Avg simulation time per mint: {:.2}ms", 
            stats.total_simulation_time.as_millis() as f64 / stats.successful_mints as f64);
        println!("   Avg send time per mint: {:.2}ms", 
            stats.total_send_time.as_millis() as f64 / stats.successful_mints as f64);
    }
    
    println!("========================================");
} 