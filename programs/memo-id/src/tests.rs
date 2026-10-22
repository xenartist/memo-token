//! Unit tests for memo-id
//!
//! Covers the pure parts of the program:
//! - label and MEMO ID rules (DNS hostname labels, 64-byte limit)
//! - prices: the length table, ceil rounding, scaling with the memo-mint reward
//! - the BurnMemo and its payload: decoding, amount, version, every payload field
//! - URIs
//! - account layouts: sizes, fixed offsets for memcmp filters, the maximum Registry
//! - controller_of (owner, one-level Vault rule), forward records, Primary IDs, is_verified
//! - PDAs
//! The CPI paths (memo-home, Core, memo-burn, Token-2022) are covered by the local-validator
//! regression suite (clients/regression/src/regression-memo-id.rs).

use super::*;

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

const CORE: Pubkey = memo_home::CORE_PROGRAM_ID;
const SYSTEM: Pubkey = anchor_lang::system_program::ID;

/// A Core AssetV1 (no plugins) owned by `owner` in `collection`
fn core_asset(owner: Pubkey, collection: Pubkey, name: &str) -> Vec<u8> {
    let mut d = vec![1u8]; // Key::AssetV1
    d.extend_from_slice(owner.as_ref());
    d.push(2); // UpdateAuthority::Collection
    d.extend_from_slice(collection.as_ref());
    for s in [name, "https://example.com/a.json"] {
        d.extend_from_slice(&(s.len() as u32).to_le_bytes());
        d.extend_from_slice(s.as_bytes());
    }
    d.push(0); // seq: None
    d
}

fn with_account<R>(address: Pubkey, owner: Pubkey, data: Vec<u8>, f: impl FnOnce(&AccountInfo) -> R) -> R {
    let mut lamports = 1_000_000u64;
    let mut data = data;
    let info = AccountInfo::new(&address, false, false, &mut lamports, &mut data, &owner, false, 0);
    f(&info)
}

/// Base64 BurnMemo with a memo-id payload
fn memo(amount: u64, payload: &MemoIdPayload) -> Vec<u8> {
    let bm = BurnMemo { version: BURN_MEMO_VERSION, burn_amount: amount, payload: payload.try_to_vec().unwrap() };
    general_purpose::STANDARD.encode(bm.try_to_vec().unwrap()).into_bytes()
}

fn payload(operation: &str, payer: &Pubkey, memo_id: &str) -> MemoIdPayload {
    MemoIdPayload {
        version: PAYLOAD_VERSION,
        category: EXPECTED_CATEGORY.into(),
        operation: operation.into(),
        user_pubkey: payer.to_string(),
        memo_id: memo_id.into(),
    }
}

fn registry(kind: MemoIdKind, asset: Pubkey, parent: Pubkey, memo_id: &str) -> Registry {
    Registry {
        kind,
        asset,
        parent,
        created_at: 0,
        nft_subids_issued: 0,
        record_subids: 0,
        bump: 255,
        target_set_by: Pubkey::default(),
        target: None,
        memo_id: memo_id.into(),
    }
}

// ============================================================================
// Names
// ============================================================================

mod names {
    use super::*;

    #[test]
    fn labels() {
        for ok in ["a", "0", "xen-artist", "0x1", "a-b-c", "x-n--a", &"a".repeat(63)] {
            assert!(valid_label(ok), "{}", ok);
        }
        let long = "a".repeat(64);
        for bad in ["", "-a", "a-", "-", "A", "aB", "a_b", "a b", "a.b", "xn--a", "é", "a/b", &long] {
            assert!(!valid_label(bad), "{}", bad);
        }
    }

    #[test]
    fn memo_ids() {
        for ok in ["a.memo", "xen-artist.memo", "blog.xen-artist.memo", "a.b.c.memo"] {
            assert!(valid_memo_id(ok), "{}", ok);
        }
        for bad in ["memo", ".memo", "a.x1", "a.memo.", "a..memo", "-a.memo", "A.memo", "amemo", "a.memox"] {
            assert!(!valid_memo_id(bad), "{}", bad);
        }
    }

    /// `n` bytes of 63-byte labels (and a shorter first one) ending in `.memo`
    fn id_of_len(n: usize) -> String {
        let mut s = String::from("memo");
        while s.len() < n {
            let room = n - s.len() - 1; // one byte for the dot
            s = format!("{}.{}", "a".repeat(room.min(63)), s);
        }
        s
    }

    #[test]
    fn full_id_is_at_most_253_bytes() {
        // the DNS hostname limit; memo-home accepts Asset names that long
        assert_eq!(MAX_MEMO_ID_LEN, 253);
        assert!(MAX_MEMO_ID_LEN <= memo_home::MAX_NAME_LEN);
        let max = id_of_len(253);
        assert_eq!(max.len(), 253);
        assert!(valid_memo_id(&max));
        let over = id_of_len(254);
        assert_eq!(over.len(), 254);
        assert!(!valid_memo_id(&over));
        // the longest second-level ID: a 63-byte label
        assert!(valid_memo_id(&format!("{}.memo", "a".repeat(63))));
        assert!(!valid_memo_id(&format!("{}.memo", "a".repeat(64))));
    }
}

// ============================================================================
// Prices
// ============================================================================

mod prices {
    use super::*;

    const UNIT: u64 = 1_000_000; // 1 MEMO

    #[test]
    fn table() {
        assert_eq!(price_centi_mints(1), 42_069_000);
        assert_eq!(price_centi_mints(2), 4_206_900);
        assert_eq!(price_centi_mints(3), 420_690);
        assert_eq!(price_centi_mints(4), 42_069);
        for len in [5, 6, 20, 59, 63] {
            assert_eq!(price_centi_mints(len), 6_900);
        }
        assert_eq!(PRICE_CENTI_MINTS_ISSUE_SUBID, 6_900);
    }

    #[test]
    fn at_reward_one_memo() {
        // supply <= 10M tokens: reward 1 MEMO
        let supply = 8_925_283 * UNIT;
        assert_eq!(memo_mint::mint_amount_at_supply(supply), UNIT);
        assert_eq!(burn_price(price_centi_mints(1), supply).unwrap(), 420_690 * UNIT);
        assert_eq!(burn_price(price_centi_mints(2), supply).unwrap(), 42_069 * UNIT);
        assert_eq!(burn_price(price_centi_mints(3), supply).unwrap(), 4_206_900_000); // 4,206.9
        assert_eq!(burn_price(price_centi_mints(4), supply).unwrap(), 420_690_000); // 420.69
        assert_eq!(burn_price(price_centi_mints(5), supply).unwrap(), 69 * UNIT);
        assert_eq!(burn_price(PRICE_CENTI_MINTS_ISSUE_SUBID, supply).unwrap(), 69 * UNIT);
    }

    #[test]
    fn follows_the_curve() {
        for supply in [0, 10_000_000 * UNIT, 10_000_000 * UNIT + 1, 100_000_000 * UNIT + 1, 5_000_000_000_000 * UNIT] {
            let reward = memo_mint::mint_amount_at_supply(supply);
            for centi in [42_069_000, 4_206_900, 420_690, 42_069, 6_900] {
                let price = burn_price(centi, supply).unwrap();
                let exact = reward as u128 * centi as u128;
                assert_eq!(price as u128, exact.div_ceil(100), "supply {} centi {}", supply, centi);
                assert!(price as u128 * 100 >= exact && (price as u128 - 1) * 100 < exact);
            }
        }
    }

    #[test]
    fn rounds_up_at_the_tail() {
        // max supply: reward 1 unit; 420.69 mints = 420.69 units -> 421
        let supply = memo_mint::MAX_SUPPLY_LAMPORTS;
        assert_eq!(memo_mint::mint_amount_at_supply(supply), 1);
        assert_eq!(burn_price(42_069, supply).unwrap(), 421);
        assert_eq!(burn_price(420_690, supply).unwrap(), 4_207);
        assert_eq!(burn_price(6_900, supply).unwrap(), 69);
        assert_eq!(burn_price(42_069_000, supply).unwrap(), 420_690);
    }

    #[test]
    fn prices_fit_one_burn() {
        // the highest price at the highest reward stays below memo-burn's per-instruction cap
        let max = burn_price(PRICE_CENTI_MINTS_1, 0).unwrap();
        assert!(max <= memo_burn::MAX_BURN_PER_INSTRUCTION);
        assert!(max >= memo_burn::MIN_BURN_AMOUNT);
    }
}

// ============================================================================
// BurnMemo
// ============================================================================

mod burn_memo {
    use super::*;

    #[test]
    fn valid() {
        let payer = key(7);
        let p = payload(OPERATION_REGISTER, &payer, "a.memo");
        let m = memo(69_000_000, &p);
        assert!(m.len() >= MEMO_MIN_LENGTH, "the shortest register memo must pass memo-burn's minimum");
        let parsed = parse_burn_memo(&m, 69_000_000).unwrap();
        assert_eq!(parsed, p);
        assert!(parsed.validate(OPERATION_REGISTER, &payer, "a.memo").is_ok());
    }

    #[test]
    fn amount_and_version() {
        let p = payload(OPERATION_REGISTER, &key(7), "a.memo");
        assert!(parse_burn_memo(&memo(69_000_000, &p), 69_000_001).is_err());
        let bm = BurnMemo { version: 2, burn_amount: 1, payload: p.try_to_vec().unwrap() };
        let m = general_purpose::STANDARD.encode(bm.try_to_vec().unwrap()).into_bytes();
        assert!(parse_burn_memo(&m, 1).is_err());
    }

    #[test]
    fn malformed() {
        assert!(parse_burn_memo(&[b'A'; 68], 0).is_err()); // too short
        assert!(parse_burn_memo(&[b'!'; 100], 0).is_err()); // not Base64
        // a valid BurnMemo whose payload is not a MemoIdPayload
        let bm = BurnMemo { version: 1, burn_amount: 5, payload: vec![1; 80] };
        let m = general_purpose::STANDARD.encode(bm.try_to_vec().unwrap()).into_bytes();
        assert!(parse_burn_memo(&m, 5).is_err());
        // trailing bytes after the payload
        let p = payload(OPERATION_REGISTER, &key(7), "a.memo");
        let mut raw = BurnMemo { version: 1, burn_amount: 5, payload: p.try_to_vec().unwrap() }.try_to_vec().unwrap();
        raw.push(0);
        assert!(parse_burn_memo(general_purpose::STANDARD.encode(raw).as_bytes(), 5).is_err());
    }

    #[test]
    fn payload_fields() {
        let payer = key(7);
        let good = payload(OPERATION_ISSUE_SUBID, &payer, "blog.a.memo");
        assert!(good.validate(OPERATION_ISSUE_SUBID, &payer, "blog.a.memo").is_ok());
        assert!(good.validate(OPERATION_REGISTER, &payer, "blog.a.memo").is_err());
        assert!(good.validate(OPERATION_ISSUE_SUBID, &key(8), "blog.a.memo").is_err());
        assert!(good.validate(OPERATION_ISSUE_SUBID, &payer, "shop.a.memo").is_err());
        let mut p = good.clone();
        p.version = 2;
        assert!(p.validate(OPERATION_ISSUE_SUBID, &payer, "blog.a.memo").is_err());
        let mut p = good.clone();
        p.category = "profile".into();
        assert!(p.validate(OPERATION_ISSUE_SUBID, &payer, "blog.a.memo").is_err());
    }
}

// ============================================================================
// URIs
// ============================================================================

#[test]
fn uris() {
    for ok in ["https://example.com/a.json", "ar://abc", "ipfs://bafy"] {
        assert!(require_uri(ok).is_ok(), "{}", ok);
    }
    let long = format!("https://{}", "a".repeat(249));
    assert_eq!(long.len(), 257);
    for bad in ["", "https://", "ar://", "http://example.com", "data:x", "HTTPS://a", long.as_str()] {
        assert!(require_uri(bad).is_err(), "{}", bad);
    }
    assert!(require_uri(&long[..256]).is_ok());
}

// ============================================================================
// Accounts
// ============================================================================

mod accounts {
    use super::*;

    #[test]
    fn registry_is_sized_for_its_id() {
        // with a target, the serialized Registry fills exactly space_for(len)
        for id in ["a.memo", "blog.xen-artist.memo", &format!("{}.memo", "a".repeat(63))] {
            let mut r = registry(MemoIdKind::Record, key(1), key(2), id);
            r.target = Some(Target::Asset(key(3)));
            r.target_set_by = key(4);
            let mut data = vec![];
            r.try_serialize(&mut data).unwrap();
            assert_eq!(data.len(), Registry::space_for(id.len()), "{}", id);
        }
        assert_eq!(Registry::BASE_SPACE, 160);
        assert_eq!(Registry::space_for(6), 166); // a.memo
        assert_eq!(Registry::space_for(MAX_MEMO_ID_LEN), 413);
        // without a target it is 33 bytes shorter, so set_target never needs more space
        let r = registry(MemoIdKind::Nft, key(1), Pubkey::default(), "a.memo");
        let mut data = vec![];
        r.try_serialize(&mut data).unwrap();
        assert_eq!(data.len() + 33, Registry::space_for(6));
    }

    #[test]
    fn registry_offsets() {
        let mut r = registry(MemoIdKind::Record, key(1), key(2), "blog.a.memo");
        r.created_at = 0x0102030405060708;
        r.nft_subids_issued = 3;
        r.record_subids = 4;
        let mut data = vec![];
        r.try_serialize(&mut data).unwrap();
        assert_eq!(data[8], 1); // kind: Record
        assert_eq!(&data[9..41], key(1).as_ref()); // asset
        assert_eq!(&data[41..73], key(2).as_ref()); // parent
        assert_eq!(&data[81..85], &3u32.to_le_bytes()); // nft_subids_issued
        assert_eq!(&data[85..89], &4u32.to_le_bytes()); // record_subids
        let back = Registry::try_deserialize(&mut &data[..]).unwrap();
        assert_eq!(back.memo_id, "blog.a.memo");
    }

    #[test]
    fn other_sizes() {
        let mut data = vec![];
        SubidIndex { registry: key(1), bump: 1 }.try_serialize(&mut data).unwrap();
        assert_eq!(data.len(), SubidIndex::SPACE);
        let mut data = vec![];
        PrimaryId { target: Target::Address(key(1)), memo_id_registry: key(2), set_by: key(3), bump: 1 }
            .try_serialize(&mut data)
            .unwrap();
        assert_eq!(data.len(), PrimaryId::SPACE);
    }

    #[test]
    fn pdas() {
        assert_eq!(collection_seed(), hash(b"memo").to_bytes());
        let authority = Pubkey::find_program_address(&[b"authority"], &crate::ID).0;
        assert_eq!(authority_address(), authority);
        let (collection, _) = Pubkey::find_program_address(
            &[b"collection", authority.as_ref(), hash(b"memo").as_ref()],
            &memo_home::ID,
        );
        assert_eq!(memo_id_collection(), collection);
        let (asset, _) = Pubkey::find_program_address(
            &[b"asset", collection.as_ref(), hash(b"a.memo").as_ref()],
            &memo_home::ID,
        );
        assert_eq!(asset_address("a.memo"), asset);
        assert_eq!(
            registry_address("a.memo"),
            Pubkey::find_program_address(&[b"memo_id", hash(b"a.memo").as_ref()], &crate::ID).0
        );
        let parent = registry_address("a.memo");
        assert_eq!(
            subid_index_address(&parent, 2),
            Pubkey::find_program_address(&[b"subid", parent.as_ref(), &[2, 0, 0, 0]], &crate::ID).0
        );
        assert_ne!(subid_index_address(&parent, 0), subid_index_address(&parent, 1));
        // a wallet and an Asset with the same key share one Primary ID slot
        assert_eq!(primary_id_address(&Target::Address(key(9))), primary_id_address(&Target::Asset(key(9))));
    }
}

// ============================================================================
// Control and verification
// ============================================================================

mod control {
    use super::*;

    #[test]
    fn controller_is_the_owner() {
        let data = core_asset(key(1), key(50), "a.memo");
        with_account(key(10), CORE, data.clone(), |a| {
            assert_eq!(controller_of(a, None).unwrap(), key(1));
        });
        // not a Core account
        with_account(key(10), SYSTEM, data, |a| assert!(controller_of(a, None).is_err()));
    }

    #[test]
    fn one_level_vault_rule() {
        let holder = key(20);
        let vault = memo_home::vault_of(&holder).0;
        let mut lamports = (1u64, 1u64);
        let (mut id_data, mut holder_data) = (core_asset(vault, key(50), "a.memo"), core_asset(key(2), key(51), "room"));
        let id_key = key(10);
        let id = AccountInfo::new(&id_key, false, false, &mut lamports.0, &mut id_data, &CORE, false, 0);
        let h = AccountInfo::new(&holder, false, false, &mut lamports.1, &mut holder_data, &CORE, false, 0);
        // the Vault owns the ID; the holder's owner controls it
        assert_eq!(controller_of(&id, None).unwrap(), vault);
        assert_eq!(controller_of(&id, Some(&h)).unwrap(), key(2));

        // a holder whose Vault does not own the ID is rejected
        let other = key(21);
        let mut l = 1u64;
        let mut d = core_asset(key(2), key(51), "room");
        let wrong = AccountInfo::new(&other, false, false, &mut l, &mut d, &CORE, false, 0);
        assert!(controller_of(&id, Some(&wrong)).is_err());
    }

    #[test]
    fn nft_and_record_controllers() {
        let asset = key(10);
        let parent = registry(MemoIdKind::Nft, asset, Pubkey::default(), "a.memo");
        with_account(asset, CORE, core_asset(key(1), key(50), "a.memo"), |a| {
            assert_eq!(require_nft_controller(&parent, a, None, &key(1)).unwrap(), key(1));
            assert!(require_nft_controller(&parent, a, None, &key(2)).is_err()); // not the owner
        });
        // another Asset than the registry's
        with_account(key(11), CORE, core_asset(key(1), key(50), "b.memo"), |a| {
            assert!(require_nft_controller(&parent, a, None, &key(1)).is_err());
        });
        // a Record has no Asset of its own
        let record = registry(MemoIdKind::Record, Pubkey::default(), key(30), "blog.a.memo");
        with_account(Pubkey::default(), CORE, core_asset(key(1), key(50), "x"), |a| {
            assert!(require_nft_controller(&record, a, None, &key(1)).is_err());
        });
    }

    #[test]
    fn forward_lapses_with_the_controller() {
        let mut r = registry(MemoIdKind::Nft, key(10), Pubkey::default(), "a.memo");
        assert_eq!(r.forward(&key(1)), None);
        r.target = Some(Target::Address(key(5)));
        r.target_set_by = key(1);
        assert_eq!(r.forward(&key(1)), Some(Target::Address(key(5))));
        assert_eq!(r.forward(&key(2)), None); // sold
    }

    #[test]
    fn verification() {
        let registry_key = key(40);
        let mut r = registry(MemoIdKind::Nft, key(10), Pubkey::default(), "a.memo");
        r.target = Some(Target::Asset(key(60)));
        r.target_set_by = key(1);
        let p = PrimaryId { target: Target::Asset(key(60)), memo_id_registry: registry_key, set_by: key(2), bump: 1 };
        assert!(is_verified(&registry_key, &r, &key(1), &p, &key(2)));
        assert!(!is_verified(&registry_key, &r, &key(3), &p, &key(2))); // the ID changed hands
        assert!(!is_verified(&registry_key, &r, &key(1), &p, &key(3))); // the Target changed hands
        assert!(!is_verified(&key(41), &r, &key(1), &p, &key(2))); // the Primary names another ID
        let wrong_kind = PrimaryId { target: Target::Address(key(60)), ..p.clone() };
        assert!(!is_verified(&registry_key, &r, &key(1), &wrong_kind, &key(2)));
        r.target = None;
        assert!(!is_verified(&registry_key, &r, &key(1), &p, &key(2)));
    }

    #[test]
    fn target_controllers() {
        // an Address is its own controller
        assert!(require_target_controller(&Target::Address(key(1)), None, None, &key(1)).is_ok());
        assert!(require_target_controller(&Target::Address(key(1)), None, None, &key(2)).is_err());
        // an Asset needs its account, and its controller signs
        assert!(require_target_controller(&Target::Asset(key(10)), None, None, &key(1)).is_err());
        with_account(key(10), CORE, core_asset(key(1), key(50), "room"), |a| {
            assert!(require_target_controller(&Target::Asset(key(10)), Some(a), None, &key(1)).is_ok());
            assert!(require_target_controller(&Target::Asset(key(10)), Some(a), None, &key(2)).is_err());
            assert!(require_target_controller(&Target::Asset(key(11)), Some(a), None, &key(1)).is_err());
        });
        // a wallet's key used as an Asset target is not a Core account
        with_account(key(1), SYSTEM, vec![], |a| {
            assert!(require_target_controller(&Target::Asset(key(1)), Some(a), None, &key(1)).is_err());
        });
    }
}
