#[cfg(test)]
mod tests {
    use crate::*;

    // ============================================================================
    // Constants Tests
    // ============================================================================

    #[test]
    fn test_decimal_factor() {
        assert_eq!(DECIMAL_FACTOR, 1_000_000);
    }

    #[test]
    fn test_burn_amount_constants() {
        assert_eq!(MIN_PROJECT_CREATION_BURN_TOKENS, 42069);
        assert_eq!(MIN_PROJECT_CREATION_BURN_AMOUNT, 42069 * 1_000_000);
        assert_eq!(MIN_PROJECT_BURN_TOKENS, 420);
        assert_eq!(MIN_PROJECT_BURN_AMOUNT, 420 * 1_000_000);
        assert_eq!(MIN_PROJECT_UPDATE_BURN_TOKENS, 42069);
        assert_eq!(MIN_PROJECT_UPDATE_BURN_AMOUNT, 42069 * 1_000_000);
        assert_eq!(MAX_BURN_PER_TX, 1_000_000_000_000 * 1_000_000);
    }

    #[test]
    fn test_string_length_constants() {
        assert_eq!(MAX_PROJECT_NAME_LENGTH, 64);
        assert_eq!(MAX_PROJECT_DESCRIPTION_LENGTH, 256);
        assert_eq!(MAX_PROJECT_IMAGE_LENGTH, 256);
        assert_eq!(MAX_PROJECT_WEBSITE_LENGTH, 128);
        assert_eq!(MAX_TAGS_COUNT, 4);
        assert_eq!(MAX_TAG_LENGTH, 32);
    }

    #[test]
    fn test_memo_length_constants() {
        assert_eq!(MEMO_MIN_LENGTH, 69);
        assert_eq!(MEMO_MAX_LENGTH, 800);
        assert_eq!(MAX_PAYLOAD_LENGTH, 787); // 800 - 13
        assert_eq!(MAX_BORSH_DATA_SIZE, 800);
    }

    #[test]
    fn test_version_constants() {
        assert_eq!(BURN_MEMO_VERSION, 1);
        assert_eq!(PROJECT_CREATION_DATA_VERSION, 1);
        assert_eq!(PROJECT_UPDATE_DATA_VERSION, 1);
    }

    #[test]
    fn test_expected_strings() {
        assert_eq!(EXPECTED_CATEGORY, "project");
        assert_eq!(EXPECTED_OPERATION, "create_project");
        assert_eq!(EXPECTED_UPDATE_OPERATION, "update_project");
        assert_eq!(EXPECTED_BURN_FOR_PROJECT_OPERATION, "burn_for_project");
        assert_eq!(MAX_BURN_MESSAGE_LENGTH, 696);
    }

    // ============================================================================
    // ProjectCreationData Validation Tests
    // ============================================================================

    fn create_valid_project_creation_data(project_id: u64) -> ProjectCreationData {
        ProjectCreationData {
            version: PROJECT_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            project_id,
            name: "Test Project".to_string(),
            description: "Test description".to_string(),
            image: "https://example.com/image.png".to_string(),
            website: "https://example.com".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        }
    }

    #[test]
    fn test_project_creation_data_valid() {
        let data = create_valid_project_creation_data(1);
        assert!(data.validate(1).is_ok());
    }

    #[test]
    fn test_project_creation_data_minimal() {
        let data = ProjectCreationData {
            version: PROJECT_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            project_id: 0,
            name: "A".to_string(), // minimum 1 char
            description: String::new(),
            image: String::new(),
            website: String::new(),
            tags: vec![],
        };
        assert!(data.validate(0).is_ok());
    }

    #[test]
    fn test_project_creation_data_max_lengths() {
        let data = ProjectCreationData {
            version: PROJECT_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            project_id: 0,
            name: "A".repeat(MAX_PROJECT_NAME_LENGTH),
            description: "D".repeat(MAX_PROJECT_DESCRIPTION_LENGTH),
            image: "I".repeat(MAX_PROJECT_IMAGE_LENGTH),
            website: "W".repeat(MAX_PROJECT_WEBSITE_LENGTH),
            tags: vec![
                "T".repeat(MAX_TAG_LENGTH),
                "T".repeat(MAX_TAG_LENGTH),
                "T".repeat(MAX_TAG_LENGTH),
                "T".repeat(MAX_TAG_LENGTH),
            ],
        };
        assert!(data.validate(0).is_ok());
    }

    #[test]
    fn test_project_creation_data_invalid_version() {
        let mut data = create_valid_project_creation_data(1);
        data.version = 99;
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_invalid_category() {
        let mut data = create_valid_project_creation_data(1);
        data.category = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_invalid_operation() {
        let mut data = create_valid_project_creation_data(1);
        data.operation = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_project_id_mismatch() {
        let data = create_valid_project_creation_data(1);
        assert!(data.validate(2).is_err());
    }

    #[test]
    fn test_project_creation_data_empty_name() {
        let mut data = create_valid_project_creation_data(1);
        data.name = String::new();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_name_too_long() {
        let mut data = create_valid_project_creation_data(1);
        data.name = "A".repeat(MAX_PROJECT_NAME_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_description_too_long() {
        let mut data = create_valid_project_creation_data(1);
        data.description = "D".repeat(MAX_PROJECT_DESCRIPTION_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_image_too_long() {
        let mut data = create_valid_project_creation_data(1);
        data.image = "I".repeat(MAX_PROJECT_IMAGE_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_website_too_long() {
        let mut data = create_valid_project_creation_data(1);
        data.website = "W".repeat(MAX_PROJECT_WEBSITE_LENGTH + 1);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_too_many_tags() {
        let mut data = create_valid_project_creation_data(1);
        data.tags = vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "tag3".to_string(),
            "tag4".to_string(),
            "tag5".to_string(), // exceeds MAX_TAGS_COUNT
        ];
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_empty_tag() {
        let mut data = create_valid_project_creation_data(1);
        data.tags = vec![String::new()];
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_creation_data_tag_too_long() {
        let mut data = create_valid_project_creation_data(1);
        data.tags = vec!["T".repeat(MAX_TAG_LENGTH + 1)];
        assert!(data.validate(1).is_err());
    }

    // ============================================================================
    // ProjectUpdateData Validation Tests
    // ============================================================================

    fn create_valid_project_update_data(project_id: u64) -> ProjectUpdateData {
        ProjectUpdateData {
            version: PROJECT_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            project_id,
            name: Some("Updated Project".to_string()),
            description: Some("Updated description".to_string()),
            image: Some("https://example.com/new-image.png".to_string()),
            website: Some("https://newwebsite.com".to_string()),
            tags: Some(vec!["newtag".to_string()]),
        }
    }

    #[test]
    fn test_project_update_data_valid() {
        let data = create_valid_project_update_data(1);
        assert!(data.validate(1).is_ok());
    }

    #[test]
    fn test_project_update_data_all_none() {
        let data = ProjectUpdateData {
            version: PROJECT_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            project_id: 1,
            name: None,
            description: None,
            image: None,
            website: None,
            tags: None,
        };
        assert!(data.validate(1).is_ok());
    }

    #[test]
    fn test_project_update_data_partial_update() {
        let data = ProjectUpdateData {
            version: PROJECT_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            project_id: 1,
            name: Some("New Name".to_string()),
            description: None,
            image: None,
            website: None,
            tags: None,
        };
        assert!(data.validate(1).is_ok());
    }

    #[test]
    fn test_project_update_data_invalid_version() {
        let mut data = create_valid_project_update_data(1);
        data.version = 99;
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_invalid_category() {
        let mut data = create_valid_project_update_data(1);
        data.category = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_invalid_operation() {
        let mut data = create_valid_project_update_data(1);
        data.operation = "invalid".to_string();
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_project_id_mismatch() {
        let data = create_valid_project_update_data(1);
        assert!(data.validate(2).is_err());
    }

    #[test]
    fn test_project_update_data_empty_name() {
        let mut data = create_valid_project_update_data(1);
        data.name = Some(String::new());
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_name_too_long() {
        let mut data = create_valid_project_update_data(1);
        data.name = Some("A".repeat(MAX_PROJECT_NAME_LENGTH + 1));
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_description_too_long() {
        let mut data = create_valid_project_update_data(1);
        data.description = Some("D".repeat(MAX_PROJECT_DESCRIPTION_LENGTH + 1));
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_image_too_long() {
        let mut data = create_valid_project_update_data(1);
        data.image = Some("I".repeat(MAX_PROJECT_IMAGE_LENGTH + 1));
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_website_too_long() {
        let mut data = create_valid_project_update_data(1);
        data.website = Some("W".repeat(MAX_PROJECT_WEBSITE_LENGTH + 1));
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_too_many_tags() {
        let mut data = create_valid_project_update_data(1);
        data.tags = Some(vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "tag3".to_string(),
            "tag4".to_string(),
            "tag5".to_string(),
        ]);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_empty_tag() {
        let mut data = create_valid_project_update_data(1);
        data.tags = Some(vec![String::new()]);
        assert!(data.validate(1).is_err());
    }

    #[test]
    fn test_project_update_data_tag_too_long() {
        let mut data = create_valid_project_update_data(1);
        data.tags = Some(vec!["T".repeat(MAX_TAG_LENGTH + 1)]);
        assert!(data.validate(1).is_err());
    }

    // ============================================================================
    // ProjectBurnData Validation Tests
    // ============================================================================

    fn create_valid_project_burn_data(project_id: u64, burner: Pubkey) -> ProjectBurnData {
        ProjectBurnData {
            version: PROJECT_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_PROJECT_OPERATION.to_string(),
            project_id,
            burner: burner.to_string(),
            message: "Burning for project support".to_string(),
        }
    }

    #[test]
    fn test_project_burn_data_valid() {
        let burner = Pubkey::new_unique();
        let data = create_valid_project_burn_data(1, burner);
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_project_burn_data_empty_message() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.message = String::new();
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_project_burn_data_max_message_length() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.message = "M".repeat(MAX_BURN_MESSAGE_LENGTH);
        assert!(data.validate(1, burner).is_ok());
    }

    #[test]
    fn test_project_burn_data_invalid_version() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.version = 99;
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_project_burn_data_invalid_category() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.category = "invalid".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_project_burn_data_invalid_operation() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.operation = "invalid".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_project_burn_data_project_id_mismatch() {
        let burner = Pubkey::new_unique();
        let data = create_valid_project_burn_data(1, burner);
        assert!(data.validate(2, burner).is_err());
    }

    #[test]
    fn test_project_burn_data_invalid_burner_format() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.burner = "invalid_pubkey".to_string();
        assert!(data.validate(1, burner).is_err());
    }

    #[test]
    fn test_project_burn_data_burner_mismatch() {
        let burner1 = Pubkey::new_unique();
        let burner2 = Pubkey::new_unique();
        let data = create_valid_project_burn_data(1, burner1);
        assert!(data.validate(1, burner2).is_err());
    }

    #[test]
    fn test_project_burn_data_message_too_long() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_project_burn_data(1, burner);
        data.message = "M".repeat(MAX_BURN_MESSAGE_LENGTH + 1);
        assert!(data.validate(1, burner).is_err());
    }

    // ============================================================================
    // BurnLeaderboard Tests
    // ============================================================================

    fn create_leaderboard() -> BurnLeaderboard {
        let mut lb = BurnLeaderboard {
            entries: Vec::new(),
        };
        lb.initialize();
        lb
    }

    #[test]
    fn test_leaderboard_initialize() {
        let mut lb = BurnLeaderboard {
            entries: Vec::new(),
        };
        lb.initialize();
        
        assert_eq!(lb.entries.len(), 0);
        assert_eq!(lb.entries.capacity(), 100);
    }

    #[test]
    fn test_leaderboard_add_first_project() {
        let mut lb = create_leaderboard();
        let result = lb.update_leaderboard(1, 1000).unwrap();
        
        assert!(result);
        assert_eq!(lb.entries.len(), 1);
        assert_eq!(lb.entries[0].project_id, 1);
        assert_eq!(lb.entries[0].burned_amount, 1000);
    }

    #[test]
    fn test_leaderboard_update_existing_project() {
        let mut lb = create_leaderboard();
        
        lb.update_leaderboard(1, 1000).unwrap();
        let result = lb.update_leaderboard(1, 2000).unwrap();
        
        assert!(result);
        assert_eq!(lb.entries.len(), 1);
        assert_eq!(lb.entries[0].burned_amount, 2000);
    }

    #[test]
    fn test_leaderboard_add_multiple_projects() {
        let mut lb = create_leaderboard();
        
        for i in 0..10 {
            let result = lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
            assert!(result);
        }
        
        assert_eq!(lb.entries.len(), 10);
    }

    #[test]
    fn test_leaderboard_fill_to_100() {
        let mut lb = create_leaderboard();
        
        for i in 0..100 {
            let result = lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
            assert!(result);
        }
        
        assert_eq!(lb.entries.len(), 100);
    }

    #[test]
    fn test_leaderboard_replace_min_when_full() {
        let mut lb = create_leaderboard();
        
        // Fill with 100 projects (amounts 1000-100000)
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add with amount less than minimum (should fail)
        let result = lb.update_leaderboard(200, 500).unwrap();
        assert!(!result);
        assert_eq!(lb.entries.len(), 100);
        
        // Try to add with amount greater than minimum (should succeed)
        let result = lb.update_leaderboard(201, 1500).unwrap();
        assert!(result);
        assert_eq!(lb.entries.len(), 100);
        
        // Verify that project 0 (with amount 1000) was replaced
        let has_project_0 = lb.entries.iter().any(|e| e.project_id == 0);
        assert!(!has_project_0);
        
        let has_project_201 = lb.entries.iter().any(|e| e.project_id == 201);
        assert!(has_project_201);
    }

    #[test]
    fn test_leaderboard_reject_when_equal_to_min() {
        let mut lb = create_leaderboard();
        
        // Fill with 100 projects (1000, 2000, ..., 100000)
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add a new project with burn amount EQUAL to minimum (1000)
        // Should be rejected because we require new_burned_amount > min_amount
        let result = lb.update_leaderboard(200, 1000).unwrap();
        assert!(!result); // Should not enter leaderboard
        assert_eq!(lb.entries.len(), 100);
        
        // Verify that project 0 (with 1000) is still there
        let has_project_0 = lb.entries.iter().any(|e| e.project_id == 0);
        assert!(has_project_0);
        
        // Verify that project 200 was not added
        let has_project_200 = lb.entries.iter().any(|e| e.project_id == 200);
        assert!(!has_project_200);
    }

    #[test]
    fn test_leaderboard_replace_exact_min_plus_one() {
        let mut lb = create_leaderboard();
        
        // Fill with 100 projects (1000, 2000, ..., 100000)
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Try to add with amount = min + 1 (should succeed)
        let result = lb.update_leaderboard(200, 1001).unwrap();
        assert!(result); // Should enter leaderboard
        assert_eq!(lb.entries.len(), 100);
        
        // Verify that project 0 (with 1000) was replaced
        let has_project_0 = lb.entries.iter().any(|e| e.project_id == 0);
        assert!(!has_project_0);
        
        // Verify that project 200 was added
        let has_project_200 = lb.entries.iter().any(|e| e.project_id == 200 && e.burned_amount == 1001);
        assert!(has_project_200);
    }

    #[test]
    fn test_leaderboard_multiple_replacements() {
        let mut lb = create_leaderboard();
        
        // Fill with 100 projects (1000, 2000, ..., 100000)
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Replace multiple times with increasing amounts
        // Use amounts that are all greater than the smallest 10 (1000-10000)
        // to ensure we're replacing original entries, not newly added ones
        for i in 0..10 {
            let new_amount = 10500 + (i * 1000); // 10500, 11500, ..., 19500
            let result = lb.update_leaderboard(200 + i, new_amount).unwrap();
            assert!(result);
            assert_eq!(lb.entries.len(), 100);
        }
        
        // Verify that the smallest 10 original projects were replaced
        for i in 0..10 {
            let has_project = lb.entries.iter().any(|e| e.project_id == i);
            assert!(!has_project, "Project {} should have been replaced", i);
        }
        
        // Verify that all new projects are in the leaderboard
        for i in 0..10 {
            let has_project = lb.entries.iter().any(|e| e.project_id == 200 + i);
            assert!(has_project, "Project {} should be in leaderboard", 200 + i);
        }
    }

    #[test]
    fn test_leaderboard_update_existing_when_full() {
        let mut lb = create_leaderboard();
        
        // Fill with 100 projects
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Update an existing project (should always succeed)
        let result = lb.update_leaderboard(50, 999999999).unwrap();
        assert!(result);
        assert_eq!(lb.entries.len(), 100);
        
        // Verify the update
        let entry = lb.entries.iter().find(|e| e.project_id == 50).unwrap();
        assert_eq!(entry.burned_amount, 999999999);
    }

    #[test]
    fn test_leaderboard_find_project_position_and_min() {
        let mut lb = create_leaderboard();
        
        lb.update_leaderboard(1, 5000).unwrap();
        lb.update_leaderboard(2, 2000).unwrap(); // Min
        lb.update_leaderboard(3, 3000).unwrap();
        
        // Find existing project
        let (project_pos, min_pos) = lb.find_project_position_and_min(2);
        assert_eq!(project_pos, Some(1));
        assert_eq!(min_pos, Some(1));
        
        // Find non-existing project
        let (project_pos, min_pos) = lb.find_project_position_and_min(99);
        assert_eq!(project_pos, None);
        assert_eq!(min_pos, Some(1));
    }

    #[test]
    fn test_leaderboard_empty() {
        let lb = create_leaderboard();
        let (project_pos, min_pos) = lb.find_project_position_and_min(1);
        
        assert_eq!(project_pos, None);
        assert_eq!(min_pos, None);
    }

    #[test]
    fn test_leaderboard_multiple_same_min_amount() {
        let mut lb = create_leaderboard();
        
        lb.update_leaderboard(1, 1000).unwrap();
        lb.update_leaderboard(2, 1000).unwrap();
        lb.update_leaderboard(3, 2000).unwrap();
        
        let (_, min_pos) = lb.find_project_position_and_min(99);
        
        // Should return the first one found (either 0 or 1)
        assert!(min_pos == Some(0) || min_pos == Some(1));
        if let Some(pos) = min_pos {
            assert_eq!(lb.entries[pos].burned_amount, 1000);
        }
    }

    #[test]
    fn test_leaderboard_update_to_zero() {
        let mut lb = create_leaderboard();
        
        lb.update_leaderboard(1, 5000).unwrap();
        let result = lb.update_leaderboard(1, 0).unwrap();
        
        assert!(result);
        assert_eq!(lb.entries[0].burned_amount, 0);
    }

    #[test]
    fn test_leaderboard_max_u64_amount() {
        let mut lb = create_leaderboard();
        
        let result = lb.update_leaderboard(1, u64::MAX).unwrap();
        assert!(result);
        assert_eq!(lb.entries[0].burned_amount, u64::MAX);
    }

    #[test]
    fn test_leaderboard_replacement_preserves_higher_amounts() {
        let mut lb = create_leaderboard();
        
        // Fill leaderboard with amounts 1000, 2000, ..., 100000
        for i in 0..100 {
            lb.update_leaderboard(i, (i + 1) * 1000).unwrap();
        }
        
        // Add project with amount 50500 (should replace project 0 with 1000)
        lb.update_leaderboard(200, 50500).unwrap();
        
        // Verify all entries >= 2000 are still present
        for i in 1..100 {
            let found = lb.entries.iter().any(|e| e.project_id == i);
            assert!(found, "Project {} should still be in leaderboard", i);
        }
    }

    #[test]
    fn test_leaderboard_update_existing_multiple_times() {
        let mut lb = create_leaderboard();
        
        lb.update_leaderboard(1, 1000).unwrap();
        lb.update_leaderboard(1, 2000).unwrap();
        lb.update_leaderboard(1, 3000).unwrap();
        
        assert_eq!(lb.entries.len(), 1);
        assert_eq!(lb.entries[0].burned_amount, 3000);
    }

    // ============================================================================
    // Project Space Calculation Tests
    // ============================================================================

    #[test]
    fn test_project_space_calculation() {
        let space = Project::calculate_space_max();
        
        // Calculate expected space
        let expected = 8 + // discriminator
            8 + // project_id
            32 + // creator
            8 + // created_at
            8 + // last_updated
            8 + // memo_count
            8 + // burned_amount
            8 + // last_memo_time
            1 + // bump
            4 + 64 + // name
            4 + 256 + // description
            4 + 256 + // image
            4 + 128 + // website
            4 + (4 + 32) * 4 + // tags
            128; // safety buffer
        
        assert_eq!(space, expected);
    }

    #[test]
    fn test_project_space_has_buffer() {
        let space = Project::calculate_space_max();
        
        // Minimum required (without buffer)
        let minimum = 8 + 8 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 
                     (4 + 64) + (4 + 256) + (4 + 256) + (4 + 128) + 
                     (4 + (4 + 32) * 4);
        
        // Space should be greater than minimum due to buffer
        assert!(space > minimum);
        assert_eq!(space - minimum, 128); // 128 byte buffer
    }

    // ============================================================================
    // BurnMemo Serialization Tests
    // ============================================================================

    #[test]
    fn test_burn_memo_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 1000 * DECIMAL_FACTOR,
            payload: vec![1, 2, 3, 4, 5],
        };
        
        let serialized = memo.try_to_vec().unwrap();
        let deserialized = BurnMemo::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, memo.version);
        assert_eq!(deserialized.burn_amount, memo.burn_amount);
        assert_eq!(deserialized.payload, memo.payload);
    }

    #[test]
    fn test_burn_memo_size_calculation() {
        use borsh::BorshSerialize;
        
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 1000 * DECIMAL_FACTOR,
            payload: vec![0u8; MAX_PAYLOAD_LENGTH],
        };
        
        let serialized = memo.try_to_vec().unwrap();
        
        // Size should be version(1) + burn_amount(8) + vec_len(4) + payload(787)
        assert_eq!(serialized.len(), 1 + 8 + 4 + MAX_PAYLOAD_LENGTH);
    }

    // ============================================================================
    // ProjectCreationData Serialization Tests
    // ============================================================================

    #[test]
    fn test_project_creation_data_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let data = create_valid_project_creation_data(1);
        let serialized = data.try_to_vec().unwrap();
        let deserialized = ProjectCreationData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, data.version);
        assert_eq!(deserialized.category, data.category);
        assert_eq!(deserialized.operation, data.operation);
        assert_eq!(deserialized.project_id, data.project_id);
        assert_eq!(deserialized.name, data.name);
    }

    // ============================================================================
    // ProjectUpdateData Serialization Tests
    // ============================================================================

    #[test]
    fn test_project_update_data_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let data = create_valid_project_update_data(1);
        let serialized = data.try_to_vec().unwrap();
        let deserialized = ProjectUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, data.version);
        assert_eq!(deserialized.category, data.category);
        assert_eq!(deserialized.operation, data.operation);
        assert_eq!(deserialized.project_id, data.project_id);
    }

    #[test]
    fn test_project_update_data_serialization_with_none() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let data = ProjectUpdateData {
            version: PROJECT_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            project_id: 1,
            name: None,
            description: None,
            image: None,
            website: None,
            tags: None,
        };
        
        let serialized = data.try_to_vec().unwrap();
        let deserialized = ProjectUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.name, None);
        assert_eq!(deserialized.description, None);
    }

    // ============================================================================
    // LeaderboardEntry Tests
    // ============================================================================

    #[test]
    fn test_leaderboard_entry_default() {
        let entry = LeaderboardEntry::default();
        assert_eq!(entry.project_id, 0);
        assert_eq!(entry.burned_amount, 0);
    }

    #[test]
    fn test_leaderboard_entry_creation() {
        let entry = LeaderboardEntry {
            project_id: 42,
            burned_amount: 123456,
        };
        
        assert_eq!(entry.project_id, 42);
        assert_eq!(entry.burned_amount, 123456);
    }
}

