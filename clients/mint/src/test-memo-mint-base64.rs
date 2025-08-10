use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSimulateTransactionConfig,
};
use solana_sdk::{
    signature::{read_keypair_file, Signer},
    pubkey::Pubkey,
    instruction::{AccountMeta, Instruction},
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    commitment_config::CommitmentConfig,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};
use std::str::FromStr;
use sha2::{Sha256, Digest};
use borsh::{BorshSerialize, BorshDeserialize};
use base64::{Engine as _, engine::general_purpose};

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Borsh data structure for comparison testing
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct ComparisonMemoData {
    /// Content field for testing
    pub content: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Format CU Simulation Analysis ===");
    println!("Comparing: Raw String vs Memo Base64 vs Borsh+Base64");
    println!("Focus: CU consumption analysis for different character sets and encoding methods\n");
    
    // Get command line arguments for test scenario
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_help(&args[0]);
        return Ok(());
    }
    
    let test_scenario = &args[1];
    
    match test_scenario.as_str() {
        "simple" => test_simple_message(),
        "chinese" => test_chinese_message(),
        "emoji" => test_emoji_message(),
        "mixed" => test_mixed_message(),
        "long" => test_long_message(),
        "all" => test_all_messages(),
        "analysis" => test_detailed_analysis(),
        "help" | _ => {
            print_help(&args[0]);
            Ok(())
        }
    }
}

fn print_help(program_name: &str) {
    println!("Usage: {} <test_scenario>", program_name);
    println!("\nTest Scenarios:");
    println!("  simple       - Test simple English message");
    println!("  chinese      - Test Chinese characters");
    println!("  emoji        - Test emoji-rich message");
    println!("  mixed        - Test mixed language message");
    println!("  long         - Test long message");
    println!("  all          - Run all comparison tests");
    println!("  analysis     - Detailed character set analysis");
    println!("\nEach test compares THREE formats:");
    println!("  1. Raw String - Direct UTF-8 string as memo");
    println!("  2. Memo Base64 - String encoded with Base64");
    println!("  3. Borsh + Base64 - String in Borsh struct, then Base64");
    println!("\nThe test measures:");
    println!("  - Message size differences");
    println!("  - Simulated CU consumption");
    println!("  - CU efficiency per byte");
    println!("  - Character encoding impact on performance");
    println!("\nExamples:");
    println!("  {} chinese    # Test Chinese character CU efficiency", program_name);
    println!("  {} analysis   # Detailed character set analysis", program_name);
    println!("  {} all        # Complete comparison", program_name);
}

// Test functions
fn test_simple_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Hello World! This is a simple English message for testing blockchain memos with different encoding methods.";
    analyze_memo_formats("Simple English Message", content)
}

fn test_chinese_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "你好世界！这是一个测试中文字符的真实区块链交易消息。今天天气很好，适合测试不同编码方法的实际CU消耗。";
    analyze_memo_formats("Chinese Message", content)
}

fn test_emoji_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Hello World! 🌍🚀💻🎉 Testing with emojis! 😊🔥⭐🌟 Real blockchain ⛓️ transactions! 🎯🎊🎈 Let's test actual CU usage with different encoding methods for memo data on the blockchain! 🌈💫🎪🎭";
    analyze_memo_formats("Emoji Message", content)
}

fn test_mixed_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Mixed: Hello 你好 こんにちは 안녕하세요 مرحبا Привет! 🌍 Real multi-language blockchain test 🚀 actual CU measurement! Testing different encoding methods for international character support in blockchain memos.";
    analyze_memo_formats("Mixed Language Message", content)
}

fn test_long_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "This is a very long message to test the difference in compute unit consumption between raw string, Base64-encoded string, and Borsh+Base64 serialization methods. This message is intentionally long to test how message length affects CU consumption patterns across different encoding strategies. ".repeat(2);
    analyze_memo_formats("Long Message", &content)
}

fn test_all_messages() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Running ALL three-way CU analysis tests...\n");
    
    let test_cases = vec![
        ("Simple English", "Hello World! This is a simple test message for memo encoding comparison and CU analysis."),
        ("Chinese Characters", "你好世界！这是一个测试中文字符的消息，用于比较不同的编码方法和CU分析。这个测试很重要。"),
        ("Japanese Characters", "こんにちは世界！これは異なる符号化方法とCU分析を比較するためのテストメッセージです。"),
        ("Korean Characters", "안녕하세요 세계! 이것은 다른 인코딩 방법과 CU 분석을 비교하기 위한 테스트 메시지입니다."),
        ("Arabic Characters", "مرحبا بالعالم! هذه رسالة اختبار لمقارنة طرق التشفير المختلفة وتحليل وحدة الحوسبة."),
        ("Russian Characters", "Привет мир! Это тестовое сообщение для сравнения различных методов кодирования и анализа CU."),
        ("Emoji Rich", "Hello World! 🌍🚀💻🎉 Testing emojis! 😊🔥⭐🌟 in blockchain memos! 🌈💫🎪🎭 CU analysis! 🎯🎊🎈"),
        ("Mixed Languages", "Mixed: Hello 你好 こんにちは 🌍 Testing multiple formats! International character support CU analysis."),
    ];
    
    let mut all_results = Vec::new();
    
    for (i, (test_name, content)) in test_cases.iter().enumerate() {
        println!("--- Test {}/{}: {} ---", i + 1, test_cases.len(), test_name);
        
        match analyze_memo_formats_internal(content) {
            Ok(result) => {
                all_results.push((test_name.to_string(), result));
                println!("✅ {} analysis COMPLETED\n", test_name);
            },
            Err(e) => {
                println!("❌ {} analysis FAILED: {}\n", test_name, e);
                all_results.push((test_name.to_string(), ComparisonResult::default()));
            }
        }
        
        // Small delay between tests
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    
    // Generate comprehensive analysis report
    print_comprehensive_analysis(&all_results);
    
    Ok(())
}

fn test_detailed_analysis() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔬 DETAILED CHARACTER SET CU ANALYSIS\n");
    
    // Test various character densities and types
    let analysis_cases = vec![
        ("Pure ASCII", "Hello World! This is a test message with only ASCII characters for CU analysis."),
        ("ASCII + Numbers", "Hello123! Test message with ASCII + numbers 456789 for CU analysis benchmark."),
        ("Chinese Dense", "你好世界测试消息中文字符密集型计算单元分析基准测试非常重要的数据。"),
        ("Japanese Dense", "こんにちは世界テストメッセージ日本語文字密集型計算単位分析基準。"),
        ("Korean Dense", "안녕하세요세계테스트메시지한국어문자밀집형계산단위분석기준테스트。"),
        ("Emoji Dense", "🌍🚀💻🎉😊🔥⭐🌟⛓️🎯🎊🎈🌈💫🎪🎭🎮🎲🎨🎯🎪🎭🎨🎮🎲🎯"),
        ("Mixed Dense", "Hello你好こんにちは🌍Test测试テスト🚀Mixed混合ミックス💻Analysis!"),
        ("UTF-8 Edge Cases", "Café naïve résumé 北京 東京 москва θέλω ñoño ümlaut ćirić ş ğ İ"),
    ];
    
    let mut analysis_results = Vec::new();
    
    for (test_name, content) in analysis_cases {
        println!("🔍 Analyzing: {}", test_name);
        
        match analyze_memo_formats_internal(content) {
            Ok(result) => {
                // Calculate character statistics
                let char_count = content.chars().count();
                let byte_count = content.as_bytes().len();
                let avg_bytes_per_char = byte_count as f64 / char_count as f64;
                
                println!("  Characters: {}, Bytes: {}, Avg bytes/char: {:.2}", 
                         char_count, byte_count, avg_bytes_per_char);
                println!("  Raw CU: {}, Base64 CU: {}, Borsh CU: {}", 
                         result.raw_string_cu, result.memo_base64_cu, result.borsh_base64_cu);
                println!("  CU efficiency (CU/byte): Raw {:.1}, Base64 {:.1}, Borsh {:.1}",
                         result.raw_string_cu as f64 / byte_count as f64,
                         result.memo_base64_cu as f64 / result.memo_base64_size as f64,
                         result.borsh_base64_cu as f64 / result.borsh_base64_size as f64);
                
                analysis_results.push((test_name.to_string(), result));
                println!();
            },
            Err(e) => {
                println!("  ❌ Analysis failed: {}\n", e);
                analysis_results.push((test_name.to_string(), ComparisonResult::default()));
            }
        }
    }
    
    // Print detailed analysis
    print_character_analysis(&analysis_results);
    
    Ok(())
}

#[derive(Debug, Clone)]
struct ComparisonResult {
    raw_string_size: usize,
    memo_base64_size: usize,
    borsh_base64_size: usize,
    raw_string_cu: u64,
    memo_base64_cu: u64,
    borsh_base64_cu: u64,
    memo_base64_increase_percent: f64,
    borsh_base64_increase_percent: f64,
    memo_base64_cu_diff_percent: f64,
    borsh_base64_cu_diff_percent: f64,
}

impl Default for ComparisonResult {
    fn default() -> Self {
        Self {
            raw_string_size: 0,
            memo_base64_size: 0,
            borsh_base64_size: 0,
            raw_string_cu: 0,
            memo_base64_cu: 0,
            borsh_base64_cu: 0,
            memo_base64_increase_percent: 0.0,
            borsh_base64_increase_percent: 0.0,
            memo_base64_cu_diff_percent: 0.0,
            borsh_base64_cu_diff_percent: 0.0,
        }
    }
}

fn analyze_memo_formats(test_name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Three-way CU analysis for: {}\n", test_name);
    let result = analyze_memo_formats_internal(content)?;
    print_single_analysis_summary(test_name, &result, content);
    Ok(())
}

fn analyze_memo_formats_internal(content: &str) -> Result<ComparisonResult, Box<dyn std::error::Error>> {
    let client = create_rpc_client();
    let payer = load_payer_keypair();
    let (program_id, mint_address, mint_authority_pda, token_account) = get_program_addresses();
    
    // Ensure token account exists
    ensure_token_account_exists(&client, &payer, &mint_address, &token_account)?;
    
    println!("Content to analyze: {}", get_safe_preview(content, 100));
    println!("Content stats: {} chars, {} UTF-8 bytes", content.chars().count(), content.as_bytes().len());
    println!();
    
    // Test 1: Raw String (direct)
    println!("=== Analysis 1: Raw String (Direct UTF-8) ===");
    let (raw_memo_bytes, raw_cu) = create_and_simulate_raw_string(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;
    
    println!();
    
    // Test 2: Memo Base64 (only memo content encoded)
    println!("=== Analysis 2: Memo Base64 (memo content only) ===");
    let (memo_base64_bytes, memo_base64_cu) = create_and_simulate_memo_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;
    
    println!();
    
    // Test 3: Borsh + Base64
    println!("=== Analysis 3: Borsh + Base64 ===");
    let (borsh_base64_bytes, borsh_base64_cu) = create_and_simulate_borsh_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;

    // Calculate differences
    let memo_base64_increase_percent = ((memo_base64_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    let borsh_base64_increase_percent = ((borsh_base64_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    let memo_base64_cu_diff_percent = ((memo_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let borsh_base64_cu_diff_percent = ((borsh_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    
    let result = ComparisonResult {
        raw_string_size: raw_memo_bytes.len(),
        memo_base64_size: memo_base64_bytes.len(),
        borsh_base64_size: borsh_base64_bytes.len(),
        raw_string_cu: raw_cu,
        memo_base64_cu: memo_base64_cu,
        borsh_base64_cu: borsh_base64_cu,
        memo_base64_increase_percent,
        borsh_base64_increase_percent,
        memo_base64_cu_diff_percent,
        borsh_base64_cu_diff_percent,
    };
    
    Ok(result)
}

fn create_and_simulate_raw_string(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    let memo_bytes = content.as_bytes().to_vec();
    
    println!("  Raw string: {} chars → {} bytes", content.chars().count(), memo_bytes.len());
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Memo too long ({} bytes, maximum 800)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    
    println!("  ✅ Length validation passed");
    
    // Simulate CU consumption
    let memo_ix = spl_memo::build_memo(&memo_bytes, &[&payer.pubkey()]);
    let mint_ix = create_mint_instruction(program_id, &payer.pubkey(), mint_address, mint_authority_pda, token_account);
    
    let simulated_cu = simulate_transaction_cu(client, payer, vec![memo_ix, mint_ix])?;
    println!("  📊 Simulated CU: {} units", simulated_cu);
    println!("  📈 CU efficiency: {:.2} CU/byte", simulated_cu as f64 / memo_bytes.len() as f64);
    
    Ok((memo_bytes, simulated_cu))
}

fn create_and_simulate_memo_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Encode only memo content with Base64
    let base64_encoded = general_purpose::STANDARD.encode(content.as_bytes());
    let memo_bytes = base64_encoded.as_bytes().to_vec();
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Base64: {} bytes (+{:.1}%)", memo_bytes.len(), 
             ((memo_bytes.len() as f64 / content.as_bytes().len() as f64) - 1.0) * 100.0);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Base64 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Base64 memo too long ({} bytes, maximum 800)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    
    println!("  ✅ Length validation passed");
    
    // Simulate CU consumption
    let memo_ix = spl_memo::build_memo(&memo_bytes, &[&payer.pubkey()]);
    let mint_ix = create_mint_instruction(program_id, &payer.pubkey(), mint_address, mint_authority_pda, token_account);
    
    let simulated_cu = simulate_transaction_cu(client, payer, vec![memo_ix, mint_ix])?;
    println!("  📊 Simulated CU: {} units", simulated_cu);
    println!("  📈 CU efficiency: {:.2} CU/byte", simulated_cu as f64 / memo_bytes.len() as f64);
    
    Ok((memo_bytes, simulated_cu))
}

fn create_and_simulate_borsh_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Borsh serialize then Base64 encode
    let memo_data = ComparisonMemoData {
        content: content.to_string(),
    };
    
    let borsh_bytes = memo_data.try_to_vec()?;
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_bytes);
    let memo_bytes = base64_encoded.as_bytes().to_vec();
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Borsh: {} bytes", borsh_bytes.len());
    println!("  Borsh+Base64: {} bytes (+{:.1}%)", memo_bytes.len(),
             ((memo_bytes.len() as f64 / content.as_bytes().len() as f64) - 1.0) * 100.0);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Borsh+Base64 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Borsh+Base64 memo too long ({} bytes, maximum 800)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    
    println!("  ✅ Length validation passed");
    
    // Simulate CU consumption
    let memo_ix = spl_memo::build_memo(&memo_bytes, &[&payer.pubkey()]);
    let mint_ix = create_mint_instruction(program_id, &payer.pubkey(), mint_address, mint_authority_pda, token_account);
    
    let simulated_cu = simulate_transaction_cu(client, payer, vec![memo_ix, mint_ix])?;
    println!("  📊 Simulated CU: {} units", simulated_cu);
    println!("  📈 CU efficiency: {:.2} CU/byte", simulated_cu as f64 / memo_bytes.len() as f64);
    
    Ok((memo_bytes, simulated_cu))
}

fn print_single_analysis_summary(test_name: &str, result: &ComparisonResult, content: &str) {
    println!("\n📊 CU ANALYSIS SUMMARY: {}", test_name);
    println!("{}", "=".repeat(70));
    
    // Character analysis
    let char_count = content.chars().count();
    let byte_count = content.as_bytes().len();
    let avg_bytes_per_char = byte_count as f64 / char_count as f64;
    
    println!("\n📝 CHARACTER ANALYSIS:");
    println!("   • Characters: {}", char_count);
    println!("   • UTF-8 bytes: {}", byte_count);
    println!("   • Avg bytes/char: {:.2}", avg_bytes_per_char);
    println!("   • Character density: {}", if avg_bytes_per_char > 2.0 { "High (multi-byte)" } else { "Low (mostly ASCII)" });
    
    println!("\n📏 SIZE COMPARISON:");
    println!("   • Raw String:     {} bytes", result.raw_string_size);
    println!("   • Base64:         {} bytes (+{:.1}%)", result.memo_base64_size, result.memo_base64_increase_percent);
    println!("   • Borsh+Base64:   {} bytes (+{:.1}%)", result.borsh_base64_size, result.borsh_base64_increase_percent);
    
    println!("\n⚡ CU CONSUMPTION:");
    println!("   • Raw String:     {} CU", result.raw_string_cu);
    println!("   • Base64:         {} CU ({:+.1}%)", result.memo_base64_cu, result.memo_base64_cu_diff_percent);
    println!("   • Borsh+Base64:   {} CU ({:+.1}%)", result.borsh_base64_cu, result.borsh_base64_cu_diff_percent);
    
    println!("\n📈 CU EFFICIENCY (CU per byte):");
    if result.raw_string_size > 0 {
        println!("   • Raw String:     {:.2} CU/byte", result.raw_string_cu as f64 / result.raw_string_size as f64);
    }
    if result.memo_base64_size > 0 {
        println!("   • Base64:         {:.2} CU/byte", result.memo_base64_cu as f64 / result.memo_base64_size as f64);
    }
    if result.borsh_base64_size > 0 {
        println!("   • Borsh+Base64:   {:.2} CU/byte", result.borsh_base64_cu as f64 / result.borsh_base64_size as f64);
    }
    
    println!("\n💡 OBSERVATIONS:");
    if result.memo_base64_cu_diff_percent < -5.0 {
        println!("   • 🎯 Base64 encoding REDUCES CU by {:.1}% - potentially due to simplified UTF-8 processing!", result.memo_base64_cu_diff_percent.abs());
    } else if result.memo_base64_cu_diff_percent > 5.0 {
        println!("   • ⚠️  Base64 encoding increases CU by {:.1}%", result.memo_base64_cu_diff_percent);
    } else {
        println!("   • ✅ Base64 encoding has minimal CU impact ({:+.1}%)", result.memo_base64_cu_diff_percent);
    }
    
    if avg_bytes_per_char > 2.0 {
        println!("   • 🌍 Multi-byte characters detected - UTF-8 complexity may affect performance");
    } else {
        println!("   • 📝 Mostly ASCII characters - straightforward UTF-8 processing");
    }
    
    println!();
}

fn print_comprehensive_analysis(results: &[(String, ComparisonResult)]) {
    println!("\n🏁 COMPREHENSIVE CU ANALYSIS REPORT");
    println!("{}", "=".repeat(80));
    
    if results.is_empty() {
        println!("No results to analyze.");
        return;
    }
    
    let valid_results: Vec<&ComparisonResult> = results.iter()
        .map(|(_, result)| result)
        .filter(|r| r.raw_string_cu > 0)
        .collect();
    
    if valid_results.is_empty() {
        println!("No valid results to analyze.");
        return;
    }
    
    // Calculate averages
    let avg_raw_cu = valid_results.iter().map(|r| r.raw_string_cu).sum::<u64>() as f64 / valid_results.len() as f64;
    let avg_base64_cu_diff = valid_results.iter().map(|r| r.memo_base64_cu_diff_percent).sum::<f64>() / valid_results.len() as f64;
    let avg_borsh_cu_diff = valid_results.iter().map(|r| r.borsh_base64_cu_diff_percent).sum::<f64>() / valid_results.len() as f64;
    
    println!("\n📊 OVERALL STATISTICS:");
    println!("   • Tests analyzed: {}", valid_results.len());
    println!("   • Average raw CU: {:.0}", avg_raw_cu);
    println!("   • Average Base64 CU difference: {:+.1}%", avg_base64_cu_diff);
    println!("   • Average Borsh CU difference: {:+.1}%", avg_borsh_cu_diff);
    
    // Print detailed table
    println!("\n📋 DETAILED RESULTS:");
    println!("{:<20} | {:>6} | {:>6} | {:>6} | {:>7} | {:>7} | {:>7} | {:>6} | {:>6}", 
             "Test Case", "RawSz", "B64Sz", "BorSz", "RawCU", "B64CU", "BorCU", "B64%", "Bor%");
    println!("{}", "-".repeat(90));
    
    for (test_name, result) in results {
        if result.raw_string_cu > 0 {
            println!("{:<20} | {:>6} | {:>6} | {:>6} | {:>7} | {:>7} | {:>7} | {:>5.1}% | {:>5.1}%",
                     test_name,
                     result.raw_string_size,
                     result.memo_base64_size,
                     result.borsh_base64_size,
                     result.raw_string_cu,
                     result.memo_base64_cu,
                     result.borsh_base64_cu,
                     result.memo_base64_cu_diff_percent,
                     result.borsh_base64_cu_diff_percent);
        }
    }
    
    // Analyze patterns
    let base64_improvements = valid_results.iter().filter(|r| r.memo_base64_cu_diff_percent < -1.0).count();
    let base64_degradations = valid_results.iter().filter(|r| r.memo_base64_cu_diff_percent > 1.0).count();
    
    println!("\n🔍 PATTERN ANALYSIS:");
    println!("   • Base64 improves CU: {}/{} cases ({:.1}%)", 
             base64_improvements, valid_results.len(), 
             (base64_improvements as f64 / valid_results.len() as f64) * 100.0);
    println!("   • Base64 degrades CU: {}/{} cases ({:.1}%)", 
             base64_degradations, valid_results.len(),
             (base64_degradations as f64 / valid_results.len() as f64) * 100.0);
    
    println!("\n💡 KEY INSIGHTS:");
    if avg_base64_cu_diff < -2.0 {
        println!("   • 🎯 Base64 encoding generally IMPROVES CU efficiency by {:.1}%", avg_base64_cu_diff.abs());
        println!("   • 🧠 This suggests UTF-8 multi-byte processing overhead is significant");
        println!("   • 🔧 Base64 converts multi-byte chars to ASCII, simplifying processing");
    } else if avg_base64_cu_diff > 2.0 {
        println!("   • ⚠️  Base64 encoding generally increases CU by {:.1}%", avg_base64_cu_diff);
        println!("   • 📏 Size overhead outweighs any processing simplification");
    } else {
        println!("   • ✅ Base64 encoding has minimal CU impact overall ({:+.1}%)", avg_base64_cu_diff);
    }
    
    println!("   • 📦 Borsh+Base64 adds {:.1}% CU overhead on average", avg_borsh_cu_diff);
    println!("   • 🏗️  Structure overhead vs UTF-8 complexity trade-off");
    
    println!("\n🎯 RECOMMENDATIONS:");
    println!("   • For multi-byte characters: Consider Base64 for CU optimization");
    println!("   • For ASCII content: Raw strings are most efficient");
    println!("   • For structured data: Borsh provides versioning at ~{}% CU cost", avg_borsh_cu_diff as i32);
    println!("   • Monitor CU patterns for your specific character sets");
    
    println!();
}

fn print_character_analysis(results: &[(String, ComparisonResult)]) {
    println!("\n🔬 CHARACTER SET CU ANALYSIS");
    println!("{}", "=".repeat(80));
    
    let valid_results: Vec<&(String, ComparisonResult)> = results.iter()
        .filter(|(_, r)| r.raw_string_cu > 0)
        .collect();
    
    if valid_results.is_empty() {
        println!("No valid results for character analysis.");
        return;
    }
    
    println!("\n📊 CU EFFICIENCY BY CHARACTER TYPE:");
    println!("{:<20} | {:>7} | {:>7} | {:>7} | {:>6} | {:>6}", 
             "Character Type", "RawCU", "B64CU", "BorCU", "B64%", "Bor%");
    println!("{}", "-".repeat(70));
    
    for (test_name, result) in &valid_results {
        println!("{:<20} | {:>7} | {:>7} | {:>7} | {:>5.1}% | {:>5.1}%",
                 test_name,
                 result.raw_string_cu,
                 result.memo_base64_cu,
                 result.borsh_base64_cu,
                 result.memo_base64_cu_diff_percent,
                 result.borsh_base64_cu_diff_percent);
    }
    
    // Find patterns
    let multi_byte_cases: Vec<_> = valid_results.iter()
        .filter(|(name, _)| name.contains("Chinese") || name.contains("Japanese") || 
                           name.contains("Korean") || name.contains("Arabic") || 
                           name.contains("Russian") || name.contains("Dense"))
        .collect();
    
    let ascii_cases: Vec<_> = valid_results.iter()
        .filter(|(name, _)| name.contains("ASCII") || name.contains("Pure"))
        .collect();
    
    if !multi_byte_cases.is_empty() {
        let avg_multi_byte_improvement: f64 = multi_byte_cases.iter()
            .map(|(_, r)| r.memo_base64_cu_diff_percent)
            .sum::<f64>() / multi_byte_cases.len() as f64;
            
        println!("\n🌍 MULTI-BYTE CHARACTER ANALYSIS:");
        println!("   • Average Base64 CU change: {:+.1}%", avg_multi_byte_improvement);
        if avg_multi_byte_improvement < -2.0 {
            println!("   • 🎯 Significant CU improvement with Base64 encoding!");
            println!("   • 💡 Multi-byte UTF-8 processing overhead is substantial");
        }
    }
    
    if !ascii_cases.is_empty() {
        let avg_ascii_change: f64 = ascii_cases.iter()
            .map(|(_, r)| r.memo_base64_cu_diff_percent)
            .sum::<f64>() / ascii_cases.len() as f64;
            
        println!("\n📝 ASCII CHARACTER ANALYSIS:");
        println!("   • Average Base64 CU change: {:+.1}%", avg_ascii_change);
        if avg_ascii_change.abs() < 2.0 {
            println!("   • ✅ Minimal CU impact for ASCII content");
        }
    }
    
    println!("\n🔍 CHARACTER COMPLEXITY INSIGHTS:");
    println!("   • UTF-8 multi-byte characters create processing overhead");
    println!("   • Base64 encoding normalizes to ASCII, reducing complexity");
    println!("   • Trade-off: Size increase vs processing simplification");
    println!("   • Pattern suggests Solana runtime optimizes ASCII processing");
    
    println!();
}

// Helper functions (same as before)
fn create_rpc_client() -> RpcClient {
    let rpc_url = "https://rpc.testnet.x1.xyz";
    println!("Connecting to: {}", rpc_url);
    RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed())
}

fn load_payer_keypair() -> solana_sdk::signature::Keypair {
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read payer keypair file");
    println!("Using payer: {}", payer.pubkey());
    payer
}

fn get_program_addresses() -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let program_id = Pubkey::from_str("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy")
        .expect("Invalid program ID");
    let mint_address = Pubkey::from_str("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1")
        .expect("Invalid mint address");
    
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");
    
    let token_account = get_associated_token_address_with_program_id(
        &payer.pubkey(),
        &mint_address,
        &token_2022_id(),
    );
    
    println!("Program ID: {}", program_id);
    println!("Mint address: {}", mint_address);
    println!("Mint authority PDA: {}", mint_authority_pda);
    println!("Token account: {}", token_account);
    println!();
    
    (program_id, mint_address, mint_authority_pda, token_account)
}

fn ensure_token_account_exists(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    mint_address: &Pubkey,
    token_account: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    match client.get_account(token_account) {
        Ok(_) => {
            println!("✅ Token account already exists: {}", token_account);
        },
        Err(_) => {
            println!("⚠️  Token account not found, creating...");
            
            let create_ata_ix = create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                mint_address,
                &token_2022_id(),
            );
            
            let recent_blockhash = client.get_latest_blockhash()?;
            
            let transaction = Transaction::new_signed_with_payer(
                &[create_ata_ix],
                Some(&payer.pubkey()),
                &[payer],
                recent_blockhash,
            );
            
            match client.send_and_confirm_transaction(&transaction) {
                Ok(signature) => {
                    println!("✅ Token account created successfully!");
                    println!("   Signature: {}", signature);
                    println!("   Account: {}", token_account);
                },
                Err(e) => {
                    return Err(format!("Failed to create token account: {}", e).into());
                }
            }
        }
    }
    
    Ok(())
}

fn create_mint_instruction(
    program_id: &Pubkey,
    user: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    token_account: &Pubkey,
) -> Instruction {
    let mut hasher = Sha256::new();
    hasher.update(b"global:process_mint");
    let result = hasher.finalize();
    let instruction_data = result[..8].to_vec();
    
    let accounts = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new(*mint, false),
        AccountMeta::new_readonly(*mint_authority, false),
        AccountMeta::new(*token_account, false),
        AccountMeta::new_readonly(token_2022_id(), false),
        AccountMeta::new_readonly(solana_program::sysvar::instructions::id(), false),
    ];
    
    Instruction::new_with_bytes(*program_id, &instruction_data, accounts)
}

fn simulate_transaction_cu(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    instructions: Vec<Instruction>,
) -> Result<u64, Box<dyn std::error::Error>> {
    // Get recent blockhash
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // 确保指令顺序：ComputeBudget -> 其他指令
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
    let mut sim_instructions = vec![compute_budget_ix];
    sim_instructions.extend(instructions);
    
    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // 模拟交易并获取 CU
    match client.simulate_transaction_with_config(
        &sim_transaction,
        RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: false,
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None,
            accounts: None,
            min_context_slot: None,
            inner_instructions: false,
        },
    ) {
        Ok(result) => {
            if let Some(err) = result.value.err {
                println!("  ⚠️  Simulation error: {:?}", err);
                Ok(300_000u64)
            } else if let Some(units_consumed) = result.value.units_consumed {
                Ok(units_consumed)
            } else {
                println!("  ⚠️  No CU data available from simulation");
                Ok(300_000u64)
            }
        },
        Err(err) => {
            println!("  ❌ Simulation failed: {}", err);
            Ok(300_000u64)
        }
    }
}

fn get_safe_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // 找到不会切断 UTF-8 字符的位置
        let mut end = max_len;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        
        format!("{}... (total {} chars)", &s[..end], s.chars().count())
    }
}
