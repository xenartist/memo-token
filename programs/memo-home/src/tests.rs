//! Unit tests for memo-home
//!
//! Covers the pure parts of the program:
//! - share_of: rounding down, the cap, the 1-unit tail of the emission curve, overflow
//! - RoyaltyArgs::validate, name/URI limits, the max_assets check
//! - Core account parsing: parse_asset, parse_collection, asset_data_is_listed
//! - The exported account helpers: owner_of, collection_of, is_listed, is_home_asset,
//!   require_owner_not_listed, vault_nonce_of
//! - Core instruction data: encode_create_v1, encode_create_collection_v2
//! - Account sizes and PDAs
//! The CPI paths (Core, memo-mint, memo-burn, Token-2022) are covered by the local-validator
//! regression suite (clients/regression/src/regression-memo-home.rs).
//!
//! Core values in the fixtures are written as literals from mpl-core (not the program's own
//! constants), so a wrong constant in the program fails here.

use super::*;

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

// ============================================================================
// Core account builders (Borsh layouts of mpl-core AssetV1 / CollectionV1)
// ============================================================================

// mpl-core Key
const K_ASSET: u8 = 1;
const K_HEADER: u8 = 3;
const K_REGISTRY: u8 = 4;
const K_COLLECTION: u8 = 5;
// mpl-core PluginType
const P_ROYALTIES: u8 = 0;
const P_FREEZE: u8 = 1;
const P_BURN: u8 = 2;
const P_TRANSFER: u8 = 3;
const P_UPDATE_DELEGATE: u8 = 4;
const P_PERM_FREEZE: u8 = 5;
const P_ATTRIBUTES: u8 = 6;
const P_PERM_TRANSFER: u8 = 7;
const P_PERM_BURN: u8 = 8;
const P_EDITION: u8 = 9;
const P_MASTER_EDITION: u8 = 10;
const P_ADD_BLOCKER: u8 = 11;
const P_IMMUTABLE_METADATA: u8 = 12;
const P_VERIFIED_CREATORS: u8 = 13;
const P_AUTOGRAPH: u8 = 14;
const P_BUBBLEGUM_V2: u8 = 15;
const P_FREEZE_EXECUTE: u8 = 16;
const P_PERM_FREEZE_EXECUTE: u8 = 17;
const P_GROUPS: u8 = 18;
// mpl-core Authority
const NONE_AUTH: u8 = 0;
const OWNER: u8 = 1;
const UPDATE_AUTH: u8 = 2;
const ADDR: u8 = 3;

#[derive(Clone, Copy)]
enum Ua {
    None,
    Address(Pubkey),
    Collection(Pubkey),
}

/// (plugin type, authority tag, authority address, plugin data after the Plugin tag)
type PluginSpec = (u8, u8, Option<Pubkey>, Vec<u8>);

fn asset_bytes(owner: Pubkey, ua: Ua, name: &str, uri: &str, seq: Option<u64>, plugins: &[PluginSpec]) -> Vec<u8> {
    let mut d = vec![K_ASSET];
    d.extend_from_slice(owner.as_ref());
    match ua {
        Ua::None => d.push(0),
        Ua::Address(k) => {
            d.push(1);
            d.extend_from_slice(k.as_ref());
        }
        Ua::Collection(k) => {
            d.push(2);
            d.extend_from_slice(k.as_ref());
        }
    }
    borsh_str(&mut d, name);
    borsh_str(&mut d, uri);
    match seq {
        None => d.push(0),
        Some(s) => {
            d.push(1);
            d.extend_from_slice(&s.to_le_bytes());
        }
    }
    if plugins.is_empty() {
        return d;
    }
    // PluginHeaderV1 { key, plugin_registry_offset }, then the plugins, then PluginRegistryV1
    let header = d.len();
    d.push(K_HEADER);
    d.extend_from_slice(&0u64.to_le_bytes());
    let mut offsets = vec![];
    for (ptype, _, _, data) in plugins {
        offsets.push(d.len() as u64);
        d.push(*ptype);
        d.extend_from_slice(data);
    }
    let registry = d.len() as u64;
    d[header + 1..header + 9].copy_from_slice(&registry.to_le_bytes());
    d.push(K_REGISTRY);
    d.extend_from_slice(&(plugins.len() as u32).to_le_bytes());
    for ((ptype, auth, addr, _), off) in plugins.iter().zip(offsets) {
        d.push(*ptype);
        d.push(*auth);
        if *auth == ADDR {
            d.extend_from_slice(addr.unwrap().as_ref());
        }
        d.extend_from_slice(&off.to_le_bytes());
    }
    d.extend_from_slice(&0u32.to_le_bytes()); // external_registry: empty
    d
}

fn simple_asset(plugins: &[PluginSpec]) -> Vec<u8> {
    asset_bytes(key(1), Ua::Collection(key(9)), "HOME #1", "https://example.com/1.json", None, plugins)
}

fn collection_bytes(ua: Pubkey, name: &str, uri: &str, num_minted: u32, current_size: u32) -> Vec<u8> {
    let mut d = vec![K_COLLECTION];
    d.extend_from_slice(ua.as_ref());
    borsh_str(&mut d, name);
    borsh_str(&mut d, uri);
    d.extend_from_slice(&num_minted.to_le_bytes());
    d.extend_from_slice(&current_size.to_le_bytes());
    d
}

fn freeze(auth: u8, frozen: bool) -> PluginSpec {
    (P_FREEZE, auth, (auth == ADDR).then(|| key(7)), vec![frozen as u8])
}
fn delegate(ptype: u8, auth: u8) -> PluginSpec {
    (ptype, auth, (auth == ADDR).then(|| key(7)), vec![])
}

/// Runs `f` with an AccountInfo over `data`, owned by `owner`
fn with_account<R>(address: Pubkey, owner: Pubkey, data: Vec<u8>, f: impl FnOnce(&AccountInfo) -> R) -> R {
    let mut lamports = 1_000_000u64;
    let mut data = data;
    let info = AccountInfo::new(&address, false, false, &mut lamports, &mut data, &owner, false, 0);
    f(&info)
}

fn config_bytes(collection: Pubkey) -> Vec<u8> {
    let c = CollectionConfig { collection, authority: key(2), seed: [3; 32], max_share_bps: 3000, max_assets: None,
                               creator: key(4), created_at: 0, bump: 255 };
    let mut data = vec![];
    c.try_serialize(&mut data).unwrap();
    data
}

// ============================================================================
// share_of
// ============================================================================

mod share_tests {
    use super::*;

    #[test]
    fn rounds_down() {
        assert_eq!(share_of(1_000_000, 3000).unwrap(), 300_000);
        assert_eq!(share_of(999, 3000).unwrap(), 299); // 299.7
        assert_eq!(share_of(10_001, 1).unwrap(), 1); // 1.0001
        assert_eq!(share_of(9_999, 1).unwrap(), 0); // 0.9999
    }

    #[test]
    fn one_unit_tail_never_exceeds_cap() {
        // At the end of the emission curve one mint is 1 unit: any share below 100% is 0
        for bps in [1u16, 3000, 5000, 9999] {
            assert_eq!(share_of(1, bps).unwrap(), 0, "bps {}", bps);
        }
        assert_eq!(share_of(1, MAX_BPS).unwrap(), 1);
    }

    #[test]
    fn bounds() {
        assert_eq!(share_of(0, 5000).unwrap(), 0);
        assert_eq!(share_of(123_456, 0).unwrap(), 0);
        assert_eq!(share_of(123_456, MAX_BPS).unwrap(), 123_456);
        assert_eq!(share_of(u64::MAX, MAX_BPS).unwrap(), u64::MAX);
        assert_eq!(share_of(u64::MAX, 5000).unwrap(), u64::MAX / 2);
    }

    #[test]
    fn share_never_above_reward_times_bps() {
        for reward in [1u64, 2, 3, 7, 99, 1_000_001, 123_456_789] {
            for bps in [0u16, 1, 333, 3000, 6667, 9999, 10_000] {
                let s = share_of(reward, bps).unwrap() as u128;
                assert!(s * 10_000 <= reward as u128 * bps as u128);
                assert!((s + 1) * 10_000 > reward as u128 * bps as u128);
            }
        }
    }

    #[test]
    fn rejects_bps_above_max() {
        assert!(share_of(100, MAX_BPS + 1).is_err());
    }
}

// ============================================================================
// Argument checks
// ============================================================================

mod argument_tests {
    use super::*;

    fn r(bps: u16, pcts: &[u8]) -> RoyaltyArgs {
        RoyaltyArgs {
            basis_points: bps,
            creators: pcts.iter().enumerate().map(|(i, p)| RoyaltyCreator { address: key(i as u8 + 1), percentage: *p }).collect(),
        }
    }

    #[test]
    fn royalty_valid() {
        assert!(r(500, &[100]).validate().is_ok());
        assert!(r(0, &[50, 50]).validate().is_ok());
        assert!(r(MAX_BPS, &[20, 20, 20, 20, 20]).validate().is_ok());
    }

    #[test]
    fn royalty_invalid() {
        assert!(r(MAX_BPS + 1, &[100]).validate().is_err());
        assert!(r(500, &[]).validate().is_err());
        assert!(r(500, &[99]).validate().is_err());
        assert!(r(500, &[60, 50]).validate().is_err());
        assert!(r(500, &[20, 20, 20, 20, 10, 10]).validate().is_err()); // 6 creators
        assert!(r(500, &[255, 255, 255, 255, 140]).validate().is_err()); // sums without u8 overflow
    }

    #[test]
    fn royalty_rejects_duplicate_creators() {
        let mut a = r(500, &[50, 50]);
        a.creators[1].address = a.creators[0].address;
        assert!(a.validate().is_err());
        let mut a = r(500, &[20, 20, 20, 20, 20]);
        a.creators[4].address = a.creators[2].address;
        assert!(a.validate().is_err());
    }

    #[test]
    fn name_and_uri_limits() {
        let name = "n".repeat(MAX_NAME_LEN);
        let uri = "u".repeat(MAX_URI_LEN);
        assert!(require_name_uri(Some(&name), Some(&uri)).is_ok());
        assert!(require_name_uri(None, None).is_ok());
        assert!(require_name_uri(Some(""), Some("")).is_ok());
        assert!(require_name_uri(Some(&format!("{}x", name)), None).is_err());
        assert!(require_name_uri(None, Some(&format!("{}x", uri))).is_err());
        assert_eq!(MAX_NAME_LEN, 64);
        assert_eq!(MAX_URI_LEN, 256);
        // the limit is in bytes: 22 three-byte characters are 66 bytes
        assert!(require_name_uri(Some(&"家".repeat(22)), None).is_err());
        assert!(require_name_uri(Some(&"家".repeat(21)), None).is_ok());
    }

    #[test]
    fn max_assets_uses_num_minted() {
        let c = |num_minted, current_size| CoreCollection { update_authority: key(1), num_minted, current_size };
        assert!(check_max_assets(&c(1, 1), Some(2)).is_ok());
        assert!(check_max_assets(&c(2, 2), Some(2)).is_err());
        // an Asset that left the Collection lowers current_size but not num_minted
        assert!(check_max_assets(&c(2, 1), Some(2)).is_err());
        assert!(check_max_assets(&c(u32::MAX, 0), None).is_ok());
        assert!(check_max_assets(&c(0, 0), Some(1)).is_ok());
    }
}

// ============================================================================
// parse_asset / parse_collection
// ============================================================================

mod parse_tests {
    use super::*;

    #[test]
    fn asset_update_authority_variants() {
        let a = parse_asset(&asset_bytes(key(1), Ua::Collection(key(9)), "n", "u", None, &[])).unwrap();
        assert_eq!(a, CoreAsset { owner: key(1), collection: Some(key(9)), plugin_header: None });

        let a = parse_asset(&asset_bytes(key(2), Ua::Address(key(8)), "n", "u", None, &[])).unwrap();
        assert_eq!(a.owner, key(2));
        assert_eq!(a.collection, None);

        let a = parse_asset(&asset_bytes(key(3), Ua::None, "n", "u", Some(7), &[])).unwrap();
        assert_eq!(a.owner, key(3));
        assert_eq!(a.collection, None);
    }

    #[test]
    fn asset_with_seq_and_plugins() {
        let d = asset_bytes(key(1), Ua::Collection(key(9)), "a longer name", "", Some(42), &[freeze(OWNER, false)]);
        let a = parse_asset(&d).unwrap();
        assert_eq!(a.collection, Some(key(9)));
        assert_eq!(d[a.plugin_header.unwrap()], K_HEADER);
    }

    #[test]
    fn asset_rejects_wrong_key_and_truncation() {
        let mut d = simple_asset(&[]);
        d[0] = K_COLLECTION;
        assert!(parse_asset(&d).is_err());

        let d = simple_asset(&[]);
        for cut in [0, 1, 33, 34, 65, 66, 70, d.len() - 1] {
            assert!(parse_asset(&d[..cut]).is_err(), "cut at {}", cut);
        }
    }

    #[test]
    fn asset_rejects_truncated_seq() {
        let d = asset_bytes(key(1), Ua::Collection(key(9)), "n", "u", Some(5), &[]);
        assert!(parse_asset(&d).is_ok());
        for cut in 1..=8 {
            assert!(parse_asset(&d[..d.len() - cut]).is_err(), "seq cut by {}", cut);
        }
    }

    #[test]
    fn asset_rejects_bad_tags() {
        let mut d = simple_asset(&[]);
        d[33] = 3; // update authority tag out of range
        assert!(parse_asset(&d).is_err());

        let mut d = simple_asset(&[]);
        let seq_at = d.len() - 1;
        d[seq_at] = 2; // seq tag out of range
        assert!(parse_asset(&d).is_err());

        let mut d = simple_asset(&[]);
        d.push(K_REGISTRY); // trailing byte that is not a plugin header
        assert!(parse_asset(&d).is_err());
    }

    #[test]
    fn asset_rejects_huge_string_length() {
        let mut d = simple_asset(&[]);
        d[66..70].copy_from_slice(&u32::MAX.to_le_bytes()); // name length
        assert!(parse_asset(&d).is_err());
    }

    #[test]
    fn collection() {
        let c = parse_collection(&collection_bytes(key(5), "Rooms", "https://x/c.json", 37, 37)).unwrap();
        assert_eq!(c, CoreCollection { update_authority: key(5), num_minted: 37, current_size: 37 });
        // num_minted and current_size are read from their own fields
        let c = parse_collection(&collection_bytes(key(5), "Rooms", "u", 37, 36)).unwrap();
        assert_eq!((c.num_minted, c.current_size), (37, 36));

        let d = collection_bytes(key(5), "Rooms", "u", 1, 1);
        assert!(parse_collection(&d[..d.len() - 1]).is_err());
        let mut d = d;
        d[0] = K_ASSET;
        assert!(parse_collection(&d).is_err());
    }
}

// ============================================================================
// asset_data_is_listed
// ============================================================================

mod listed_tests {
    use super::*;

    fn listed(plugins: &[PluginSpec]) -> bool {
        asset_data_is_listed(&simple_asset(plugins)).unwrap()
    }

    #[test]
    fn no_plugins() {
        assert!(!listed(&[]));
    }

    #[test]
    fn freeze_delegate() {
        assert!(!listed(&[freeze(OWNER, false)]));
        assert!(listed(&[freeze(OWNER, true)]));
        assert!(listed(&[freeze(ADDR, false)]));
        assert!(listed(&[freeze(ADDR, true)]));
        assert!(listed(&[freeze(UPDATE_AUTH, false)]));
        assert!(listed(&[freeze(NONE_AUTH, false)]));
    }

    #[test]
    fn transfer_and_burn_delegates() {
        for ptype in [P_TRANSFER, P_BURN] {
            assert!(!listed(&[delegate(ptype, OWNER)]));
            assert!(listed(&[delegate(ptype, ADDR)]));
            assert!(listed(&[delegate(ptype, UPDATE_AUTH)]));
            assert!(listed(&[delegate(ptype, NONE_AUTH)]));
        }
    }

    #[test]
    fn permanent_delegates_under_every_authority() {
        for ptype in [P_PERM_FREEZE, P_PERM_TRANSFER, P_PERM_BURN] {
            for auth in [NONE_AUTH, OWNER, UPDATE_AUTH, ADDR] {
                assert!(listed(&[delegate(ptype, auth)]), "type {} auth {}", ptype, auth);
            }
        }
    }

    #[test]
    fn other_plugins_do_not_count() {
        // Royalties { basis_points, creators: [], rule_set: None } and Attributes { [] }
        let royalties = (P_ROYALTIES, UPDATE_AUTH, None, vec![0xf4, 0x01, 0, 0, 0, 0, 0]);
        let attributes = (P_ATTRIBUTES, UPDATE_AUTH, None, vec![0, 0, 0, 0]);
        assert!(!listed(&[royalties.clone(), attributes.clone()]));
        // a delegate after other plugins is still found
        assert!(listed(&[royalties, attributes, delegate(P_TRANSFER, ADDR)]));
        // an Address authority before a frozen Freeze Delegate keeps the records aligned
        assert!(listed(&[(P_ATTRIBUTES, ADDR, Some(key(6)), vec![0, 0, 0, 0]), freeze(OWNER, true)]));
        assert!(!listed(&[(P_ATTRIBUTES, ADDR, Some(key(6)), vec![0, 0, 0, 0]), freeze(OWNER, false)]));
        // known types without listing effect, also when delegated (FreezeExecute guards only
        // Core's Execute, which memo-home never uses; Groups needs the update authority)
        for ptype in [P_UPDATE_DELEGATE, P_EDITION, P_MASTER_EDITION, P_ADD_BLOCKER, P_IMMUTABLE_METADATA,
                      P_VERIFIED_CREATORS, P_AUTOGRAPH, P_BUBBLEGUM_V2, P_FREEZE_EXECUTE, P_PERM_FREEZE_EXECUTE, P_GROUPS] {
            assert!(!listed(&[delegate(ptype, ADDR)]), "type {}", ptype);
        }
    }

    #[test]
    fn unknown_values_fail_closed() {
        // a plugin type newer than this program counts as listed
        assert!(listed(&[delegate(P_GROUPS + 1, OWNER)]));
        assert!(listed(&[delegate(255, OWNER)]));
        // an unknown authority tag counts as listed (its length is unknown)
        let mut d = simple_asset(&[delegate(P_ATTRIBUTES, OWNER)]);
        let header = parse_asset(&d).unwrap().plugin_header.unwrap();
        let reg = u64::from_le_bytes(d[header + 1..header + 9].try_into().unwrap()) as usize;
        d[reg + 5 + 1] = 4; // authority tag of the first record
        assert!(asset_data_is_listed(&d).unwrap());
    }

    #[test]
    fn several_owner_delegates_unlisted() {
        assert!(!listed(&[freeze(OWNER, false), delegate(P_TRANSFER, OWNER), delegate(P_BURN, OWNER)]));
    }

    #[test]
    fn malformed_registry_is_an_error() {
        let mut d = simple_asset(&[freeze(OWNER, false)]);
        let header = parse_asset(&d).unwrap().plugin_header.unwrap();
        // registry offset past the end
        let past_end = d.len() as u64 + 10;
        d[header + 1..header + 9].copy_from_slice(&past_end.to_le_bytes());
        assert!(asset_data_is_listed(&d).is_err());

        let mut d = simple_asset(&[freeze(OWNER, false)]);
        let reg = u64::from_le_bytes(d[header + 1..header + 9].try_into().unwrap()) as usize;
        d[reg] = K_HEADER; // wrong registry key
        assert!(asset_data_is_listed(&d).is_err());

        let mut d = simple_asset(&[freeze(OWNER, false)]);
        d[reg + 1..reg + 5].copy_from_slice(&1000u32.to_le_bytes()); // more records than data
        assert!(asset_data_is_listed(&d).is_err());

        // offsets at the top of the range are errors, not panics
        for bad in [u64::MAX, usize::MAX as u64, u64::MAX - 1] {
            let mut d = simple_asset(&[freeze(OWNER, false)]);
            let at = reg + 5 + 2; // plugin offset of the first record (type, Owner tag)
            d[at..at + 8].copy_from_slice(&bad.to_le_bytes());
            assert!(asset_data_is_listed(&d).is_err());
            let mut d = simple_asset(&[freeze(OWNER, false)]);
            d[header + 1..header + 9].copy_from_slice(&bad.to_le_bytes());
            assert!(asset_data_is_listed(&d).is_err());
        }
    }
}

// ============================================================================
// Exported account helpers
// ============================================================================

mod helper_tests {
    use super::*;

    const SYSTEM: Pubkey = anchor_lang::system_program::ID;

    #[test]
    fn core_owner_is_required() {
        let d = simple_asset(&[delegate(P_TRANSFER, ADDR)]);
        for owner in [SYSTEM, crate::ID, key(42)] {
            with_account(key(1), owner, d.clone(), |a| {
                assert!(owner_of(a).is_err());
                assert!(collection_of(a).is_err());
                assert!(is_listed(a).is_err());
            });
        }
        with_account(key(1), CORE_PROGRAM_ID, d, |a| {
            assert_eq!(owner_of(a).unwrap(), key(1));
            assert_eq!(collection_of(a).unwrap(), Some(key(9)));
            assert!(is_listed(a).unwrap());
        });
    }

    #[test]
    fn home_asset_and_owner_gate() {
        let cfg_key = key(50);
        let mut lamports = 1_000_000u64;
        let mut cfg_data = config_bytes(key(9));
        let program_id = crate::ID;
        let cfg_info = AccountInfo::new(&cfg_key, false, false, &mut lamports, &mut cfg_data, &program_id, false, 0);
        let config: Account<CollectionConfig> = Account::try_from(&cfg_info).unwrap();

        let unlisted = simple_asset(&[]);
        with_account(key(1), CORE_PROGRAM_ID, unlisted.clone(), |a| {
            assert!(is_home_asset(a, &config));
            assert!(require_owner_not_listed(a, &config, &key(1)).is_ok());
            assert!(require_owner_not_listed(a, &config, &key(2)).is_err()); // not the owner
        });
        with_account(key(1), SYSTEM, unlisted, |a| {
            assert!(!is_home_asset(a, &config));
            assert!(require_owner_not_listed(a, &config, &key(1)).is_err());
        });
        // another Collection
        let other = asset_bytes(key(1), Ua::Collection(key(8)), "n", "u", None, &[]);
        with_account(key(1), CORE_PROGRAM_ID, other, |a| {
            assert!(!is_home_asset(a, &config));
            assert!(require_owner_not_listed(a, &config, &key(1)).is_err());
        });
        // listed
        with_account(key(1), CORE_PROGRAM_ID, simple_asset(&[freeze(OWNER, true)]), |a| {
            assert!(is_home_asset(a, &config));
            assert!(require_owner_not_listed(a, &config, &key(1)).is_err());
        });
    }

    #[test]
    fn config_must_be_owned_by_memo_home() {
        let cfg_key = key(50);
        let mut lamports = 1_000_000u64;
        let mut cfg_data = config_bytes(key(9));
        let fake_owner = key(77);
        let cfg_info = AccountInfo::new(&cfg_key, false, false, &mut lamports, &mut cfg_data, &fake_owner, false, 0);
        assert!(Account::<CollectionConfig>::try_from(&cfg_info).is_err());
    }

    #[test]
    fn vault_nonce_checks_the_address() {
        let asset = key(1);
        let (addr, bump) = vault_state_of(&asset);
        let state = |nonce, bump| {
            let mut data = vec![];
            VaultState { nonce, bump }.try_serialize(&mut data).unwrap();
            data
        };
        with_account(addr, crate::ID, state(7, bump), |s| assert_eq!(vault_nonce_of(s, &asset).unwrap(), 7));
        // the Vault State of another Asset
        with_account(addr, crate::ID, state(7, bump), |s| assert!(vault_nonce_of(s, &key(2)).is_err()));
        // right bytes at another address, or not owned by memo-home
        with_account(key(3), crate::ID, state(7, bump), |s| assert!(vault_nonce_of(s, &asset).is_err()));
        with_account(addr, SYSTEM, state(7, bump), |s| assert!(vault_nonce_of(s, &asset).is_err()));
        // another account type at the address
        with_account(addr, crate::ID, config_bytes(key(9)), |s| assert!(vault_nonce_of(s, &asset).is_err()));
    }
}

// ============================================================================
// Core instruction data
// ============================================================================

mod encode_tests {
    use super::*;

    fn s(v: &mut Vec<u8>, text: &str) {
        v.extend_from_slice(&(text.len() as u32).to_le_bytes());
        v.extend_from_slice(text.as_bytes());
    }

    fn oracle_tail(guard: &Pubkey) -> Vec<u8> {
        let mut v = vec![1, 1, 0, 0, 0, 1]; // Some, 1 adapter, ExternalPluginAdapterInitInfo::Oracle
        v.extend_from_slice(guard.as_ref());
        v.extend_from_slice(&[1, 0]); // init_plugin_authority: Some(None)
        v.extend_from_slice(&[1, 0, 0, 0, 2, 4, 0, 0, 0]); // [(HookableLifecycleEvent::Burn, can_reject)]
        v.push(0); // base_address_config: None
        v.extend_from_slice(&[1, 1]); // results_offset: Some(Anchor)
        v
    }

    #[test]
    fn create_v1() {
        let mut want = vec![0, 0];
        s(&mut want, "HOME #1");
        s(&mut want, "https://x/1.json");
        want.push(0);
        assert_eq!(encode_create_v1("HOME #1", "https://x/1.json"), want);
    }

    #[test]
    fn create_collection_without_plugins() {
        let guard = key(4);
        let mut want = vec![21];
        s(&mut want, "Rooms");
        s(&mut want, "https://x/c.json");
        want.push(0); // plugins: None
        want.extend(oracle_tail(&guard));
        assert_eq!(encode_create_collection_v2("Rooms", "https://x/c.json", &guard, None, None), want);
    }

    #[test]
    fn create_collection_with_max_assets_and_royalty() {
        let guard = key(4);
        let royalty = RoyaltyArgs {
            basis_points: 500,
            creators: vec![
                RoyaltyCreator { address: key(10), percentage: 70 },
                RoyaltyCreator { address: key(11), percentage: 30 },
            ],
        };
        let mut want = vec![21];
        s(&mut want, "Ltd");
        s(&mut want, "u");
        want.extend_from_slice(&[1, 2, 0, 0, 0]); // plugins: Some(2 pairs)
        // MasterEdition { max_supply: Some(100), name: None, uri: None }, authority Some(None)
        want.extend_from_slice(&[P_MASTER_EDITION, 1, 100, 0, 0, 0, 0, 0, 1, 0]);
        // Royalties { 500, [(k10, 70), (k11, 30)], RuleSet::None }, authority Some(None)
        want.extend_from_slice(&[P_ROYALTIES, 0xf4, 0x01, 2, 0, 0, 0]);
        want.extend_from_slice(key(10).as_ref());
        want.push(70);
        want.extend_from_slice(key(11).as_ref());
        want.push(30);
        want.extend_from_slice(&[0, 1, 0]);
        want.extend(oracle_tail(&guard));
        assert_eq!(encode_create_collection_v2("Ltd", "u", &guard, Some(100), Some(&royalty)), want);
    }

    #[test]
    fn create_collection_with_only_one_plugin() {
        let guard = key(4);
        let d = encode_create_collection_v2("a", "b", &guard, Some(u32::MAX), None);
        let start = 1 + 4 + 1 + 4 + 1;
        assert_eq!(&d[start..start + 5], &[1, 1, 0, 0, 0]);
        assert_eq!(&d[start + 5..start + 15], &[P_MASTER_EDITION, 1, 0xff, 0xff, 0xff, 0xff, 0, 0, 1, 0]);
        assert_eq!(&d[start + 15..], &oracle_tail(&guard)[..]);
    }
}

// ============================================================================
// Accounts and PDAs
// ============================================================================

mod account_tests {
    use super::*;

    #[test]
    fn burn_guard_layout() {
        let g = BurnGuard { validation: ORACLE_VALIDATION_V1, create: ORACLE_PASS, transfer: ORACLE_PASS,
                            burn: ORACLE_REJECTED, update: ORACLE_PASS, bump: 255 };
        let mut data = vec![];
        g.try_serialize(&mut data).unwrap();
        assert_eq!(data.len(), BurnGuard::SPACE);
        // Core reads OracleValidation::V1 { create, transfer, burn, update } right after the
        // discriminator: V1 = 1, Rejected = 1, Pass = 2
        assert_eq!(&data[8..13], &[1, 2, 2, 1, 2]);
    }

    #[test]
    fn collection_config_size() {
        let c = CollectionConfig { collection: key(1), authority: key(2), seed: [9; 32], max_share_bps: MAX_BPS,
                                   max_assets: Some(u32::MAX), creator: key(3), created_at: i64::MAX, bump: 255 };
        let mut data = vec![];
        c.try_serialize(&mut data).unwrap();
        assert_eq!(data.len(), CollectionConfig::SPACE);
        assert_eq!(CollectionConfig::SPACE, 152);
    }

    #[test]
    fn vault_state_size() {
        let s = VaultState { nonce: u64::MAX, bump: 255 };
        let mut data = vec![];
        s.try_serialize(&mut data).unwrap();
        assert_eq!(data.len(), VaultState::SPACE);
        assert_eq!(VaultState::SPACE, 17);
    }

    #[test]
    fn pdas() {
        let asset = key(1);
        let (vault, bump) = vault_of(&asset);
        assert_eq!(Pubkey::create_program_address(&[b"vault", asset.as_ref(), &[bump]], &crate::ID).unwrap(), vault);
        let (state, bump) = vault_state_of(&asset);
        assert_eq!(Pubkey::create_program_address(&[b"vault_state", asset.as_ref(), &[bump]], &crate::ID).unwrap(), state);
        assert_eq!(
            vault_token_account(&asset, &MEMO_MINT, &anchor_spl::token_2022::ID),
            get_associated_token_address_with_program_id(&vault, &MEMO_MINT, &anchor_spl::token_2022::ID)
        );
        let col = key(2);
        assert_eq!(
            collection_config_of(&col),
            Pubkey::find_program_address(&[b"collection_config", col.as_ref()], &crate::ID).0
        );
        let (c, bump) = collection_address(&key(3), &[4; 32]);
        assert_eq!(Pubkey::create_program_address(&[b"collection", key(3).as_ref(), &[4; 32], &[bump]], &crate::ID).unwrap(), c);
        let (a, bump) = asset_address(&c, &[5; 32]);
        assert_eq!(Pubkey::create_program_address(&[b"asset", c.as_ref(), &[5; 32], &[bump]], &crate::ID).unwrap(), a);
        // PDAs have no private key
        assert!(!c.is_on_curve() && !a.is_on_curve());
        // different Authorities or seeds give different addresses
        assert_ne!(collection_address(&key(3), &[4; 32]).0, collection_address(&key(4), &[4; 32]).0);
        assert_ne!(asset_address(&c, &[5; 32]).0, asset_address(&c, &[6; 32]).0);
    }

    #[test]
    fn constants() {
        assert_eq!(CORE_PROGRAM_ID.to_string(), "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
        assert_eq!(MEMO_MINT, memo_mint::AUTHORIZED_MINT_PUBKEY);
        assert_eq!(MAX_BPS, 10_000);
        // memo-home is built for the same network as memo-mint and memo-burn (also enforced at
        // compile time)
        #[cfg(not(feature = "mainnet"))]
        {
            assert_eq!(memo_mint::ID.to_string(), "A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");
            assert_eq!(memo_burn::ID.to_string(), "FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");
            assert_eq!(MEMO_MINT.to_string(), "HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");
        }
        #[cfg(feature = "mainnet")]
        {
            assert_eq!(memo_mint::ID.to_string(), "8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX");
            assert_eq!(memo_burn::ID.to_string(), "2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh");
            assert_eq!(MEMO_MINT.to_string(), "memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");
        }
    }

    #[test]
    fn signed_core_instructions_are_fixed() {
        // The Program Authority signs only CreateV1, UpdateV1 (name/uri) and UpdateCollectionV1,
        // and the Vault only TransferV1; memo-home never signs a Core instruction that could add
        // or remove plugins (the Burn Guard Oracle stays because memo-home never removes it).
        // A new Core instruction constant in the program fails this test on purpose.
        let src = include_str!("lib.rs");
        let mut found: Vec<&str> = src.lines().filter_map(|l| l.strip_prefix("const CORE_IX_"))
            .map(|l| l.split(':').next().unwrap()).collect();
        found.sort();
        assert_eq!(found, ["CREATE_COLLECTION_V2", "CREATE_V1", "TRANSFER_V1", "UPDATE_COLLECTION_V1", "UPDATE_V1"]);
        // new_update_authority is always None in update_asset / update_collection
        assert!(src.contains("data.push(0); // new_update_authority: None"));
        assert!(src.contains("AccountMeta::new_readonly(core, false), // new_update_authority: none"));
    }

    #[test]
    fn program_constants_match_core() {
        // the program's own Core constants against the mpl-core literals above
        assert_eq!(
            [CORE_KEY_ASSET_V1, CORE_KEY_PLUGIN_HEADER_V1, CORE_KEY_PLUGIN_REGISTRY_V1, CORE_KEY_COLLECTION_V1],
            [K_ASSET, K_HEADER, K_REGISTRY, K_COLLECTION]
        );
        assert_eq!(
            [CORE_PLUGIN_ROYALTIES, CORE_PLUGIN_FREEZE_DELEGATE, CORE_PLUGIN_BURN_DELEGATE, CORE_PLUGIN_TRANSFER_DELEGATE,
             CORE_PLUGIN_PERMANENT_FREEZE_DELEGATE, CORE_PLUGIN_PERMANENT_TRANSFER_DELEGATE,
             CORE_PLUGIN_PERMANENT_BURN_DELEGATE, CORE_PLUGIN_MASTER_EDITION, CORE_PLUGIN_LAST_KNOWN],
            [P_ROYALTIES, P_FREEZE, P_BURN, P_TRANSFER, P_PERM_FREEZE, P_PERM_TRANSFER, P_PERM_BURN, P_MASTER_EDITION, P_GROUPS]
        );
        assert_eq!(
            [CORE_AUTHORITY_NONE, CORE_AUTHORITY_OWNER, CORE_AUTHORITY_UPDATE_AUTHORITY, CORE_AUTHORITY_ADDRESS],
            [NONE_AUTH, OWNER, UPDATE_AUTH, ADDR]
        );
        assert_eq!([CORE_IX_CREATE_V1, CORE_IX_TRANSFER_V1, CORE_IX_UPDATE_V1, CORE_IX_UPDATE_COLLECTION_V1,
                    CORE_IX_CREATE_COLLECTION_V2], [0, 14, 15, 16, 21]);
    }
}
