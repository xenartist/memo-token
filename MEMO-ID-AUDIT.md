# Memo-ID Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-id (`programs/memo-id`)
**Audit Dates**: October 27, 2026 (pre-deployment review); October 28, 2026 (testnet deployment and wallet tests; second, adversarial review and fixes)
**Auditor**: Pre-deployment security review. The reviewer read the whole program, the memo-home, memo-burn and memo-mint code it calls, and the design; ran the unit tests and the local-validator regression suite; and checked the X1 behaviour the program depends on (v0 transactions with a frozen Address Lookup Table) on X1 testnet. The second review was a multi-agent audit: six review dimensions (payment binding, name space, control, accounts and CPI, composition with memo-home and Core, free-form attacker), one skeptic trying to refute and one verifier assessing impact for each finding, and a completeness critic whose findings were verified the same way. 20 agents; 94 further attack ideas were checked and ruled out.
**Version**: 0.1.0, 10 instructions. Program IDs: testnet `3U7nmWArstoRfkuGDsirEwiY6nngqjcooZxJ4Eh87MBt` (redeployed 2026-10-30 with the final `COLLECTION_URI`, slot 199245650, upgradeable; the earlier testnet programs `8FgBnyrx…` of 2026-10-27/28 and `GBuh5nWV…` of 2026-10-29 are retired); mainnet still the placeholder `Gji1DjBF…` until its keypair is set up.
**Language**: Rust (Anchor 0.32.1)
**Network**: X1 (SVM-based)
**Design reference**: `MEMO-ID-DESIGN.md`

### Overall Assessment

**Risk Level**: ✅ **LOW**. Deployed to testnet and tested there with the X1 wallet (see Testnet deployment and wallet tests); mainnet after the mainnet checklist.

**Scope note**: both reviews are internal reviews by the development side, not an independent third-party audit. Testing is example-based (unit and regression cases); no fuzzing or formal verification was done.

No critical or high-severity issue was found. Name uniqueness, the binding of each burn to its operation, payer and ID, the controller rules and the integrity of the MEMO ID Collection all hold. One medium finding concerns marketplaces rather than the program itself: a listing does not stop the seller from issuing NFT SubIds, which are permanent, before the sale (F01). It has a cheap, exact check, like the Vault State nonce in memo-home.

The second review found no critical, high or medium issue either. It found two low-severity program issues, both fixed: a keypair-addressed Core Asset's key holder could replace or close the Primary ID of the Asset's later owner (F13, which corrects F06), and the label `memo` let an ID contain `.memo.`, so a truncated name could read as someone else's (F14). It also corrected the resolution and display rules for apps (F15, F16), a transaction-size claim in the design (F17) and gaps in the regression suite (F18).

Changes made during the second review (testnet upgraded 2026-10-28):
- Primary ID seed `[b"primary_id", kind, target]` (F13); the label `memo` is rejected at every level (F14).
- The `COLLECTION_URI` guard skips `cfg(test)`, so the unit tests run with the mainnet IDs; a mainnet program build still fails until the URI is set.
- 11 regression cases (R34, S11, K01–K05, V08–V11); design §2, §3, §5, §6 and §7 updated.

Changes made during the first review:
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
| Low | 7 | Documented or accepted (F02–F05); fixed (F13, F14); rule changed (F15) |
| Informational | 10 | Documented, by design, or checklist (F06–F12); F16–F18 documented or covered |

Tests: 24 unit tests (`programs/memo-id/src/tests.rs`, testnet and mainnet builds) and 111 local-validator regression cases (`clients/regression/src/regression-memo-id.rs`), all passing on 2026-10-28. On X1 testnet: `smoke-test-memo-id` and X1 wallet tests up to a 253-byte ID, all passing on 2026-10-27/28.

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
| Primary ID | `[b"primary_id", kind (0 Address, 1 Asset), target key]` | target, `memo_id_registry`, `set_by`, bump (106 bytes) |

### Cost and size (local validator, X1 mainnet Core / Token-2022 builds)

| Instruction | CU | Transaction size |
|---|---|---|
| `register` | 108k | 1051 bytes as v0 with the lookup table, for a 68-byte ID and a 256-byte URI |
| `issue_subid` | 112k | 1150 bytes as v0 for a 64-byte SubId with two signers; 1182 bytes with a holder; 253-byte IDs fit too |

The XNT cost is about 0.0064 XNT for `register` (rent, Core fees, base fee) and 0.0055 XNT for `issue_subid`. The same `issue_subid` as a legacy transaction is 1309 bytes, above the 1232-byte limit (F12).

Measured on X1 testnet with short URIs; the transaction fee is the requested CU limit (simulated CU + 10%) at X1's price, rent and Core fees excluded:

| Transaction | ID length | Size (v0 / legacy) | CU | Fee (XNT) |
|---|---|---|---|---|
| `register` | 68 bytes | 840 / 1176 bytes | 190,788 | 0.00210 |
| `create_subid` (legacy) | 63-byte label | 452 bytes | 18,693–20,517 | 0.00021–0.00023 |
| `issue_subid` | 132 bytes | 934 / 1696 bytes | 228,832 | 0.00252 |
| `issue_subid` | 196 bytes | 1018 / 1808 bytes | 262,235 | 0.00289 |
| `issue_subid` | 253 bytes | 1094 / 1908 bytes | 291,162 | 0.00320 |

CU on testnet is higher than on the local validator, mostly in SPL Memo: about 67k CU for a 148-byte BurnMemo, and more for longer IDs because the payload carries the full ID. The largest case uses about 21% of the 1.4M CU transaction limit and leaves 138 bytes of the size limit.

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

A Primary ID for an `Asset` target can only be cleared by that Asset's controller. If the Asset is later burned, the Primary ID can never be closed (before the F13 fix, the holder of a keypair Asset's key could close it as an `Address` record; it no longer can). memo-home Assets cannot be burned, but other Core Assets can. About 0.0016 XNT of rent stays locked. The record cannot be verified, so apps already treat it as invalid.

### ℹ️ Informational

- **F06 — Shared Primary ID address (superseded by F13).** `Address(X)` and `Asset(X)` targets shared the Primary ID address, because the seed was the target key only. The first review argued that only one party can act on both, because a keypair-addressed Core Asset needs that key's signature to be created. That holds at creation only: after a sale the key holder and the owner are different parties. The kind is now part of the seed (F13).
- **F07 — Forward records can revive (by design).** A forward record is valid while `target_set_by` is the controller. If control returns to an earlier controller who set a record that nobody replaced, that record becomes valid again. This is the documented rule.
- **F08 — Assets stuck in a non-memo-home Vault address.** An ID Asset sent to the Vault address of a non-memo-home Core Asset H is still controlled by H's owner through `holder`. memo-home cannot move it out, because Vault operations need a memo-home Asset. This is a user error; wallets and apps should transfer IDs only to wallets or to memo-home Assets' Vaults.
- **F09 — Rent of a deleted Record SubId.** The rent goes to the parent's current controller, not to whoever paid it.
- **F10 — `COLLECTION_URI` is fixed at `init`.**
  - Testnet: the URI is whatever the build had at `init`; the current testnet Collection has the final URI (2026-10-30).
  - Mainnet: a `--features mainnet` build fails to compile while the URI is empty, and the deploy check reports it.
- **F11 — "No admin key" depends on the upgrade authority.** The upgrade authority is the deploy admin, a hot key. The guarantee holds only once memo-id is immutable. It also depends on memo-home, memo-burn and memo-mint, so those are locked first.
- **F12 — `register` and `issue_subid` need v0 transactions.** They need a v0 transaction with the frozen lookup table (12 addresses: memo-id and memo-home PDAs, Core, memo-home, MEMO mint, GlobalBurnStats, Token-2022, memo-burn, the instructions sysvar, System).
  - The table is frozen right after it is filled. Nobody can change, deactivate or close it.
  - Clients use the address recorded in `Anchor.toml` `[lookup_tables.<env>]`.
  - On X1 testnet (2026-10-27), the CLI created, filled and froze a table. A v0 memo-mint transaction loaded five accounts from it (`sudzoBoM…`, version 0, 413 bytes instead of 532).
  - The memo-id testnet table is `CcNJEzWdEFL3mEz67T1Fg5GAqi4rXzvZMrajqeD8tk57` (12 addresses, frozen; the measurements below used the first program's table `D8r17Ta8…`). Every `issue_subid` below the top level is over the size limit as a legacy transaction (1696 bytes for a 132-byte ID), so v0 is required in practice, not just recommended. `create_subid` fits as legacy (about 450 bytes).
  - Browser clients: web3.js 1.98's `AddressLookupTable` decoder needs a global `Buffer`. Pages without one should parse the table account directly (addresses start at byte 56).

### Second review (2026-10-28)

#### 🟡 F13 (Low, fixed): the key holder of a keypair Core Asset could replace or close its owner's Primary ID

Metaplex Core Assets created outside memo-home usually sit at a keypair address X, and whoever generated the keypair (a minter, a launchpad, a mint page) can keep it. Core does not need that key after creation, but Solana still accepts its signature.

The Primary ID seed was `[b"primary_id", target key]`, so `Address(X)` and `Asset(X)` shared one account. `set_primary` (`init_if_needed`) checked only that the signer controls the new target, and `clear_primary` checked the stored one. After X was sold to Bob:
1. Bob set `Asset(X)` → `bob.memo` (verified).
2. The key holder called `set_primary(Address(X))` signed by key X: the check passed (signer = X) and Bob's record was overwritten.
3. The key holder could then `clear_primary` and take Bob's rent, and repeat after every restore.

No verification could be forged (`is_verified` compares the kind), and memo-home and MEMO ID Assets are PDAs, so they were not affected. The harm was a lasting denial of a verified reverse record for third-party NFTs, plus about 0.0016 XNT of rent per round. In reverse, the owner could also wipe the key holder's `Address(X)` record.

**Fix**: the kind is part of the seed, `[b"primary_id", kind (0 Address, 1 Asset), target]`, in the `SetPrimary` constraint and in `primary_id_address`. The two records are now separate accounts. Regression K01–K05: a keypair Core Asset is created by the wallet and sold to Bob; Bob sets `Asset(X)`; the key holder sets `Address(X)` at its own address without touching Bob's record; writing `Address(X)` into Bob's account fails with `ConstraintSeeds`; clearing Bob's record fails with `NotController`.

#### 🟡 F14 (Low, fixed): the label `memo` let an ID contain `.memo.`

`valid_label` accepted `memo`. For 2–3 × 69 mints, an attacker could issue `memo.evil.memo` and then `xen-artist.memo.evil.memo`, set it as their Primary ID and have it verified. Wallets cut long names at the end (observed on testnet with the X1 wallet), so it could display as `xen-artist.memo…`.

**Fix**: `valid_label` rejects `memo`, so `register` and `create_subid` reject it at every level and `valid_memo_id` rejects any ID containing `.memo.`. Regression R34 and S11; unit tests for `memo`, `memo.memo` and `a.memo.b.memo`. Other look-alikes, for example `xen-artist-memo.evil.memo`, remain possible and are a display matter (F16).

#### 🟡 F15 (Low, rule changed): default resolution could send funds to a PDA

The design said that with no valid forward record a MEMO ID resolves to its controller's wallet. The controller is the Asset's Core owner (or the holder's owner), which can be a program-owned account: an escrow listing, an Asset signer, or the Vault of a non-memo-home Asset (F08). Funds sent there are usually unrecoverable. The first review's note that escrow listings are not affected (F01) is about NFT SubIds, not payments.

**Change**: apps resolve to the controller only when it is a wallet (System-owned, not a PDA); otherwise the ID does not resolve (`MEMO-ID-DESIGN.md` §5, `MEMO-HOME-DESIGN.md`). The program is unchanged.

#### ℹ️ F16 (Informational): the metadata JSON can name another ID

`require_uri` checks only the scheme and length. A controller can point a cheap ID at JSON whose `name` is an expensive ID. The on-chain Asset name and `Registry.memo_id` are always the real ID, and verification is unaffected, but a market that shows the JSON name would mislead buyers. The skeptic rated this as by design, the impact verifier as low. **Rule**: apps and markets show the on-chain name, never the JSON name, and shorten long IDs in the middle (design §5).

#### ℹ️ F17 (Informational): `issue_subid` to a fourth party near 253 bytes does not fit

The design's size table assumed the new owner is one of the signers. A separate new owner adds a 32-byte static key: with a 253-byte SubId, payer, controller, holder and new owner all distinct and a 48-byte URI, the transaction is about 1,257 bytes (1,272 with the 63-byte default gateway URI). A byte-exact model reproduces A03 (1,150 / 1,182 bytes). **Change**: design §6 states the case; clients check the size before signing and fall back to issuing to the controller and transferring.

#### ℹ️ F18 (Informational): untested holder paths

`issue_subid`, `delete_subid`, `update_uri` and `clear_primary` through a `holder` were only built for size measurements, never executed. **Covered**: V08–V11 run each of them while the ID sits in another MEMO ID Asset's Vault.

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

Forward and reverse records carry the controller that set them, so they lapse when control changes. Primary IDs are separate per target kind (F13). The regression suite covers lapsing on transfer and on sale, and every controller action through a holder (V01–V14), and a keypair Core Asset whose key holder and owner differ (K01–K05).

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
- [x] Testnet program keypair (`keys.sh new testnet programs memo_id`, backed up offline by the owner). The mainnet keypair is a mainnet checklist item.
- [x] Testnet `declare_id!` and `Anchor.toml` `[programs.testnet]` set to `8FgBnyrx…`, then to `GBuh5nWV…` for the rehearsal (2026-10-29), then to `3U7nmWAr…` for the final URI (2026-10-30).
- [x] Deploy tooling:
  - memo_id is in `setup-keypairs.sh` and `deploy-common.sh`, after memo_home.
  - The configuration check accepts `MEMO_MINT = memo_burn::AUTHORIZED_MINT_PUBKEY` with the mainnet cascade and checks `COLLECTION_URI`.
  - A deploy runs the idempotent `admin-init-memo-id` and `admin-init-memo-id-alt`, and warns when memo_id is deployed without memo_home.
  - Verified on the local validator 2026-10-27: error paths, creation, re-runs that only verify, and recording in `Anchor.toml`.
- [x] memo-home accepts 253-byte names on testnet: the on-chain bytecode equals the current build (checked 2026-10-27).
- [x] Deployed to testnet 2026-10-27 (slot 198521402, commit 124cdbe3). On-chain bytecode `a8388b2c…` equals the testnet build. IDL `BvbXTVPk…`, authority = deploy admin. `admin-init-memo-id`: Collection `7a5ykCgCjL9tafPw56EN4ZaQ5XjvnAQkCWLf2JNExaaD` (update authority = memo-home Authority), memo-home Config `DjnUPsTR…` (authority = memo-id Authority `3NQoANBe…`), tx `mPnKY7wn…`. `admin-init-memo-id-alt`: table `D8r17Ta8…` created (`3urZGDDM…`), frozen (`4AGMynD4…`) and recorded in `Anchor.toml` `[lookup_tables.testnet]`.
- [x] `smoke-test-memo-id` on testnet with the test wallet: burn stats init, `register` (v0, 723 bytes), `set_primary`, `create_subid`, `issue_subid` (v0, 788 bytes); every Asset is in the Collection and owned as expected.
- [x] Wallet test with the X1 wallet extension (standalone test page, not yet memo-app), see Testnet deployment and wallet tests. The 253-byte name is stored in full; the wallet's NFT tab truncates it in display.

### 🔴 Required before mainnet
- [ ] Mainnet program keypair; `declare_id!` (mainnet cfg) and `Anchor.toml` `[programs.mainnet]` set to it.
- [x] Set `COLLECTION_URI` = `https://ardrive.net/Ff6UIKWrVHgbQkj68w-VHc8GLoi4gKR690F8jJAXkBA` (2026-10-30, replacing `QSwzOaPU…` of 2026-10-29; the shared MEMO ID metadata JSON, its `image` also an https `ardrive.net` URL; see the wallet tests). It cannot be changed after `init` (F10).
- [ ] Verify that memo-home mainnet bytecode equals the build that accepts 253-byte names. It was redeployed in slot 87726048, after that change.
- [ ] Publish the integration rules:
  - markets: record and recheck `nft_subids_issued` (F01);
  - clients: create the payer's burn stats first (F03), retry on `BurnAmountMismatch` (F02), use v0 with the recorded lookup table (F12), check the size before signing `issue_subid` (F17);
  - apps: resolve only to wallet controllers (F15); show the on-chain name, never the JSON name, and shorten long IDs in the middle (F14, F16).
- [ ] memo-app MEMO ID page: v0 + lookup table for `register` and `issue_subid`, including the Backpack path (which currently builds legacy `Transaction`s), and lookup-table parsing without `Buffer`.
- [x] Independent adversarial review of memo-id and its memo-home interaction (second review, 2026-10-28; fixes F13 and F14).
- [x] Testnet upgraded with the F13/F14 fixes on 2026-10-28 (slot 198692085, tx `4QewGuF8…`); on-chain bytecode `ce10b4fc…` equals the testnet build; IDL updated; Collection and lookup table unchanged. `smoke-test-memo-id` passed afterwards, including `set_primary` at the new kind-separated address (`smoke261028041132.memo`).
- [x] Full testnet rehearsal with the 2026-10-29 `COLLECTION_URI` (`QSwzOaPU…`, without the `collection` field): the first program's keypair archived (`testnet/archive/memo_id_v1-keypair.json.gpg`), new program `GBuh5nWVqztwvLnNzfoUCToqYJxLBLSXjEA7LqezkTYh` deployed at slot 199023254 (tx `4KRGdvMT…`); on-chain bytecode `966a7963…` equals the testnet build; IDL `BSDptgCf…`, authority = deploy admin. `init`: Collection `7YgyZPCaGf5v6cpYRjxXTGp18PQBGDK7pGAfXgX3XLfr` named `MEMO ID` with URI `https://ardrive.net/QSwzOaPU…` (read back from the account), Config `5C98dv2C…`. Lookup table `3vQa61rme77vyRnxEcfKyUS2DTVc93h2czUxp2j4bRaZ` frozen with 12 addresses. Unit tests 24/24 and regression 111/111 before the deployment. `smoke-test-memo-id` passed (`smoke261029124909.memo`: `register` 179,449 CU / 723 bytes v0; `issue_subid` 189,653 CU / 788 bytes v0); the smoke test now registers with the shared JSON as its URI.
- [x] Second full testnet rehearsal on 2026-10-30, so that the testnet Collection has the final, single JSON `Ff6UIKWr…`: keypair `GBuh5nWV…` archived (`testnet/archive/memo_id_v2-keypair.json.gpg`), new program `3U7nmWArstoRfkuGDsirEwiY6nngqjcooZxJ4Eh87MBt` deployed at slot 199245650 (tx `Zj3rWZRX…`); on-chain bytecode `c72d556f…` equals the testnet build; IDL `6cNv1qoS…`, authority = deploy admin. `init` (tx `5Pc7ZcAj…`, 56,882 CU): Collection `5rpxS7zHRc4tyzTTfx1zUBfvTgjoJn8KV7t8HXMywHwx` named `MEMO ID` with URI `https://ardrive.net/Ff6UIKWrVHgbQkj68w-VHc8GLoi4gKR690F8jJAXkBA` (read back from the account), Config `B6MWr8Fe…`. Lookup table `CcNJEzWdEFL3mEz67T1Fg5GAqi4rXzvZMrajqeD8tk57` frozen with 12 addresses. Unit tests 24/24 (testnet and mainnet features) and regression 111/111 before the deployment. `smoke-test-memo-id` passed (`smoke261030104437.memo`: `register` 182,998 CU / 749 bytes v0; `issue_subid` 188,392 CU / 813 bytes v0).
- [ ] A third-party audit if IDs will carry significant market value.
- [ ] Lock order (F11): memo-mint and memo-burn, then memo-home, then memo-id.

---

## Testing

### Unit tests (`programs/memo-id/src/tests.rs`, 24 tests, both networks)
- **Labels and IDs**: label rules (including `memo`, F14), normal-form IDs, the 253-byte limit; Primary ID addresses per kind (F13).
- **Prices**: the price table; prices at a reward of 1 MEMO; prices along the curve, rounding up at its 1-unit tail; every price fits one burn.
- **BurnMemo**: valid memos, amount and version, malformed input, payload fields.
- **Other**: URIs; Registry sizing and offsets; other account sizes; PDAs; controllers (owner, one-level Vault rule, NFT and Record IDs); forward lapsing; verification; Target controllers.

### Local-validator regression (`clients/regression/src/regression-memo-id.rs`, 111 cases)
`./scripts/run-regression-tests.sh id` loads X1 mainnet's Metaplex Core and Token-2022 builds, pinned by hash, together with memo-mint, memo-burn and memo-home. Each rejection asserts the exact error code and that the watched state did not change.

| Area | Cases |
|---|---|
| `init` and the Collection address | I01–I03 |
| `register`: labels, prices by length against an independent curve reference, BurnMemo and payload, URIs, signers | R01–R34 |
| Record SubIds | S01–S11 |
| NFT SubIds, SubId Index and counters | N01–N14 |
| v0 transactions with the frozen lookup table | A01–A03 |
| 253-byte IDs | L01–L05 |
| Forward records | T01–T07 |
| Primary IDs | P01–P10 |
| Primary IDs of a keypair Core Asset with a separate key holder (F13) | K01–K05 |
| Holder Vaults (every controller action through a holder), transfers and sales | V01–V14 |
| `update_uri` | U01–U07 |

### Testnet deployment and wallet tests (2026-10-27/28)

**`smoke-test-memo-id`** (`./scripts/run-smoke-tests.sh memo-id`, test wallet `~/.config/solana/id.json`): checks the Collection and the frozen table, then registers `smoke<timestamp>.memo`, sets the payer's Primary ID, and creates and issues `www.<id>`. Passed: `register` 166,267 CU / 723 bytes (1059 as legacy); `issue_subid` 176,481 CU / 788 bytes (1124 as legacy).

**X1 wallet extension** (wallet `6AZ7h7qX…`, a local test page building transactions with web3.js 1.98):

| Test | Result |
|---|---|
| `register` of `wtest261027122509.memo`, v0 with the table, transaction passed to `window.x1.signTransaction` as bytes | Signed and confirmed (`4pnXmCbV…`); the wallet returned the signed bytes with the message unchanged; 69 MEMO burned, fee 0.00183 XNT; the Asset shows in the wallet under the MEMO ID Collection |
| Same, transaction passed as a web3.js `VersionedTransaction` object | Signed and confirmed |
| Four-level chain: `register` 68 bytes, then `create_subid` + `issue_subid` for 132, 196 and 253 bytes (7 transactions, 276 MEMO) | All confirmed; final Asset `5nEz7cGeVHZw3MxQjbUtySavZ3nfgNQgoh9qxCGdWZZU` holds the full 253-byte name; every Asset is in the one Collection |
| Display of the 253-byte name in the wallet's NFT tab | Truncated; the `.memo` suffix is not visible. Display only; apps should use a middle ellipsis and show the full ID on demand |

The wallet changed none of the messages it signed. No program change resulted from these tests.

**Metadata URI forms** (2026-10-29, X1 wallet holding the test wallet's smoke IDs; URIs set with `memo-id-update-uri`). Drafts on Arweave via ArDrive/Turbo: image `UUgSL1jU…` (512×512 PNG), JSON-B (`image` = `ar://…`) `LN3-RDtn…`, JSON-C (`image` = `https://ardrive.net/…`) `WD0s4dfT…`. Turbo reported every item `FINALIZED`; `ardrive.net` and `turbo-gateway.com` served them, `arweave.net` still returned 404 a day later.

| Asset URI | JSON `image` | Image shown |
|---|---|---|
| GitHub raw `https://` (control) | `https://` | yes |
| `https://ardrive.net/<JSON-C>` | `https://ardrive.net/…` | yes |
| `ar://<JSON-C>` | `https://ardrive.net/…` | no |
| `https://ardrive.net/<JSON-B>` | `ar://…` | no |
| `ar://<JSON-B>` | `ar://…` | no |

The wallet resolves neither an `ar://` URI nor an `ar://` image, so clients use an https gateway URL for both (63 bytes for the URI instead of 48; F17 changes only in how often the fallback applies). `COLLECTION_URI` follows the same rule.

**Final metadata** (2026-10-29): image `YtT8Mte_…` (512×512 PNG, 15,756 bytes) and JSON `QSwzOaPU…` (323 bytes: name `MEMO ID`, description "An on-chain name on X1 ending in .memo.", no symbol), both uploaded free through Turbo and served by `https://ardrive.net/`. With `smoke261028041132.memo` pointed at it, the X1 wallet showed the new image. `arweave.net` was not chosen: a day after Turbo reported the drafts final it still answered 404 for most of them.

**Collection title in wallets** (2026-10-30): both the X1 wallet and Backpack group the IDs by Collection but title the group with the name of one of its Assets, not with the Collection's on-chain name `MEMO ID` (old Collection: `smoke261027112448.memo` in X1, `www.smoke261027112448.memo` in Backpack; new Collection: `www.smoke261029124909.memo` in Backpack). The Collection account and its JSON both say `MEMO ID`, so this is wallet behavior. It can look as if one ID owned the others; apps show the Collection by its on-chain name and verify membership by the Collection address (F16). The data is permanent whichever gateway is named: a client can take the txid from the URL and use another gateway.

Follow-up the same day: JSON `Ff6UIKWr…` (380 bytes) is the final JSON plus `"collection": { "name": "MEMO ID", "family": "MEMO" }`. With both IDs of the new Collection pointed at it (`update_uri`, 58,728 and 58,672 CU), the X1 wallet titled the group `MEMO ID` after a restart; Backpack kept an Asset's name. This JSON is now both the default URI for IDs (the smoke test registers with it) and `COLLECTION_URI`, so there is one shared JSON; testnet was redeployed so its Collection has it too (checklist, second rehearsal). Backpack's title remains wallet behavior, covered by the rule above.

---

## Audit Conclusion

memo-id meets its design invariants for:
- one Registry and one Asset per ID;
- burns bound to operation, payer and ID;
- control that follows the Asset;
- records that lapse when control changes.

The only medium finding (F01) is a marketplace concern with an exact, cheap check, like memo-home's Vault State nonce. The adversarial second review found no critical, high or medium issue; its two program findings (F13, F14) are fixed and covered by regression cases. Deployed to testnet on 2026-10-27 and upgraded with the fixes on 2026-10-28; the smoke test passes. **Mainnet after the mainnet checklist.**
