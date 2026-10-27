# Memo-ID Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-id (`programs/memo-id`)
**Audit Date**: October 27, 2026
**Auditor**: Pre-deployment security review. The reviewer read the whole program, the memo-home, memo-burn and memo-mint code it calls, and the design; ran the unit tests and the local-validator regression suite; and checked the X1 behaviour the program depends on (v0 transactions with a frozen Address Lookup Table) on X1 testnet.
**Version**: 0.1.0 (pre-deployment), 10 instructions. Program ID: placeholder `Gji1DjBF…` on both networks until the real keypairs are set up.
**Language**: Rust (Anchor 0.32.1)
**Network**: X1 (SVM-based)
**Design reference**: `MEMO-ID-DESIGN.md`

### Overall Assessment

**Risk Level**: ✅ **LOW**. Ready for testnet deployment after the testnet checklist; mainnet after the mainnet checklist.

No critical or high-severity issue was found. Name uniqueness, the binding of each burn to its operation, payer and ID, the controller rules and the integrity of the MEMO ID Collection all hold. One medium finding concerns marketplaces rather than the program itself: a listing does not stop the seller from issuing NFT SubIds, which are permanent, before the sale (F01). It has a cheap, exact check, like the Vault State nonce in memo-home.

Changes made during this review:
- Deploy tooling: memo_id is in `setup-keypairs.sh` and `deploy-common.sh`.
  - The configuration check verifies the mint cascade and `COLLECTION_URI`.
  - A deploy runs `admin-init-memo-id`, then `admin-init-memo-id-alt`, which creates the frozen lookup table and records it in `Anchor.toml`.
- One shared list of lookup-table addresses (`clients/src/memo_id.rs`), used by both the deploy client and the regression suite.
- `MEMO-ID-DESIGN.md` §5: the note on listings now says what does not lapse on a sale (F01).

### Summary Statistics

| Severity | Count | Status |
|---|---|---|
| Critical | 0 | — |
| High | 0 | — |
| Medium | 1 | Documented; market-level check (F01) |
| Low | 4 | Documented or accepted (F02–F05) |
| Informational | 7 | Documented, by design, or checklist (F06–F12) |

Tests: 23 unit tests (`programs/memo-id/src/tests.rs`) and 100 local-validator regression cases (`clients/regression/src/regression-memo-id.rs`), all passing on 2026-10-27.

---

## Contract Overview

### Purpose

memo-id gives globally unique names such as `xen-artist.memo`. Each name is owned as a memo-home NFT. Whoever controls the Asset controls the name:
- **forward record**: points the name at a wallet or an Asset;
- **Record SubIds**: free sub-names under it;
- **NFT SubIds**: issued as NFTs of their own, one way.

The Target confirms the link with a **Primary ID** (reverse record). Apps show only names whose two records match.

There is no admin key and no fee. The only payment is MEMO burned through memo-burn, priced in mints at the current supply.

### Instructions

| Instruction | Signer | Effect |
|---|---|---|
| `init` | anyone, once | memo-home `create_collection` of the MEMO ID Collection at a fixed PDA, signed by the memo-id Authority PDA; name `MEMO ID`, `COLLECTION_URI`, no share, no supply limit, no royalties |
| `register(hash, label, uri)` | payer | `<label>.memo`. Burns the price for the label's length (420,690 / 42,069 / 4,206.9 / 420.69 / 69 mints for 1 / 2 / 3 / 4 / 5+ bytes), with the BurnMemo at index 0. Creates the Registry and the Asset (owner = payer, name = the ID) |
| `create_subid(hash, label, target?)` | parent's controller (+ payer) | Record SubId `<label>.<parent>`, optionally with a forward record; rent only |
| `delete_subid` | parent's controller | closes a Record SubId; rent to the controller |
| `issue_subid(index, uri)` + `new_owner` | parent's controller (+ payer) | Turns a Record SubId into an NFT owned by `new_owner`, one way. Burns 69 mints. `index` must equal the parent's `nft_subids_issued`. Creates SubId Index entry `index` |
| `set_target(target)` / `clear_target` | the ID's controller | forward record, stamped `target_set_by = controller` |
| `set_primary(target)` | the Target's controller (+ payer) | creates or replaces the Primary ID, stamped `set_by = signer` |
| `clear_primary` | the Target's current controller | closes the Primary ID; rent to the signer |
| `update_uri(uri)` | the ID's controller (+ payer) | memo-home `update_asset`, URI only; the name always stays the ID |

**Controller** of an NFT ID: the Core owner of its Asset. If that owner is the Vault of a memo-home Asset H (passed as `holder`), the controller is the owner of H. Only one level is followed. A Record SubId's controller is its parent's controller.

### Accounts

| Account | Seeds (program) | Notes |
|---|---|---|
| Authority | `[b"authority"]` (memo-id) | Authority of the MEMO ID Collection in memo-home; signs only `create_collection`, `create_asset` and `update_asset` |
| MEMO ID Collection | `[b"collection", authority, sha256("memo")]` (memo-home) | Core update authority is memo-home's Program Authority; no private key exists |
| Registry | `[b"memo_id", sha256(memo_id)]` | kind, asset, parent, created_at, `nft_subids_issued` (offset 81), `record_subids`, bump, `target_set_by`, target, memo_id; 160 + ID length bytes |
| Asset | `[b"asset", collection, sha256(memo_id)]` (memo-home) | one per NFT ID; memo-home's Burn Guard rejects burns |
| SubId Index | `[b"subid", parent registry, index u32 LE]` | registry, bump (41 bytes); never closed |
| Primary ID | `[b"primary_id", target key]` | target, `memo_id_registry`, `set_by`, bump (106 bytes) |

### Cost and size (local validator, X1 mainnet Core / Token-2022 builds)

| Instruction | CU | Transaction size |
|---|---|---|
| `register` | 108k | 1051 bytes as v0 with the lookup table, for a 68-byte ID and a 256-byte URI |
| `issue_subid` | 112k | 1150 bytes as v0 for a 64-byte SubId with two signers; 1182 bytes with a holder; 253-byte IDs fit too |

The XNT cost is about 0.0064 XNT for `register` (rent, Core fees, base fee) and 0.0055 XNT for `issue_subid`. The same `issue_subid` as a legacy transaction is 1309 bytes, above the 1232-byte limit (F12).

---

## Findings

### 🟠 F01 (Medium, documented): a listing does not stop the seller from issuing NFT SubIds

Some listings leave the seller as the Core owner: delegate listings, frozen listings and offers. In those cases the seller remains the controller until the sale. Most of what the seller changes in that time goes back to the buyer:
- forward records lapse with the controller (`target_set_by`);
- Record SubIds move with the parent and can be deleted by the buyer.

**NFT SubIds are permanent.** Between listing and sale, a seller can issue `www.<id>`, `pay.<id>` and similar names to their own wallet for 69 mints each. The buyer gets a parent whose most valuable sub-names belong to someone else forever. The same applies to an ID held in a memo-home Vault: `issue_subid` is not a Vault outflow, so the holder's Vault State nonce does not change.

The URI can also change before the sale, though it is display-only.

**Mitigation**:
- `nft_subids_issued` only grows. A market or bidder records it at listing (Registry offset 81) and requires it unchanged at the sale. For an Asset sold with IDs in its Vault, the check covers each held ID, alongside the Vault State nonce.
- Escrow listings are not affected, because the escrow becomes the controller.
- `MEMO-ID-DESIGN.md` §5 now says that issued NFT SubIds do not lapse.
- Publish this check in the integration rules for markets (mainnet checklist).

### 🟡 F02 (Low, documented): the price is bound to the supply at execution

`burn_price` uses the mint's supply when the instruction runs, and the BurnMemo's `burn_amount` must equal it exactly. The transaction fails with `BurnAmountMismatch` (6019) if, between building and execution:
- the supply crosses a step of the emission curve; or
- burns move it back across one.

Nothing is burned when this happens. Up to 10M MEMO the reward is constant, so this cannot happen now. Above 10M it happens only around a step boundary. Clients re-read the supply and rebuild.

### 🟡 F03 (Low, documented): first-time burners need UserGlobalBurnStats

memo-burn's `process_burn` requires the payer's `UserGlobalBurnStats` to exist. A wallet that has never burned therefore fails `register` and `issue_subid` until it runs memo-burn `initialize_user_global_burn_stats`. Clients should add that instruction in a prior transaction when the account is missing; this keeps the register transaction within the size limit.

### 🟡 F04 (Low, accepted): registrations can be front-run

There is no commit-reveal step. The payload names the payer, so another party cannot reuse a pending transaction's memo. They can, however, submit their own `register` for the same label. This holds for any first-come name registry on a public chain. Commit-reveal would add a second transaction and a waiting period for every user, so it was not adopted.

### 🟡 F05 (Low, accepted): a Primary ID can outlive its Target Asset

A Primary ID for an `Asset` target can only be cleared by that Asset's controller. If the Asset is later burned, the Primary ID can never be closed. memo-home Assets cannot be burned, but other Core Assets can. About 0.0016 XNT of rent stays locked. The record cannot be verified, so apps already treat it as invalid.

### ℹ️ Informational

- **F06 — Shared Primary ID address.** `Address(X)` and `Asset(X)` targets share the Primary ID address, because the seed is the target key only. Only one party can act on both:
  - a memo-home Asset is a PDA, so no wallet exists at its address;
  - a keypair-addressed Core Asset needs that key's signature to be created.

  The kind is stored and compared in `is_verified`.
- **F07 — Forward records can revive (by design).** A forward record is valid while `target_set_by` is the controller. If control returns to an earlier controller who set a record that nobody replaced, that record becomes valid again. This is the documented rule.
- **F08 — Assets stuck in a non-memo-home Vault address.** An ID Asset sent to the Vault address of a non-memo-home Core Asset H is still controlled by H's owner through `holder`. memo-home cannot move it out, because Vault operations need a memo-home Asset. This is a user error; wallets and apps should transfer IDs only to wallets or to memo-home Assets' Vaults.
- **F09 — Rent of a deleted Record SubId.** The rent goes to the parent's current controller, not to whoever paid it.
- **F10 — `COLLECTION_URI` is fixed at `init`.**
  - Testnet: the URI stays empty for the life of the testnet Collection.
  - Mainnet: a `--features mainnet` build fails to compile while the URI is empty, and the deploy check reports it.
- **F11 — "No admin key" depends on the upgrade authority.** The upgrade authority is the deploy admin, a hot key. The guarantee holds only once memo-id is immutable. It also depends on memo-home, memo-burn and memo-mint, so those are locked first.
- **F12 — `register` and `issue_subid` need v0 transactions.** They need a v0 transaction with the frozen lookup table (12 addresses: memo-id and memo-home PDAs, Core, memo-home, MEMO mint, GlobalBurnStats, Token-2022, memo-burn, the instructions sysvar, System).
  - The table is frozen right after it is filled. Nobody can change, deactivate or close it.
  - Clients use the address recorded in `Anchor.toml` `[lookup_tables.<env>]`.
  - On X1 testnet (2026-10-27), the CLI created, filled and froze a table. A v0 memo-mint transaction loaded five accounts from it (`sudzoBoM…`, version 0, 413 bytes instead of 532).

---

## Security Analysis by Category

### 1. Name uniqueness ✅ SECURE
Every Registry is at `[b"memo_id", sha256(memo_id)]` and is created with `init`. The handler recomputes the full ID and requires the given hash to match (`HashMismatch`), so each ID has exactly one possible Registry address. The Asset address comes from the same hash, and memo-home checks it. `register` creates only second-level IDs (labels have no dot). SubIds are always deeper, so the two paths cannot collide. Labels are DNS hostname labels: `a-z`, `0-9` and `-`; no leading or trailing `-`; no `xn--`, which rules out punycode homographs. Full IDs are at most 253 bytes.

### 2. Payment binding ✅ SECURE
`register` and `issue_subid` require index 0 to be an SPL Memo. It must hold a Base64 Borsh BurnMemo whose `burn_amount` equals the computed price. Its payload must name the operation, the payer and the exact ID. The burn itself goes through memo-burn, which reads the same memo. A memo therefore pays for one operation, one payer and one ID.

### 3. Collection integrity ✅ SECURE
Only the memo-id Authority PDA can sign memo-home `create_asset` and `update_asset` for the MEMO ID Collection. memo-id calls `create_asset` only:
- in `register`, for a freshly initialized Registry;
- in `issue_subid`, for an existing Record SubId of that parent.

`update_uri` never changes the name. Assets cannot be burned (memo-home Burn Guard). Their PDA addresses have no private key (memo-home F01).

### 4. Control ✅ SECURE
Every controller action recomputes the controller from the Asset's Core owner on chain:
- it checks that the Asset is the Registry's own (`WrongAsset`);
- for a Record SubId, it checks the parent link (`WrongParent`);
- it follows the holder rule one level only (`NotInHolderVault`).

Forward and reverse records carry the controller that set them, so they lapse when control changes. The regression suite covers lapsing on transfer and on sale through a holder (V01–V10).

### 5. Arithmetic ✅ SECURE
The price is computed in u128 with a checked multiply and a ceiling divide, then converted to u64 with a checked conversion. `nft_subids_issued` uses `checked_add`. `record_subids` uses checked increments and saturating decrements. Release builds have overflow checks.

### 6. Accounts and CPI ✅ SECURE
- Anchor checks the owner and discriminator of every Registry, SubId Index and Primary ID.
- The MEMO mint is checked against memo-burn's authorized mint.
- The Collection is checked against the derived address.
- The instructions sysvar is checked by address.
- memo-home checks its own accounts: Config, Asset address, Program Authority, Core.
- memo-burn checks the token account, burn stats and global stats.
- Optional accounts (`holder`, `target_asset`, `parent`) are verified before use.

### 7. Network guard ✅ SECURE
A compile-time check requires memo-home, memo-mint, memo-burn and the MEMO mint to be the expected IDs for the build's network. A mixed build cannot compile.

### 8. External dependencies ⚠️ ACCEPTED
memo-id relies on:
- memo-home (Collections, Assets, Burn Guard, Vault addresses);
- memo-burn (the burn and its memo check);
- memo-mint (`mint_amount_at_supply`);
- Metaplex Core and Token-2022.

Their risks are recorded in their own audits (`MEMO-HOME-AUDIT.md` §9).

---

## Pre-Deployment Checklist

### 🔴 Required before testnet deployment
- [ ] Real program keypairs per network (`./scripts/setup-keypairs.sh <env>`, backed up offline).
- [ ] `declare_id!` (both cfgs) and `Anchor.toml` (`[programs.testnet]`, `[programs.mainnet]`) set to the new IDs.
- [x] Deploy tooling:
  - memo_id is in `setup-keypairs.sh` and `deploy-common.sh`, after memo_home.
  - The configuration check accepts `MEMO_MINT = memo_burn::AUTHORIZED_MINT_PUBKEY` with the mainnet cascade and checks `COLLECTION_URI`.
  - A deploy runs the idempotent `admin-init-memo-id` and `admin-init-memo-id-alt`, and warns when memo_id is deployed without memo_home.
  - Verified on the local validator 2026-10-27: error paths, creation, re-runs that only verify, and recording in `Anchor.toml`.
- [x] memo-home accepts 253-byte names on testnet: the on-chain bytecode equals the current build (checked 2026-10-27).
- [ ] Deploy to testnet. Check the IDL authority, the Collection (`admin-init-memo-id`) and the frozen lookup table (`admin-init-memo-id-alt`). Commit `Anchor.toml` with the recorded table.
- [ ] Register and issue a SubId on testnet with the test wallet, as v0 transactions with the lookup table.
- [ ] Wallet test: X1 wallet signs a v0 `register` from memo-app; the Asset shows its 253-byte name.

### 🔴 Required before mainnet
- [ ] Set `COLLECTION_URI` (the shared MEMO ID metadata JSON on Arweave). It cannot be changed after `init` (F10).
- [ ] Verify that memo-home mainnet bytecode equals the build that accepts 253-byte names. It was redeployed in slot 87726048, after that change.
- [ ] Publish the integration rules:
  - markets: record and recheck `nft_subids_issued` (F01);
  - clients: create the payer's burn stats first (F03), retry on `BurnAmountMismatch` (F02), use v0 with the recorded lookup table (F12).
- [ ] Lock order (F11): memo-mint and memo-burn, then memo-home, then memo-id.

---

## Testing

### Unit tests (`programs/memo-id/src/tests.rs`, 23 tests, both networks)
- **Labels and IDs**: label rules, normal-form IDs, the 253-byte limit.
- **Prices**: the price table; prices at a reward of 1 MEMO; prices along the curve, rounding up at its 1-unit tail; every price fits one burn.
- **BurnMemo**: valid memos, amount and version, malformed input, payload fields.
- **Other**: URIs; Registry sizing and offsets; other account sizes; PDAs; controllers (owner, one-level Vault rule, NFT and Record IDs); forward lapsing; verification; Target controllers.

### Local-validator regression (`clients/regression/src/regression-memo-id.rs`, 100 cases)
`./scripts/run-regression-tests.sh id` loads X1 mainnet's Metaplex Core and Token-2022 builds, pinned by hash, together with memo-mint, memo-burn and memo-home. Each rejection asserts the exact error code and that the watched state did not change.

| Area | Cases |
|---|---|
| `init` and the Collection address | I01–I03 |
| `register`: labels, prices by length against an independent curve reference, BurnMemo and payload, URIs, signers | R01–R31 |
| Record SubIds | S01–S10 |
| NFT SubIds, SubId Index and counters | N01–N14 |
| v0 transactions with the frozen lookup table | A01–A03 |
| 253-byte IDs | L01–L05 |
| Forward records | T01–T07 |
| Primary IDs | P01–P10 |
| Holder Vaults, transfers and sales | V01–V10 |
| `update_uri` | U01–U07 |

---

## Audit Conclusion

memo-id meets its design invariants for:
- one Registry and one Asset per ID;
- burns bound to operation, payer and ID;
- control that follows the Asset;
- records that lapse when control changes.

The only medium finding (F01) is a marketplace concern with an exact, cheap check, like memo-home's Vault State nonce. **Approved for testnet deployment** after the testnet checklist; mainnet after the mainnet checklist.
