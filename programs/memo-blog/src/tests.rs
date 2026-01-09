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
        assert_eq!(MIN_BLOG_BURN_TOKENS, 1);
        assert_eq!(MIN_BLOG_BURN_AMOUNT, 1 * 1_000_000);
        assert_eq!(MAX_BURN_PER_TX, 1_000_000_000_000 * 1_000_000);
    }

    #[test]
    fn test_string_length_constants() {
        assert_eq!(MAX_BLOG_NAME_LENGTH, 64);
        assert_eq!(MAX_BLOG_DESCRIPTION_LENGTH, 256);
        assert_eq!(MAX_BLOG_IMAGE_LENGTH, 256);
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
        assert_eq!(BLOG_CREATION_DATA_VERSION, 1);
        assert_eq!(BLOG_UPDATE_DATA_VERSION, 1);
    }

    #[test]
    fn test_expected_strings() {
        assert_eq!(EXPECTED_CATEGORY, "blog");
        assert_eq!(EXPECTED_OPERATION, "create_blog");
        assert_eq!(EXPECTED_UPDATE_OPERATION, "update_blog");
        assert_eq!(EXPECTED_BURN_FOR_BLOG_OPERATION, "burn_for_blog");
        assert_eq!(EXPECTED_MINT_FOR_BLOG_OPERATION, "mint_for_blog");
        assert_eq!(MAX_MESSAGE_LENGTH, 696);
    }

    // ============================================================================
    // BlogCreationData Validation Tests
    // ============================================================================

    fn create_valid_blog_creation_data(creator: Pubkey) -> BlogCreationData {
        BlogCreationData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            creator: creator.to_string(),
            name: "Test Blog".to_string(),
            description: "Test blog description".to_string(),
            image: "https://example.com/blog-image.png".to_string(),
        }
    }

    #[test]
    fn test_blog_creation_data_valid() {
        let creator = Pubkey::new_unique();
        let data = create_valid_blog_creation_data(creator);
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_creation_data_minimal() {
        let creator = Pubkey::new_unique();
        let data = BlogCreationData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            creator: creator.to_string(),
            name: "A".to_string(), // minimum 1 char
            description: String::new(),
            image: String::new(),
        };
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_creation_data_max_lengths() {
        let creator = Pubkey::new_unique();
        let data = BlogCreationData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_OPERATION.to_string(),
            creator: creator.to_string(),
            name: "A".repeat(MAX_BLOG_NAME_LENGTH),
            description: "D".repeat(MAX_BLOG_DESCRIPTION_LENGTH),
            image: "I".repeat(MAX_BLOG_IMAGE_LENGTH),
        };
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_creation_data_invalid_version() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.version = 99;
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_invalid_category() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.category = "invalid".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_invalid_operation() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.operation = "invalid".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_creator_mismatch() {
        let creator1 = Pubkey::new_unique();
        let creator2 = Pubkey::new_unique();
        let data = create_valid_blog_creation_data(creator1);
        assert!(data.validate(creator2).is_err());
    }

    #[test]
    fn test_blog_creation_data_invalid_creator_format() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.creator = "invalid_pubkey".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_empty_name() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.name = String::new();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_name_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.name = "A".repeat(MAX_BLOG_NAME_LENGTH + 1);
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_description_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.description = "D".repeat(MAX_BLOG_DESCRIPTION_LENGTH + 1);
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_creation_data_image_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_creation_data(creator);
        data.image = "I".repeat(MAX_BLOG_IMAGE_LENGTH + 1);
        assert!(data.validate(creator).is_err());
    }

    // ============================================================================
    // BlogUpdateData Validation Tests
    // ============================================================================

    fn create_valid_blog_update_data(creator: Pubkey) -> BlogUpdateData {
        BlogUpdateData {
            version: BLOG_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            creator: creator.to_string(),
            name: Some("Updated Blog".to_string()),
            description: Some("Updated blog description".to_string()),
            image: Some("https://example.com/new-blog-image.png".to_string()),
        }
    }

    #[test]
    fn test_blog_update_data_valid() {
        let creator = Pubkey::new_unique();
        let data = create_valid_blog_update_data(creator);
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_update_data_all_none() {
        let creator = Pubkey::new_unique();
        let data = BlogUpdateData {
            version: BLOG_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            creator: creator.to_string(),
            name: None,
            description: None,
            image: None,
        };
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_update_data_partial_update() {
        let creator = Pubkey::new_unique();
        let data = BlogUpdateData {
            version: BLOG_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            creator: creator.to_string(),
            name: Some("New Name".to_string()),
            description: None,
            image: None,
        };
        assert!(data.validate(creator).is_ok());
    }

    #[test]
    fn test_blog_update_data_invalid_version() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.version = 99;
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_invalid_category() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.category = "invalid".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_invalid_operation() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.operation = "invalid".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_creator_mismatch() {
        let creator1 = Pubkey::new_unique();
        let creator2 = Pubkey::new_unique();
        let data = create_valid_blog_update_data(creator1);
        assert!(data.validate(creator2).is_err());
    }

    #[test]
    fn test_blog_update_data_invalid_creator_format() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.creator = "invalid_pubkey".to_string();
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_empty_name() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.name = Some(String::new());
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_name_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.name = Some("A".repeat(MAX_BLOG_NAME_LENGTH + 1));
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_description_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.description = Some("D".repeat(MAX_BLOG_DESCRIPTION_LENGTH + 1));
        assert!(data.validate(creator).is_err());
    }

    #[test]
    fn test_blog_update_data_image_too_long() {
        let creator = Pubkey::new_unique();
        let mut data = create_valid_blog_update_data(creator);
        data.image = Some("I".repeat(MAX_BLOG_IMAGE_LENGTH + 1));
        assert!(data.validate(creator).is_err());
    }

    // ============================================================================
    // BlogBurnData Validation Tests
    // ============================================================================

    fn create_valid_blog_burn_data(burner: Pubkey) -> BlogBurnData {
        BlogBurnData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_BLOG_OPERATION.to_string(),
            burner: burner.to_string(),
            message: "Burning for blog support".to_string(),
        }
    }

    #[test]
    fn test_blog_burn_data_valid() {
        let burner = Pubkey::new_unique();
        let data = create_valid_blog_burn_data(burner);
        assert!(data.validate(burner).is_ok());
    }

    #[test]
    fn test_blog_burn_data_empty_message() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.message = String::new();
        assert!(data.validate(burner).is_ok());
    }

    #[test]
    fn test_blog_burn_data_max_message_length() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.message = "M".repeat(MAX_MESSAGE_LENGTH);
        assert!(data.validate(burner).is_ok());
    }

    #[test]
    fn test_blog_burn_data_invalid_version() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.version = 99;
        assert!(data.validate(burner).is_err());
    }

    #[test]
    fn test_blog_burn_data_invalid_category() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.category = "invalid".to_string();
        assert!(data.validate(burner).is_err());
    }

    #[test]
    fn test_blog_burn_data_invalid_operation() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.operation = "invalid".to_string();
        assert!(data.validate(burner).is_err());
    }

    #[test]
    fn test_blog_burn_data_invalid_burner_format() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.burner = "invalid_pubkey".to_string();
        assert!(data.validate(burner).is_err());
    }

    #[test]
    fn test_blog_burn_data_burner_mismatch() {
        let burner1 = Pubkey::new_unique();
        let burner2 = Pubkey::new_unique();
        let data = create_valid_blog_burn_data(burner1);
        assert!(data.validate(burner2).is_err());
    }

    #[test]
    fn test_blog_burn_data_message_too_long() {
        let burner = Pubkey::new_unique();
        let mut data = create_valid_blog_burn_data(burner);
        data.message = "M".repeat(MAX_MESSAGE_LENGTH + 1);
        assert!(data.validate(burner).is_err());
    }

    // ============================================================================
    // BlogMintData Validation Tests
    // ============================================================================

    fn create_valid_blog_mint_data(minter: Pubkey) -> BlogMintData {
        BlogMintData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_BLOG_OPERATION.to_string(),
            minter: minter.to_string(),
            message: "Minting for blog reward".to_string(),
        }
    }

    #[test]
    fn test_blog_mint_data_valid() {
        let minter = Pubkey::new_unique();
        let data = create_valid_blog_mint_data(minter);
        assert!(data.validate(minter).is_ok());
    }

    #[test]
    fn test_blog_mint_data_empty_message() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.message = String::new();
        assert!(data.validate(minter).is_ok());
    }

    #[test]
    fn test_blog_mint_data_max_message_length() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.message = "M".repeat(MAX_MESSAGE_LENGTH);
        assert!(data.validate(minter).is_ok());
    }

    #[test]
    fn test_blog_mint_data_invalid_version() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.version = 99;
        assert!(data.validate(minter).is_err());
    }

    #[test]
    fn test_blog_mint_data_invalid_category() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.category = "invalid".to_string();
        assert!(data.validate(minter).is_err());
    }

    #[test]
    fn test_blog_mint_data_invalid_operation() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.operation = "invalid".to_string();
        assert!(data.validate(minter).is_err());
    }

    #[test]
    fn test_blog_mint_data_invalid_minter_format() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.minter = "invalid_pubkey".to_string();
        assert!(data.validate(minter).is_err());
    }

    #[test]
    fn test_blog_mint_data_minter_mismatch() {
        let minter1 = Pubkey::new_unique();
        let minter2 = Pubkey::new_unique();
        let data = create_valid_blog_mint_data(minter1);
        assert!(data.validate(minter2).is_err());
    }

    #[test]
    fn test_blog_mint_data_message_too_long() {
        let minter = Pubkey::new_unique();
        let mut data = create_valid_blog_mint_data(minter);
        data.message = "M".repeat(MAX_MESSAGE_LENGTH + 1);
        assert!(data.validate(minter).is_err());
    }

    // ============================================================================
    // Blog Space Calculation Tests
    // ============================================================================

    #[test]
    fn test_blog_space_calculation() {
        let space = Blog::calculate_space_max();
        
        // Calculate expected space (no blog_id anymore, creator is the unique identifier)
        let expected = 8 + // discriminator
            32 + // creator
            8 + // created_at
            8 + // last_updated
            8 + // memo_count
            8 + // burned_amount
            8 + // minted_amount
            8 + // last_memo_time
            1 + // bump
            4 + 64 + // name
            4 + 256 + // description
            4 + 256 + // image
            128; // safety buffer
        
        assert_eq!(space, expected);
    }

    #[test]
    fn test_blog_space_has_buffer() {
        let space = Blog::calculate_space_max();
        
        // Minimum required (without buffer) - no blog_id anymore
        let minimum = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 
                     (4 + 64) + (4 + 256) + (4 + 256);
        
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
    fn test_burn_memo_zero_amount_for_mint() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        // For mint operations, burn_amount should be 0
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 0,
            payload: vec![1, 2, 3, 4, 5],
        };
        
        let serialized = memo.try_to_vec().unwrap();
        let deserialized = BurnMemo::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.burn_amount, 0);
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
    // BlogCreationData Serialization Tests
    // ============================================================================

    #[test]
    fn test_blog_creation_data_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let creator = Pubkey::new_unique();
        let data = create_valid_blog_creation_data(creator);
        let serialized = data.try_to_vec().unwrap();
        let deserialized = BlogCreationData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, data.version);
        assert_eq!(deserialized.category, data.category);
        assert_eq!(deserialized.operation, data.operation);
        assert_eq!(deserialized.creator, data.creator);
        assert_eq!(deserialized.name, data.name);
    }

    // ============================================================================
    // BlogUpdateData Serialization Tests
    // ============================================================================

    #[test]
    fn test_blog_update_data_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let creator = Pubkey::new_unique();
        let data = create_valid_blog_update_data(creator);
        let serialized = data.try_to_vec().unwrap();
        let deserialized = BlogUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, data.version);
        assert_eq!(deserialized.category, data.category);
        assert_eq!(deserialized.operation, data.operation);
        assert_eq!(deserialized.creator, data.creator);
    }

    #[test]
    fn test_blog_update_data_serialization_with_none() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let creator = Pubkey::new_unique();
        let data = BlogUpdateData {
            version: BLOG_UPDATE_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_UPDATE_OPERATION.to_string(),
            creator: creator.to_string(),
            name: None,
            description: None,
            image: None,
        };
        
        let serialized = data.try_to_vec().unwrap();
        let deserialized = BlogUpdateData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.name, None);
        assert_eq!(deserialized.description, None);
        assert_eq!(deserialized.image, None);
    }

    // ============================================================================
    // Integration-style Tests
    // ============================================================================

    #[test]
    fn test_blog_lifecycle_data_structures() {
        // Test a full lifecycle: create -> update -> burn -> mint
        let creator = Pubkey::new_unique();
        
        // 1. Creation
        let create_data = create_valid_blog_creation_data(creator);
        assert!(create_data.validate(creator).is_ok());
        
        // 2. Update
        let update_data = create_valid_blog_update_data(creator);
        assert!(update_data.validate(creator).is_ok());
        
        // 3. Burn
        let burn_data = create_valid_blog_burn_data(creator);
        assert!(burn_data.validate(creator).is_ok());
        
        // 4. Mint
        let mint_data = create_valid_blog_mint_data(creator);
        assert!(mint_data.validate(creator).is_ok());
    }

    #[test]
    fn test_multiple_operations_on_same_blog() {
        let creator = Pubkey::new_unique();
        
        // Multiple burns with different messages
        let burn1 = BlogBurnData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_BLOG_OPERATION.to_string(),
            burner: creator.to_string(),
            message: "First burn".to_string(),
        };
        assert!(burn1.validate(creator).is_ok());
        
        let burn2 = BlogBurnData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_BLOG_OPERATION.to_string(),
            burner: creator.to_string(),
            message: "Second burn".to_string(),
        };
        assert!(burn2.validate(creator).is_ok());
        
        // Multiple mints with different messages
        let mint1 = BlogMintData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_BLOG_OPERATION.to_string(),
            minter: creator.to_string(),
            message: "First mint".to_string(),
        };
        assert!(mint1.validate(creator).is_ok());
        
        let mint2 = BlogMintData {
            version: BLOG_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_BLOG_OPERATION.to_string(),
            minter: creator.to_string(),
            message: "Second mint".to_string(),
        };
        assert!(mint2.validate(creator).is_ok());
    }

    #[test]
    fn test_min_burn_amount_is_one_token() {
        // This is a key difference from memo-project (which requires 42,069 tokens)
        assert_eq!(MIN_BLOG_BURN_TOKENS, 1);
        assert_eq!(MIN_BLOG_BURN_AMOUNT, 1_000_000); // 1 token in units
    }

    #[test]
    fn test_no_website_or_tags_in_blog() {
        // Blog is simpler than Project - no website or tags fields
        let creator = Pubkey::new_unique();
        let data = create_valid_blog_creation_data(creator);
        
        // Should only have name, description, and image
        assert!(!data.name.is_empty());
        // Description and image can be empty but exist as fields
        let _ = data.description;
        let _ = data.image;
    }
}
