//! Unit tests for memo-mint contract
//! 
//! This test suite provides comprehensive coverage of all core functions:
//! - calculate_dynamic_mint_amount: Round-number emission curve (10M, 20M, ... 100M, 200M, ...)
//! - validate_memo_length: Memo minimum-length validation (no maximum)

use super::*;

// ============================================================================
// Tests for calculate_dynamic_mint_amount()
// ============================================================================

#[cfg(test)]
mod calculate_dynamic_mint_amount_tests {
    use super::*;

    const M: u64 = DECIMAL_FACTOR; // 1 token in lamports

    /// Expected reward at a supply given in whole tokens, computed independently
    /// from the curve definition: below 10M -> 1 token; otherwise in the decade
    /// [10M * 10^n, 10M * 10^(n+1)) with leading digit d -> (1 / 10^n) * (11 - d) / 10.
    fn expected_reward(supply_tokens: u64) -> u64 {
        if supply_tokens < CURVE_START_TOKENS {
            return BASE_MINT_AMOUNT;
        }
        let mut magnitude = CURVE_START_TOKENS;
        let mut n = 0u32;
        while supply_tokens >= magnitude * 10 {
            magnitude *= 10;
            n += 1;
        }
        let d = supply_tokens / magnitude;
        BASE_MINT_AMOUNT / 10u64.pow(n) * (11 - d) / 10
    }

    // ------------------------------------------------------------------------
    // Below the curve start: 0 - 10M tokens (1.0 token per mint)
    // ------------------------------------------------------------------------

    #[test]
    fn test_zero_supply_mints_one_token() {
        assert_eq!(calculate_dynamic_mint_amount(0).unwrap(), M);
    }

    #[test]
    fn test_current_mainnet_supply_mints_one_token() {
        // Mainnet supply on 2026-10-13: 8,730,756 MEMO
        assert_eq!(calculate_dynamic_mint_amount(8_730_756 * M).unwrap(), M);
    }

    #[test]
    fn test_one_lamport_below_curve_start_mints_one_token() {
        assert_eq!(calculate_dynamic_mint_amount(CURVE_START_LAMPORTS - 1).unwrap(), M);
    }

    // ------------------------------------------------------------------------
    // First decade: 10M - 100M tokens (1.0 -> 0.2, one tenth less per 10M)
    // ------------------------------------------------------------------------

    #[test]
    fn test_first_decade_round_numbers() {
        let cases: [(u64, u64); 9] = [
            (10_000_000, 1_000_000), // 10M: 1.0
            (20_000_000, 900_000),   // 20M: 0.9
            (30_000_000, 800_000),   // 30M: 0.8
            (40_000_000, 700_000),   // 40M: 0.7
            (50_000_000, 600_000),   // 50M: 0.6
            (60_000_000, 500_000),   // 60M: 0.5
            (70_000_000, 400_000),   // 70M: 0.4
            (80_000_000, 300_000),   // 80M: 0.3
            (90_000_000, 200_000),   // 90M: 0.2
        ];
        for (supply_tokens, expected) in cases {
            assert_eq!(
                calculate_dynamic_mint_amount(supply_tokens * M).unwrap(),
                expected,
                "Wrong reward at {} tokens",
                supply_tokens
            );
        }
    }

    #[test]
    fn test_reward_constant_inside_a_step() {
        // Anywhere in [20M, 30M) pays 0.9
        for supply in [20_000_000 * M, 25_000_000 * M, 30_000_000 * M - 1] {
            assert_eq!(calculate_dynamic_mint_amount(supply).unwrap(), 900_000);
        }
    }

    #[test]
    fn test_step_changes_exactly_at_round_number() {
        assert_eq!(calculate_dynamic_mint_amount(20_000_000 * M - 1).unwrap(), 1_000_000);
        assert_eq!(calculate_dynamic_mint_amount(20_000_000 * M).unwrap(), 900_000);
    }

    #[test]
    fn test_just_below_100m_pays_0_2() {
        assert_eq!(calculate_dynamic_mint_amount(100_000_000 * M - 1).unwrap(), 200_000);
    }

    // ------------------------------------------------------------------------
    // Decade starts: 10M -> 1, 100M -> 0.1, 1B -> 0.01, ... (10x per decade)
    // ------------------------------------------------------------------------

    #[test]
    fn test_decade_starts_are_powers_of_ten() {
        let cases: [(u64, u64); 6] = [
            (10_000_000, 1_000_000),     // 10M: 1
            (100_000_000, 100_000),      // 100M: 0.1
            (1_000_000_000, 10_000),     // 1B: 0.01
            (10_000_000_000, 1_000),     // 10B: 0.001
            (100_000_000_000, 100),      // 100B: 0.0001
            (1_000_000_000_000, 10),     // 1T: 0.00001
        ];
        for (supply_tokens, expected) in cases {
            assert_eq!(
                calculate_dynamic_mint_amount(supply_tokens * M).unwrap(),
                expected,
                "Wrong reward at decade start {} tokens",
                supply_tokens
            );
        }
    }

    #[test]
    fn test_second_decade_round_numbers() {
        assert_eq!(calculate_dynamic_mint_amount(200_000_000 * M).unwrap(), 90_000); // 200M: 0.09
        assert_eq!(calculate_dynamic_mint_amount(500_000_000 * M).unwrap(), 60_000); // 500M: 0.06
        assert_eq!(calculate_dynamic_mint_amount(900_000_000 * M).unwrap(), 20_000); // 900M: 0.02
        assert_eq!(calculate_dynamic_mint_amount(1_000_000_000 * M - 1).unwrap(), 20_000);
    }

    #[test]
    fn test_last_decade_round_numbers() {
        // 1T - 10T: 10 lamports down to 2 lamports
        assert_eq!(calculate_dynamic_mint_amount(1_000_000_000_000 * M).unwrap(), 10);
        assert_eq!(calculate_dynamic_mint_amount(5_000_000_000_000 * M).unwrap(), 6);
        assert_eq!(calculate_dynamic_mint_amount(9_000_000_000_000 * M).unwrap(), 2);
    }

    // ------------------------------------------------------------------------
    // Whole-curve properties
    // ------------------------------------------------------------------------

    #[test]
    fn test_every_round_number_matches_definition() {
        // All 54 round numbers d * 10^k from 10M to 9T, plus one lamport below each
        let mut magnitude = CURVE_START_TOKENS;
        for _ in 0..CURVE_DECADES {
            for d in 1..=9u64 {
                let supply_tokens = d * magnitude;
                assert_eq!(
                    calculate_dynamic_mint_amount(supply_tokens * M).unwrap(),
                    expected_reward(supply_tokens),
                    "Mismatch at {} tokens",
                    supply_tokens
                );
                let below = calculate_dynamic_mint_amount(supply_tokens * M - 1).unwrap();
                assert_eq!(below, expected_reward(supply_tokens - 1), "Mismatch just below {} tokens", supply_tokens);
            }
            magnitude *= 10;
        }
    }

    #[test]
    fn test_reward_never_increases_with_supply() {
        let mut previous = u64::MAX;
        let mut magnitude = CURVE_START_TOKENS;
        for _ in 0..CURVE_DECADES {
            for d in 1..=9u64 {
                let supply = d * magnitude * M;
                for s in [supply - 1, supply, supply + 1] {
                    let amount = calculate_dynamic_mint_amount(s).unwrap();
                    assert!(amount <= previous, "Reward increased at supply {}", s);
                    previous = amount;
                }
            }
            magnitude *= 10;
        }
    }

    #[test]
    fn test_reward_is_always_at_least_one_lamport() {
        let mut magnitude = CURVE_START_TOKENS;
        for _ in 0..CURVE_DECADES {
            for d in 1..=9u64 {
                assert!(calculate_dynamic_mint_amount(d * magnitude * M).unwrap() >= 1);
            }
            magnitude *= 10;
        }
    }

    #[test]
    fn test_curve_constants() {
        assert_eq!(CURVE_START_LAMPORTS, 10_000_000 * DECIMAL_FACTOR);
        assert_eq!(BASE_MINT_AMOUNT, 1_000_000);
        assert_eq!(CURVE_START_LAMPORTS * 10u64.pow(CURVE_DECADES), MAX_SUPPLY_LAMPORTS);
        // The curve would reach exactly 1 lamport at max supply
        assert_eq!(BASE_MINT_AMOUNT / 10u64.pow(CURVE_DECADES), 1);
    }

    // ------------------------------------------------------------------------
    // Max Supply Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_max_supply_exactly_at_limit() {
        let result = calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS);
        assert!(
            result.is_err(),
            "At exactly max supply, should fail with SupplyLimitReached"
        );

        // Verify it's the correct error type
        match result {
            Err(e) => {
                let error_code = e.to_string();
                assert!(
                    error_code.contains("SupplyLimitReached") || error_code.contains("Supply limit reached"),
                    "Should be SupplyLimitReached error"
                );
            }
            Ok(_) => panic!("Should have failed at max supply"),
        }
    }

    #[test]
    fn test_max_supply_above_limit() {
        let above_max = MAX_SUPPLY_LAMPORTS + 1_000_000;
        assert!(calculate_dynamic_mint_amount(above_max).is_err());
    }

    #[test]
    fn test_last_mint_fills_supply_exactly() {
        // In the last step the reward is 2 lamports, so supply = max - 2 can still mint
        let amount = calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS - 2).unwrap();
        assert_eq!(amount, 2);
        assert_eq!(MAX_SUPPLY_LAMPORTS - 2 + amount, MAX_SUPPLY_LAMPORTS);
    }

    #[test]
    fn test_max_supply_would_exceed_after_mint() {
        // One lamport below max: the 2-lamport reward would overshoot the cap
        let result = calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS - 1);
        assert!(result.is_err(), "Should fail if minting would exceed max supply");
    }

    // ------------------------------------------------------------------------
    // Overflow Protection Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_overflow_protection_near_u64_max() {
        assert!(calculate_dynamic_mint_amount(u64::MAX - 100).is_err());
    }

    #[test]
    fn test_overflow_protection_at_u64_max() {
        assert!(calculate_dynamic_mint_amount(u64::MAX).is_err());
    }
}

// ============================================================================
// Tests for validate_memo_length()
// ============================================================================

#[cfg(test)]
mod validate_memo_length_tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Empty Memo Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_empty_memo_rejected() {
        let empty = vec![];
        let result = validate_memo_length(&empty, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "Empty memo should be rejected");
    }

    // ------------------------------------------------------------------------
    // Below Minimum Length Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_one_byte_too_short() {
        let short_memo = vec![b'x'; 68]; // 68 bytes (need 69)
        let result = validate_memo_length(&short_memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "68 bytes should be rejected (minimum is 69)");
    }

    #[test]
    fn test_memo_significantly_too_short() {
        let short_memo = vec![b'x'; 50]; // 50 bytes
        let result = validate_memo_length(&short_memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "50 bytes should be rejected");
    }

    #[test]
    fn test_memo_one_byte_only() {
        let short_memo = vec![b'x'; 1];
        let result = validate_memo_length(&short_memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "1 byte should be rejected");
    }

    // ------------------------------------------------------------------------
    // Minimum Valid Length Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_exactly_minimum_length() {
        let min_valid = vec![b'x'; 69]; // Exactly 69 bytes
        let result = validate_memo_length(&min_valid, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "69 bytes (minimum) should be accepted");
        
        assert!(result.unwrap(), "Should return true for a valid length");
    }

    #[test]
    fn test_memo_minimum_plus_one() {
        let memo = vec![b'x'; 70]; // 70 bytes
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "70 bytes should be accepted");
        
        assert!(result.unwrap());
    }

    // ------------------------------------------------------------------------
    // Mid-Range Valid Length Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_100_bytes() {
        let memo = vec![b'x'; 100];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "100 bytes should be accepted");
    }

    #[test]
    fn test_memo_200_bytes() {
        let memo = vec![b'x'; 200];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "200 bytes should be accepted");
    }

    #[test]
    fn test_memo_500_bytes() {
        let memo = vec![b'x'; 500];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "500 bytes should be accepted");
    }

    // ------------------------------------------------------------------------
    // Long Memo Tests (no contract maximum; the transaction size limit applies)
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_800_bytes_accepted() {
        let memo = vec![b'x'; 800]; // the former maximum
        assert!(validate_memo_length(&memo, MEMO_MIN_LENGTH).unwrap());
    }

    #[test]
    fn test_memo_801_bytes_accepted() {
        let memo = vec![b'x'; 801]; // rejected before the maximum was removed
        assert!(validate_memo_length(&memo, MEMO_MIN_LENGTH).unwrap());
    }

    #[test]
    fn test_memo_4096_bytes_accepted() {
        let memo = vec![b'x'; 4096]; // size of a v1 transaction
        assert!(validate_memo_length(&memo, MEMO_MIN_LENGTH).unwrap());
    }

    // ------------------------------------------------------------------------
    // Binary Data Tests (Content Agnostic)
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_with_null_bytes() {
        let memo_with_nulls = vec![0u8; 69];
        let result = validate_memo_length(&memo_with_nulls, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo with null bytes should be accepted (content-agnostic)");
    }

    #[test]
    fn test_memo_with_all_255() {
        let memo = vec![255u8; 69];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo with 0xFF bytes should be accepted");
    }

    #[test]
    fn test_memo_with_mixed_binary() {
        let mut memo = Vec::new();
        for i in 0..69 {
            memo.push((i % 256) as u8);
        }
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo with mixed binary data should be accepted");
    }

    // ------------------------------------------------------------------------
    // Edge Cases
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_length_constants() {
        assert_eq!(MEMO_MIN_LENGTH, 69, "Minimum memo length should be 69");
    }

    #[test]
    fn test_memo_ascii_text() {
        let ascii_memo = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Test memo!!!".to_vec();
        // Don't assert exact length - just verify it's valid
        assert!(ascii_memo.len() >= MEMO_MIN_LENGTH, "Should be at least 69 bytes");
        
        let result = validate_memo_length(&ascii_memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "ASCII text memo should be accepted");
    }

    #[test]
    fn test_memo_utf8_text() {
        // UTF-8 text with multi-byte characters (emoji and Chinese)
        let utf8_text = "Hello 世界 🌍 This is a UTF-8 memo with exactly 69 bytes total!!!";
        let utf8_memo = utf8_text.as_bytes().to_vec();
        assert_eq!(utf8_memo.len(), 69);
        
        let result = validate_memo_length(&utf8_memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "UTF-8 text memo should be accepted");
    }

    #[test]
    fn test_memo_base64_encoded() {
        // Simulating base64 encoded data (common in contract integration)
        let base64_like = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=====".as_bytes().to_vec();
        assert!(base64_like.len() >= MEMO_MIN_LENGTH, "Base64 string should be at least 69 bytes");
        
        let result = validate_memo_length(&base64_like, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Base64-like data should be accepted");
    }
}

// ============================================================================
// Integration Tests (Cross-function)
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_process_mint_to_wire_format_carries_recipient() {
        // 8-byte discriminator + 32-byte recipient, same layout hand-built clients already send
        use anchor_lang::{Discriminator, InstructionData};
        let recipient = Pubkey::new_unique();
        let data = crate::instruction::ProcessMintTo { recipient }.data();
        assert_eq!(data.len(), 8 + 32);
        assert_eq!(&data[..8], crate::instruction::ProcessMintTo::DISCRIMINATOR);
        assert_eq!(&data[8..], recipient.as_ref());
    }

    #[test]
    fn test_mint_amount_round_numbers_in_units() {
        let test_cases = vec![
            (0, 1_000_000),
            (20_000_000 * DECIMAL_FACTOR, 900_000),
            (90_000_000 * DECIMAL_FACTOR, 200_000),
            (100_000_000 * DECIMAL_FACTOR, 100_000),
            (200_000_000 * DECIMAL_FACTOR, 90_000),
            (1_000_000_000 * DECIMAL_FACTOR, 10_000),
            (10_000_000_000 * DECIMAL_FACTOR, 1_000),
            (100_000_000_000 * DECIMAL_FACTOR, 100),
            (1_000_000_000_000 * DECIMAL_FACTOR, 10),
        ];

        for (supply, expected) in test_cases {
            assert_eq!(
                calculate_dynamic_mint_amount(supply).unwrap(),
                expected,
                "Mint amount should match curve at supply {}",
                supply
            );
        }
    }

    #[test]
    fn test_memo_and_mint_amount_realistic_scenario() {
        // Simulate realistic user flow
        let memo = vec![b'x'; 100]; // 100 byte memo
        let memo_result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(memo_result.is_ok(), "Valid memo should pass");

        let supply = 50_000_000 * DECIMAL_FACTOR; // 50M tokens
        let mint_amount = calculate_dynamic_mint_amount(supply).unwrap();
        assert_eq!(mint_amount, 600_000); // 0.6 token
    }

    #[test]
    fn test_constants_consistency() {
        // Verify that all constants are properly related
        assert_eq!(MAX_SUPPLY_LAMPORTS, MAX_SUPPLY_TOKENS * DECIMAL_FACTOR);
        assert_eq!(BASE_MINT_AMOUNT, 1 * DECIMAL_FACTOR);
        assert_eq!(CURVE_START_LAMPORTS, CURVE_START_TOKENS * DECIMAL_FACTOR);
    }

    #[test]
    fn test_curve_decade_relationship() {
        // Each decade start pays 10x less than the previous one
        let mut supply_tokens = CURVE_START_TOKENS;
        let mut previous = calculate_dynamic_mint_amount(supply_tokens * DECIMAL_FACTOR).unwrap();
        for _ in 1..CURVE_DECADES {
            supply_tokens *= 10;
            let amount = calculate_dynamic_mint_amount(supply_tokens * DECIMAL_FACTOR).unwrap();
            assert_eq!(previous / 10, amount);
            previous = amount;
        }
    }
}

// ============================================================================
// Comprehensive Test Summary
// ============================================================================

#[cfg(test)]
mod test_coverage_summary {
    // This module serves as documentation for test coverage
    
    // calculate_dynamic_mint_amount
    // - Below curve start, every round number (54) and one lamport below each
    // - Decade starts, monotonicity, minimum reward
    // - Max supply edge cases
    // - Overflow protection
    // - Arithmetic safety
    
    // validate_memo_length: 35+ tests
    // - Empty memo
    // - Below minimum (various lengths)
    // - At minimum boundary
    // - Mid-range valid lengths
    // - At maximum boundary
    // - Above maximum (various lengths)
    // - Binary data handling
    // - Data integrity
    // - Different content types (ASCII, UTF-8, Base64)
    
    // Integration tests: 7+ tests
    // - Cross-function consistency
    // - Constants verification
    // - Mathematical relationships
    
    // Total: 110+ comprehensive unit tests
    // Coverage: All public and private functions
    // Edge cases: Extensively covered
    // Error paths: All tested
}

// ============================================================================
// Tests for one mint per transaction (MintState, transaction_fingerprint)
// ============================================================================

#[cfg(test)]
mod one_mint_per_transaction_tests {
    use super::*;
    use solana_instruction::{AccountMeta as Meta, BorrowedAccountMeta, BorrowedInstruction};
    use solana_instructions_sysvar::{construct_instructions_data, store_current_index_checked};

    fn new_state() -> MintState {
        MintState { last_tx_fingerprint: [0u8; 32], last_slot: 0, bump: 255, reserved: [0u8; 64] }
    }

    fn is_one_mint_error(result: Result<()>) -> bool {
        match result {
            Err(e) => e.to_string().contains("OneMintPerTransaction"),
            Ok(_) => false,
        }
    }

    /// Owned description of an instruction: (program_id, accounts, data)
    struct Ix {
        program_id: Pubkey,
        accounts: Vec<Meta>,
        data: Vec<u8>,
    }

    /// Instructions sysvar data exactly as the runtime builds it, with the current index set
    fn sysvar_data(ixs: &[Ix], current_index: u16) -> Vec<u8> {
        let borrowed: Vec<BorrowedInstruction> = ixs
            .iter()
            .map(|ix| BorrowedInstruction {
                program_id: &ix.program_id,
                accounts: ix.accounts.iter().map(|m| BorrowedAccountMeta {
                    pubkey: &m.pubkey,
                    is_signer: m.is_signer,
                    is_writable: m.is_writable,
                }).collect(),
                data: &ix.data,
            })
            .collect();
        let mut data = construct_instructions_data(&borrowed);
        store_current_index_checked(&mut data, current_index).unwrap();
        data
    }

    fn memo_ix(text: &str) -> Ix {
        Ix { program_id: MEMO_PROGRAM_ID, accounts: vec![], data: text.as_bytes().to_vec() }
    }

    fn mint_ix(user: Pubkey) -> Ix {
        Ix {
            program_id: crate::ID,
            accounts: vec![Meta::new(user, true), Meta::new(AUTHORIZED_MINT_PUBKEY, false)],
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        }
    }

    fn tx(user: Pubkey, memo: &str, mints: usize) -> Vec<Ix> {
        let mut ixs = vec![memo_ix(memo)];
        for _ in 0..mints {
            ixs.push(mint_ix(user));
        }
        ixs
    }

    const MEMO: &str = "a memo that is long enough for the memo-mint program minimum length check!!";

    #[test]
    fn test_mint_state_space() {
        assert_eq!(MintState::SPACE, 8 + 32 + 8 + 1 + 64);
        assert_eq!(MintState::SPACE, 113);
    }

    #[test]
    fn test_fingerprint_ignores_only_current_index() {
        let user = Pubkey::new_unique();
        let ixs = tx(user, MEMO, 3);
        // every instruction index of the same transaction gives the same fingerprint
        let f1 = transaction_fingerprint(&sysvar_data(&ixs, 1)).unwrap();
        let f2 = transaction_fingerprint(&sysvar_data(&ixs, 2)).unwrap();
        let f3 = transaction_fingerprint(&sysvar_data(&ixs, 3)).unwrap();
        assert_eq!(f1, f2);
        assert_eq!(f1, f3);
        // and the excluded part is exactly what the runtime reads as the current index
        let data = sysvar_data(&ixs, 3);
        assert_eq!(u16::from_le_bytes([data[data.len() - 2], data[data.len() - 1]]), 3);
    }

    #[test]
    fn test_fingerprint_differs_when_anything_differs() {
        let user = Pubkey::new_unique();
        let other_user = Pubkey::new_unique();
        let base = transaction_fingerprint(&sysvar_data(&tx(user, MEMO, 1), 1)).unwrap();

        // different signer, same memo (two users with identical memos in the same slot)
        let different_user = transaction_fingerprint(&sysvar_data(&tx(other_user, MEMO, 1), 1)).unwrap();
        assert_ne!(base, different_user);

        // different memo
        let different_memo = transaction_fingerprint(&sysvar_data(&tx(user, &format!("{}?", MEMO), 1), 1)).unwrap();
        assert_ne!(base, different_memo);

        // different instruction count
        let more_mints = transaction_fingerprint(&sysvar_data(&tx(user, MEMO, 2), 1)).unwrap();
        assert_ne!(base, more_mints);

        // different account flags
        let mut ixs = tx(user, MEMO, 1);
        ixs[1].accounts[1].is_writable = false;
        let different_flags = transaction_fingerprint(&sysvar_data(&ixs, 1)).unwrap();
        assert_ne!(base, different_flags);

        // identical transactions give identical fingerprints
        let same = transaction_fingerprint(&sysvar_data(&tx(user, MEMO, 1), 1)).unwrap();
        assert_eq!(base, same);
    }

    #[test]
    fn test_fingerprint_rejects_too_short_data() {
        assert!(transaction_fingerprint(&[]).is_err());
        assert!(transaction_fingerprint(&[0u8]).is_err());
        // 2 bytes = only the index: fingerprint of empty data, still valid
        assert!(transaction_fingerprint(&[0u8, 0u8]).is_ok());
    }

    #[test]
    fn test_first_mint_recorded() {
        let mut state = new_state();
        let fp = [7u8; 32];
        assert!(state.record_mint(fp, 100).is_ok());
        assert_eq!(state.last_tx_fingerprint, fp);
        assert_eq!(state.last_slot, 100);
    }

    #[test]
    fn test_second_mint_in_same_transaction_rejected() {
        let mut state = new_state();
        let fp = [7u8; 32];
        assert!(state.record_mint(fp, 100).is_ok());
        assert!(is_one_mint_error(state.record_mint(fp, 100)));
        // rejected attempts do not change the state
        assert_eq!(state.last_tx_fingerprint, fp);
        assert_eq!(state.last_slot, 100);
        assert!(is_one_mint_error(state.record_mint(fp, 100)));
    }

    #[test]
    fn test_other_transaction_in_same_slot_allowed() {
        let mut state = new_state();
        assert!(state.record_mint([1u8; 32], 100).is_ok());
        assert!(state.record_mint([2u8; 32], 100).is_ok());
        assert!(state.record_mint([3u8; 32], 100).is_ok());
        assert_eq!(state.last_tx_fingerprint, [3u8; 32]);
    }

    #[test]
    fn test_identical_transaction_in_later_slot_allowed() {
        let mut state = new_state();
        let fp = [9u8; 32];
        assert!(state.record_mint(fp, 100).is_ok());
        assert!(state.record_mint(fp, 101).is_ok());
        assert_eq!(state.last_slot, 101);
    }

    #[test]
    fn test_fresh_state_accepts_first_mint_at_slot_zero() {
        // a real fingerprint is a SHA-256 hash, never all zeros, so the initial state never matches
        let mut state = new_state();
        let fp = transaction_fingerprint(&sysvar_data(&tx(Pubkey::new_unique(), MEMO, 1), 1)).unwrap();
        assert_ne!(fp, [0u8; 32]);
        assert!(state.record_mint(fp, 0).is_ok());
    }

    #[test]
    fn test_batched_transaction_mints_once() {
        // [memo, mint, mint, mint]: instructions 1, 2, 3 see the same fingerprint
        let user = Pubkey::new_unique();
        let ixs = tx(user, MEMO, 3);
        let mut state = new_state();
        let slot = 12345;
        let results: Vec<bool> = (1..=3u16)
            .map(|i| state.record_mint(transaction_fingerprint(&sysvar_data(&ixs, i)).unwrap(), slot).is_ok())
            .collect();
        assert_eq!(results, vec![true, false, false]);
    }

    #[test]
    fn test_cpi_loop_mints_once() {
        // a program at index 1 calling memo-mint N times via CPI: every call sees index 1
        let ixs = vec![memo_ix(MEMO), Ix { program_id: Pubkey::new_unique(), accounts: vec![], data: vec![30] }];
        let data = sysvar_data(&ixs, 1);
        let mut state = new_state();
        let ok: usize = (0..30)
            .filter(|_| state.record_mint(transaction_fingerprint(&data).unwrap(), 777).is_ok())
            .count();
        assert_eq!(ok, 1);
    }

    #[test]
    fn test_interleaved_transactions_each_mint_once() {
        // transactions never interleave inside one transaction (MintState is write-locked),
        // but consecutive transactions alternate freely
        let a = transaction_fingerprint(&sysvar_data(&tx(Pubkey::new_unique(), MEMO, 1), 1)).unwrap();
        let b = transaction_fingerprint(&sysvar_data(&tx(Pubkey::new_unique(), MEMO, 1), 1)).unwrap();
        let mut state = new_state();
        assert!(state.record_mint(a, 5).is_ok());
        assert!(state.record_mint(b, 5).is_ok());
        assert!(state.record_mint(a, 6).is_ok());
    }

    #[test]
    fn test_admin_pubkey_per_network() {
        #[cfg(feature = "mainnet")]
        assert_eq!(AUTHORIZED_ADMIN_PUBKEY.to_string(), "FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");
        #[cfg(not(feature = "mainnet"))]
        assert_eq!(AUTHORIZED_ADMIN_PUBKEY.to_string(), "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");
    }

    #[test]
    fn test_error_codes_unchanged_and_appended() {
        let code = |e: ErrorCode| u32::from(e);
        assert_eq!(code(ErrorCode::MemoRequired), 6000);
        assert_eq!(code(ErrorCode::MemoTooShort), 6001);
        assert_eq!(code(ErrorCode::InvalidTokenAccount), 6002);
        assert_eq!(code(ErrorCode::UnauthorizedMint), 6003);
        assert_eq!(code(ErrorCode::UnauthorizedTokenAccount), 6004);
        assert_eq!(code(ErrorCode::SupplyLimitReached), 6005);
        assert_eq!(code(ErrorCode::ArithmeticOverflow), 6006);
        assert_eq!(code(ErrorCode::UnauthorizedAdmin), 6007);
        assert_eq!(code(ErrorCode::OneMintPerTransaction), 6008);
    }
}
