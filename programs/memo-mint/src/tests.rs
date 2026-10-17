//! Unit tests for memo-mint contract
//! 
//! This test suite provides comprehensive coverage of all core functions:
//! - calculate_dynamic_mint_amount: Round-number emission curve (steps at 10M, 20M, ... 100M, 200M, ...)
//! - validate_memo_length: Memo minimum-length validation (no maximum)

use super::*;

// ============================================================================
// Tests for calculate_dynamic_mint_amount()
// ============================================================================

#[cfg(test)]
mod calculate_dynamic_mint_amount_tests {
    use super::*;

    const M: u64 = DECIMAL_FACTOR; // 1 token in lamports

    /// Expected reward (lamports) at a supply given in lamports, from the curve definition:
    /// up to and including 10M tokens -> 1 token; above that, supply in (d * 10^k, (d+1) * 10^k]
    /// inside the decade (10M * 10^n, 10M * 10^(n+1)] -> (1 token / 10^n) * (10 - d) / 10.
    /// Uses u128 and explicit interval checks rather than the program's loop.
    fn expected_reward(supply: u64) -> u64 {
        let s = supply as u128;
        let start = CURVE_START_LAMPORTS as u128;
        if s <= start {
            return BASE_MINT_AMOUNT;
        }
        for n in 0..CURVE_DECADES {
            let magnitude = start * 10u128.pow(n);
            for d in 1..=9u128 {
                if s > d * magnitude && s <= (d + 1) * magnitude {
                    return (BASE_MINT_AMOUNT as u128 / 10u128.pow(n) * (10 - d) / 10) as u64;
                }
            }
        }
        panic!("supply {} is beyond the curve", supply);
    }

    // ------------------------------------------------------------------------
    // Up to and including the curve start: 0 - 10M tokens (1.0 token per mint)
    // ------------------------------------------------------------------------

    #[test]
    fn test_zero_supply_mints_one_token() {
        assert_eq!(calculate_dynamic_mint_amount(0).unwrap(), M);
    }

    #[test]
    fn test_current_mainnet_supply_mints_one_token() {
        // Mainnet supply on 2026-10-15: 8,730,756 MEMO
        assert_eq!(calculate_dynamic_mint_amount(8_730_756 * M).unwrap(), M);
    }

    #[test]
    fn test_exactly_10m_still_mints_one_token() {
        assert_eq!(calculate_dynamic_mint_amount(CURVE_START_LAMPORTS - 1).unwrap(), M);
        assert_eq!(calculate_dynamic_mint_amount(CURVE_START_LAMPORTS).unwrap(), M);
    }

    #[test]
    fn test_first_mint_above_10m_pays_0_9() {
        assert_eq!(calculate_dynamic_mint_amount(CURVE_START_LAMPORTS + 1).unwrap(), 900_000);
        // e.g. after the 1-token mint at exactly 10M
        assert_eq!(calculate_dynamic_mint_amount(10_000_001 * M).unwrap(), 900_000);
    }

    // ------------------------------------------------------------------------
    // First decade: above 10M up to 100M (1 -> 0.1, one tenth less above 10M, 20M, ... 90M)
    // ------------------------------------------------------------------------

    #[test]
    fn test_first_decade_steps() {
        // (round number in tokens, reward at exactly it, reward one lamport above it)
        let cases: [(u64, u64, u64); 9] = [
            (10_000_000, 1_000_000, 900_000), // 10M: 1, above: 0.9
            (20_000_000, 900_000, 800_000),   // 20M: 0.9, above: 0.8
            (30_000_000, 800_000, 700_000),
            (40_000_000, 700_000, 600_000),
            (50_000_000, 600_000, 500_000),
            (60_000_000, 500_000, 400_000),
            (70_000_000, 400_000, 300_000),
            (80_000_000, 300_000, 200_000),
            (90_000_000, 200_000, 100_000),   // 90M: 0.2, above: 0.1
        ];
        for (tokens, at, above) in cases {
            assert_eq!(calculate_dynamic_mint_amount(tokens * M).unwrap(), at, "Wrong reward at {} tokens", tokens);
            assert_eq!(calculate_dynamic_mint_amount(tokens * M + 1).unwrap(), above, "Wrong reward above {} tokens", tokens);
        }
    }

    #[test]
    fn test_reward_constant_inside_a_step() {
        // Anywhere in (20M, 30M] pays 0.8
        for supply in [20_000_000 * M + 1, 25_000_000 * M, 30_000_000 * M] {
            assert_eq!(calculate_dynamic_mint_amount(supply).unwrap(), 800_000);
        }
    }

    #[test]
    fn test_exactly_100m_pays_0_1_and_above_pays_0_09() {
        assert_eq!(calculate_dynamic_mint_amount(100_000_000 * M).unwrap(), 100_000);
        assert_eq!(calculate_dynamic_mint_amount(100_000_000 * M + 1).unwrap(), 90_000);
    }

    // ------------------------------------------------------------------------
    // Decade ends: 10M -> 1, 100M -> 0.1, 1B -> 0.01, ... (10x per decade)
    // ------------------------------------------------------------------------

    #[test]
    fn test_decade_ends_are_powers_of_ten() {
        // (decade end in tokens, reward at exactly it, reward one lamport above it)
        let cases: [(u64, u64, u64); 6] = [
            (10_000_000, 1_000_000, 900_000),   // 10M: 1 / 0.9
            (100_000_000, 100_000, 90_000),     // 100M: 0.1 / 0.09
            (1_000_000_000, 10_000, 9_000),     // 1B: 0.01 / 0.009
            (10_000_000_000, 1_000, 900),       // 10B: 0.001 / 0.0009
            (100_000_000_000, 100, 90),         // 100B: 0.0001 / 0.00009
            (1_000_000_000_000, 10, 9),         // 1T: 0.00001 / 0.000009
        ];
        for (tokens, at, above) in cases {
            assert_eq!(calculate_dynamic_mint_amount(tokens * M).unwrap(), at, "Wrong reward at {} tokens", tokens);
            assert_eq!(calculate_dynamic_mint_amount(tokens * M + 1).unwrap(), above, "Wrong reward above {} tokens", tokens);
        }
    }

    #[test]
    fn test_second_decade_steps() {
        // Above 100M up to 1B: 0.09 -> 0.01
        assert_eq!(calculate_dynamic_mint_amount(200_000_000 * M).unwrap(), 90_000); // 200M: 0.09
        assert_eq!(calculate_dynamic_mint_amount(200_000_000 * M + 1).unwrap(), 80_000); // above 200M: 0.08
        assert_eq!(calculate_dynamic_mint_amount(500_000_000 * M + 1).unwrap(), 50_000); // above 500M: 0.05
        assert_eq!(calculate_dynamic_mint_amount(900_000_000 * M + 1).unwrap(), 10_000); // above 900M: 0.01
        assert_eq!(calculate_dynamic_mint_amount(1_000_000_000 * M).unwrap(), 10_000); // 1B: 0.01
    }

    #[test]
    fn test_last_decade_steps() {
        // Above 1T up to the cap: 9 lamports down to 1 lamport
        assert_eq!(calculate_dynamic_mint_amount(1_000_000_000_000 * M + 1).unwrap(), 9);
        assert_eq!(calculate_dynamic_mint_amount(5_000_000_000_000 * M + 1).unwrap(), 5);
        assert_eq!(calculate_dynamic_mint_amount(9_000_000_000_000 * M).unwrap(), 2);
        assert_eq!(calculate_dynamic_mint_amount(9_000_000_000_000 * M + 1).unwrap(), 1);
    }

    // ------------------------------------------------------------------------
    // Whole-curve properties
    // ------------------------------------------------------------------------

    #[test]
    fn test_every_round_number_matches_definition() {
        // All 54 round numbers d * 10^k from 10M to 9T: one lamport below, exactly at, one lamport above
        let mut magnitude = CURVE_START_TOKENS;
        for _ in 0..CURVE_DECADES {
            for d in 1..=9u64 {
                let supply = d * magnitude * M;
                for s in [supply - 1, supply, supply + 1] {
                    assert_eq!(calculate_dynamic_mint_amount(s).unwrap(), expected_reward(s), "Mismatch at {} lamports", s);
                }
            }
            magnitude *= 10;
        }
        // And the cap side of the last step
        assert_eq!(calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS - 1).unwrap(), expected_reward(MAX_SUPPLY_LAMPORTS - 1));
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
                assert!(calculate_dynamic_mint_amount(d * magnitude * M + 1).unwrap() >= 1);
            }
            magnitude *= 10;
        }
    }

    #[test]
    fn test_curve_constants() {
        assert_eq!(CURVE_START_LAMPORTS, 10_000_000 * DECIMAL_FACTOR);
        assert_eq!(BASE_MINT_AMOUNT, 1_000_000);
        assert_eq!(CURVE_START_LAMPORTS * 10u64.pow(CURVE_DECADES), MAX_SUPPLY_LAMPORTS);
        // The last step (9T, 10T) pays exactly 1 lamport
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
        // In the last step the reward is 1 lamport, so supply = max - 1 mints the very last lamport
        let amount = calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS - 1).unwrap();
        assert_eq!(amount, 1);
        assert_eq!(MAX_SUPPLY_LAMPORTS - 1 + amount, MAX_SUPPLY_LAMPORTS);
    }

    #[test]
    fn test_mint_never_overshoots_max_supply() {
        // The 1-lamport last step lands exactly on the cap, and nothing mints at the cap
        assert_eq!(calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS - 2).unwrap(), 1);
        assert!(calculate_dynamic_mint_amount(MAX_SUPPLY_LAMPORTS).is_err());
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

    // ------------------------------------------------------------------------
    // mint_amount_at_supply (public curve value, used for burn thresholds)
    // ------------------------------------------------------------------------

    #[test]
    fn test_mint_amount_at_supply_matches_mint_below_cap() {
        for supply in [0, CURVE_START_LAMPORTS, CURVE_START_LAMPORTS + 1, 55_555_555 * M,
                       1_000_000_000 * M + 1, MAX_SUPPLY_LAMPORTS - 1] {
            assert_eq!(mint_amount_at_supply(supply), calculate_dynamic_mint_amount(supply).unwrap());
        }
    }

    #[test]
    fn test_mint_amount_at_supply_is_last_step_at_and_above_cap() {
        assert_eq!(mint_amount_at_supply(MAX_SUPPLY_LAMPORTS), 1);
        assert_eq!(mint_amount_at_supply(MAX_SUPPLY_LAMPORTS + 1), 1);
        assert_eq!(mint_amount_at_supply(u64::MAX), 1);
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
            (10_000_000 * DECIMAL_FACTOR, 1_000_000),
            (10_000_000 * DECIMAL_FACTOR + 1, 900_000),
            (20_000_000 * DECIMAL_FACTOR + 1, 800_000),
            (90_000_000 * DECIMAL_FACTOR + 1, 100_000),
            (100_000_000 * DECIMAL_FACTOR, 100_000),
            (100_000_000 * DECIMAL_FACTOR + 1, 90_000),
            (1_000_000_000 * DECIMAL_FACTOR + 1, 9_000),
            (10_000_000_000 * DECIMAL_FACTOR + 1, 900),
            (100_000_000_000 * DECIMAL_FACTOR + 1, 90),
            (1_000_000_000_000 * DECIMAL_FACTOR + 1, 9),
            (9_000_000_000_000 * DECIMAL_FACTOR + 1, 1),
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

        let supply = 55_000_000 * DECIMAL_FACTOR; // 55M tokens
        let mint_amount = calculate_dynamic_mint_amount(supply).unwrap();
        assert_eq!(mint_amount, 500_000); // 0.5 token
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
        // Each decade end (10M, 100M, 1B, ...) pays 10x less than the previous one
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
    //
    // Unit-tested here (run `cargo test -p memo-mint --lib` for the exact count):
    // - calculate_dynamic_mint_amount / mint_amount_at_supply against two references that do not
    //   share code with the program (interval search and decimal-digit oracle), cap and overflow
    // - validate_memo_length (minimum 69 bytes, no maximum)
    // - check_memo_instruction against real instructions-sysvar data (memo position rules)
    // - transaction_fingerprint and MintState::record_mint (one mint per transaction)
    // - error code numbering
    //
    // Not unit-testable without changing lib.rs, covered by the on-chain regression suite
    // (scripts/run-regression-tests.sh, clients/regression/src/):
    // - process_mint / process_mint_to handlers and Accounts constraints (mint, owner, recipient,
    //   sysvar address, mint_state seeds), one mint per transaction across CPI (app + top-level)
    // - exact minted amounts at several supply tiers and at the 10T cap
    // - initialize_mint_state admin check (fresh state)
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

    /// Only shows that record_mint rejects a repeated (fingerprint, slot) pair when it is fed the
    /// same sysvar bytes 30 times. It does NOT prove that the runtime hands a CPI the same sysvar
    /// bytes as the top-level instruction; that is covered on-chain by the regression suite
    /// (clients/regression/src/regression-memo-mint.rs, app CPI double-mint cases).
    #[test]
    fn test_repeated_record_with_same_sysvar_mints_once() {
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

// ============================================================================
// Tests for check_memo_instruction (memo position rules)
// ============================================================================

#[cfg(test)]
mod check_memo_instruction_tests {
    use super::*;
    use solana_instruction::{AccountMeta as Meta, BorrowedAccountMeta, BorrowedInstruction};
    use solana_instructions_sysvar::{construct_instructions_data, store_current_index_checked};

    const LEGACY_MEMO_V1: Pubkey = pubkey!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
    const COMPUTE_BUDGET: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");

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

    /// Run check_memo_instruction on an AccountInfo with the given key and sysvar data
    fn check_with_key(key: Pubkey, ixs: &[Ix], current_index: u16) -> Result<bool> {
        let mut data = sysvar_data(ixs, current_index);
        let owner = anchor_lang::solana_program::sysvar::ID;
        let mut lamports = 0u64;
        let info = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false, 0);
        check_memo_instruction(&info)
    }

    fn check(ixs: &[Ix], current_index: u16) -> Result<bool> {
        check_with_key(INSTRUCTIONS_ID, ixs, current_index)
    }

    fn is_memo_too_short(result: Result<bool>) -> bool {
        matches!(result, Err(e) if e.to_string().contains("MemoTooShort"))
    }

    fn memo(len: usize) -> Ix {
        Ix { program_id: MEMO_PROGRAM_ID, accounts: vec![], data: vec![b'm'; len] }
    }

    fn mint() -> Ix {
        Ix { program_id: crate::ID, accounts: vec![Meta::new(Pubkey::new_unique(), true)], data: vec![0; 8] }
    }

    fn other(program_id: Pubkey, len: usize) -> Ix {
        Ix { program_id, accounts: vec![], data: vec![b'x'; len] }
    }

    #[test]
    fn test_memo_at_index_0_and_mint_at_index_1_accepted() {
        assert_eq!(check(&[memo(MEMO_MIN_LENGTH), mint()], 1).unwrap(), true);
        assert_eq!(check(&[memo(5000), mint()], 1).unwrap(), true);
    }

    #[test]
    fn test_mint_later_than_index_1_accepted() {
        // compute budget (or anything else) between the memo and the mint is fine
        assert_eq!(check(&[memo(MEMO_MIN_LENGTH), other(COMPUTE_BUDGET, 5), mint()], 2).unwrap(), true);
        assert_eq!(check(&[memo(MEMO_MIN_LENGTH), mint(), mint(), mint()], 3).unwrap(), true);
    }

    #[test]
    fn test_mint_at_index_0_rejected() {
        assert_eq!(check(&[mint()], 0).unwrap(), false);
        assert_eq!(check(&[mint(), memo(MEMO_MIN_LENGTH)], 0).unwrap(), false);
    }

    #[test]
    fn test_memo_only_right_before_the_mint_rejected() {
        // the memo must be at index 0, not at current_index - 1
        assert_eq!(check(&[other(COMPUTE_BUDGET, 5), memo(MEMO_MIN_LENGTH), mint()], 2).unwrap(), false);
        // and a memo at index 0 is found even when the instruction before the mint is not a memo
        assert_eq!(check(&[memo(MEMO_MIN_LENGTH), other(COMPUTE_BUDGET, 5), mint()], 2).unwrap(), true);
    }

    #[test]
    fn test_non_memo_program_at_index_0_rejected() {
        assert_eq!(check(&[other(COMPUTE_BUDGET, 80), mint()], 1).unwrap(), false);
        assert_eq!(check(&[other(Pubkey::new_unique(), 80), mint()], 1).unwrap(), false);
        assert_eq!(check(&[other(crate::ID, 80), mint()], 1).unwrap(), false);
    }

    #[test]
    fn test_legacy_memo_v1_rejected() {
        assert_eq!(check(&[other(LEGACY_MEMO_V1, 80), mint()], 1).unwrap(), false);
    }

    #[test]
    fn test_short_or_empty_memo_at_index_0_is_an_error() {
        assert!(is_memo_too_short(check(&[memo(MEMO_MIN_LENGTH - 1), mint()], 1)));
        assert!(is_memo_too_short(check(&[memo(0), mint()], 1)));
        // a valid memo later does not help
        assert!(is_memo_too_short(check(&[memo(10), memo(MEMO_MIN_LENGTH), mint()], 2)));
    }

    #[test]
    fn test_wrong_sysvar_account_is_an_error() {
        // on-chain the address constraint fires first; the function itself also refuses
        assert!(check_with_key(Pubkey::new_unique(), &[memo(MEMO_MIN_LENGTH), mint()], 1).is_err());
    }
}

// ============================================================================
// Emission curve against a decimal-digit oracle
// ============================================================================

#[cfg(test)]
mod curve_digit_oracle_tests {
    use super::*;

    /// Reference written from the documented curve using the decimal representation of the
    /// supply (shares no code or loop structure with the program).
    /// None at or above the 10T cap.
    fn digit_oracle(supply: u64) -> Option<u64> {
        const START: u64 = 10_000_000_000_000; // 10M tokens in units (14 digits)
        const MAX: u64 = 10_000_000_000_000_000_000; // 10T tokens in units
        if supply >= MAX {
            return None;
        }
        if supply <= START {
            return Some(1_000_000);
        }
        let below = (supply - 1).to_string();
        let decade = below.len() as u32 - 14; // 0..=5
        let digit = (below.as_bytes()[0] - b'0') as u64; // 1..=9
        Some(10u64.pow(6 - decade) * (10 - digit) / 10)
    }

    fn compare(supply: u64) {
        match digit_oracle(supply) {
            Some(expected) => {
                assert_eq!(calculate_dynamic_mint_amount(supply).unwrap(), expected, "supply {}", supply);
                assert_eq!(mint_amount_at_supply(supply), expected, "supply {}", supply);
            }
            None => {
                assert!(calculate_dynamic_mint_amount(supply).is_err(), "supply {}", supply);
                assert_eq!(mint_amount_at_supply(supply), 1, "supply {}", supply);
            }
        }
    }

    #[test]
    fn test_round_numbers_and_neighbours() {
        let mut power: u64 = 1;
        for _ in 0..20 {
            for d in 1..=9u64 {
                if let Some(x) = power.checked_mul(d) {
                    for delta in [-2i128, -1, 0, 1, 2] {
                        let v = x as i128 + delta;
                        if v >= 0 && v <= u64::MAX as i128 {
                            compare(v as u64);
                        }
                    }
                }
            }
            power = power.saturating_mul(10);
        }
        compare(0);
        compare(u64::MAX);
    }

    #[test]
    fn test_pseudo_random_supplies() {
        // xorshift64: deterministic, no extra dependency
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in 0..200_000u32 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let supply = if i % 2 == 0 { x % (MAX_SUPPLY_LAMPORTS + 1000) } else { x >> (i % 64) };
            compare(supply);
        }
    }

    #[test]
    fn test_hard_coded_tier_values() {
        // Values written out by hand (units), independent of any formula
        let m = DECIMAL_FACTOR;
        let cases: &[(u64, u64)] = &[
            (5_000_000 * m, 1_000_000),
            (10_000_000 * m, 1_000_000),
            (10_000_000 * m + 1, 900_000),
            (20_000_000 * m, 900_000),
            (20_000_000 * m + 1, 800_000),
            (100_000_000 * m, 100_000),
            (100_000_000 * m + 1, 90_000),
            (1_000_000_000 * m, 10_000),
            (1_000_000_000 * m + 1, 9_000),
            (1_000_000_000_000 * m + 1, 9),
            (5_000_000_000_000 * m, 6),
            (5_000_000_000_000 * m + 1, 5),
            (9_000_000_000_000 * m, 2),
            (9_000_000_000_000 * m + 1, 1),
            (10_000_000_000_000 * m - 1, 1),
        ];
        for &(supply, expected) in cases {
            assert_eq!(calculate_dynamic_mint_amount(supply).unwrap(), expected, "supply {}", supply);
        }
    }
}
