//! Unit tests for memo-burn contract
//! 
//! This test suite provides comprehensive coverage of all core functions:
//! - validate_memo_amount: Borsh+Base64 memo validation with burn amount verification
//! - validate_memo_length: Memo minimum-length validation (69 bytes, no maximum)
//! - Payload length prefix check (rejects forged Borsh length prefixes before allocation)
//! - BurnMemo structure: Serialization and deserialization
//! - Constants: Verify all constant values and relationships

use super::*;
use base64::engine::general_purpose;

/// Largest payload that fit under the former 800-byte memo limit (600 decoded bytes - 13-byte header).
/// Used as a "large payload" reference; the contract no longer enforces a maximum.
const LARGE_PAYLOAD: usize = 587;

// ============================================================================
// Tests for validate_memo_amount()
// ============================================================================

#[cfg(test)]
mod validate_memo_amount_tests {
    use super::*;

    // Helper function to create valid Borsh+Base64 memo
    fn create_valid_memo(burn_amount: u64, payload: Vec<u8>) -> Vec<u8> {
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        base64_encoded.into_bytes()
    }

    // ------------------------------------------------------------------------
    // Valid Memo Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_valid_memo_minimum_burn() {
        let burn_amount = MIN_BURN_AMOUNT; // 1 unit (0.000001 token)
        let payload = b"Valid burn operation".to_vec();
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with minimum burn should succeed");
    }

    #[test]
    fn test_valid_memo_large_burn() {
        let burn_amount = 1_000_000 * DECIMAL_FACTOR; // 1 million tokens
        let payload = b"Large burn operation".to_vec();
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with large burn should succeed");
    }

    #[test]
    fn test_valid_memo_empty_payload() {
        let burn_amount = 10 * DECIMAL_FACTOR; // 10 tokens
        let payload = vec![]; // Empty payload is allowed
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with empty payload should succeed");
    }

    #[test]
    fn test_valid_memo_small_payload() {
        let burn_amount = 5 * DECIMAL_FACTOR; // 5 tokens
        let payload = b"x".to_vec(); // 1 byte payload
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with small payload should succeed");
    }

    #[test]
    fn test_valid_memo_maximum_payload() {
        let burn_amount = 100 * DECIMAL_FACTOR; // 100 tokens
        let payload = vec![b'x'; LARGE_PAYLOAD]; // Maximum allowed payload
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with maximum payload should succeed");
    }

    #[test]
    fn test_valid_memo_near_maximum_payload() {
        let burn_amount = 50 * DECIMAL_FACTOR; // 50 tokens
        let payload = vec![b'x'; LARGE_PAYLOAD - 1]; // Just under maximum
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with near-maximum payload should succeed");
    }

    #[test]
    fn test_valid_memo_utf8_payload() {
        let burn_amount = 10 * DECIMAL_FACTOR; // 10 tokens
        let payload = "Hello, 世界! 🔥".as_bytes().to_vec(); // UTF-8 with emoji
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with UTF-8 payload should succeed");
    }

    #[test]
    fn test_valid_memo_binary_payload() {
        let burn_amount = 25 * DECIMAL_FACTOR; // 25 tokens
        let payload = vec![0u8, 1, 2, 255, 128, 64]; // Binary data
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Valid memo with binary payload should succeed");
    }

    // ------------------------------------------------------------------------
    // Invalid Format Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_invalid_not_base64() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let memo_data = b"This is not base64 encoded!!!".to_vec();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Non-base64 memo should fail");
        // Check error contains the expected message
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("InvalidMemoFormat") || err_str.contains("Invalid memo format"));
    }

    #[test]
    fn test_invalid_not_utf8() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let memo_data = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Non-UTF-8 memo should fail");
    }

    #[test]
    fn test_invalid_base64_not_borsh() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let random_data = b"random data that is not borsh";
        let base64_encoded = general_purpose::STANDARD.encode(random_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Base64 data that's not Borsh should fail");
    }

    #[test]
    fn test_invalid_empty_memo() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let memo_data = vec![];
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Empty memo should fail");
    }

    #[test]
    fn test_invalid_truncated_borsh() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"test".to_vec();
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        let mut borsh_data = borsh::to_vec(&memo).unwrap();
        borsh_data.truncate(5); // Truncate to make it invalid
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Truncated Borsh data should fail");
    }

    // ------------------------------------------------------------------------
    // Version Mismatch Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_invalid_version_zero() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"test".to_vec();
        let memo = BurnMemo {
            version: 0, // Wrong version
            burn_amount,
            payload,
        };
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Version 0 should fail");
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("UnsupportedMemoVersion") || err_str.contains("Unsupported memo version"));
    }

    #[test]
    fn test_invalid_version_two() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"test".to_vec();
        let memo = BurnMemo {
            version: 2, // Future version
            burn_amount,
            payload,
        };
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Version 2 should fail");
    }

    #[test]
    fn test_invalid_version_255() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"test".to_vec();
        let memo = BurnMemo {
            version: 255, // Maximum u8 value
            burn_amount,
            payload,
        };
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_err(), "Version 255 should fail");
    }

    // ------------------------------------------------------------------------
    // Burn Amount Mismatch Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_burn_amount_mismatch_higher() {
        let actual_burn = 10 * DECIMAL_FACTOR;
        let memo_burn = 20 * DECIMAL_FACTOR; // Higher than actual
        let payload = b"test".to_vec();
        let memo_data = create_valid_memo(memo_burn, payload);
        
        let result = validate_memo_amount(&memo_data, actual_burn);
        assert!(result.is_err(), "Higher burn amount in memo should fail");
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("BurnAmountMismatch") || err_str.contains("Burn amount mismatch"));
    }

    #[test]
    fn test_burn_amount_mismatch_lower() {
        let actual_burn = 20 * DECIMAL_FACTOR;
        let memo_burn = 10 * DECIMAL_FACTOR; // Lower than actual
        let payload = b"test".to_vec();
        let memo_data = create_valid_memo(memo_burn, payload);
        
        let result = validate_memo_amount(&memo_data, actual_burn);
        assert!(result.is_err(), "Lower burn amount in memo should fail");
    }

    #[test]
    fn test_burn_amount_mismatch_off_by_one() {
        let actual_burn = 10 * DECIMAL_FACTOR;
        let memo_burn = 10 * DECIMAL_FACTOR + 1; // Off by 1 unit
        let payload = b"test".to_vec();
        let memo_data = create_valid_memo(memo_burn, payload);
        
        let result = validate_memo_amount(&memo_data, actual_burn);
        assert!(result.is_err(), "Off-by-one burn amount should fail");
    }

    #[test]
    fn test_burn_amount_zero_in_memo() {
        let actual_burn = 10 * DECIMAL_FACTOR;
        let memo_burn = 0; // Zero in memo
        let payload = b"test".to_vec();
        let memo_data = create_valid_memo(memo_burn, payload);
        
        let result = validate_memo_amount(&memo_data, actual_burn);
        assert!(result.is_err(), "Zero burn amount in memo should fail");
    }

    // ------------------------------------------------------------------------
    // Payload Length Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_payload_above_former_limit_accepted() {
        // No maximum payload length: the transaction size limit bounds it
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![b'x'; LARGE_PAYLOAD + 1];
        let memo_data = create_valid_memo(burn_amount, payload);
        assert!(validate_memo_amount(&memo_data, burn_amount).is_ok());
    }

    #[test]
    fn test_payload_v1_transaction_size_accepted() {
        // A payload that would only fit in a 4096-byte v1 transaction
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![b'x'; 2500];
        let memo_data = create_valid_memo(burn_amount, payload);
        assert!(validate_memo_amount(&memo_data, burn_amount).is_ok());
    }

    #[test]
    fn test_payload_extremely_long() {
        // Far beyond any transaction size; validation itself still succeeds
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![b'x'; 10000];
        let memo_data = create_valid_memo(burn_amount, payload);
        assert!(validate_memo_amount(&memo_data, burn_amount).is_ok());
    }

    // ------------------------------------------------------------------------
    // Decoded Data Size Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_decoded_data_at_former_max_size() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        // 800-byte memo, which decodes to 600 bytes (the former maximum)
        let payload = vec![b'x'; LARGE_PAYLOAD];
        let memo_data = create_valid_memo(burn_amount, payload);
        assert_eq!(memo_data.len(), 800);
        
        let base64_str = std::str::from_utf8(&memo_data).unwrap();
        let decoded_data = general_purpose::STANDARD.decode(base64_str).unwrap();
        assert_eq!(decoded_data.len(), 600);
        
        assert!(validate_memo_amount(&memo_data, burn_amount).is_ok());
    }

    #[test]
    fn test_forged_payload_length_prefix_rejected() {
        // Valid memo whose payload length prefix claims 4 GiB: must fail cleanly, not abort on allocation
        let burn_amount = 10 * DECIMAL_FACTOR;
        let mut raw = borsh::to_vec(&BurnMemo { version: BURN_MEMO_VERSION, burn_amount, payload: vec![b'x'; 60] }).unwrap();
        raw[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
        let memo_data = general_purpose::STANDARD.encode(&raw).into_bytes();
        let err = validate_memo_amount(&memo_data, burn_amount).unwrap_err();
        assert!(err.to_string().contains("InvalidMemoFormat"));
    }

    #[test]
    fn test_payload_length_prefix_too_small_rejected() {
        // Prefix shorter than the bytes present (trailing data) is rejected
        let burn_amount = 10 * DECIMAL_FACTOR;
        let mut raw = borsh::to_vec(&BurnMemo { version: BURN_MEMO_VERSION, burn_amount, payload: vec![b'x'; 60] }).unwrap();
        raw[9..13].copy_from_slice(&59u32.to_le_bytes());
        let memo_data = general_purpose::STANDARD.encode(&raw).into_bytes();
        assert!(validate_memo_amount(&memo_data, burn_amount).is_err());
    }

    #[test]
    fn test_decoded_data_shorter_than_header_rejected() {
        let memo_data = general_purpose::STANDARD.encode([1u8; 12]).into_bytes();
        assert!(validate_memo_amount(&memo_data, DECIMAL_FACTOR).is_err());
    }

    #[test]
    fn test_decoded_data_above_former_max_size_accepted() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![b'x'; LARGE_PAYLOAD + 1]; // decodes to 601 bytes
        let memo_data = create_valid_memo(burn_amount, payload);
        assert!(validate_memo_amount(&memo_data, burn_amount).is_ok());
    }

    // ------------------------------------------------------------------------
    // Edge Case Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_maximum_burn_amount() {
        let burn_amount = MAX_BURN_PER_INSTRUCTION; // Maximum allowed burn
        let payload = b"Maximum burn test".to_vec();
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Maximum burn amount should succeed");
    }

    #[test]
    fn test_various_burn_amounts() {
        let test_amounts = vec![
            1 * DECIMAL_FACTOR,
            10 * DECIMAL_FACTOR,
            100 * DECIMAL_FACTOR,
            1_000 * DECIMAL_FACTOR,
            10_000 * DECIMAL_FACTOR,
            100_000 * DECIMAL_FACTOR,
            1_000_000 * DECIMAL_FACTOR,
        ];

        for burn_amount in test_amounts {
            let payload = format!("Burn {} tokens", burn_amount / DECIMAL_FACTOR).into_bytes();
            let memo_data = create_valid_memo(burn_amount, payload);
            
            let result = validate_memo_amount(&memo_data, burn_amount);
            assert!(result.is_ok(), "Burn amount {} should succeed", burn_amount);
        }
    }

    #[test]
    fn test_payload_with_special_characters() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"Special chars: !@#$%^&*()_+-=[]{}|;':\",./<>?`~".to_vec();
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Payload with special characters should succeed");
    }

    #[test]
    fn test_payload_with_newlines() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = b"Line 1\nLine 2\nLine 3\r\nLine 4".to_vec();
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Payload with newlines should succeed");
    }

    #[test]
    fn test_payload_all_zeros() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![0u8; 100];
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Payload with all zeros should succeed");
    }

    #[test]
    fn test_payload_all_ones() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![1u8; 100];
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Payload with all ones should succeed");
    }

    #[test]
    fn test_payload_all_255() {
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![255u8; 100];
        let memo_data = create_valid_memo(burn_amount, payload);
        
        let result = validate_memo_amount(&memo_data, burn_amount);
        assert!(result.is_ok(), "Payload with all 255s should succeed");
    }
}

// ============================================================================
// Tests for validate_memo_length()
// ============================================================================

#[cfg(test)]
mod validate_memo_length_tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Valid Length Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_at_minimum_length() {
        let memo = vec![b'x'; MEMO_MIN_LENGTH]; // Exactly 69 bytes
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo at minimum length (69) should succeed");
        assert!(result.unwrap());
    }

    #[test]
    fn test_memo_at_former_maximum_length() {
        let memo = vec![b'x'; 800]; // the former maximum
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo at maximum length (800) should succeed");
        assert!(result.unwrap());
    }

    #[test]
    fn test_memo_just_above_minimum() {
        let memo = vec![b'x'; MEMO_MIN_LENGTH + 1]; // 70 bytes
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo at 70 bytes should succeed");
    }

    #[test]
    fn test_memo_799_bytes() {
        let memo = vec![b'x'; 799];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo at 799 bytes should succeed");
    }

    #[test]
    fn test_memo_mid_range() {
        let memo = vec![b'x'; 400]; // Middle of range
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Memo at 400 bytes should succeed");
    }

    #[test]
    fn test_memo_various_valid_lengths() {
        let valid_lengths = vec![69, 70, 100, 200, 300, 400, 500, 600, 700, 799, 800];
        
        for length in valid_lengths {
            let memo = vec![b'x'; length];
            let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
            assert!(result.is_ok(), "Memo at {} bytes should succeed", length);
        }
    }

    // ------------------------------------------------------------------------
    // Too Short Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_empty() {
        let memo = vec![];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "Empty memo should fail");
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("MemoTooShort") || err_str.contains("Memo too short"));
    }

    #[test]
    fn test_memo_one_byte() {
        let memo = vec![b'x'; 1];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "1-byte memo should fail");
    }

    #[test]
    fn test_memo_just_below_minimum() {
        let memo = vec![b'x'; MEMO_MIN_LENGTH - 1]; // 68 bytes
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_err(), "Memo at 68 bytes should fail");
    }

    #[test]
    fn test_memo_various_short_lengths() {
        let short_lengths = vec![0, 1, 10, 20, 30, 40, 50, 60, 68];
        
        for length in short_lengths {
            let memo = vec![b'x'; length];
            let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
            assert!(result.is_err(), "Memo at {} bytes should fail (too short)", length);
        }
    }

    // ------------------------------------------------------------------------
    // Long Memo Tests (no contract maximum; the transaction size limit applies)
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_801_bytes_accepted() {
        let memo = vec![b'x'; 801]; // rejected before the maximum was removed
        assert!(validate_memo_length(&memo, MEMO_MIN_LENGTH).unwrap());
    }

    #[test]
    fn test_memo_various_long_lengths_accepted() {
        for length in [801, 850, 900, 1000, 1500, 2000, 4096] {
            let memo = vec![b'x'; length];
            assert!(validate_memo_length(&memo, MEMO_MIN_LENGTH).unwrap(), "Memo at {} bytes should be accepted", length);
        }
    }

    // ------------------------------------------------------------------------
    // Content Type Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_memo_ascii_text() {
        // Create exactly 69 bytes (69 'x' characters)
        let memo = vec![b'x'; 69];
        assert_eq!(memo.len(), 69, "Memo should be exactly 69 bytes");
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "ASCII text memo should succeed");
    }

    #[test]
    fn test_memo_utf8_text() {
        let mut memo = "Hello 世界 🔥 ".repeat(10).into_bytes();
        memo.truncate(100); // Make it valid length
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "UTF-8 text memo should succeed");
    }

    #[test]
    fn test_memo_base64_data() {
        // Base64 encoding increases size by ~33%, so 52 bytes raw -> ~69 bytes base64
        let data = vec![0u8; 52];
        let base64_encoded = general_purpose::STANDARD.encode(&data);
        let memo = base64_encoded.into_bytes();
        assert!(memo.len() >= MEMO_MIN_LENGTH, "Base64 memo should be at least 69 bytes, got {}", memo.len());
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Base64 encoded memo should succeed");
    }

    #[test]
    fn test_memo_binary_data() {
        let memo = (0..100).map(|i| (i % 256) as u8).collect::<Vec<u8>>();
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Binary data memo should succeed");
    }

    #[test]
    fn test_memo_all_zeros() {
        let memo = vec![0u8; 100];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "All-zeros memo should succeed");
    }

    #[test]
    fn test_memo_all_ones() {
        let memo = vec![1u8; 100];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "All-ones memo should succeed");
    }

    #[test]
    fn test_memo_all_255() {
        let memo = vec![255u8; 100];
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "All-255 memo should succeed");
    }

    #[test]
    fn test_memo_mixed_binary() {
        let memo = vec![0, 1, 2, 3, 255, 254, 253, 128, 127, 64].repeat(10);
        let result = validate_memo_length(&memo, MEMO_MIN_LENGTH);
        assert!(result.is_ok(), "Mixed binary memo should succeed");
    }
}

// ============================================================================
// Tests for BurnMemo Structure
// ============================================================================

#[cfg(test)]
mod burn_memo_structure_tests {
    use super::*;

    #[test]
    fn test_burn_memo_serialization_deserialization() {
        let memo = BurnMemo {
            version: 1,
            burn_amount: 10_000_000,
            payload: b"test payload".to_vec(),
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();

        assert_eq!(deserialized.version, memo.version);
        assert_eq!(deserialized.burn_amount, memo.burn_amount);
        assert_eq!(deserialized.payload, memo.payload);
    }

    #[test]
    fn test_burn_memo_empty_payload() {
        let memo = BurnMemo {
            version: 1,
            burn_amount: 1_000_000,
            payload: vec![],
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();

        assert_eq!(deserialized.payload.len(), 0);
    }

    #[test]
    fn test_burn_memo_large_payload() {
        let memo = BurnMemo {
            version: 1,
            burn_amount: 100_000_000,
            payload: vec![b'x'; 500],
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();

        assert_eq!(deserialized.payload.len(), 500);
    }

    #[test]
    fn test_burn_memo_maximum_payload() {
        let memo = BurnMemo {
            version: 1,
            burn_amount: 1_000_000_000,
            payload: vec![b'x'; LARGE_PAYLOAD],
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        assert_eq!(serialized.len(), BORSH_FIXED_OVERHEAD + LARGE_PAYLOAD);
        
        let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();
        assert_eq!(deserialized.payload.len(), LARGE_PAYLOAD);
    }

    #[test]
    fn test_burn_memo_borsh_size_calculation() {
        // Test that BORSH_FIXED_OVERHEAD is correct
        let memo = BurnMemo {
            version: 1,
            burn_amount: 1_000_000,
            payload: vec![],
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        assert_eq!(serialized.len(), BORSH_FIXED_OVERHEAD, 
            "Empty payload should result in exactly BORSH_FIXED_OVERHEAD bytes");
    }

    #[test]
    fn test_burn_memo_with_various_amounts() {
        let amounts = vec![
            1,
            1_000_000,
            10_000_000,
            100_000_000,
            1_000_000_000,
            u64::MAX,
        ];

        for amount in amounts {
            let memo = BurnMemo {
                version: 1,
                burn_amount: amount,
                payload: b"test".to_vec(),
            };

            let serialized = borsh::to_vec(&memo).unwrap();
            let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();

            assert_eq!(deserialized.burn_amount, amount);
        }
    }

    #[test]
    fn test_burn_memo_binary_payload() {
        let memo = BurnMemo {
            version: 1,
            burn_amount: 5_000_000,
            payload: vec![0, 1, 2, 255, 254, 253],
        };

        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized: BurnMemo = BurnMemo::try_from_slice(&serialized).unwrap();

        assert_eq!(deserialized.payload, vec![0, 1, 2, 255, 254, 253]);
    }
}

// ============================================================================
// Tests for UserGlobalBurnStats
// ============================================================================

#[cfg(test)]
mod user_global_burn_stats_tests {
    use super::*;

    #[test]
    fn test_user_global_burn_stats_space_constant() {
        // Verify SPACE constant is correct
        let expected_space = 8 + // discriminator
            32 + // user (Pubkey)
            8 +  // total_burned (u64)
            8 +  // burn_count (u64)
            8 +  // last_burn_time (i64)
            1;   // bump (u8)
        
        assert_eq!(UserGlobalBurnStats::SPACE, expected_space);
        assert_eq!(UserGlobalBurnStats::SPACE, 65);
    }

    // The user stats update (18T cap, burn_count, last_burn_time) is inline in process_burn and
    // cannot be unit-tested without changing lib.rs. It is checked exactly on-chain by
    // clients/regression/src/regression-memo-curve.rs (cap) and regression-memo-burn.rs (deltas).
}

// ============================================================================
// Tests for Constants
// ============================================================================

#[cfg(test)]
mod constants_tests {
    use super::*;

    #[test]
    fn test_decimal_factor() {
        assert_eq!(DECIMAL_FACTOR, 1_000_000, "Decimal factor should be 1,000,000 for 6 decimals");
    }

    #[test]
    fn test_min_burn_amount() {
        assert_eq!(MIN_BURN_AMOUNT, 1, "Minimum burn should be 1 unit");
    }

    #[test]
    fn test_max_burn_per_tx() {
        assert_eq!(MAX_BURN_PER_INSTRUCTION, 1_000_000_000_000 * DECIMAL_FACTOR, 
            "Maximum burn should be 1 trillion tokens");
    }

    #[test]
    fn test_burn_memo_version() {
        assert_eq!(BURN_MEMO_VERSION, 1, "Current version should be 1");
    }

    #[test]
    fn test_memo_length_constants() {
        assert_eq!(MEMO_MIN_LENGTH, 69, "Minimum memo length should be 69 bytes");
    }

    #[test]
    fn test_borsh_overhead_constants() {
        assert_eq!(BORSH_U8_SIZE, 1);
        assert_eq!(BORSH_U64_SIZE, 8);
        assert_eq!(BORSH_VEC_LENGTH_SIZE, 4);
        assert_eq!(BORSH_FIXED_OVERHEAD, 13, "Fixed overhead should be 1 + 8 + 4 = 13");
    }

    #[test]
    fn test_large_payload_fits_in_800_byte_memo() {
        // Reference: a 587-byte payload encodes to exactly 800 Base64 bytes
        let memo = BurnMemo { version: BURN_MEMO_VERSION, burn_amount: DECIMAL_FACTOR, payload: vec![b'x'; LARGE_PAYLOAD] };
        let encoded = general_purpose::STANDARD.encode(borsh::to_vec(&memo).unwrap());
        assert_eq!(encoded.len(), 800);
    }

    #[test]
    fn test_max_user_global_burn_amount() {
        assert_eq!(MAX_USER_GLOBAL_BURN_AMOUNT, 18_000_000_000_000 * DECIMAL_FACTOR);
        assert!(MAX_USER_GLOBAL_BURN_AMOUNT > MAX_BURN_PER_INSTRUCTION, 
            "Max global burn should be greater than max per tx");
    }

    #[test]
    fn test_constants_no_overflow() {
        // Verify that constants don't cause overflow
        let _ = MAX_BURN_PER_INSTRUCTION; // Should not panic
        let _ = MAX_USER_GLOBAL_BURN_AMOUNT; // Should not panic
        
        // Verify arithmetic operations
        let test_add = MAX_BURN_PER_INSTRUCTION.checked_add(DECIMAL_FACTOR);
        assert!(test_add.is_some(), "Should not overflow");
    }

    #[test]
    fn test_constants_relationships() {
        // Verify logical relationships between constants
        assert!(MEMO_MIN_LENGTH > BORSH_FIXED_OVERHEAD, 
            "Min memo length should be greater than Borsh overhead");
        assert!(MIN_BURN_AMOUNT <= MAX_BURN_PER_INSTRUCTION, 
            "Min burn should be less than or equal to max burn");
    }
}

// ============================================================================
// Integration Tests (Cross-function)
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_memo_validation_flow() {
        // Simulate complete memo validation flow
        let burn_amount = 10 * DECIMAL_FACTOR;
        // Use longer payload to ensure Base64 encoded result is >= 69 bytes
        let payload = b"Integration test payload with enough data to meet minimum length requirements".to_vec();
        
        // Create Borsh memo
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        
        // Serialize to Borsh
        let borsh_data = borsh::to_vec(&memo).unwrap();
        
        // Encode to Base64
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_bytes = base64_encoded.into_bytes();
        
        // Ensure memo is long enough
        assert!(memo_bytes.len() >= MEMO_MIN_LENGTH, 
            "Base64 memo should be at least {} bytes, got {}", MEMO_MIN_LENGTH, memo_bytes.len());
        
        // Validate length
        let length_result = validate_memo_length(&memo_bytes, MEMO_MIN_LENGTH);
        assert!(length_result.is_ok(), "Length validation should pass");
        
        // Validate amount
        let amount_result = validate_memo_amount(&memo_bytes, burn_amount);
        assert!(amount_result.is_ok(), "Amount validation should pass");
    }

    #[test]
    fn test_memo_size_boundaries() {
        // An empty payload gives a 20-byte memo: a valid BurnMemo but below the 69-byte minimum
        let burn_amount = 1 * DECIMAL_FACTOR;
        let memo = BurnMemo { version: BURN_MEMO_VERSION, burn_amount, payload: vec![] };
        let memo_bytes = general_purpose::STANDARD.encode(borsh::to_vec(&memo).unwrap()).into_bytes();
        assert_eq!(memo_bytes.len(), 20);
        assert!(validate_memo_amount(&memo_bytes, burn_amount).is_ok());
        assert!(validate_memo_length(&memo_bytes, MEMO_MIN_LENGTH).is_err());
    }

    #[test]
    fn test_various_burn_scenarios() {
        // Payloads of at least 39 bytes so every memo also passes the 69-byte minimum
        let scenarios = vec![
            (1, b"Burn 1 unit (0.000001 token), padded to the minimum".to_vec()),
            (1_234_567, b"Burn 1.234567 tokens, a fractional amount, padded".to_vec()),
            (1 * DECIMAL_FACTOR, b"Burn 1 token, padded up to the minimum memo length".to_vec()),
            (1_000 * DECIMAL_FACTOR, b"Burn 1,000 tokens, padded up to the minimum length".to_vec()),
        ];

        for (burn_amount, payload) in scenarios {
            let memo = BurnMemo {
                version: BURN_MEMO_VERSION,
                burn_amount,
                payload,
            };
            let borsh_data = borsh::to_vec(&memo).unwrap();
            let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
            let memo_bytes = base64_encoded.into_bytes();
            
            assert!(validate_memo_length(&memo_bytes, MEMO_MIN_LENGTH).is_ok(), "memo for {} units is too short", burn_amount);
            assert!(validate_memo_amount(&memo_bytes, burn_amount).is_ok(), "Scenario with {} units should succeed", burn_amount);
            assert!(validate_memo_amount(&memo_bytes, burn_amount + 1).is_err(), "memo for {} units must not match +1", burn_amount);
        }
    }

    #[test]
    fn test_payload_size_calculation() {
        // Verify that payload size calculation is correct
        for payload_size in [0, 10, 50, 100, 200, 500, LARGE_PAYLOAD] {
            let burn_amount = 10 * DECIMAL_FACTOR;
            let payload = vec![b'x'; payload_size];
            
            let memo = BurnMemo {
                version: BURN_MEMO_VERSION,
                burn_amount,
                payload: payload.clone(),
            };
            
            let borsh_data = borsh::to_vec(&memo).unwrap();
            let expected_size = BORSH_FIXED_OVERHEAD + payload_size;
            
            assert_eq!(borsh_data.len(), expected_size, 
                "Borsh data size should be overhead + payload for {} byte payload", payload_size);
        }
    }

    #[test]
    fn test_base64_encoding_overhead() {
        // Verify Base64 encoding overhead
        let burn_amount = 10 * DECIMAL_FACTOR;
        let payload = vec![b'x'; 100];
        
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        
        // Base64 encoding increases size by ~33% (4/3 ratio)
        let expected_min_size = (borsh_data.len() * 4) / 3;
        let expected_max_size = expected_min_size + 4; // Padding
        
        assert!(base64_encoded.len() >= expected_min_size && base64_encoded.len() <= expected_max_size,
            "Base64 size {} should be between {} and {}", 
            base64_encoded.len(), expected_min_size, expected_max_size);
    }
}

// ============================================================================
// Comprehensive Test Summary
// ============================================================================

#[cfg(test)]
mod test_coverage_summary {
    // This module serves as documentation for test coverage
    //
    // Unit-tested here (run `cargo test -p memo-burn --lib` for the exact count):
    // - validate_memo_amount: strict Base64 (standard alphabet, padding required, canonical
    //   trailing bits, no whitespace), payload length prefix, version, exact amount binding
    //   (fractional amounts included), differential check against BurnMemo::try_from_slice
    // - validate_memo_length (minimum 69 bytes, no maximum) and the effective 72-byte minimum
    // - check_memo_instruction against real instructions-sysvar data (memo position rules)
    // - GlobalBurnStats::record_burn (accumulation and saturation)
    // - constants and error code numbering
    //
    // Not unit-testable without changing lib.rs, covered by the on-chain regression suite
    // (scripts/run-regression-tests.sh, clients/regression/src/):
    // - process_burn amount bounds (0, 1 unit, fractional, 1T, 1T + 1, u64::MAX)
    // - user stats update (total, 18T cap, burn_count, last_burn_time) and GlobalBurnStats deltas
    // - Accounts constraints (owner, delegate, mint, stats seeds, sysvar address)
    // - initialize_global_burn_stats admin check (fresh state)
}


// ============================================================================
// Tests for GlobalBurnStats
// ============================================================================

#[cfg(test)]
mod global_burn_stats_tests {
    use super::*;

    fn empty_stats() -> GlobalBurnStats {
        GlobalBurnStats { total_burned: 0, burn_count: 0, last_burn_time: 0, bump: 255, reserved: [0u8; 64] }
    }

    #[test]
    fn test_global_burn_stats_space() {
        // discriminator + u128 + u64 + i64 + u8 + 64 reserved
        assert_eq!(GlobalBurnStats::SPACE, 8 + 16 + 8 + 8 + 1 + 64);
        let serialized = borsh::to_vec(&empty_stats()).unwrap();
        assert_eq!(serialized.len() + 8, GlobalBurnStats::SPACE, "SPACE must match the serialized size plus discriminator");
    }

    #[test]
    fn test_record_burn_accumulates() {
        let mut stats = empty_stats();
        stats.record_burn(5 * DECIMAL_FACTOR, 100);
        stats.record_burn(7 * DECIMAL_FACTOR, 200);
        assert_eq!(stats.total_burned, 12 * DECIMAL_FACTOR as u128);
        assert_eq!(stats.burn_count, 2);
        assert_eq!(stats.last_burn_time, 200);
    }

    #[test]
    fn test_total_burned_beyond_u64_range() {
        // u128 keeps counting past what a u64 could hold
        let mut stats = empty_stats();
        stats.total_burned = u64::MAX as u128;
        stats.record_burn(MAX_BURN_PER_INSTRUCTION, 1);
        assert_eq!(stats.total_burned, u64::MAX as u128 + MAX_BURN_PER_INSTRUCTION as u128);
    }

    #[test]
    fn test_total_burned_saturates_at_max() {
        let mut stats = empty_stats();
        stats.total_burned = u128::MAX - 1;
        stats.record_burn(MAX_BURN_PER_INSTRUCTION, 1);
        assert_eq!(stats.total_burned, u128::MAX, "total_burned must stop at u128::MAX");
        stats.record_burn(DECIMAL_FACTOR, 2);
        assert_eq!(stats.total_burned, u128::MAX, "total_burned must stay at u128::MAX");
    }

    #[test]
    fn test_burn_count_saturates_at_max() {
        let mut stats = empty_stats();
        stats.burn_count = u64::MAX;
        stats.record_burn(DECIMAL_FACTOR, 1);
        assert_eq!(stats.burn_count, u64::MAX, "burn_count must stop at u64::MAX");
        assert_eq!(stats.total_burned, DECIMAL_FACTOR as u128, "total_burned still updates when burn_count is capped");
    }

    #[test]
    fn test_admin_pubkey_matches_other_programs() {
        // Same admin as memo-chat / memo-project / memo-forum for the build's network
        #[cfg(not(feature = "mainnet"))]
        assert_eq!(AUTHORIZED_ADMIN_PUBKEY.to_string(), "Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");
        #[cfg(feature = "mainnet")]
        assert_eq!(AUTHORIZED_ADMIN_PUBKEY.to_string(), "FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ");
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

    fn check_with_key(key: Pubkey, ixs: &[Ix], current_index: u16) -> Result<Option<Vec<u8>>> {
        let mut data = sysvar_data(ixs, current_index);
        let owner = anchor_lang::solana_program::sysvar::ID;
        let mut lamports = 0u64;
        let info = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false, 0);
        check_memo_instruction(&info)
    }

    fn check(ixs: &[Ix], current_index: u16) -> Result<Option<Vec<u8>>> {
        check_with_key(INSTRUCTIONS_ID, ixs, current_index)
    }

    fn burn_memo_bytes(amount: u64) -> Vec<u8> {
        let memo = BurnMemo { version: BURN_MEMO_VERSION, burn_amount: amount, payload: vec![b'p'; 60] };
        general_purpose::STANDARD.encode(borsh::to_vec(&memo).unwrap()).into_bytes()
    }

    fn memo(data: Vec<u8>) -> Ix {
        Ix { program_id: MEMO_PROGRAM_ID, accounts: vec![], data }
    }

    fn burn() -> Ix {
        Ix { program_id: crate::ID, accounts: vec![Meta::new(Pubkey::new_unique(), true)], data: vec![0; 16] }
    }

    fn other(program_id: Pubkey, data: Vec<u8>) -> Ix {
        Ix { program_id, accounts: vec![], data }
    }

    fn is_memo_too_short(result: Result<Option<Vec<u8>>>) -> bool {
        matches!(result, Err(e) if e.to_string().contains("MemoTooShort"))
    }

    #[test]
    fn test_memo_at_index_0_returned_exactly() {
        let good = burn_memo_bytes(7);
        assert!(good.len() >= MEMO_MIN_LENGTH);
        assert_eq!(check(&[memo(good.clone()), burn()], 1).unwrap(), Some(good.clone()));
        // burn later than index 1, with other instructions in between
        assert_eq!(check(&[memo(good.clone()), other(COMPUTE_BUDGET, vec![2; 5]), burn(), burn()], 3).unwrap(), Some(good));
    }

    #[test]
    fn test_memo_only_right_before_the_burn_rejected() {
        let good = burn_memo_bytes(7);
        // the memo must be at index 0, not at current_index - 1
        assert_eq!(check(&[other(COMPUTE_BUDGET, vec![2; 5]), memo(good.clone()), burn()], 2).unwrap(), None);
        // and the memo at index 0 is used even when the instruction right before the burn is something else
        assert_eq!(check(&[memo(good.clone()), other(COMPUTE_BUDGET, vec![2; 5]), burn()], 2).unwrap(), Some(good));
    }

    #[test]
    fn test_index_0_must_be_spl_memo_v3() {
        let good = burn_memo_bytes(7);
        assert_eq!(check(&[other(LEGACY_MEMO_V1, good.clone()), burn()], 1).unwrap(), None);
        assert_eq!(check(&[other(COMPUTE_BUDGET, good.clone()), burn()], 1).unwrap(), None);
        assert_eq!(check(&[other(crate::ID, good), burn()], 1).unwrap(), None);
    }

    #[test]
    fn test_burn_at_index_0_rejected() {
        assert_eq!(check(&[burn()], 0).unwrap(), None);
        assert_eq!(check(&[burn(), memo(burn_memo_bytes(7))], 0).unwrap(), None);
    }

    #[test]
    fn test_short_memo_at_index_0_is_an_error() {
        assert!(is_memo_too_short(check(&[memo(vec![b'x'; MEMO_MIN_LENGTH - 1]), burn()], 1)));
        assert!(is_memo_too_short(check(&[memo(vec![]), burn()], 1)));
        assert!(is_memo_too_short(check(&[memo(vec![b'x'; 10]), memo(burn_memo_bytes(7)), burn()], 2)));
        assert!(check(&[memo(vec![b'x'; MEMO_MIN_LENGTH]), burn()], 1).unwrap().is_some());
    }

    #[test]
    fn test_wrong_sysvar_account_is_an_error() {
        assert!(check_with_key(Pubkey::new_unique(), &[memo(burn_memo_bytes(7)), burn()], 1).is_err());
    }
}

// ============================================================================
// Strict Base64, amount binding and memo length boundaries
// ============================================================================

#[cfg(test)]
mod strict_memo_format_tests {
    use super::*;

    fn raw(version: u8, amount: u64, declared_len: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![version];
        v.extend(amount.to_le_bytes());
        v.extend(declared_len.to_le_bytes());
        v.extend(payload);
        v
    }

    fn valid_raw(amount: u64, payload_len: usize) -> Vec<u8> {
        let payload: Vec<u8> = (0..payload_len).map(|i| (i * 7 + 3) as u8).collect();
        raw(BURN_MEMO_VERSION, amount, payload_len as u32, &payload)
    }

    fn is_invalid_format(result: Result<()>) -> bool {
        matches!(result, Err(e) if e.to_string().contains("InvalidMemoFormat"))
    }

    #[test]
    fn test_padding_is_required() {
        // 13 + 58 = 71 decoded bytes: the standard encoding ends with one '='
        let r = valid_raw(5, 58);
        let padded = general_purpose::STANDARD.encode(&r);
        let unpadded = general_purpose::STANDARD_NO_PAD.encode(&r);
        assert!(padded.ends_with('='));
        assert_ne!(padded, unpadded);
        assert!(validate_memo_amount(padded.as_bytes(), 5).is_ok());
        assert!(is_invalid_format(validate_memo_amount(unpadded.as_bytes(), 5)));
        // extra padding is rejected too
        assert!(is_invalid_format(validate_memo_amount(format!("{}==", padded).as_bytes(), 5)));
    }

    #[test]
    fn test_non_canonical_trailing_bits_rejected() {
        let r = valid_raw(5, 58);
        let mut enc = general_purpose::STANDARD.encode(&r).into_bytes();
        let pad = enc.iter().filter(|&&c| c == b'=').count();
        assert!(pad > 0);
        let idx = enc.len() - pad - 1;
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let pos = alphabet.iter().position(|&c| c == enc[idx]).unwrap();
        enc[idx] = alphabet[pos ^ 1]; // flips an unused low bit: decodes to the same bytes in a lenient engine
        assert!(is_invalid_format(validate_memo_amount(&enc, 5)));
    }

    #[test]
    fn test_url_safe_alphabet_rejected() {
        // bytes FB EF BE encode to "++++" (standard) / "----" (URL-safe)
        let mut payload = vec![b'p'; 60];
        payload[2] = 0xFB;
        payload[3] = 0xEF;
        payload[4] = 0xBE;
        let r = raw(BURN_MEMO_VERSION, 5, 60, &payload);
        let std_enc = general_purpose::STANDARD.encode(&r);
        let url_enc = general_purpose::URL_SAFE.encode(&r);
        assert!(std_enc.contains('+') && url_enc.contains('-'));
        assert!(validate_memo_amount(std_enc.as_bytes(), 5).is_ok());
        assert!(is_invalid_format(validate_memo_amount(url_enc.as_bytes(), 5)));
    }

    #[test]
    fn test_whitespace_and_trailing_data_rejected() {
        let enc = general_purpose::STANDARD.encode(valid_raw(5, 60));
        assert!(validate_memo_amount(enc.as_bytes(), 5).is_ok());
        for bad in [
            format!("{}\n{}", &enc[..40], &enc[40..]),
            format!("{}\n", enc),
            format!(" {}", enc),
            format!("{} ", enc),
            format!("{}AAAA", enc),
            format!("{}!!", enc),
        ] {
            assert!(is_invalid_format(validate_memo_amount(bad.as_bytes(), 5)), "accepted {:?}", bad);
        }
    }

    #[test]
    fn test_amount_binding_is_exact_in_units() {
        for amount in [1u64, 2, 999_999, 1_000_000, 1_000_001, 1_234_567, MAX_BURN_PER_INSTRUCTION, MAX_BURN_PER_INSTRUCTION + 1, u64::MAX] {
            let enc = general_purpose::STANDARD.encode(valid_raw(amount, 60));
            assert!(validate_memo_amount(enc.as_bytes(), amount).is_ok(), "amount {}", amount);
            if amount > 0 {
                let r = validate_memo_amount(enc.as_bytes(), amount - 1);
                assert!(matches!(r, Err(e) if e.to_string().contains("BurnAmountMismatch")), "amount {} - 1", amount);
            }
            if amount < u64::MAX {
                let r = validate_memo_amount(enc.as_bytes(), amount + 1);
                assert!(matches!(r, Err(e) if e.to_string().contains("BurnAmountMismatch")), "amount {} + 1", amount);
            }
            // a memo for the whole-token rounding of a fractional amount does not match
            let rounded = amount / DECIMAL_FACTOR * DECIMAL_FACTOR;
            if rounded != amount {
                let enc_rounded = general_purpose::STANDARD.encode(valid_raw(rounded, 60));
                assert!(validate_memo_amount(enc_rounded.as_bytes(), amount).is_err(), "rounded memo for {}", amount);
            }
        }
    }

    #[test]
    fn test_effective_minimum_memo_length_is_72() {
        // validate_memo_length accepts 69..=71 bytes, but no canonical padded Base64 string has that
        // length, so such memos fail validate_memo_amount; 72 is the shortest memo passing both
        let canonical = |payload_len: usize| general_purpose::STANDARD.encode(valid_raw(5, payload_len)).into_bytes();
        let m68 = canonical(38);
        assert_eq!(m68.len(), 68);
        assert!(validate_memo_length(&m68, MEMO_MIN_LENGTH).is_err());

        let mut m69 = canonical(38);
        m69.push(b'=');
        let m70 = general_purpose::STANDARD_NO_PAD.encode(valid_raw(5, 39)).into_bytes();
        let m71 = general_purpose::STANDARD_NO_PAD.encode(valid_raw(5, 40)).into_bytes();
        for (len, m) in [(69, &m69), (70, &m70), (71, &m71)] {
            assert_eq!(m.len(), len);
            assert!(validate_memo_length(m, MEMO_MIN_LENGTH).is_ok());
            assert!(is_invalid_format(validate_memo_amount(m, 5)), "{}-byte memo accepted", len);
        }

        let m72 = canonical(39);
        assert_eq!(m72.len(), 72);
        assert!(validate_memo_length(&m72, MEMO_MIN_LENGTH).is_ok());
        assert!(validate_memo_amount(&m72, 5).is_ok());
    }

    /// Reference semantics: strict standard Base64, then BurnMemo::try_from_slice, version and amount
    fn reference(memo: &[u8], expected: u64) -> bool {
        let Ok(s) = std::str::from_utf8(memo) else { return false };
        let Ok(decoded) = general_purpose::STANDARD.decode(s) else { return false };
        // a declared payload length beyond the bytes present can never deserialize (and would
        // make Borsh try to allocate it)
        if decoded.len() >= BORSH_FIXED_OVERHEAD {
            let declared = u32::from_le_bytes(decoded[BORSH_PAYLOAD_LEN_OFFSET..BORSH_FIXED_OVERHEAD].try_into().unwrap()) as usize;
            if declared > decoded.len() - BORSH_FIXED_OVERHEAD {
                return false;
            }
        }
        match BurnMemo::try_from_slice(&decoded) {
            Ok(m) => m.version == BURN_MEMO_VERSION && m.burn_amount == expected,
            Err(_) => false,
        }
    }

    #[test]
    fn test_differential_against_borsh_deserialization() {
        // xorshift64: deterministic, no extra dependency
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let (mut accepted, mut rejected) = (0u32, 0u32);
        for i in 0..100_000u32 {
            let amount = match i % 4 {
                0 => next(),
                1 => next() % 10,
                2 => MAX_BURN_PER_INSTRUCTION,
                _ => next() % (u32::MAX as u64),
            };
            let payload_len = (next() % 120) as usize;
            let mut r = valid_raw(amount, payload_len);
            let mut expected = amount;
            match next() % 10 {
                0 => {}
                1 => { let j = (next() as usize) % r.len(); r[j] = next() as u8; }
                2 => { r[0] = next() as u8; }
                3 => {
                    let d = (next() % 7) as i64 - 3;
                    let l = (payload_len as i64 + d).max(0) as u32;
                    r[9..13].copy_from_slice(&l.to_le_bytes());
                }
                4 => r.push(next() as u8),
                5 => { let l = (next() as usize) % r.len(); r.truncate(l); }
                6 => expected = expected.wrapping_add(1 + next() % 2),
                7 => r[9..13].copy_from_slice(&(next() as u32).to_le_bytes()),
                8 => { let j = 1 + (next() as usize) % 8; r[j] ^= 1 << (next() % 8); }
                _ => { r = (0..(next() % 40)).map(|_| next() as u8).collect(); }
            }
            let mut memo = general_purpose::STANDARD.encode(&r).into_bytes();
            match next() % 12 {
                0 => if !memo.is_empty() { let j = (next() as usize) % memo.len(); memo[j] = next() as u8; },
                1 => while memo.last() == Some(&b'=') { memo.pop(); },
                2 => { let j = (next() as usize) % (memo.len() + 1); memo.insert(j, b'\n'); }
                3 => { let j = (next() as usize) % (memo.len() + 1); memo.insert(j, b' '); }
                4 => for c in memo.iter_mut() { if *c == b'+' { *c = b'-' } else if *c == b'/' { *c = b'_' } },
                5 => memo.extend_from_slice(b"=="),
                _ => {}
            }
            let real = validate_memo_amount(&memo, expected).is_ok();
            assert_eq!(real, reference(&memo, expected), "divergence on {:?} (expected {})", String::from_utf8_lossy(&memo), expected);
            if real { accepted += 1 } else { rejected += 1 }
        }
        assert!(accepted > 5_000 && rejected > 5_000, "accepted {} rejected {}", accepted, rejected);
    }
}

// ============================================================================
// Error code numbering
// ============================================================================

#[cfg(test)]
mod error_code_tests {
    use super::*;

    /// Clients match error codes by number, so variants may only be appended.
    /// InvalidBurnAmount (6005) is no longer returned but must keep its slot.
    #[test]
    fn test_error_codes_unchanged_and_appended() {
        let code = |e: ErrorCode| u32::from(e);
        assert_eq!(code(ErrorCode::MemoRequired), 6000);
        assert_eq!(code(ErrorCode::InvalidMemoFormat), 6001);
        assert_eq!(code(ErrorCode::UnsupportedMemoVersion), 6002);
        assert_eq!(code(ErrorCode::BurnAmountTooSmall), 6003);
        assert_eq!(code(ErrorCode::BurnAmountTooLarge), 6004);
        assert_eq!(code(ErrorCode::InvalidBurnAmount), 6005);
        assert_eq!(code(ErrorCode::InvalidTokenAccount), 6006);
        assert_eq!(code(ErrorCode::UnauthorizedMint), 6007);
        assert_eq!(code(ErrorCode::UnauthorizedTokenAccount), 6008);
        assert_eq!(code(ErrorCode::BurnAmountMismatch), 6009);
        assert_eq!(code(ErrorCode::MemoTooShort), 6010);
        assert_eq!(code(ErrorCode::UnauthorizedUser), 6011);
        assert_eq!(code(ErrorCode::UnauthorizedAdmin), 6012);
    }
}
