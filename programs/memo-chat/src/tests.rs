#[cfg(test)]
mod tests {
    use crate::*;
    use anchor_lang::prelude::Pubkey;

    // ============================================================================
    // Constants Tests
    // ============================================================================

    #[test]
    fn test_decimal_factor() {
        assert_eq!(DECIMAL_FACTOR, 1_000_000);
    }

    #[test]
    fn test_burn_amount_constants() {
        assert_eq!(MIN_GROUP_CREATION_BURN_TOKENS, 42_069);
        assert_eq!(MIN_GROUP_CREATION_BURN_AMOUNT, 42_069 * 1_000_000);
        assert_eq!(MIN_BURN_AMOUNT, 1 * 1_000_000);
        assert_eq!(MAX_BURN_PER_TX, 1_000_000_000_000 * 1_000_000);
    }

    #[test]
    fn test_time_constants() {
        assert_eq!(DEFAULT_MEMO_INTERVAL_SECONDS, 60);
        assert_eq!(MAX_MEMO_INTERVAL_SECONDS, 86400);
    }

    #[test]
    fn test_string_length_constants() {
        assert_eq!(MAX_GROUP_NAME_LENGTH, 64);
        assert_eq!(MAX_GROUP_DESCRIPTION_LENGTH, 128);
        assert_eq!(MAX_GROUP_IMAGE_LENGTH, 256);
        assert_eq!(MAX_TAGS_COUNT, 4);
        assert_eq!(MAX_TAG_LENGTH, 32);
        assert_eq!(MAX_MESSAGE_LENGTH, 512);
        assert_eq!(MAX_BURN_MESSAGE_LENGTH, 512);
    }

    #[test]
    fn test_memo_length_constants() {
        assert_eq!(MEMO_MIN_LENGTH, 69);
        assert_eq!(MEMO_MAX_LENGTH, 800);
        assert_eq!(MAX_PAYLOAD_LENGTH, 787); // 800 - 13
        assert_eq!(MAX_BORSH_DATA_SIZE, 800);
        assert_eq!(SIGNATURE_LENGTH_BYTES, 64);
    }

    #[test]
    fn test_version_constants() {
        assert_eq!(BURN_MEMO_VERSION, 1);
        assert_eq!(CHAT_GROUP_CREATION_DATA_VERSION, 1);
    }

    #[test]
    fn test_expected_strings() {
        assert_eq!(EXPECTED_CATEGORY, "chat");
        assert_eq!(EXPECTED_OPERATION, "create_group");
        assert_eq!(EXPECTED_SEND_MESSAGE_OPERATION, "send_message");
        assert_eq!(EXPECTED_BURN_FOR_GROUP_OPERATION, "burn_for_group");
    }

    // ============================================================================
    // ChatGroupCreationData Validation Tests
    // ============================================================================

    fn create_valid_group_creation_data(group_id: u64) -> ChatGroupCreationData {
        ChatGroupCreationData {
            version: CHAT_GROUP_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            group_id,
            name: "Test Group".to_string(),
            description: "Test description".to_string(),
            image: "https://example.com/image.png".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            min_memo_interval: Some(60),
        }
    }

    #[test]
    fn test_group_creation_data_valid() {
        let data = create_valid_group_creation_data(1);
        assert!(data.validate(1).is_ok());
    }

    #[test]
    fn test_group_creation_data_minimal() {
        let data = ChatGroupCreationData {
            version: CHAT_GROUP_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            group_id: 0,
            name: "A".to_string(), // minimum 1 char
            description: String::new(),
            image: String::new(),
            tags: vec![],
            min_memo_interval: None,
        };
        assert!(data.validate(0).is_ok());
    }

    #[test]
    fn test_group_creation_data_max_lengths() {
        let data = ChatGroupCreationData {
            version: CHAT_GROUP_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            group_id: 0,
            name: "A".repeat(MAX_GROUP_NAME_LENGTH),
            description: "B".repeat(MAX_GROUP_DESCRIPTION_LENGTH),
            image: "C".repeat(MAX_GROUP_IMAGE_LENGTH),
            tags: vec!["D".repeat(MAX_TAG_LENGTH); MAX_TAGS_COUNT],
            min_memo_interval: Some(MAX_MEMO_INTERVAL_SECONDS),
        };
        assert!(data.validate(0).is_ok());
    }

    #[test]
    fn test_group_creation_data_invalid_version() {
        let mut data = create_valid_group_creation_data(1);
        data.version = 99;
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_invalid_category() {
        let mut data = create_valid_group_creation_data(1);
        data.category = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_invalid_operation() {
        let mut data = create_valid_group_creation_data(1);
        data.operation = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_group_id_mismatch() {
        let data = create_valid_group_creation_data(1);
        assert!(data.validate(2).is_err());
    }

    #[test]
    fn test_group_creation_data_empty_name() {
        let mut data = create_valid_group_creation_data(1);
        data.name = String::new();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_name_too_long() {
        let mut data = create_valid_group_creation_data(1);
        data.name = "A".repeat(MAX_GROUP_NAME_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_description_too_long() {
        let mut data = create_valid_group_creation_data(1);
        data.description = "B".repeat(MAX_GROUP_DESCRIPTION_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_image_too_long() {
        let mut data = create_valid_group_creation_data(1);
        data.image = "C".repeat(MAX_GROUP_IMAGE_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_too_many_tags() {
        let mut data = create_valid_group_creation_data(1);
        data.tags = vec!["tag".to_string(); MAX_TAGS_COUNT + 1];
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_empty_tag() {
        let mut data = create_valid_group_creation_data(1);
        data.tags = vec![String::new()];
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_tag_too_long() {
        let mut data = create_valid_group_creation_data(1);
        data.tags = vec!["X".repeat(MAX_TAG_LENGTH + 1)];
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_interval_negative() {
        let mut data = create_valid_group_creation_data(1);
        data.min_memo_interval = Some(-1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_interval_too_large() {
        let mut data = create_valid_group_creation_data(1);
        data.min_memo_interval = Some(MAX_MEMO_INTERVAL_SECONDS + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_group_creation_data_interval_zero() {
        let mut data = create_valid_group_creation_data(1);
        data.min_memo_interval = Some(0);
        assert!(data.validate(1).is_ok());
    }

    // ============================================================================
    // ChatMessageData Validation Tests
    // ============================================================================

    fn create_valid_message_data(group_id: u64, sender: Pubkey) -> ChatMessageData {
        ChatMessageData {
            version: CHAT_GROUP_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_SEND_MESSAGE_OPERATION.to_string(),
            group_id,
            sender: sender.to_string(),
            message: "Hello, world!".to_string(),
            receiver: None,
            reply_to_sig: None,
        }
    }

    #[test]
    fn test_message_data_valid() {
        let sender = Pubkey::new_unique();
        let data = create_valid_message_data(1, sender);
        assert!(data.validate(1, sender).is_ok());
    }

    #[test]
    fn test_message_data_max_message_length() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.message = "X".repeat(MAX_MESSAGE_LENGTH);
        assert!(data.validate(1, sender).is_ok());
    }

    #[test]
    fn test_message_data_with_receiver() {
        let sender = Pubkey::new_unique();
        let receiver = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.receiver = Some(receiver.to_string());
        assert!(data.validate(1, sender).is_ok());
    }

    #[test]
    fn test_message_data_with_reply_to() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        // Generate a valid signature (64 bytes)
        let sig_bytes = vec![0u8; 64];
        data.reply_to_sig = Some(bs58::encode(&sig_bytes).into_string());
        assert!(data.validate(1, sender).is_ok());
    }

    #[test]
    fn test_message_data_invalid_version() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.version = 99;
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_invalid_category() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.category = "invalid".to_string();
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_invalid_operation() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.operation = "invalid".to_string();
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_group_id_mismatch() {
        let sender = Pubkey::new_unique();
        let data = create_valid_message_data(1, sender);
        assert!(data.validate(2, sender).is_err());
    }

    #[test]
    fn test_message_data_sender_mismatch() {
        let sender1 = Pubkey::new_unique();
        let sender2 = Pubkey::new_unique();
        let data = create_valid_message_data(1, sender1);
        assert!(data.validate(1, sender2).is_err());
    }

    #[test]
    fn test_message_data_invalid_sender_format() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.sender = "invalid_pubkey".to_string();
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_empty_message() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.message = String::new();
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_message_too_long() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.message = "X".repeat(MAX_MESSAGE_LENGTH + 1);
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_invalid_receiver_format() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.receiver = Some("invalid_pubkey".to_string());
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_empty_receiver_string() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.receiver = Some(String::new());
        // Empty string is allowed (treated as None)
        assert!(data.validate(1, sender).is_ok());
    }

    #[test]
    fn test_message_data_invalid_reply_sig_format() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.reply_to_sig = Some("invalid_signature".to_string());
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_reply_sig_wrong_length() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        // Generate a signature with wrong length (32 bytes instead of 64)
        let sig_bytes = vec![0u8; 32];
        data.reply_to_sig = Some(bs58::encode(&sig_bytes).into_string());
        assert!(data.validate(1, sender).is_err());
    }

    #[test]
    fn test_message_data_empty_reply_sig_string() {
        let sender = Pubkey::new_unique();
        let mut data = create_valid_message_data(1, sender);
        data.reply_to_sig = Some(String::new());
        // Empty string is allowed (treated as None)
        assert!(data.validate(1, sender).is_ok());
    }

    // ============================================================================
    // ChatGroupBurnData Validation Tests
    // ============================================================================

    fn create_valid_burn_data(group_id: u64, burner: Pubkey) -> ChatGroupBurnData {
        ChatGroupBurnData {
            version: CHAT_GROUP_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_GROUP_OPERATION.to_string(),
            group_id,
            burner: burner.to_string(),
            message: "Burning for the group!".to_string(),
        }
    }

    #[test]
    fn test_burn_data_valid() {
        let burner = Pubkey::new_unique();
        let data = create_valid_burn_data(1, burner);
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_burn_data_empty_message() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.message = String::new();
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_burn_data_max_message_length() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.message = "X".repeat(MAX_BURN_MESSAGE_LENGTH);
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_burn_data_invalid_version() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.version = 99;
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_burn_data_invalid_category() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.category = "invalid".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_burn_data_invalid_operation() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.operation = "invalid".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_burn_data_group_id_mismatch() {
        let burner = Pubkey::new_unique();
        let data = create_valid_burn_data(1, burner);
        assert!(data.validate(2, burner).is_err());
    }

    #[test]
    fn test_burn_data_burner_mismatch() {
        let burner1 = Pubkey::new_unique();
        let burner2 = Pubkey::new_unique();
        let data = create_valid_burn_data(1, burner1);
        assert!(data.validate(1, burner2).is_err());
    }

    #[test]
    fn test_burn_data_invalid_burner_format() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.burner = "invalid_pubkey".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_burn_data_message_too_long() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_burn_data(1, burner);
        data.message = "X".repeat(MAX_BURN_MESSAGE_LENGTH + 1);
        assert!(data.validate(1, burner).is_err());
    }

    // ============================================================================
    // BurnLeaderboard Tests
    // ============================================================================

    #[test]
    fn test_leaderboard_initialize() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        assert_eq!(leaderboard.entries.len(), 0);
        assert_eq!(leaderboard.entries.capacity(), 100);
    }

    #[test]
    fn test_leaderboard_add_first_group() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        let result = leaderboard.update_leaderboard(1, 1000).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries.len(), 1);
        assert_eq!(leaderboard.entries[0].group_id, 1);
        assert_eq!(leaderboard.entries[0].burned_amount, 1000);
    }

    #[test]
    fn test_leaderboard_update_existing_group() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![
                LeaderboardEntry { group_id: 1, burned_amount: 1000 },
                LeaderboardEntry { group_id: 2, burned_amount: 2000 },
            ],
        };
        
        let result = leaderboard.update_leaderboard(1, 5000).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries.len(), 2);
        assert_eq!(leaderboard.entries[0].burned_amount, 5000);
    }

    #[test]
    fn test_leaderboard_add_groups_up_to_100() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Add 100 groups
        for i in 0..100 {
            let result = leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
            assert!(result);
        }
        
        assert_eq!(leaderboard.entries.len(), 100);
    }

    #[test]
    fn test_leaderboard_replace_min_when_full() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups (1000, 2000, ..., 100000)
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add a new group with higher burn amount than minimum
        let result = leaderboard.update_leaderboard(200, 150000).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries.len(), 100);
        
        // Verify that group_id 0 (with 1000) was replaced
        let has_group_0 = leaderboard.entries.iter().any(|e| e.group_id == 0);
        let has_group_200 = leaderboard.entries.iter().any(|e| e.group_id == 200);
        assert!(!has_group_0);
        assert!(has_group_200);
    }

    #[test]
    fn test_leaderboard_reject_when_full_and_too_small() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups (1000, 2000, ..., 100000)
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add a new group with lower burn amount than minimum
        let result = leaderboard.update_leaderboard(200, 500).unwrap();
        assert!(!result); // Should not enter leaderboard
        assert_eq!(leaderboard.entries.len(), 100);
        
        // Verify that group 200 was not added
        let has_group_200 = leaderboard.entries.iter().any(|e| e.group_id == 200);
        assert!(!has_group_200);
    }

    #[test]
    fn test_leaderboard_reject_when_equal_to_min() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups (1000, 2000, ..., 100000)
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add a new group with burn amount EQUAL to minimum (1000)
        // Should be rejected because we require new_burned_amount > min_amount
        let result = leaderboard.update_leaderboard(200, 1000).unwrap();
        assert!(!result); // Should not enter leaderboard
        assert_eq!(leaderboard.entries.len(), 100);
        
        // Verify that group 0 (with 1000) is still there
        let has_group_0 = leaderboard.entries.iter().any(|e| e.group_id == 0);
        assert!(has_group_0);
        
        // Verify that group 200 was not added
        let has_group_200 = leaderboard.entries.iter().any(|e| e.group_id == 200);
        assert!(!has_group_200);
    }

    #[test]
    fn test_leaderboard_replace_exact_min_plus_one() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups (1000, 2000, ..., 100000)
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add with amount = min + 1 (should succeed)
        let result = leaderboard.update_leaderboard(200, 1001).unwrap();
        assert!(result); // Should enter leaderboard
        assert_eq!(leaderboard.entries.len(), 100);
        
        // Verify that group 0 (with 1000) was replaced
        let has_group_0 = leaderboard.entries.iter().any(|e| e.group_id == 0);
        assert!(!has_group_0);
        
        // Verify that group 200 was added
        let has_group_200 = leaderboard.entries.iter().any(|e| e.group_id == 200 && e.burned_amount == 1001);
        assert!(has_group_200);
    }

    #[test]
    fn test_leaderboard_multiple_replacements() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups (1000, 2000, ..., 100000)
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Replace multiple times with increasing amounts
        // Use amounts that are all greater than the smallest 10 (1000-10000)
        // to ensure we're replacing original entries, not newly added ones
        for i in 0..10 {
            let new_amount = 10500 + (i * 1000); // 10500, 11500, ..., 19500
            let result = leaderboard.update_leaderboard(200 + i, new_amount).unwrap();
            assert!(result);
            assert_eq!(leaderboard.entries.len(), 100);
        }
        
        // Verify that the smallest 10 original groups were replaced
        for i in 0..10 {
            let has_group = leaderboard.entries.iter().any(|e| e.group_id == i);
            assert!(!has_group, "Group {} should have been replaced", i);
        }
        
        // Verify that all new groups are in the leaderboard
        for i in 0..10 {
            let has_group = leaderboard.entries.iter().any(|e| e.group_id == 200 + i);
            assert!(has_group, "Group {} should be in leaderboard", 200 + i);
        }
    }

    #[test]
    fn test_leaderboard_update_existing_when_full() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Fill with 100 groups
        for i in 0..100 {
            leaderboard.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Update an existing group (should always succeed)
        let result = leaderboard.update_leaderboard(50, 999999999).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries.len(), 100);
        
        // Verify the update
        let entry = leaderboard.entries.iter().find(|e| e.group_id == 50).unwrap();
        assert_eq!(entry.burned_amount, 999999999);
    }

    #[test]
    fn test_leaderboard_find_group_position_empty() {
        let leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        
        let (group_pos, min_pos) = leaderboard.find_group_position_and_min(1);
        assert_eq!(group_pos, None);
        assert_eq!(min_pos, None);
    }

    #[test]
    fn test_leaderboard_find_group_position_single() {
        let leaderboard = BurnLeaderboard {
            entries: vec![
                LeaderboardEntry { group_id: 1, burned_amount: 1000 },
            ],
        };
        
        let (group_pos, min_pos) = leaderboard.find_group_position_and_min(1);
        assert_eq!(group_pos, Some(0));
        assert_eq!(min_pos, Some(0));
    }

    #[test]
    fn test_leaderboard_find_group_position_multiple() {
        let leaderboard = BurnLeaderboard {
            entries: vec![
                LeaderboardEntry { group_id: 1, burned_amount: 5000 },
                LeaderboardEntry { group_id: 2, burned_amount: 1000 }, // min
                LeaderboardEntry { group_id: 3, burned_amount: 3000 },
            ],
        };
        
        let (group_pos, min_pos) = leaderboard.find_group_position_and_min(3);
        assert_eq!(group_pos, Some(2));
        assert_eq!(min_pos, Some(1));
    }

    #[test]
    fn test_leaderboard_find_group_not_found() {
        let leaderboard = BurnLeaderboard {
            entries: vec![
                LeaderboardEntry { group_id: 1, burned_amount: 5000 },
                LeaderboardEntry { group_id: 2, burned_amount: 1000 },
            ],
        };
        
        let (group_pos, min_pos) = leaderboard.find_group_position_and_min(99);
        assert_eq!(group_pos, None);
        assert_eq!(min_pos, Some(1)); // Still finds min
    }

    #[test]
    fn test_leaderboard_update_with_zero_amount() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        let result = leaderboard.update_leaderboard(1, 0).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries[0].burned_amount, 0);
    }

    #[test]
    fn test_leaderboard_update_with_max_amount() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        let result = leaderboard.update_leaderboard(1, u64::MAX).unwrap();
        assert!(result);
        assert_eq!(leaderboard.entries[0].burned_amount, u64::MAX);
    }

    #[test]
    fn test_leaderboard_entries_remain_unsorted() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![],
        };
        leaderboard.initialize();
        
        // Add groups in non-sorted order
        leaderboard.update_leaderboard(1, 5000).unwrap();
        leaderboard.update_leaderboard(2, 1000).unwrap();
        leaderboard.update_leaderboard(3, 10000).unwrap();
        leaderboard.update_leaderboard(4, 3000).unwrap();
        
        // Verify entries are NOT sorted (they remain in insertion order)
        assert_eq!(leaderboard.entries[0].burned_amount, 5000);
        assert_eq!(leaderboard.entries[1].burned_amount, 1000);
        assert_eq!(leaderboard.entries[2].burned_amount, 10000);
        assert_eq!(leaderboard.entries[3].burned_amount, 3000);
    }

    #[test]
    fn test_leaderboard_update_existing_maintains_position() {
        let mut leaderboard = BurnLeaderboard {
            entries: vec![
                LeaderboardEntry { group_id: 1, burned_amount: 5000 },
                LeaderboardEntry { group_id: 2, burned_amount: 1000 },
                LeaderboardEntry { group_id: 3, burned_amount: 10000 },
            ],
        };
        
        // Update group 2's amount
        leaderboard.update_leaderboard(2, 20000).unwrap();
        
        // Verify group 2 is still at index 1 (not moved)
        assert_eq!(leaderboard.entries[1].group_id, 2);
        assert_eq!(leaderboard.entries[1].burned_amount, 20000);
    }

    // ============================================================================
    // Space Calculation Tests
    // ============================================================================

    #[test]
    fn test_chat_group_space_calculation() {
        let space = ChatGroup::calculate_space_max();
        
        // Verify minimum expected space
        assert!(space >= 8); // discriminator
        assert!(space >= 8 + 8); // + group_id
        assert!(space >= 8 + 8 + 32); // + creator
        
        // Space should be reasonable (not too large)
        assert!(space < 2000);
    }

    #[test]
    fn test_burn_leaderboard_space() {
        let expected_space = 8 + // discriminator
            4 + // Vec length prefix
            100 * 16 + // max entries (100 * (8 + 8) bytes each)
            64; // safety buffer
        
        assert_eq!(BurnLeaderboard::SPACE, expected_space);
        assert_eq!(BurnLeaderboard::SPACE, 1676);
    }

    #[test]
    fn test_global_group_counter_space() {
        let expected_space = 8 + // discriminator
            8; // total_groups (u64)
        
        assert_eq!(GlobalGroupCounter::SPACE, expected_space);
        assert_eq!(GlobalGroupCounter::SPACE, 16);
    }

    #[test]
    fn test_leaderboard_entry_size() {
        use std::mem;
        
        // LeaderboardEntry should be exactly 16 bytes (8 + 8)
        assert_eq!(mem::size_of::<LeaderboardEntry>(), 16);
    }
}

