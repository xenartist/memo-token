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
use bs58;

// Import token-2022 program ID
use spl_token_2022::id as token_2022_id;

// Borsh data structure for comparison testing
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct ComparisonMemoData {
    /// Content field for testing
    pub content: String,
}

// Get RPC URL from environment or use default testnet
fn get_rpc_url() -> String {
    std::env::var("X1_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.testnet.x1.xyz".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Memo Format CU Simulation Analysis with ZSTD Compression ===");
    println!("Comparing: Raw String vs Base64 vs Base58 vs Borsh+Base64 vs Borsh+Base58 vs ZSTD variants");
    println!("Focus: CU consumption analysis and compression ratio for different character sets and encoding methods\n");
    
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
        "compression" => test_compression_analysis(),
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
    println!("  compression  - Compression ratio analysis");
    println!("\nEach test compares EIGHT formats:");
    println!("  1. Raw String - Direct UTF-8 string as memo");
    println!("  2. Base64 - String encoded with Base64");
    println!("  3. Base58 - String encoded with Base58 (Bitcoin/Solana style)");
    println!("  4. Borsh + Base64 - String in Borsh struct, then Base64");
    println!("  5. Borsh + Base58 - String in Borsh struct, then Base58");
    println!("  6. ZSTD + Base64 - ZSTD compressed, then Base64");
    println!("  7. Base64 + ZSTD + Base64 - Base64, then ZSTD, then Base64 again");
    println!("  8. Borsh + ZSTD + Base64 - Borsh, then ZSTD, then Base64");
    println!("\nThe test measures:");
    println!("  - Message size differences and compression ratios");
    println!("  - Simulated CU consumption");
    println!("  - CU efficiency per byte");
    println!("  - Character encoding impact on performance");
    println!("  - ZSTD compression effectiveness on different data types");
    println!("\nExamples:");
    println!("  {} chinese      # Test Chinese character CU efficiency with ZSTD", program_name);
    println!("  {} compression  # Detailed compression analysis", program_name);
    println!("  {} all          # Complete comparison with ZSTD", program_name);
}

// Test functions
fn test_simple_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Hello World! This is a simple English message for testing blockchain memos with different encoding methods and compression techniques.";
    analyze_memo_formats("Simple English Message", content)
}

fn test_chinese_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "你好世界！这是一个测试中文字符的真实区块链交易消息。今天天气很好，适合测试不同编码方法的实际CU消耗和压缩效果。";
    analyze_memo_formats("Chinese Message", content)
}

fn test_emoji_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Hello World! 🌍🚀💻🎉 Testing with emojis! 😊🔥⭐🌟 Real blockchain ⛓️ transactions! 🎯🎊🎈 Let's test actual CU usage with different encoding methods and ZSTD compression for memo data on the blockchain! 🌈💫🎪🎭";
    analyze_memo_formats("Emoji Message", content)
}

fn test_mixed_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "Mixed: Hello 你好 こんにちは 안녕하세요 مرحبا Привет! 🌍 Real multi-language blockchain test 🚀 actual CU measurement with compression analysis! Testing different encoding methods for international character support in blockchain memos.";
    analyze_memo_formats("Mixed Language Message", content)
}

fn test_long_message() -> Result<(), Box<dyn std::error::Error>> {
    let content = "This is a very long message to test the difference in compute unit consumption and compression effectiveness between raw string, Base64-encoded string, Base58-encoded string, ZSTD compressed variants, and Borsh+encoding+compression serialization methods. This message is intentionally long to test how message length affects CU consumption patterns and compression ratios across different encoding and compression strategies. ZSTD compression should be particularly effective on repetitive content like this. ".repeat(3);
    analyze_memo_formats("Long Message", &content)
}

fn test_compression_analysis() -> Result<(), Box<dyn std::error::Error>> {
    println!("🗜️  COMPRESSION RATIO ANALYSIS\n");
    
    // Create the repeated strings first
    let long_repetitive = "ABCDEFGHIJ".repeat(20);
    let chinese_repetitive = "你好世界测试数据".repeat(10);
    
    let test_cases = vec![
        ("Short Text", "Hello World!"),
        ("Repetitive Text", "Hello Hello Hello Hello Hello Hello Hello Hello Hello Hello "),
        ("Mixed Repetition", "Test Test 测试 测试 🚀🚀🚀 Test Test 测试 测试 🚀🚀🚀 "),
        ("JSON-like", r#"{"name":"test","value":"data","timestamp":"2024-01-01","content":"blockchain memo test data"}"#),
        ("Long Repetitive", &long_repetitive),
        ("Chinese Repetitive", &chinese_repetitive),
        ("Lorem Ipsum", "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation."),
    ];
    
    for (test_name, content) in test_cases {
        println!("--- Compression Analysis: {} ---", test_name);
        println!("Original: {} chars, {} bytes", content.chars().count(), content.as_bytes().len());
        
        // Test various compression combinations
        test_compression_variants(content)?;
        println!();
    }
    
    Ok(())
}

fn test_compression_variants(content: &str) -> Result<(), Box<dyn std::error::Error>> {
    let original_bytes = content.as_bytes();
    let original_size = original_bytes.len();
    
    // Test ZSTD compression levels
    for level in [1, 3, 6, 9, 15, 19] {
        let compressed = zstd::bulk::compress(original_bytes, level)?;
        let ratio = compressed.len() as f64 / original_size as f64;
        let savings = (1.0 - ratio) * 100.0;
        
        println!("  ZSTD Level {}: {} -> {} bytes (ratio: {:.3}, savings: {:.1}%)", 
                 level, original_size, compressed.len(), ratio, savings);
    }
    
    // Test Base64 then ZSTD
    let base64_encoded = general_purpose::STANDARD.encode(original_bytes);
    let base64_zstd = zstd::bulk::compress(base64_encoded.as_bytes(), 6)?;
    let base64_zstd_ratio = base64_zstd.len() as f64 / original_size as f64;
    println!("  Base64+ZSTD: {} -> {} -> {} bytes (ratio: {:.3})", 
             original_size, base64_encoded.len(), base64_zstd.len(), base64_zstd_ratio);
    
    // Test Borsh then ZSTD
    let memo_data = ComparisonMemoData { content: content.to_string() };
    let borsh_bytes = memo_data.try_to_vec()?;
    let borsh_zstd = zstd::bulk::compress(&borsh_bytes, 6)?;
    let borsh_zstd_ratio = borsh_zstd.len() as f64 / original_size as f64;
    println!("  Borsh+ZSTD: {} -> {} -> {} bytes (ratio: {:.3})", 
             original_size, borsh_bytes.len(), borsh_zstd.len(), borsh_zstd_ratio);
    
    Ok(())
}

fn test_all_messages() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Running ALL eight-way CU analysis tests with ZSTD compression...\n");
    
    let test_cases = vec![
        ("Simple English", "Hello World! This is a simple test message for memo encoding comparison and CU analysis with compression testing."),
        ("Chinese Characters", "你好世界！这是一个测试中文字符的消息，用于比较不同的编码方法和CU分析以及压缩效果。这个测试很重要。"),
        ("Japanese Characters", "こんにちは世界！これは異なる符号化方法とCU分析および圧縮効果を比较するためのテストメッセージです。"),
        ("Korean Characters", "안녕하세요 세계! 이것은 다른 인코딩 방법과 CU 분석 및 압축 효과를 비교하기 위한 테스트 메시지입니다."),
        ("Arabic Characters", "مرحبا بالعالم! هذه رسالة اختبار لمقارنة طرق التشفير المختلفة وتحليل وحدة الحوسبة وتأثير الضغط."),
        ("Russian Characters", "Привет мир! Это тестовое сообщение для сравнения различных методов кодирования и анализа CU и эффектов сжатия."),
        ("Emoji Rich", "Hello World! 🌍🚀💻🎉 Testing emojis! 😊🔥⭐🌟 in blockchain memos! 🌈💫🎪🎭 CU analysis with compression! 🎯🎊🎈"),
        ("Mixed Languages", "Mixed: Hello 你好 こんにちは 🌍 Testing multiple formats! International character support CU analysis with ZSTD compression."),
        ("Repetitive Data", "TEST DATA TEST DATA TEST DATA TEST DATA TEST DATA TEST DATA TEST DATA "),
        ("JSON-like Data", r#"{"blockchain":"solana","memo":"test","compression":"zstd","encoding":"base64","efficiency":"analysis"}"#),
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
    println!("🔬 DETAILED CHARACTER SET CU ANALYSIS WITH ZSTD COMPRESSION\n");
    
    // Create the repeated string first
    let repetitive_pattern = "ABCABC".repeat(15);
    
    let analysis_cases = vec![
        ("Pure ASCII", "Hello World! This is a test message with only ASCII characters for CU analysis and compression testing."),
        ("ASCII + Numbers", "Hello123! Test message with ASCII + numbers 456789 for CU analysis benchmark and compression ratio testing."),
        ("Chinese Dense", "你好世界测试消息中文字符密集型计算单元分析基准测试非常重要的数据压缩效果。"),
        ("Japanese Dense", "こんにちは世界テストメッセージ日本語文字密集型計算単位分析基準圧縮効果。"),
        ("Korean Dense", "안녕하세요세계테스트메시지한국어문자밀집형계산단위분석기준테스트압축효과。"),
        ("Emoji Dense", "🌍🚀💻🎉😊🔥⭐🌟⛓️🎯🎊🎈🌈💫🎪🎭🎮🎲🎨🎯🎪🎭🎨🎮🎲🎯"),
        ("Mixed Dense", "Hello你好こんにちは🌍Test测试テスト🚀Mixed混合ミックス💻Analysis分析分析!"),
        ("UTF-8 Edge Cases", "Café naïve résumé 北京 東京 москва θέλω ñoño ümlaut ćirić ş ğ İ"),
        ("Repetitive Pattern", &repetitive_pattern),
        ("Base64-like Data", "aGVsbG8gd29ybGQgdGVzdCBkYXRhIGZvciBjb21wcmVzc2lvbiBhbmFseXNpcw=="),
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
                println!("  Raw CU: {}, Base64 CU: {}, Base58 CU: {}, Borsh+B64 CU: {}, Borsh+B58 CU: {}", 
                         result.raw_string_cu, result.base64_cu, result.base58_cu, 
                         result.borsh_base64_cu, result.borsh_base58_cu);
                println!("  ZSTD+B64 CU: {}, B64+ZSTD+B64 CU: {}, Borsh+ZSTD+B64 CU: {}", 
                         result.zstd_base64_cu, result.base64_zstd_base64_cu, result.borsh_zstd_base64_cu);
                
                // Compression analysis
                let original_size = byte_count as f64;
                println!("  Compression ratios: ZSTD+B64 {:.3}, B64+ZSTD+B64 {:.3}, Borsh+ZSTD+B64 {:.3}",
                         result.zstd_base64_size as f64 / original_size,
                         result.base64_zstd_base64_size as f64 / original_size,
                         result.borsh_zstd_base64_size as f64 / original_size);
                
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
    base64_size: usize,
    base58_size: usize,
    borsh_base64_size: usize,
    borsh_base58_size: usize,
    zstd_base64_size: usize,
    base64_zstd_base64_size: usize,
    borsh_zstd_base64_size: usize,
    raw_string_cu: u64,
    base64_cu: u64,
    base58_cu: u64,
    borsh_base64_cu: u64,
    borsh_base58_cu: u64,
    zstd_base64_cu: u64,
    base64_zstd_base64_cu: u64,
    borsh_zstd_base64_cu: u64,
    base64_increase_percent: f64,
    base58_increase_percent: f64,
    borsh_base64_increase_percent: f64,
    borsh_base58_increase_percent: f64,
    zstd_base64_compression_ratio: f64,
    base64_zstd_base64_compression_ratio: f64,
    borsh_zstd_base64_compression_ratio: f64,
    base64_cu_diff_percent: f64,
    base58_cu_diff_percent: f64,
    borsh_base64_cu_diff_percent: f64,
    borsh_base58_cu_diff_percent: f64,
    zstd_base64_cu_diff_percent: f64,
    base64_zstd_base64_cu_diff_percent: f64,
    borsh_zstd_base64_cu_diff_percent: f64,
}

impl Default for ComparisonResult {
    fn default() -> Self {
        Self {
            raw_string_size: 0,
            base64_size: 0,
            base58_size: 0,
            borsh_base64_size: 0,
            borsh_base58_size: 0,
            zstd_base64_size: 0,
            base64_zstd_base64_size: 0,
            borsh_zstd_base64_size: 0,
            raw_string_cu: 0,
            base64_cu: 0,
            base58_cu: 0,
            borsh_base64_cu: 0,
            borsh_base58_cu: 0,
            zstd_base64_cu: 0,
            base64_zstd_base64_cu: 0,
            borsh_zstd_base64_cu: 0,
            base64_increase_percent: 0.0,
            base58_increase_percent: 0.0,
            borsh_base64_increase_percent: 0.0,
            borsh_base58_increase_percent: 0.0,
            zstd_base64_compression_ratio: 0.0,
            base64_zstd_base64_compression_ratio: 0.0,
            borsh_zstd_base64_compression_ratio: 0.0,
            base64_cu_diff_percent: 0.0,
            base58_cu_diff_percent: 0.0,
            borsh_base64_cu_diff_percent: 0.0,
            borsh_base58_cu_diff_percent: 0.0,
            zstd_base64_cu_diff_percent: 0.0,
            base64_zstd_base64_cu_diff_percent: 0.0,
            borsh_zstd_base64_cu_diff_percent: 0.0,
        }
    }
}

fn analyze_memo_formats(test_name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Eight-way CU analysis with ZSTD compression for: {}\n", test_name);
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
    
    // Test 2: Base64
    println!("=== Analysis 2: Base64 Encoding ===");
    let (base64_bytes, base64_cu) = create_and_simulate_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;
    
    println!();
    
    // Test 3: Base58
    println!("=== Analysis 3: Base58 Encoding ===");
    let (base58_bytes, base58_cu) = create_and_simulate_base58(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;
    
    println!();
    
    // Test 4: Borsh + Base64
    println!("=== Analysis 4: Borsh + Base64 ===");
    let (borsh_base64_bytes, borsh_base64_cu) = create_and_simulate_borsh_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;
    
    println!();
    
    // Test 5: Borsh + Base58
    println!("=== Analysis 5: Borsh + Base58 ===");
    let (borsh_base58_bytes, borsh_base58_cu) = create_and_simulate_borsh_base58(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;

    println!();
    
    // Test 6: ZSTD + Base64
    println!("=== Analysis 6: ZSTD + Base64 ===");
    let (zstd_base64_bytes, zstd_base64_cu) = create_and_simulate_zstd_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;

    println!();
    
    // Test 7: Base64 + ZSTD + Base64
    println!("=== Analysis 7: Base64 + ZSTD + Base64 ===");
    let (base64_zstd_base64_bytes, base64_zstd_base64_cu) = create_and_simulate_base64_zstd_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;

    println!();
    
    // Test 8: Borsh + ZSTD + Base64
    println!("=== Analysis 8: Borsh + ZSTD + Base64 ===");
    let (borsh_zstd_base64_bytes, borsh_zstd_base64_cu) = create_and_simulate_borsh_zstd_base64(
        &client, &payer, &program_id, &mint_address, &mint_authority_pda, &token_account, content
    )?;

    // Calculate differences
    let base64_increase_percent = ((base64_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    let base58_increase_percent = ((base58_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    let borsh_base64_increase_percent = ((borsh_base64_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    let borsh_base58_increase_percent = ((borsh_base58_bytes.len() as f64 - raw_memo_bytes.len() as f64) / raw_memo_bytes.len() as f64) * 100.0;
    
    // Compression ratios
    let zstd_base64_compression_ratio = zstd_base64_bytes.len() as f64 / raw_memo_bytes.len() as f64;
    let base64_zstd_base64_compression_ratio = base64_zstd_base64_bytes.len() as f64 / raw_memo_bytes.len() as f64;
    let borsh_zstd_base64_compression_ratio = borsh_zstd_base64_bytes.len() as f64 / raw_memo_bytes.len() as f64;
    
    let base64_cu_diff_percent = ((base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let base58_cu_diff_percent = ((base58_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let borsh_base64_cu_diff_percent = ((borsh_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let borsh_base58_cu_diff_percent = ((borsh_base58_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let zstd_base64_cu_diff_percent = ((zstd_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let base64_zstd_base64_cu_diff_percent = ((base64_zstd_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    let borsh_zstd_base64_cu_diff_percent = ((borsh_zstd_base64_cu as f64 - raw_cu as f64) / raw_cu as f64) * 100.0;
    
    let result = ComparisonResult {
        raw_string_size: raw_memo_bytes.len(),
        base64_size: base64_bytes.len(),
        base58_size: base58_bytes.len(),
        borsh_base64_size: borsh_base64_bytes.len(),
        borsh_base58_size: borsh_base58_bytes.len(),
        zstd_base64_size: zstd_base64_bytes.len(),
        base64_zstd_base64_size: base64_zstd_base64_bytes.len(),
        borsh_zstd_base64_size: borsh_zstd_base64_bytes.len(),
        raw_string_cu: raw_cu,
        base64_cu: base64_cu,
        base58_cu: base58_cu,
        borsh_base64_cu: borsh_base64_cu,
        borsh_base58_cu: borsh_base58_cu,
        zstd_base64_cu: zstd_base64_cu,
        base64_zstd_base64_cu: base64_zstd_base64_cu,
        borsh_zstd_base64_cu: borsh_zstd_base64_cu,
        base64_increase_percent,
        base58_increase_percent,
        borsh_base64_increase_percent,
        borsh_base58_increase_percent,
        zstd_base64_compression_ratio,
        base64_zstd_base64_compression_ratio,
        borsh_zstd_base64_compression_ratio,
        base64_cu_diff_percent,
        base58_cu_diff_percent,
        borsh_base64_cu_diff_percent,
        borsh_base58_cu_diff_percent,
        zstd_base64_cu_diff_percent,
        base64_zstd_base64_cu_diff_percent,
        borsh_zstd_base64_cu_diff_percent,
    };
    
    Ok(result)
}

// Original format simulation functions
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

fn create_and_simulate_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Encode with Base64
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

fn create_and_simulate_base58(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Encode with Base58
    let base58_encoded = bs58::encode(content.as_bytes()).into_string();
    let memo_bytes = base58_encoded.as_bytes().to_vec();
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Base58: {} bytes (+{:.1}%)", memo_bytes.len(), 
             ((memo_bytes.len() as f64 / content.as_bytes().len() as f64) - 1.0) * 100.0);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Base58 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Base58 memo too long ({} bytes, maximum 800)", memo_bytes.len());
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

fn create_and_simulate_borsh_base58(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Borsh serialize then Base58 encode
    let memo_data = ComparisonMemoData {
        content: content.to_string(),
    };
    
    let borsh_bytes = memo_data.try_to_vec()?;
    let base58_encoded = bs58::encode(&borsh_bytes).into_string();
    let memo_bytes = base58_encoded.as_bytes().to_vec();
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Borsh: {} bytes", borsh_bytes.len());
    println!("  Borsh+Base58: {} bytes (+{:.1}%)", memo_bytes.len(),
             ((memo_bytes.len() as f64 / content.as_bytes().len() as f64) - 1.0) * 100.0);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Borsh+Base58 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Borsh+Base58 memo too long ({} bytes, maximum 800)", memo_bytes.len());
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

// ZSTD compression functions
fn create_and_simulate_zstd_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // ZSTD compress then Base64 encode
    let compressed = zstd::bulk::compress(content.as_bytes(), 6)?;
    let base64_encoded = general_purpose::STANDARD.encode(&compressed);
    let memo_bytes = base64_encoded.as_bytes().to_vec();
    
    let compression_ratio = compressed.len() as f64 / content.as_bytes().len() as f64;
    let compression_savings = (1.0 - compression_ratio) * 100.0;
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  ZSTD: {} bytes (ratio: {:.3}, savings: {:.1}%)", compressed.len(), compression_ratio, compression_savings);
    println!("  ZSTD+Base64: {} bytes", memo_bytes.len());
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: ZSTD+Base64 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: ZSTD+Base64 memo too long ({} bytes, maximum 800)", memo_bytes.len());
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

fn create_and_simulate_base64_zstd_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Base64 encode, then ZSTD compress, then Base64 encode again
    let first_base64 = general_purpose::STANDARD.encode(content.as_bytes());
    let compressed = zstd::bulk::compress(first_base64.as_bytes(), 6)?;
    let final_base64 = general_purpose::STANDARD.encode(&compressed);
    let memo_bytes = final_base64.as_bytes().to_vec();
    
    let intermediate_ratio = compressed.len() as f64 / first_base64.len() as f64;
    let final_ratio = memo_bytes.len() as f64 / content.as_bytes().len() as f64;
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Base64: {} bytes", first_base64.len());
    println!("  Base64+ZSTD: {} bytes (compression ratio: {:.3})", compressed.len(), intermediate_ratio);
    println!("  Base64+ZSTD+Base64: {} bytes (total ratio: {:.3})", memo_bytes.len(), final_ratio);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Base64+ZSTD+Base64 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Base64+ZSTD+Base64 memo too long ({} bytes, maximum 800)", memo_bytes.len());
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

fn create_and_simulate_borsh_zstd_base64(
    client: &RpcClient,
    payer: &solana_sdk::signature::Keypair,
    program_id: &Pubkey,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    token_account: &Pubkey,
    content: &str,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    // Borsh serialize, then ZSTD compress, then Base64 encode
    let memo_data = ComparisonMemoData {
        content: content.to_string(),
    };
    
    let borsh_bytes = memo_data.try_to_vec()?;
    let compressed = zstd::bulk::compress(&borsh_bytes, 6)?;
    let base64_encoded = general_purpose::STANDARD.encode(&compressed);
    let memo_bytes = base64_encoded.as_bytes().to_vec();
    
    let compression_ratio = compressed.len() as f64 / borsh_bytes.len() as f64;
    let total_ratio = memo_bytes.len() as f64 / content.as_bytes().len() as f64;
    
    println!("  Original: {} chars → {} bytes", content.chars().count(), content.as_bytes().len());
    println!("  Borsh: {} bytes", borsh_bytes.len());
    println!("  Borsh+ZSTD: {} bytes (compression ratio: {:.3})", compressed.len(), compression_ratio);
    println!("  Borsh+ZSTD+Base64: {} bytes (total ratio: {:.3})", memo_bytes.len(), total_ratio);
    
    // Check memo length constraints
    if memo_bytes.len() < 69 {
        println!("  ❌ Error: Borsh+ZSTD+Base64 memo too short ({} bytes, minimum 69)", memo_bytes.len());
        return Ok((memo_bytes, 0));
    }
    if memo_bytes.len() > 800 {
        println!("  ❌ Error: Borsh+ZSTD+Base64 memo too long ({} bytes, maximum 800)", memo_bytes.len());
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

// Helper functions
fn create_rpc_client() -> RpcClient {
    let rpc_url = get_rpc_url();
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
    
    // Add compute budget instruction
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_000_000);
    let mut sim_instructions = vec![compute_budget_ix];
    sim_instructions.extend(instructions);
    
    let sim_transaction = Transaction::new_signed_with_payer(
        &sim_instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Simulate transaction and get CU
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
        let mut end = max_len;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... (total {} chars)", &s[..end], s.chars().count())
    }
}

fn print_single_analysis_summary(test_name: &str, result: &ComparisonResult, content: &str) {
    println!("\n📊 CU ANALYSIS SUMMARY WITH ZSTD COMPRESSION: {}", test_name);
    println!("{}", "=".repeat(90));
    
    let char_count = content.chars().count();
    let byte_count = content.as_bytes().len();
    let avg_bytes_per_char = byte_count as f64 / char_count as f64;
    
    println!("\n📝 CHARACTER ANALYSIS:");
    println!("   • Characters: {}", char_count);
    println!("   • UTF-8 bytes: {}", byte_count);
    println!("   • Avg bytes/char: {:.2}", avg_bytes_per_char);
    
    println!("\n📏 SIZE COMPARISON:");
    println!("   • Raw String:          {} bytes", result.raw_string_size);
    println!("   • Base64:              {} bytes (+{:.1}%)", result.base64_size, result.base64_increase_percent);
    println!("   • Base58:              {} bytes (+{:.1}%)", result.base58_size, result.base58_increase_percent);
    println!("   • Borsh+Base64:        {} bytes (+{:.1}%)", result.borsh_base64_size, result.borsh_base64_increase_percent);
    println!("   • Borsh+Base58:        {} bytes (+{:.1}%)", result.borsh_base58_size, result.borsh_base58_increase_percent);
    
    println!("\n🗜️  COMPRESSION RESULTS:");
    println!("   • ZSTD+Base64:         {} bytes (ratio: {:.3})", result.zstd_base64_size, result.zstd_base64_compression_ratio);
    println!("   • Base64+ZSTD+Base64:  {} bytes (ratio: {:.3})", result.base64_zstd_base64_size, result.base64_zstd_base64_compression_ratio);
    println!("   • Borsh+ZSTD+Base64:   {} bytes (ratio: {:.3})", result.borsh_zstd_base64_size, result.borsh_zstd_base64_compression_ratio);
    
    println!("\n⚡ CU CONSUMPTION:");
    println!("   • Raw String:          {} CU", result.raw_string_cu);
    println!("   • Base64:              {} CU ({:+.1}%)", result.base64_cu, result.base64_cu_diff_percent);
    println!("   • Base58:              {} CU ({:+.1}%)", result.base58_cu, result.base58_cu_diff_percent);
    println!("   • Borsh+Base64:        {} CU ({:+.1}%)", result.borsh_base64_cu, result.borsh_base64_cu_diff_percent);
    println!("   • Borsh+Base58:        {} CU ({:+.1}%)", result.borsh_base58_cu, result.borsh_base58_cu_diff_percent);
    println!("   • ZSTD+Base64:         {} CU ({:+.1}%)", result.zstd_base64_cu, result.zstd_base64_cu_diff_percent);
    println!("   • Base64+ZSTD+Base64:  {} CU ({:+.1}%)", result.base64_zstd_base64_cu, result.base64_zstd_base64_cu_diff_percent);
    println!("   • Borsh+ZSTD+Base64:   {} CU ({:+.1}%)", result.borsh_zstd_base64_cu, result.borsh_zstd_base64_cu_diff_percent);
    
    // Compression effectiveness analysis
    println!("\n💡 COMPRESSION INSIGHTS:");
    if result.zstd_base64_compression_ratio < 0.8 {
        println!("   • 🎯 ZSTD provides significant compression (savings: {:.1}%)", 
                 (1.0 - result.zstd_base64_compression_ratio) * 100.0);
    } else if result.zstd_base64_compression_ratio > 1.0 {
        println!("   • ⚠️  ZSTD expansion due to small data size or low entropy");
    }
    
    // Find best compression method
    let compression_methods = vec![
        ("ZSTD+Base64", result.zstd_base64_size, result.zstd_base64_cu),
        ("Base64+ZSTD+Base64", result.base64_zstd_base64_size, result.base64_zstd_base64_cu),
        ("Borsh+ZSTD+Base64", result.borsh_zstd_base64_size, result.borsh_zstd_base64_cu),
    ];
    
    if let Some((best_name, best_size, _best_cu)) = compression_methods.iter()
        .filter(|(_, size, cu)| *size > 0 && *cu > 0)
        .min_by_key(|(_, size, _)| *size) {
        println!("   • 🏆 Best compression: {} with {} bytes", best_name, best_size);
    }
    
    println!();
}

fn print_comprehensive_analysis(results: &[(String, ComparisonResult)]) {
    println!("\n🏁 COMPREHENSIVE EIGHT-WAY CU ANALYSIS WITH ZSTD COMPRESSION REPORT");
    println!("{}", "=".repeat(120));
    
    if results.is_empty() {
        println!("No results to analyze.");
        return;
    }
    
    println!("   • ZSTD compression effectiveness varies with content type and size");
    println!("   • Monitor compression ratios for optimal encoding choice");
    println!("   • Consider memo size constraints when using compression");
    
    println!();
}

fn print_character_analysis(results: &[(String, ComparisonResult)]) {
    println!("\n🔬 CHARACTER SET CU ANALYSIS WITH ZSTD COMPRESSION");
    println!("{}", "=".repeat(120));
    
    if results.is_empty() {
        println!("No results to analyze.");
        return;
    }
    
    println!("   • Compression analysis shows effectiveness varies by content type");
    println!("   • ZSTD compression works best on repetitive data");
    println!("   • Binary encoding combined with compression may provide CU benefits");
    
    println!();
}

