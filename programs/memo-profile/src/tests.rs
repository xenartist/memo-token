//! Unit tests for memo-profile contract
//! 
//! This test suite provides comprehensive coverage of all core functions:
//! - ProfileCreationData::validate(): Validation of profile creation data
//! - ProfileUpdateData::validate(): Validation of profile update data
//! - parse_profile_creation_borsh_memo(): Borsh+Base64 memo parsing for creation
//! - parse_profile_update_borsh_memo(): Borsh+Base64 memo parsing for updates
//! - validate_memo_length(): Memo length validation (69-800 bytes)
//! - BurnMemo structure: Serialization and deserialization
//! - Constants: Verify all constant values and relationships

use super::*;
use base64::engine::general_purpose;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a valid Borsh+Base64 encoded memo for profile creation
fn create_profile_creation_memo(
    burn_amount: u64,
    user_pubkey: Pubkey,
    username: &str,
    image: &str,
    about_me: Option<String>,
) -> Vec<u8> {
    let profile_data = ProfileCreationData {
        version: PROFILE_CREATION_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_OPERATION.to_string(),
        user_pubkey: user_pubkey.to_string(),
        username: username.to_string(),
        image: image.to_string(),
        about_me,
    };
    
    let payload = borsh::to_vec(&profile_data).unwrap();
    
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };
    
    let borsh_data = borsh::to_vec(&burn_memo).unwrap();
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    base64_encoded.into_bytes()
}

/// Create a valid Borsh+Base64 encoded memo for profile update
fn create_profile_update_memo(
    burn_amount: u64,
    user_pubkey: Pubkey,
    username: Option<String>,
    image: Option<String>,
    about_me: Option<Option<String>>,
) -> Vec<u8> {
    let profile_data = ProfileUpdateData {
        version: PROFILE_UPDATE_DATA_VERSION,
        category: EXPECTED_CATEGORY.to_string(),
        operation: EXPECTED_UPDATE_OPERATION.to_string(),
        user_pubkey: user_pubkey.to_string(),
        username,
        image,
        about_me,
    };
    
    let payload = borsh::to_vec(&profile_data).unwrap();
    
    let burn_memo = BurnMemo {
        version: BURN_MEMO_VERSION,
        burn_amount,
        payload,
    };
    
    let borsh_data = borsh::to_vec(&burn_memo).unwrap();
    let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
    base64_encoded.into_bytes()
}

// ============================================================================
// Constants Tests
// ============================================================================

#[cfg(test)]
mod constants_tests {
    use super::*;

    #[test]
    fn test_decimal_factor() {
        assert_eq!(DECIMAL_FACTOR, 1_000_000, "DECIMAL_FACTOR should be 1,000,000");
    }

    #[test]
    fn test_min_profile_creation_burn() {
        assert_eq!(MIN_PROFILE_CREATION_BURN_TOKENS, 420, "Minimum profile creation burn should be 420 tokens");
        assert_eq!(
            MIN_PROFILE_CREATION_BURN_AMOUNT,
            420 * DECIMAL_FACTOR,
            "MIN_PROFILE_CREATION_BURN_AMOUNT should match tokens * decimal factor"
        );
    }

    #[test]
    fn test_min_profile_update_burn() {
        assert_eq!(MIN_PROFILE_UPDATE_BURN_TOKENS, 420, "Minimum profile update burn should be 420 tokens");
        assert_eq!(
            MIN_PROFILE_UPDATE_BURN_AMOUNT,
            420 * DECIMAL_FACTOR,
            "MIN_PROFILE_UPDATE_BURN_AMOUNT should match tokens * decimal factor"
        );
    }

    #[test]
    fn test_max_burn_per_tx() {
        assert_eq!(
            MAX_BURN_PER_TX,
            1_000_000_000_000 * DECIMAL_FACTOR,
            "Maximum burn should be 1 trillion tokens"
        );
    }

    #[test]
    fn test_string_length_constraints() {
        assert_eq!(MAX_USERNAME_LENGTH, 32, "Maximum username length should be 32");
        assert_eq!(MAX_PROFILE_IMAGE_LENGTH, 256, "Maximum profile image length should be 256");
        assert_eq!(MAX_ABOUT_ME_LENGTH, 128, "Maximum about me length should be 128");
    }

    #[test]
    fn test_memo_length_constraints() {
        assert_eq!(MEMO_MIN_LENGTH, 69, "Minimum memo length should be 69 bytes");
        assert_eq!(MEMO_MAX_LENGTH, 800, "Maximum memo length should be 800 bytes");
    }

    #[test]
    fn test_payload_length() {
        assert_eq!(
            MAX_PAYLOAD_LENGTH,
            MEMO_MAX_LENGTH - BORSH_FIXED_OVERHEAD,
            "Maximum payload should be memo max minus Borsh overhead"
        );
        assert_eq!(MAX_PAYLOAD_LENGTH, 787, "Maximum payload should be 787 bytes");
    }

    #[test]
    fn test_borsh_overhead() {
        assert_eq!(BORSH_FIXED_OVERHEAD, 13, "Borsh fixed overhead should be 13 bytes (1+8+4)");
    }

    #[test]
    fn test_burn_memo_version() {
        assert_eq!(BURN_MEMO_VERSION, 1, "Burn memo version should be 1");
    }

    #[test]
    fn test_profile_data_versions() {
        assert_eq!(PROFILE_CREATION_DATA_VERSION, 1, "Profile creation data version should be 1");
        assert_eq!(PROFILE_UPDATE_DATA_VERSION, 1, "Profile update data version should be 1");
    }

    #[test]
    fn test_expected_strings() {
        assert_eq!(EXPECTED_CATEGORY, "profile", "Expected category should be 'profile'");
        assert_eq!(EXPECTED_OPERATION, "create_profile", "Expected operation should be 'create_profile'");
        assert_eq!(EXPECTED_UPDATE_OPERATION, "update_profile", "Expected update operation should be 'update_profile'");
    }
}

// ============================================================================
// ProfileCreationData::validate() Tests
// ============================================================================

#[cfg(test)]
mod profile_creation_data_validate_tests {
    use super::*;

    fn create_valid_profile_creation_data(user_pubkey: Pubkey) -> ProfileCreationData {
        ProfileCreationData {
            version: PROFILE_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            user_pubkey: user_pubkey.to_string(),
            username: "alice".to_string(),
            image: "profile.png".to_string(),
            about_me: Some("Hello, world!".to_string()),
        }
    }

    #[test]
    fn test_valid_profile_creation_data() {
        let user = Pubkey::new_unique();
        let data = create_valid_profile_creation_data(user);
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Valid profile creation data should pass validation");
    }

    #[test]
    fn test_valid_profile_creation_data_minimal() {
        let user = Pubkey::new_unique();
        let data = ProfileCreationData {
            version: PROFILE_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: "a".to_string(),
            image: "".to_string(),
            about_me: None,
        };
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Valid minimal profile creation data should pass validation");
    }

    #[test]
    fn test_valid_profile_creation_data_max_lengths() {
        let user = Pubkey::new_unique();
        let data = ProfileCreationData {
            version: PROFILE_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: "a".repeat(MAX_USERNAME_LENGTH),
            image: "x".repeat(MAX_PROFILE_IMAGE_LENGTH),
            about_me: Some("y".repeat(MAX_ABOUT_ME_LENGTH)),
        };
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Profile data at maximum lengths should pass validation");
    }

    #[test]
    fn test_invalid_version() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.version = 99;
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid version should fail validation");
        assert!(matches!(result.unwrap_err(), anchor_lang::error::Error::AnchorError(_)));
    }

    #[test]
    fn test_invalid_category() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.category = "wrong_category".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid category should fail validation");
    }

    #[test]
    fn test_invalid_operation() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.operation = "update_profile".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid operation should fail validation");
    }

    #[test]
    fn test_invalid_user_pubkey_format() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.user_pubkey = "not_a_valid_pubkey".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid user pubkey format should fail validation");
    }

    #[test]
    fn test_user_pubkey_mismatch() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let data = create_valid_profile_creation_data(user1);
        
        let result = data.validate(user2);
        assert!(result.is_err(), "Mismatched user pubkey should fail validation");
    }

    #[test]
    fn test_empty_username() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.username = "".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Empty username should fail validation");
    }

    #[test]
    fn test_username_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.username = "a".repeat(MAX_USERNAME_LENGTH + 1);
        
        let result = data.validate(user);
        assert!(result.is_err(), "Username too long should fail validation");
    }

    #[test]
    fn test_image_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.image = "x".repeat(MAX_PROFILE_IMAGE_LENGTH + 1);
        
        let result = data.validate(user);
        assert!(result.is_err(), "Image too long should fail validation");
    }

    #[test]
    fn test_about_me_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_creation_data(user);
        data.about_me = Some("y".repeat(MAX_ABOUT_ME_LENGTH + 1));
        
        let result = data.validate(user);
        assert!(result.is_err(), "About me too long should fail validation");
    }
}

// ============================================================================
// ProfileUpdateData::validate() Tests
// ============================================================================

#[cfg(test)]
mod profile_update_data_validate_tests {
    use super::*;

    fn create_valid_profile_update_data(user_pubkey: Pubkey) -> ProfileUpdateData {
        ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user_pubkey.to_string(),
            username: Some("bob".to_string()),
            image: Some("new_profile.png".to_string()),
            about_me: Some(Some("Updated bio".to_string())),
        }
    }

    #[test]
    fn test_valid_profile_update_data() {
        let user = Pubkey::new_unique();
        let data = create_valid_profile_update_data(user);
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Valid profile update data should pass validation");
    }

    #[test]
    fn test_valid_profile_update_data_no_changes() {
        let user = Pubkey::new_unique();
        let data = ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: None,
            image: None,
            about_me: None,
        };
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Profile update with no changes should pass validation");
    }

    #[test]
    fn test_valid_profile_update_clear_about_me() {
        let user = Pubkey::new_unique();
        let data = ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: None,
            image: None,
            about_me: Some(None), // Clear the about_me field
        };
        
        let result = data.validate(user);
        assert!(result.is_ok(), "Profile update clearing about_me should pass validation");
    }

    #[test]
    fn test_invalid_version() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.version = 99;
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid version should fail validation");
    }

    #[test]
    fn test_invalid_category() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.category = "chat".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid category should fail validation");
    }

    #[test]
    fn test_invalid_operation() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.operation = "create_profile".to_string();
        
        let result = data.validate(user);
        assert!(result.is_err(), "Invalid operation should fail validation");
    }

    #[test]
    fn test_user_pubkey_mismatch() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let data = create_valid_profile_update_data(user1);
        
        let result = data.validate(user2);
        assert!(result.is_err(), "Mismatched user pubkey should fail validation");
    }

    #[test]
    fn test_empty_username() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.username = Some("".to_string());
        
        let result = data.validate(user);
        assert!(result.is_err(), "Empty username should fail validation");
    }

    #[test]
    fn test_username_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.username = Some("a".repeat(MAX_USERNAME_LENGTH + 1));
        
        let result = data.validate(user);
        assert!(result.is_err(), "Username too long should fail validation");
    }

    #[test]
    fn test_image_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.image = Some("x".repeat(MAX_PROFILE_IMAGE_LENGTH + 1));
        
        let result = data.validate(user);
        assert!(result.is_err(), "Image too long should fail validation");
    }

    #[test]
    fn test_about_me_too_long() {
        let user = Pubkey::new_unique();
        let mut data = create_valid_profile_update_data(user);
        data.about_me = Some(Some("y".repeat(MAX_ABOUT_ME_LENGTH + 1)));
        
        let result = data.validate(user);
        assert!(result.is_err(), "About me too long should fail validation");
    }
}

// ============================================================================
// validate_memo_length() Tests
// ============================================================================

#[cfg(test)]
mod validate_memo_length_tests {
    use super::*;

    #[test]
    fn test_valid_memo_minimum_length() {
        let memo_data = vec![b'x'; MEMO_MIN_LENGTH];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_ok(), "Memo at minimum length should be valid");
    }

    #[test]
    fn test_valid_memo_maximum_length() {
        let memo_data = vec![b'x'; MEMO_MAX_LENGTH];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_ok(), "Memo at maximum length should be valid");
    }

    #[test]
    fn test_valid_memo_mid_length() {
        let memo_data = vec![b'x'; 400];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_ok(), "Memo at mid-range length should be valid");
    }

    #[test]
    fn test_memo_too_short() {
        let memo_data = vec![b'x'; MEMO_MIN_LENGTH - 1];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_err(), "Memo below minimum length should fail");
    }

    #[test]
    fn test_memo_too_long() {
        let memo_data = vec![b'x'; MEMO_MAX_LENGTH + 1];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_err(), "Memo above maximum length should fail");
    }

    #[test]
    fn test_memo_empty() {
        let memo_data = vec![];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_err(), "Empty memo should fail");
    }

    #[test]
    fn test_memo_one_byte_short() {
        let memo_data = vec![b'x'; 68];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_err(), "Memo one byte short should fail");
    }

    #[test]
    fn test_memo_one_byte_long() {
        let memo_data = vec![b'x'; 801];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_err(), "Memo one byte long should fail");
    }
}

// ============================================================================
// BurnMemo Serialization Tests
// ============================================================================

#[cfg(test)]
mod burn_memo_serialization_tests {
    use super::*;

    #[test]
    fn test_burn_memo_serialize_deserialize() {
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 420 * DECIMAL_FACTOR,
            payload: b"test payload".to_vec(),
        };
        
        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized = BurnMemo::try_from_slice(&serialized).unwrap();
        
        assert_eq!(memo.version, deserialized.version);
        assert_eq!(memo.burn_amount, deserialized.burn_amount);
        assert_eq!(memo.payload, deserialized.payload);
    }

    #[test]
    fn test_burn_memo_empty_payload() {
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 1 * DECIMAL_FACTOR,
            payload: vec![],
        };
        
        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized = BurnMemo::try_from_slice(&serialized).unwrap();
        
        assert_eq!(memo.payload.len(), 0);
        assert_eq!(deserialized.payload.len(), 0);
    }

    #[test]
    fn test_burn_memo_max_payload() {
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 1000 * DECIMAL_FACTOR,
            payload: vec![b'x'; MAX_PAYLOAD_LENGTH],
        };
        
        let serialized = borsh::to_vec(&memo).unwrap();
        let deserialized = BurnMemo::try_from_slice(&serialized).unwrap();
        
        assert_eq!(memo.payload.len(), MAX_PAYLOAD_LENGTH);
        assert_eq!(deserialized.payload.len(), MAX_PAYLOAD_LENGTH);
    }
}

// ============================================================================
// ProfileCreationData Serialization Tests
// ============================================================================

#[cfg(test)]
mod profile_creation_data_serialization_tests {
    use super::*;

    #[test]
    fn test_profile_creation_data_serialize_deserialize() {
        let user = Pubkey::new_unique();
        let data = ProfileCreationData {
            version: PROFILE_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: "alice".to_string(),
            image: "profile.png".to_string(),
            about_me: Some("Hello!".to_string()),
        };
        
        let serialized = borsh::to_vec(&data).unwrap();
        let deserialized = ProfileCreationData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(data.version, deserialized.version);
        assert_eq!(data.category, deserialized.category);
        assert_eq!(data.operation, deserialized.operation);
        assert_eq!(data.user_pubkey, deserialized.user_pubkey);
        assert_eq!(data.username, deserialized.username);
        assert_eq!(data.image, deserialized.image);
        assert_eq!(data.about_me, deserialized.about_me);
    }

    #[test]
    fn test_profile_creation_data_serialize_no_about_me() {
        let user = Pubkey::new_unique();
        let data = ProfileCreationData {
            version: PROFILE_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: "bob".to_string(),
            image: "".to_string(),
            about_me: None,
        };
        
        let serialized = borsh::to_vec(&data).unwrap();
        let deserialized = ProfileCreationData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(data.about_me, None);
        assert_eq!(deserialized.about_me, None);
    }
}

// ============================================================================
// ProfileUpdateData Serialization Tests
// ============================================================================

#[cfg(test)]
mod profile_update_data_serialization_tests {
    use super::*;

    #[test]
    fn test_profile_update_data_serialize_deserialize() {
        let user = Pubkey::new_unique();
        let data = ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: Some("alice_updated".to_string()),
            image: Some("new_image.png".to_string()),
            about_me: Some(Some("Updated bio".to_string())),
        };
        
        let serialized = borsh::to_vec(&data).unwrap();
        let deserialized = ProfileUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(data.version, deserialized.version);
        assert_eq!(data.category, deserialized.category);
        assert_eq!(data.operation, deserialized.operation);
        assert_eq!(data.user_pubkey, deserialized.user_pubkey);
        assert_eq!(data.username, deserialized.username);
        assert_eq!(data.image, deserialized.image);
        assert_eq!(data.about_me, deserialized.about_me);
    }

    #[test]
    fn test_profile_update_data_serialize_no_changes() {
        let user = Pubkey::new_unique();
        let data = ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: None,
            image: None,
            about_me: None,
        };
        
        let serialized = borsh::to_vec(&data).unwrap();
        let deserialized = ProfileUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(data.username, None);
        assert_eq!(data.image, None);
        assert_eq!(data.about_me, None);
        assert_eq!(deserialized.username, None);
        assert_eq!(deserialized.image, None);
        assert_eq!(deserialized.about_me, None);
    }

    #[test]
    fn test_profile_update_data_serialize_clear_about_me() {
        let user = Pubkey::new_unique();
        let data = ProfileUpdateData {
            version: PROFILE_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            user_pubkey: user.to_string(),
            username: None,
            image: None,
            about_me: Some(None),
        };
        
        let serialized = borsh::to_vec(&data).unwrap();
        let deserialized = ProfileUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(data.about_me, Some(None));
        assert_eq!(deserialized.about_me, Some(None));
    }
}

// ============================================================================
// Base64 Encoding/Decoding Tests
// ============================================================================

#[cfg(test)]
mod base64_encoding_tests {
    use super::*;

    #[test]
    fn test_base64_encode_decode_roundtrip() {
        let original = b"Hello, World!".to_vec();
        let encoded = general_purpose::STANDARD.encode(&original);
        let decoded = general_purpose::STANDARD.decode(&encoded).unwrap();
        
        assert_eq!(original, decoded, "Base64 encode/decode should be reversible");
    }

    #[test]
    fn test_base64_encode_burn_memo() {
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 420 * DECIMAL_FACTOR,
            payload: b"test".to_vec(),
        };
        
        let borsh_data = borsh::to_vec(&memo).unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let decoded_data = general_purpose::STANDARD.decode(&base64_encoded).unwrap();
        let decoded_memo = BurnMemo::try_from_slice(&decoded_data).unwrap();
        
        assert_eq!(memo.version, decoded_memo.version);
        assert_eq!(memo.burn_amount, decoded_memo.burn_amount);
        assert_eq!(memo.payload, decoded_memo.payload);
    }
}

// ============================================================================
// Integration Tests - Full Memo Parsing
// ============================================================================

#[cfg(test)]
mod parse_profile_creation_memo_tests {
    use super::*;

    #[test]
    fn test_parse_valid_profile_creation_memo() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_CREATION_BURN_AMOUNT;
        let memo_data = create_profile_creation_memo(
            burn_amount,
            user,
            "alice",
            "profile.png",
            Some("Hello!".to_string()),
        );
        
        let result = parse_profile_creation_borsh_memo(&memo_data, user, burn_amount);
        assert!(result.is_ok(), "Valid profile creation memo should parse successfully");
        
        let profile_data = result.unwrap();
        assert_eq!(profile_data.username, "alice");
        assert_eq!(profile_data.image, "profile.png");
        assert_eq!(profile_data.about_me, Some("Hello!".to_string()));
    }

    #[test]
    fn test_parse_profile_creation_memo_minimal() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_CREATION_BURN_AMOUNT;
        let memo_data = create_profile_creation_memo(
            burn_amount,
            user,
            "a",
            "",
            None,
        );
        
        let result = parse_profile_creation_borsh_memo(&memo_data, user, burn_amount);
        assert!(result.is_ok(), "Minimal profile creation memo should parse successfully");
        
        let profile_data = result.unwrap();
        assert_eq!(profile_data.username, "a");
        assert_eq!(profile_data.image, "");
        assert_eq!(profile_data.about_me, None);
    }

    #[test]
    fn test_parse_profile_creation_memo_wrong_burn_amount() {
        let user = Pubkey::new_unique();
        let memo_burn_amount = MIN_PROFILE_CREATION_BURN_AMOUNT;
        let expected_burn_amount = memo_burn_amount + DECIMAL_FACTOR;
        
        let memo_data = create_profile_creation_memo(
            memo_burn_amount,
            user,
            "alice",
            "profile.png",
            None,
        );
        
        let result = parse_profile_creation_borsh_memo(&memo_data, user, expected_burn_amount);
        assert!(result.is_err(), "Mismatched burn amount should fail parsing");
    }

    #[test]
    fn test_parse_profile_creation_memo_wrong_user() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_CREATION_BURN_AMOUNT;
        
        let memo_data = create_profile_creation_memo(
            burn_amount,
            user1,
            "alice",
            "profile.png",
            None,
        );
        
        let result = parse_profile_creation_borsh_memo(&memo_data, user2, burn_amount);
        assert!(result.is_err(), "Mismatched user should fail parsing");
    }

    #[test]
    fn test_parse_profile_creation_memo_invalid_base64() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_CREATION_BURN_AMOUNT;
        let invalid_base64 = b"not valid base64!!!".to_vec();
        
        let result = parse_profile_creation_borsh_memo(&invalid_base64, user, burn_amount);
        assert!(result.is_err(), "Invalid base64 should fail parsing");
    }
}

#[cfg(test)]
mod parse_profile_update_memo_tests {
    use super::*;

    #[test]
    fn test_parse_valid_profile_update_memo() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
        let memo_data = create_profile_update_memo(
            burn_amount,
            user,
            Some("bob".to_string()),
            Some("new_image.png".to_string()),
            Some(Some("Updated!".to_string())),
        );
        
        let result = parse_profile_update_borsh_memo(&memo_data, user, burn_amount);
        assert!(result.is_ok(), "Valid profile update memo should parse successfully");
        
        let profile_data = result.unwrap();
        assert_eq!(profile_data.username, Some("bob".to_string()));
        assert_eq!(profile_data.image, Some("new_image.png".to_string()));
        assert_eq!(profile_data.about_me, Some(Some("Updated!".to_string())));
    }

    #[test]
    fn test_parse_profile_update_memo_no_changes() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
        let memo_data = create_profile_update_memo(
            burn_amount,
            user,
            None,
            None,
            None,
        );
        
        let result = parse_profile_update_borsh_memo(&memo_data, user, burn_amount);
        assert!(result.is_ok(), "Profile update memo with no changes should parse successfully");
        
        let profile_data = result.unwrap();
        assert_eq!(profile_data.username, None);
        assert_eq!(profile_data.image, None);
        assert_eq!(profile_data.about_me, None);
    }

    #[test]
    fn test_parse_profile_update_memo_clear_about_me() {
        let user = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
        let memo_data = create_profile_update_memo(
            burn_amount,
            user,
            None,
            None,
            Some(None),
        );
        
        let result = parse_profile_update_borsh_memo(&memo_data, user, burn_amount);
        assert!(result.is_ok(), "Profile update memo clearing about_me should parse successfully");
        
        let profile_data = result.unwrap();
        assert_eq!(profile_data.about_me, Some(None));
    }

    #[test]
    fn test_parse_profile_update_memo_wrong_burn_amount() {
        let user = Pubkey::new_unique();
        let memo_burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
        let expected_burn_amount = memo_burn_amount + DECIMAL_FACTOR;
        
        let memo_data = create_profile_update_memo(
            memo_burn_amount,
            user,
            Some("bob".to_string()),
            None,
            None,
        );
        
        let result = parse_profile_update_borsh_memo(&memo_data, user, expected_burn_amount);
        assert!(result.is_err(), "Mismatched burn amount should fail parsing");
    }

    #[test]
    fn test_parse_profile_update_memo_wrong_user() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let burn_amount = MIN_PROFILE_UPDATE_BURN_AMOUNT;
        
        let memo_data = create_profile_update_memo(
            burn_amount,
            user1,
            Some("bob".to_string()),
            None,
            None,
        );
        
        let result = parse_profile_update_borsh_memo(&memo_data, user2, burn_amount);
        assert!(result.is_err(), "Mismatched user should fail parsing");
    }
}

// ============================================================================
// Profile Account Space Calculation Tests
// ============================================================================

#[cfg(test)]
mod profile_space_calculation_tests {
    use super::*;

    #[test]
    fn test_profile_calculate_space_max() {
        let space = Profile::calculate_space_max();
        
        // Expected: 8 (discriminator) + 32 (user) + 8 (created_at) + 8 (last_updated) + 
        //           1 (bump) + 4 + 32 (username) + 4 + 256 (image) + 
        //           1 + 4 + 128 (about_me) + 128 (safety buffer)
        let expected = 8 + 32 + 8 + 8 + 1 + (4 + 32) + (4 + 256) + (1 + 4 + 128) + 128;
        
        assert_eq!(space, expected, "Profile space calculation should match expected value");
        assert_eq!(space, 614, "Profile space should be 614 bytes");
    }

    #[test]
    fn test_profile_space_has_safety_buffer() {
        let space = Profile::calculate_space_max();
        let minimum_required = 8 + 32 + 8 + 8 + 1 + (4 + 32) + (4 + 256) + (1 + 4 + 128);
        
        assert!(space > minimum_required, "Profile space should include safety buffer");
        assert_eq!(space - minimum_required, 128, "Safety buffer should be 128 bytes");
    }
}

