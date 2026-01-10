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
        assert_eq!(MIN_POST_BURN_TOKENS, 1);
        assert_eq!(MIN_POST_BURN_AMOUNT, 1 * 1_000_000);
        assert_eq!(MAX_BURN_PER_TX, 1_000_000_000_000 * 1_000_000);
    }

    #[test]
    fn test_string_length_constants() {
        assert_eq!(MAX_POST_TITLE_LENGTH, 128);
        assert_eq!(MAX_POST_CONTENT_LENGTH, 512);
        assert_eq!(MAX_POST_IMAGE_LENGTH, 256);
        assert_eq!(MAX_REPLY_MESSAGE_LENGTH, 512);
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
        assert_eq!(POST_CREATION_DATA_VERSION, 1);
        assert_eq!(POST_BURN_DATA_VERSION, 1);
        assert_eq!(POST_MINT_DATA_VERSION, 1);
    }

    #[test]
    fn test_expected_strings() {
        assert_eq!(EXPECTED_CATEGORY, "forum");
        assert_eq!(EXPECTED_CREATE_POST_OPERATION, "create_post");
        assert_eq!(EXPECTED_BURN_FOR_POST_OPERATION, "burn_for_post");
        assert_eq!(EXPECTED_MINT_FOR_POST_OPERATION, "mint_for_post");
    }

    // ============================================================================
    // PostCreationData Validation Tests
    // ============================================================================

    fn create_valid_post_creation_data(creator: Pubkey, post_id: u64) -> PostCreationData {
        PostCreationData {
            version: POST_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_CREATE_POST_OPERATION.to_string(),
            creator: creator.to_string(),
            post_id,
            title: "Test Post Title".to_string(),
            content: "Test post content for the forum".to_string(),
            image: "https://example.com/image.png".to_string(),
        }
    }

    #[test]
    fn test_post_creation_data_valid() {
        let creator = Pubkey::new_unique();
        let post_id = 12345u64;
        let data = create_valid_post_creation_data(creator, post_id);
        assert!(data.validate(creator, post_id).is_ok());
    }

    #[test]
    fn test_post_creation_data_minimal() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let data = PostCreationData {
            version: POST_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_CREATE_POST_OPERATION.to_string(),
            creator: creator.to_string(),
            post_id,
            title: "A".to_string(), // minimum 1 char
            content: "B".to_string(), // minimum 1 char
            image: String::new(), // optional
        };
        assert!(data.validate(creator, post_id).is_ok());
    }

    #[test]
    fn test_post_creation_data_max_lengths() {
        let creator = Pubkey::new_unique();
        let post_id = u64::MAX;
        let data = PostCreationData {
            version: POST_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_CREATE_POST_OPERATION.to_string(),
            creator: creator.to_string(),
            post_id,
            title: "T".repeat(MAX_POST_TITLE_LENGTH),
            content: "C".repeat(MAX_POST_CONTENT_LENGTH),
            image: "I".repeat(MAX_POST_IMAGE_LENGTH),
        };
        assert!(data.validate(creator, post_id).is_ok());
    }

    #[test]
    fn test_post_creation_data_invalid_version() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.version = 99;
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_invalid_category() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.category = "invalid".to_string();
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_invalid_operation() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.operation = "invalid".to_string();
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_creator_mismatch() {
        let creator1 = Pubkey::new_unique();
        let creator2 = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_creation_data(creator1, post_id);
        assert!(data.validate(creator2, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_invalid_creator_format() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.creator = "invalid_pubkey".to_string();
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_post_id_mismatch() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_creation_data(creator, post_id);
        assert!(data.validate(creator, 999u64).is_err());
    }

    #[test]
    fn test_post_creation_data_empty_title() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.title = String::new();
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_title_too_long() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.title = "T".repeat(MAX_POST_TITLE_LENGTH + 1);
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_empty_content() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.content = String::new();
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_content_too_long() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.content = "C".repeat(MAX_POST_CONTENT_LENGTH + 1);
        assert!(data.validate(creator, post_id).is_err());
    }

    #[test]
    fn test_post_creation_data_image_too_long() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_creation_data(creator, post_id);
        data.image = "I".repeat(MAX_POST_IMAGE_LENGTH + 1);
        assert!(data.validate(creator, post_id).is_err());
    }

    // ============================================================================
    // PostBurnData Validation Tests
    // ============================================================================

    fn create_valid_post_burn_data(user: Pubkey, post_id: u64) -> PostBurnData {
        PostBurnData {
            version: POST_BURN_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_POST_OPERATION.to_string(),
            user: user.to_string(),
            post_id,
            message: "Burning tokens to reply to this post".to_string(),
        }
    }

    #[test]
    fn test_post_burn_data_valid() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_burn_data(user, post_id);
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_burn_data_empty_message() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.message = String::new();
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_burn_data_max_message_length() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.message = "M".repeat(MAX_REPLY_MESSAGE_LENGTH);
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_burn_data_invalid_version() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.version = 99;
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_burn_data_invalid_category() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.category = "invalid".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_burn_data_invalid_operation() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.operation = "invalid".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_burn_data_invalid_user_format() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.user = "invalid_pubkey".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_burn_data_user_mismatch() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_burn_data(user1, post_id);
        assert!(data.validate(user2, post_id).is_err());
    }

    #[test]
    fn test_post_burn_data_post_id_mismatch() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_burn_data(user, post_id);
        assert!(data.validate(user, 999u64).is_err());
    }

    #[test]
    fn test_post_burn_data_message_too_long() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_burn_data(user, post_id);
        data.message = "M".repeat(MAX_REPLY_MESSAGE_LENGTH + 1);
        assert!(data.validate(user, post_id).is_err());
    }

    // ============================================================================
    // PostMintData Validation Tests
    // ============================================================================

    fn create_valid_post_mint_data(user: Pubkey, post_id: u64) -> PostMintData {
        PostMintData {
            version: POST_MINT_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_POST_OPERATION.to_string(),
            user: user.to_string(),
            post_id,
            message: "Minting tokens to reply to this post".to_string(),
        }
    }

    #[test]
    fn test_post_mint_data_valid() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_mint_data(user, post_id);
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_mint_data_empty_message() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.message = String::new();
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_mint_data_max_message_length() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.message = "M".repeat(MAX_REPLY_MESSAGE_LENGTH);
        assert!(data.validate(user, post_id).is_ok());
    }

    #[test]
    fn test_post_mint_data_invalid_version() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.version = 99;
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_mint_data_invalid_category() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.category = "invalid".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_mint_data_invalid_operation() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.operation = "invalid".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_mint_data_invalid_user_format() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.user = "invalid_pubkey".to_string();
        assert!(data.validate(user, post_id).is_err());
    }

    #[test]
    fn test_post_mint_data_user_mismatch() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_mint_data(user1, post_id);
        assert!(data.validate(user2, post_id).is_err());
    }

    #[test]
    fn test_post_mint_data_post_id_mismatch() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let data = create_valid_post_mint_data(user, post_id);
        assert!(data.validate(user, 999u64).is_err());
    }

    #[test]
    fn test_post_mint_data_message_too_long() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let mut data = create_valid_post_mint_data(user, post_id);
        data.message = "M".repeat(MAX_REPLY_MESSAGE_LENGTH + 1);
        assert!(data.validate(user, post_id).is_err());
    }

    // ============================================================================
    // Global Counter Space Calculation Tests
    // ============================================================================

    #[test]
    fn test_global_post_counter_space() {
        let expected = 8 + // discriminator
            8; // total_posts (u64)
        
        assert_eq!(GlobalPostCounter::SPACE, expected);
    }

    // ============================================================================
    // Post Space Calculation Tests
    // ============================================================================

    #[test]
    fn test_post_space_calculation() {
        let space = Post::calculate_space_max();
        
        // Calculate expected space
        let expected = 8 + // discriminator
            8 + // post_id
            32 + // creator
            8 + // created_at
            8 + // last_updated
            8 + // reply_count
            8 + // burned_amount
            8 + // last_reply_time
            1 + // bump
            4 + 128 + // title
            4 + 512 + // content
            4 + 256 + // image
            128; // safety buffer
        
        assert_eq!(space, expected);
    }

    #[test]
    fn test_post_space_has_buffer() {
        let space = Post::calculate_space_max();
        
        // Minimum required (without buffer)
        let minimum = 8 + 8 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 
                     (4 + 128) + (4 + 512) + (4 + 256);
        
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
    // PostCreationData Serialization Tests
    // ============================================================================

    #[test]
    fn test_post_creation_data_serialization() {
        use borsh::{BorshSerialize, BorshDeserialize};
        
        let creator = Pubkey::new_unique();
        let post_id = 12345u64;
        let data = create_valid_post_creation_data(creator, post_id);
        let serialized = data.try_to_vec().unwrap();
        let deserialized = PostCreationData::try_from_slice(&serialized).unwrap();
        
        assert_eq!(deserialized.version, data.version);
        assert_eq!(deserialized.category, data.category);
        assert_eq!(deserialized.operation, data.operation);
        assert_eq!(deserialized.creator, data.creator);
        assert_eq!(deserialized.post_id, data.post_id);
        assert_eq!(deserialized.title, data.title);
        assert_eq!(deserialized.content, data.content);
    }

    // ============================================================================
    // Integration-style Tests
    // ============================================================================

    #[test]
    fn test_forum_lifecycle_data_structures() {
        // Test a full lifecycle: create post -> burn reply -> mint reply
        let creator = Pubkey::new_unique();
        let replier = Pubkey::new_unique();
        let post_id = 12345u64;
        
        // 1. Creation by creator
        let create_data = create_valid_post_creation_data(creator, post_id);
        assert!(create_data.validate(creator, post_id).is_ok());
        
        // 2. Burn reply by different user (anyone can reply)
        let burn_data = create_valid_post_burn_data(replier, post_id);
        assert!(burn_data.validate(replier, post_id).is_ok());
        
        // 3. Mint reply by different user (anyone can reply)
        let mint_data = create_valid_post_mint_data(replier, post_id);
        assert!(mint_data.validate(replier, post_id).is_ok());
        
        // 4. Burn reply by creator too
        let burn_data_creator = create_valid_post_burn_data(creator, post_id);
        assert!(burn_data_creator.validate(creator, post_id).is_ok());
    }

    #[test]
    fn test_multiple_posts_by_same_creator() {
        // Verify that the same creator can create multiple posts
        let creator = Pubkey::new_unique();
        
        // Create multiple posts with different post_ids
        let post1 = create_valid_post_creation_data(creator, 1);
        let post2 = create_valid_post_creation_data(creator, 2);
        let post3 = create_valid_post_creation_data(creator, u64::MAX);
        
        assert!(post1.validate(creator, 1).is_ok());
        assert!(post2.validate(creator, 2).is_ok());
        assert!(post3.validate(creator, u64::MAX).is_ok());
    }

    #[test]
    fn test_anyone_can_reply_to_post() {
        // Key feature: any user can burn/mint for any post
        let post_creator = Pubkey::new_unique();
        let random_user1 = Pubkey::new_unique();
        let random_user2 = Pubkey::new_unique();
        let post_id = 12345u64;
        
        // Post creator creates the post
        let create_data = create_valid_post_creation_data(post_creator, post_id);
        assert!(create_data.validate(post_creator, post_id).is_ok());
        
        // Random user 1 can burn for the post
        let burn1 = create_valid_post_burn_data(random_user1, post_id);
        assert!(burn1.validate(random_user1, post_id).is_ok());
        
        // Random user 2 can also burn for the post
        let burn2 = create_valid_post_burn_data(random_user2, post_id);
        assert!(burn2.validate(random_user2, post_id).is_ok());
        
        // Random user 1 can mint for the post
        let mint1 = create_valid_post_mint_data(random_user1, post_id);
        assert!(mint1.validate(random_user1, post_id).is_ok());
        
        // Post creator can also reply to their own post
        let burn_creator = create_valid_post_burn_data(post_creator, post_id);
        assert!(burn_creator.validate(post_creator, post_id).is_ok());
    }

    #[test]
    fn test_min_burn_amount_is_one_token() {
        // Same as memo-blog: 1 MEMO minimum
        assert_eq!(MIN_POST_BURN_TOKENS, 1);
        assert_eq!(MIN_POST_BURN_AMOUNT, 1_000_000); // 1 token in units
    }

    #[test]
    fn test_forum_has_longer_content_than_blog() {
        // Forum posts have longer content limit than blog
        assert_eq!(MAX_POST_CONTENT_LENGTH, 512);
        assert_eq!(MAX_POST_TITLE_LENGTH, 128);
        // Reply messages also have longer limit
        assert_eq!(MAX_REPLY_MESSAGE_LENGTH, 512);
    }

    // ============================================================================
    // validate_memo_length() Tests
    // ============================================================================

    #[test]
    fn test_valid_memo_minimum_length() {
        let memo_data = vec![b'x'; MEMO_MIN_LENGTH];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_ok(), "Memo at minimum length should be valid");
        let (valid, data) = result.unwrap();
        assert!(valid);
        assert_eq!(data.len(), MEMO_MIN_LENGTH);
    }

    #[test]
    fn test_valid_memo_maximum_length() {
        let memo_data = vec![b'x'; MEMO_MAX_LENGTH];
        let result = validate_memo_length(&memo_data, MEMO_MIN_LENGTH, MEMO_MAX_LENGTH);
        assert!(result.is_ok(), "Memo at maximum length should be valid");
        let (valid, data) = result.unwrap();
        assert!(valid);
        assert_eq!(data.len(), MEMO_MAX_LENGTH);
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

    // ============================================================================
    // Base64 Encoding/Decoding Tests
    // ============================================================================

    #[test]
    fn test_base64_encode_decode_roundtrip() {
        let original = b"Hello, Forum!".to_vec();
        let encoded = general_purpose::STANDARD.encode(&original);
        let decoded = general_purpose::STANDARD.decode(&encoded).unwrap();
        
        assert_eq!(original, decoded, "Base64 encode/decode should be reversible");
    }

    #[test]
    fn test_base64_encode_burn_memo() {
        use borsh::BorshSerialize;
        
        let memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: MIN_POST_BURN_AMOUNT,
            payload: b"test".to_vec(),
        };
        
        let borsh_data = memo.try_to_vec().unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let decoded_data = general_purpose::STANDARD.decode(&base64_encoded).unwrap();
        let decoded_memo = BurnMemo::try_from_slice(&decoded_data).unwrap();
        
        assert_eq!(memo.version, decoded_memo.version);
        assert_eq!(memo.burn_amount, decoded_memo.burn_amount);
        assert_eq!(memo.payload, decoded_memo.payload);
    }

    // ============================================================================
    // Helper Functions for Memo Creation (for parse tests)
    // ============================================================================

    /// Create a valid Borsh+Base64 encoded memo for post creation
    fn create_post_creation_memo(
        burn_amount: u64,
        creator: Pubkey,
        post_id: u64,
        title: &str,
        content: &str,
        image: &str,
    ) -> Vec<u8> {
        use borsh::BorshSerialize;
        
        let post_data = PostCreationData {
            version: POST_CREATION_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_CREATE_POST_OPERATION.to_string(),
            creator: creator.to_string(),
            post_id,
            title: title.to_string(),
            content: content.to_string(),
            image: image.to_string(),
        };
        
        let payload = post_data.try_to_vec().unwrap();
        
        let burn_memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        
        let borsh_data = burn_memo.try_to_vec().unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        base64_encoded.into_bytes()
    }

    /// Create a valid Borsh+Base64 encoded memo for post burn
    fn create_post_burn_memo(
        burn_amount: u64,
        user: Pubkey,
        post_id: u64,
        message: &str,
    ) -> Vec<u8> {
        use borsh::BorshSerialize;
        
        let burn_data = PostBurnData {
            version: POST_BURN_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_BURN_FOR_POST_OPERATION.to_string(),
            user: user.to_string(),
            post_id,
            message: message.to_string(),
        };
        
        let payload = burn_data.try_to_vec().unwrap();
        
        let burn_memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount,
            payload,
        };
        
        let borsh_data = burn_memo.try_to_vec().unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        base64_encoded.into_bytes()
    }

    /// Create a valid Borsh+Base64 encoded memo for post mint
    fn create_post_mint_memo(
        user: Pubkey,
        post_id: u64,
        message: &str,
    ) -> Vec<u8> {
        use borsh::BorshSerialize;
        
        let mint_data = PostMintData {
            version: POST_MINT_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_POST_OPERATION.to_string(),
            user: user.to_string(),
            post_id,
            message: message.to_string(),
        };
        
        let payload = mint_data.try_to_vec().unwrap();
        
        // For mint operations, burn_amount should be 0
        let burn_memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: 0,
            payload,
        };
        
        let borsh_data = burn_memo.try_to_vec().unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        base64_encoded.into_bytes()
    }

    // ============================================================================
    // parse_post_creation_borsh_memo() Tests
    // ============================================================================

    #[test]
    fn test_parse_valid_post_creation_memo() {
        let creator = Pubkey::new_unique();
        let post_id = 12345u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        let memo_data = create_post_creation_memo(
            burn_amount,
            creator,
            post_id,
            "Test Post",
            "Test content for the post",
            "https://example.com/image.png",
        );
        
        let result = parse_post_creation_borsh_memo(&memo_data, creator, post_id, burn_amount);
        assert!(result.is_ok(), "Valid post creation memo should parse successfully");
        
        let post_data = result.unwrap();
        assert_eq!(post_data.title, "Test Post");
        assert_eq!(post_data.content, "Test content for the post");
        assert_eq!(post_data.post_id, post_id);
    }

    #[test]
    fn test_parse_post_creation_memo_wrong_burn_amount() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let memo_burn_amount = MIN_POST_BURN_AMOUNT;
        let expected_burn_amount = memo_burn_amount + DECIMAL_FACTOR;
        
        let memo_data = create_post_creation_memo(
            memo_burn_amount,
            creator,
            post_id,
            "Test",
            "Content",
            "",
        );
        
        let result = parse_post_creation_borsh_memo(&memo_data, creator, post_id, expected_burn_amount);
        assert!(result.is_err(), "Mismatched burn amount should fail parsing");
    }

    #[test]
    fn test_parse_post_creation_memo_wrong_user() {
        let creator1 = Pubkey::new_unique();
        let creator2 = Pubkey::new_unique();
        let post_id = 1u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        
        let memo_data = create_post_creation_memo(
            burn_amount,
            creator1,
            post_id,
            "Test",
            "Content",
            "",
        );
        
        let result = parse_post_creation_borsh_memo(&memo_data, creator2, post_id, burn_amount);
        assert!(result.is_err(), "Mismatched user should fail parsing");
    }

    #[test]
    fn test_parse_post_creation_memo_invalid_base64() {
        let creator = Pubkey::new_unique();
        let post_id = 1u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        let invalid_base64 = b"not valid base64!!!".to_vec();
        
        let result = parse_post_creation_borsh_memo(&invalid_base64, creator, post_id, burn_amount);
        assert!(result.is_err(), "Invalid base64 should fail parsing");
    }

    // ============================================================================
    // parse_post_burn_borsh_memo() Tests
    // ============================================================================

    #[test]
    fn test_parse_valid_post_burn_memo() {
        let user = Pubkey::new_unique();
        let post_id = 12345u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        let memo_data = create_post_burn_memo(
            burn_amount,
            user,
            post_id,
            "Great post!",
        );
        
        let result = parse_post_burn_borsh_memo(&memo_data, burn_amount, user, post_id);
        assert!(result.is_ok(), "Valid post burn memo should parse successfully");
    }

    #[test]
    fn test_parse_post_burn_memo_wrong_user() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let post_id = 1u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        
        let memo_data = create_post_burn_memo(
            burn_amount,
            user1,
            post_id,
            "Test",
        );
        
        let result = parse_post_burn_borsh_memo(&memo_data, burn_amount, user2, post_id);
        assert!(result.is_err(), "Mismatched user should fail parsing");
    }

    #[test]
    fn test_parse_post_burn_memo_wrong_post_id() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        let burn_amount = MIN_POST_BURN_AMOUNT;
        
        let memo_data = create_post_burn_memo(
            burn_amount,
            user,
            post_id,
            "Test",
        );
        
        let result = parse_post_burn_borsh_memo(&memo_data, burn_amount, user, 999u64);
        assert!(result.is_err(), "Mismatched post_id should fail parsing");
    }

    // ============================================================================
    // parse_post_mint_borsh_memo() Tests
    // ============================================================================

    #[test]
    fn test_parse_valid_post_mint_memo() {
        let user = Pubkey::new_unique();
        let post_id = 12345u64;
        let memo_data = create_post_mint_memo(
            user,
            post_id,
            "Minting to support this post!",
        );
        
        let result = parse_post_mint_borsh_memo(&memo_data, user, post_id);
        assert!(result.is_ok(), "Valid post mint memo should parse successfully");
    }

    #[test]
    fn test_parse_post_mint_memo_wrong_user() {
        let user1 = Pubkey::new_unique();
        let user2 = Pubkey::new_unique();
        let post_id = 1u64;
        
        let memo_data = create_post_mint_memo(
            user1,
            post_id,
            "Test",
        );
        
        let result = parse_post_mint_borsh_memo(&memo_data, user2, post_id);
        assert!(result.is_err(), "Mismatched user should fail parsing");
    }

    #[test]
    fn test_parse_post_mint_memo_wrong_post_id() {
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        
        let memo_data = create_post_mint_memo(
            user,
            post_id,
            "Test",
        );
        
        let result = parse_post_mint_borsh_memo(&memo_data, user, 999u64);
        assert!(result.is_err(), "Mismatched post_id should fail parsing");
    }

    #[test]
    fn test_parse_post_mint_memo_with_nonzero_burn_amount() {
        use borsh::BorshSerialize;
        
        let user = Pubkey::new_unique();
        let post_id = 1u64;
        
        let mint_data = PostMintData {
            version: POST_MINT_DATA_VERSION,
            category: EXPECTED_CATEGORY.to_string(),
            operation: EXPECTED_MINT_FOR_POST_OPERATION.to_string(),
            user: user.to_string(),
            post_id,
            message: "Test".to_string(),
        };
        
        let payload = mint_data.try_to_vec().unwrap();
        
        // Create memo with non-zero burn_amount (should be 0 for mint)
        let burn_memo = BurnMemo {
            version: BURN_MEMO_VERSION,
            burn_amount: MIN_POST_BURN_AMOUNT, // Should be 0 for mint
            payload,
        };
        
        let borsh_data = burn_memo.try_to_vec().unwrap();
        let base64_encoded = general_purpose::STANDARD.encode(&borsh_data);
        let memo_data = base64_encoded.into_bytes();
        
        let result = parse_post_mint_borsh_memo(&memo_data, user, post_id);
        assert!(result.is_err(), "Mint memo with non-zero burn_amount should fail");
    }
}
