# Memo-ID Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-id (`programs/memo-id`)
**Audit Dates**: October 27, 2026 (pre-deployment review); October 28, 2026 (testnet deployment and wallet tests; second, adversarial review and fixes); October 30–31, 2026 (third review: static review, local-validator reproduction, mutation testing and stateful fuzzing); November 3, 2026 (version 0.2.0: fixes of F19, F20, F25, F31 and F32, with tests, mutation testing, stateful fuzzing and a review of the changes)
**Auditor**: Pre-deployment security review. The reviewer read the whole program, the memo-home, memo-burn and memo-mint code it calls, and the design; ran the unit tests and the local-validator regression suite; and checked the X1 behaviour the program depends on (v0 transactions with a frozen Address Lookup Table) on X1 testnet. The second review was a multi-agent audit: six review dimensions (payment binding, name space, control, accounts and CPI, composition with memo-home and Core, free-form attacker), one skeptic trying to refute and one verifier assessing impact for each finding, and a completeness critic whose findings were verified the same way. 20 agents; 94 further attack ideas were checked and ruled out. The third review repeated that method with eleven dimensions (adding state machine, economics and denial of service, operations and clients, specification, test gaps) against the Metaplex Core source matching X1's build, plus an offline differential fuzz of the pure functions (over 100M cases, 0 divergences) and a completeness critic. 60 findings were verified by a skeptic and an impact verifier each; 338 further ideas were ruled out. The findings that need chain state were then reproduced on the local validator, the regression suite was extended until every non-equivalent mutant of 71 was caught, and a model-based stateful fuzzer compared 8,000 random steps with the chain (Third review).
**Version**: 0.2.0 (2026-11-03), 10 instructions, tested on the local validator; it changes account layouts and interfaces, so it goes to testnet as a new program (Fixes in 0.2.0). Before it: 0.1.0, 10 instructions. Program IDs: testnet `3U7nmWArstoRfkuGDsirEwiY6nngqjcooZxJ4Eh87MBt` (redeployed 2026-10-30 with the final `COLLECTION_URI`, slot 199245650, upgradeable; the earlier testnet programs `8FgBnyrx…` of 2026-10-27/28 and `GBuh5nWV…` of 2026-10-29 were closed on 2026-10-30); mainnet still the placeholder `Gji1DjBF…` until its keypair is set up.
**Language**: Rust (Anchor 0.32.1)
**Network**: X1 (SVM-based)
**Design reference**: `MEMO-ID-DESIGN.md`

### Overall Assessment

**Risk Level**: ✅ **LOW**. Deployed to testnet and tested there with the X1 wallet (see Testnet deployment and wallet tests); mainnet after the mainnet checklist. The owner's decisions on the third review's program findings are made: F19, F20, F25, F31 and F32 are fixed in version 0.2.0, which goes to testnet as a new program.

**Scope note**: all three reviews are internal reviews by the development side, not an independent third-party audit. Testing is example-based (unit and regression cases), plus, since the third review, mutation testing and a model-based stateful fuzzer on the local validator. No formal verification was done.

No critical or high-severity issue was found. Name uniqueness, the binding of each burn to its operation, payer and ID, the controller rules and the integrity of the MEMO ID Collection all hold. One medium finding concerns marketplaces rather than the program itself: a listing does not stop the seller from issuing NFT SubIds, which are permanent, before the sale (F01). It has a cheap, exact check, like the Vault State nonce in memo-home.

The second review found no critical, high or medium issue either. It found two low-severity program issues, both fixed: a keypair-addressed Core Asset's key holder could replace or close the Primary ID of the Asset's later owner (F13, which corrects F06), and the label `memo` let an ID contain `.memo.`, so a truncated name could read as someone else's (F14). It also corrected the resolution and display rules for apps (F15, F16), a transaction-size claim in the design (F17) and gaps in the regression suite (F18).

The third review found no critical or high issue. It found one medium issue in the program: `issue_subid` ignores the Core freeze and delegate state that memo-home already treats as a listing, so a borrower or renter can strip permanent NFT SubIds from an ID pledged as collateral or rented out, and F01's counter check cannot prevent this (F19). It found nine low issues: the holder rule accepts any Core Asset, not only memo-home Assets (F20); F14 is only partly fixed and the display rule keeps the wrong labels (F21); records set through Core Asset Signers do not lapse on a sale (F22); no safe payment destination exists for a verified Asset target (F23); the exported helpers do not bind the Asset to the Registry (F24); the IDL gives wrong seeds for the Primary ID (F25); the permanent Collection URI depends on the domain `ardrive.net` (F26); and two deploy-tool issues (F27, F28). Eleven informational findings cover diagnostics, encoding, discovery, documentation and the test suite (F29–F39). The program was not changed during this review: program changes are proposed for the owner to decide (Third review, Decisions).

Fixes in 0.2.0 (2026-11-03; changes account layouts and instruction interfaces):
- `issue_subid` rejects with `Listed` (6027) when the parent's Asset or the holder is listed by `memo_home::is_listed` (frozen, or with a Freeze or Transfer delegate) (F19).
- Every instruction that takes `holder` also takes `holder_config`, the holder's memo-home Collection Config; both or neither (`HolderNotHomeAsset`, 6026), and the holder must be a member of that Collection (F20).
- `set_primary(target_kind, target_key)`: the IDL's Primary ID seeds now match the program; an unknown kind raises `InvalidTargetKind` (6028) (F25).
- Fixed-size Registry encoding (192 bytes plus the ID); `clear_target` zeroes every target field (F31).
- Registries record `target_holder`, Primary IDs record `set_holder` (Asset targets only), so held IDs can be verified from accounts readable by address, with the misses the design §5 describes (F32).
- A multi-agent review of the 0.2.0 changes found no program defect. Its confirmed findings were documentation corrections (misses of address-only verification, cost, transaction size, status line, testnet redeploy steps) and one test gap (a holder from a second memo-home Collection, now Y08–Y10); four findings were rejected. Mutation testing then added V17 and V18.

Changes made during the third review (tests only):
- Regression suite from 111 to 181 cases, run at two supplies (5M and 2.5T); rejections now assert the raising program and the Anchor error name, and every account the transaction names; skipped cases fail the run.
- New `repro-memo-id-audit3` (64 cases, the findings as they behave today) and `fuzz-memo-id` (stateful fuzzer) bins.

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
| Medium | 2 | Documented; market-level check (F01); fixed in 0.2.0 (F19) |
| Low | 16 | Documented or accepted (F02–F05); fixed (F13); partly fixed (F14, see F21); rule changed (F15); fixed in 0.2.0 (F20, F25); open (F21–F24, F26–F28) |
| Informational | 21 | Documented, by design, or checklist (F06–F12); F16–F18 documented or covered; F31 and F32 fixed in 0.2.0; F29, F30, F33–F39 open or covered |

Tests (0.2.0, 2026-11-03): 26 unit tests (testnet and mainnet builds), 202 local-validator regression cases at supplies of 5M and 2.5T, 52 reproductions, all 14 mutants of the 0.2.0 changes caught, and 8,000 stateful fuzz steps with no difference between the model and the chain. Before 0.2.0: 24 unit tests (`programs/memo-id/src/tests.rs`, testnet and mainnet builds) and 181 local-validator regression cases (`clients/regression/src/regression-memo-id.rs`, each run at supplies of 5M and 2.5T), all passing on 2026-10-31, together with the curve, mint, burn, home and admin-init suites. The suite catches all 64 non-equivalent mutants of 71; 8,000 stateful fuzz steps found no difference between the model and the chain. On X1 testnet: `smoke-test-memo-id` and X1 wallet tests up to a 253-byte ID, all passing on 2026-10-27/28.

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
| `set_primary(target_kind, target_key)` | the Target's controller (+ payer) | creates or replaces the Primary ID, stamped `set_by = signer` |
| `clear_primary` | the Target's current controller | closes the Primary ID; rent to the signer |
| `update_uri(uri)` | the ID's controller (+ payer) | memo-home `update_asset`, URI only; the name always stays the ID |

**Controller** of an NFT ID: the Core owner of its Asset. If that owner is the Vault of a memo-home Asset H (passed as `holder`), the controller is the owner of H. Only one level is followed. A Record SubId's controller is its parent's controller.

### Accounts

| Account | Seeds (program) | Notes |
|---|---|---|
| Authority | `[b"authority"]` (memo-id) | Authority of the MEMO ID Collection in memo-home; signs only `create_collection`, `create_asset` and `update_asset` |
| MEMO ID Collection | `[b"collection", authority, sha256("memo")]` (memo-home) | Core update authority is memo-home's Program Authority; no private key exists |
| Registry | `[b"memo_id", sha256(memo_id)]` | kind, asset, parent, created_at, `nft_subids_issued` (offset 81), `record_subids`, bump, `target_set_by`, `target_holder`, `has_target`, target (fixed 33 bytes), memo_id (offset 188); 192 + ID length bytes, exactly its encoding |
| Asset | `[b"asset", collection, sha256(memo_id)]` (memo-home) | one per NFT ID; memo-home's Burn Guard rejects burns |
| SubId Index | `[b"subid", parent registry, index u32 LE]` | registry, bump (41 bytes); never closed |
| Primary ID | `[b"primary_id", kind (0 Address, 1 Asset), target key]` | target, `memo_id_registry`, `set_by`, `set_holder`, bump (138 bytes) |

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

**Correction (third review, F19)**: the check works for a buyer, who can refuse the purchase. It does not protect a lender or a renter, who takes the Asset back whatever the counter says, and memo-home already locks frozen or delegated Assets for exactly this reason. F19 proposes a program-level gate.

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
- **F08 — Assets stuck in a non-memo-home Vault address (understated; see F20).** An ID Asset sent to the Vault address of a non-memo-home Core Asset H is still controlled by H's owner through `holder`. memo-home cannot move it out, because Vault operations need a memo-home Asset. This is a user error; wallets and apps should transfer IDs only to wallets or to memo-home Assets' Vaults. The third review showed that control then also passes to whoever holds delegates over H, and is lost for good if H is burned (F20).
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

#### 🟡 F14 (Low, partly fixed; see F21): the label `memo` let an ID contain `.memo.`

`valid_label` accepted `memo`. For 2–3 × 69 mints, an attacker could issue `memo.evil.memo` and then `xen-artist.memo.evil.memo`, set it as their Primary ID and have it verified. Wallets cut long names at the end (observed on testnet with the X1 wallet), so it could display as `xen-artist.memo…`.

**Fix**: `valid_label` rejects `memo`, so `register` and `create_subid` reject it at every level and `valid_memo_id` rejects any ID containing `.memo.`. Regression R34 and S11; unit tests for `memo`, `memo.memo` and `a.memo.b.memo`. Other look-alikes, for example `xen-artist-memo.evil.memo`, remain possible and are a display matter (F16). The third review found that any label that merely starts with `memo` (`memory`, `memos`, `memo1`) reproduces the original end-truncation scenario for one 69-mint registration, so the fix closes only the literal `.memo.` case (F21).

#### 🟡 F15 (Low, rule changed): default resolution could send funds to a PDA

The design said that with no valid forward record a MEMO ID resolves to its controller's wallet. The controller is the Asset's Core owner (or the holder's owner), which can be a program-owned account: an escrow listing, an Asset signer, or the Vault of a non-memo-home Asset (F08). Funds sent there are usually unrecoverable. The first review's note that escrow listings are not affected (F01) is about NFT SubIds, not payments.

**Change**: apps resolve to the controller only when it is a wallet (System-owned, not a PDA); otherwise the ID does not resolve (`MEMO-ID-DESIGN.md` §5, `MEMO-HOME-DESIGN.md`). The program is unchanged.

#### ℹ️ F16 (Informational): the metadata JSON can name another ID

`require_uri` checks only the scheme and length. A controller can point a cheap ID at JSON whose `name` is an expensive ID. The on-chain Asset name and `Registry.memo_id` are always the real ID, and verification is unaffected, but a market that shows the JSON name would mislead buyers. The skeptic rated this as by design, the impact verifier as low. **Rule**: apps and markets show the on-chain name, never the JSON name, and shorten long IDs in the middle (design §5). The third review corrected which labels the shortened form must keep (F21).

#### ℹ️ F17 (Informational): `issue_subid` to a fourth party near 253 bytes does not fit

The design's size table assumed the new owner is one of the signers. A separate new owner adds a 32-byte static key: with a 253-byte SubId, payer, controller, holder and new owner all distinct and a 48-byte URI, the transaction is about 1,257 bytes (1,272 with the 63-byte default gateway URI). A byte-exact model reproduces A03 (1,150 / 1,182 bytes). **Change**: design §6 states the case; clients check the size before signing and fall back to issuing to the controller and transferring.

#### ℹ️ F18 (Informational): untested holder paths

`issue_subid`, `delete_subid`, `update_uri` and `clear_primary` through a `holder` were only built for size measurements, never executed. **Covered**: V08–V11 run each of them while the ID sits in another MEMO ID Asset's Vault.

### Third review (2026-10-30/31)

Scope: the testnet program `3U7nmWAr…` (bytecode `c72d556f…`, rebuilt byte-identical from the source), the memo-home, memo-burn and memo-mint code it calls, Metaplex Core at the commit matching X1's build, the deploy and admin clients, the design and this report. Findings S00–S59 of the review were merged into F19–F39 below. Every finding that depends on chain state was reproduced on the local validator by `repro-memo-id-audit3` (X1 mainnet Core build, pinned hash); the Evidence lines name its cases. The program was not changed.

#### 🟠 F19 (Medium, fixed in 0.2.0): `issue_subid` ignores Core freeze and delegate state

memo-home treats an Asset with a frozen FreezeDelegate or a TransferDelegate held by someone else as listed, and refuses every outflow from its Vault (`require_owner_not_listed`, `Listed` 6011). memo-id does not: `issue_subid` checks only the Core owner (`require_nft_controller`), never the parent Asset's plugins, and an issued NFT SubId can never be revoked.

Scenario: Alice pledges `bank.memo` in an escrowless Core loan or rental; the lender L holds TransferDelegate and a frozen FreezeDelegate. Alice stays the Core owner, so she stays the controller. She issues `www.bank.memo`, `pay.bank.memo` and others to a second wallet for 69 mints each, then defaults. L unfreezes and takes `bank.memo`; its most valuable sub-names belong to Alice forever. The same works when `bank.memo` sits in the Vault of a frozen collateral Asset H: memo-home refuses to withdraw it, memo-id lets H's owner issue its SubIds through the holder. Any temporary controller (a renter, a flash-loan borrower, a custodian) can do the same.

F01's counter check does not help here: a buyer can refuse a purchase when `nft_subids_issued` changed, but a lender at liquidation or an owner ending a rental takes the Asset back regardless, so the check only detects the loss.

**Evidence**: S21-1 to S21-13. memo-home `withdraw_lamports` on the frozen Asset fails with `Listed`; Alice cannot unfreeze (Core `NoApprovals`) or revoke (`InvalidAuthority`); `issue_subid` on the frozen parent succeeds and burns exactly 69 MEMO; after L takes the parent, L gets `NotRecord` from `delete_subid` and `NotController` from `set_target` on the issued SubId; the holder variant behaves the same.

**Proposed fix (program)**: in `issue_subid`, after `require_nft_controller`, reject when `memo_home::is_listed(parent_asset)`, and, with a holder, when `memo_home::is_listed(holder)`. This mirrors memo-home's outflow rule. Other controller actions can be undone by the next controller and can stay ungated. Keep F01's counter rule for offers that do not lock the Asset on chain, and extend it to any protocol that lends out or takes custody of an ID.

**Fix (0.2.0)**: as proposed. `issue_subid` raises `Listed` (6027) after the controller check and before the burn when `memo_home::is_listed` holds for the parent's Asset or the holder. Regression W01–W07, reproductions S21-4 to S21-11.

#### 🟡 F20 (Low, fixed in 0.2.0): the holder rule accepts any Core Asset

`controller_of` follows a holder H when the Asset's owner is `vault_of(H)`, and returns H's Core owner. Nothing checks that H is a memo-home Asset, although the design and this report describe the rule as "the Vault of a memo-home Asset" and memo-home's own integration rule checks membership with the Collection Config. With a third-party H:
- whoever holds a PermanentTransferDelegate over H can move H without the owner's signature and so take control of the ID, including issuing NFT SubIds; a buyer of H loses control again;
- whoever holds a PermanentBurnDelegate can burn H, after which the ID can never be controlled again;
- memo-home cannot move the ID out of H's Vault (`withdraw_asset` fails with `AccountNotInitialized` on `vault_state`).

A related case: an ID transferred to a third-party Asset's own address X (instead of its Vault) is controlled by the holder of key X, not by X's owner. The design's sentence that "nobody can act as an Asset" holds only for memo-home and MEMO ID Assets, which are PDAs.

**Evidence**: S17-1 to S17-16 (clawback without the buyer's signature, issue by the delegate, burn, `AccountNotInitialized` 3012); the fuzzer accepted 58 controller actions through a non-memo-home holder and saw all 110 withdrawals of those IDs fail, and accepted 78 actions signed by a keypair Asset's key.

**Proposed fix (program)**: require the holder to be a memo-home Asset, for example by passing its Collection Config and checking `memo_home::is_home_asset`, as memo-home's integration rule does. This adds an account wherever a holder is used, so it is cheapest before mainnet. IDs already held by non-memo-home Assets on testnet would become uncontrollable (test data only). Clients should in any case refuse to transfer an ID to any Core Asset address or to the Vault of a non-memo-home Asset.

**Fix (0.2.0)**: every instruction with `holder` takes `holder_config: Option<Account<CollectionConfig>>` after it; both or neither (`HolderNotHomeAsset`, 6026); `controller_of` requires `memo_home::is_home_asset(holder, config)` before the Vault check. Account lists of eight instructions change. Regression Y01–Y10 (including a holder from a second memo-home Collection), reproductions S17-2 to S17-6.

#### 🟡 F21 (Low, open): F14 is only partly fixed, and the display rule keeps the wrong labels

`valid_label` rejects only the exact label `memo`. An attacker registers `memory.memo` (69 mints), creates the free Record `xen-artist.memory.memo` pointing at itself and sets its Primary ID. The verified name begins with the 15 bytes `xen-artist.memo`, which is F14's original scenario. The middle-shortening rule adopted for F16 keeps the first label and `.memo`; but the first label of a SubId is chosen by the parent's controller, and every ID ends in `.memo`, so `xen-artist.<63-byte filler>.evil.memo` renders as `xen-artist.….memo` and hides the only label that says who owns it.

**Options**: (a) reject every label that starts with `memo` (exact against prefix look-alikes, but also bans `memory`, `memorial`, `memos`); or (b) recommended: keep the label rule, and change the display rule: always show the second-level name (`<label>.memo`) in full and emphasised, shorten labels to its left, never cut the end, as browsers show the registrable domain. Optionally reserve `x1` as a label, as was done for `memo`.

#### 🟡 F22 (Low, open): records set through a Core Asset Signer do not lapse when the controlling Asset is sold

Core lets the owner of an Asset Z sign as its Asset Signer S = PDA(`["mpl-core-execute", Z]`, Core) through `ExecuteV1`, and X1's Core build has it.
- An ID owned by S has controller S. Its forward record and the Primary IDs it sets are stamped S. When Z is sold, the ID's Core owner is still S, so the seller's records stay valid; only the buyer can clear them.
- An `Address(S)` target is confirmed by S's own signature, and Address records never lapse. After a sale of Z, the seller's ID stays verified to, and resolves to, an address the buyer now signs for.
- Under `ExecuteV1`, `register`, `issue_subid` and `update_uri` fail with `ReentrancyNotAllowed` (Core is already on the stack); the other instructions work. Such IDs must be moved out first.

**Evidence**: S12-1 to S12-6, S23-1 to S23-6, S01-1 to S01-4.

**Rule (no program change needed)**: design §5 and the market rules state that IDs owned by an Asset Signer (or any PDA whose control changes without its owner field changing) keep their records across a sale of the controlling Asset; markets selling Z inspect and clear them; apps point names at `Asset(Z)`, not at `Address(S)`, and show Address targets that are PDAs as unverified or with a warning. A target kind `AssetSigner(Z)` is a possible later extension.

#### 🟡 F23 (Low, open): no safe payment destination for a verified Asset target

F15 defines resolution only when no forward record is valid. For a verified `Asset(A)` target there is no rule. Paying the Asset address loses the funds (Core's permissionless `Collect` sweeps lamports above rent to Metaplex), and memo-home's "pay `vault_of(asset)`" rule loses them when A is not a memo-home Asset, since memo-home signs only for its own Assets' Vaults.

**Rule (no program change needed)**: for `Asset(A)`, pay `vault_of(A)` only when A is a memo-home Asset; otherwise pay A's owner only when it is a wallet (the F15 rule); otherwise do not resolve for payments; never pay the Asset address. Add a client helper for it.

#### 🟡 F24 (Low, open): the exported helpers do not bind the Asset to the Registry

The program's own checks (`require_nft_controller`, `require_controller`, `require_target_controller`) bind the Asset to the Registry and a Record to its parent, but they are private. The exported `controller_of(asset, holder)` returns the owner of any Asset it is given, and `is_verified` trusts the controllers the caller passes. Design §5 instead documents `controller_of(registry, asset?, holder?)`, `forward_of` and `primary_of`, which do not exist. An integrator who follows the design, or passes an unchecked Asset, lets a former controller pass as verified for an ID it sold. The offline fuzz confirmed `is_verified` matches design §5 exactly when both controllers are correct (10M cases).

**Fix (crate API and docs)**: export bound helpers (for example `controller_of_id(registry, parent, asset, holder)` that applies the program's checks, and a `forward_of` that computes validity itself), or rewrite design §5 to list the real functions and their trust assumptions.

#### 🟡 F25 (Low, fixed in 0.2.0): the IDL gives wrong seeds for the Primary ID

The program derives the Primary ID from `[b"primary_id", target.kind_seed(), target.key()]`. Anchor 0.32.1's IDL generator turns a method-call seed into its receiver, so the published IDL (also on chain) says `["primary_id", arg target, arg target]`. `Target` serializes to 33 bytes, over the 32-byte seed limit, so IDL-driven resolvers (Anchor TS, Codama-generated SDKs) throw or derive an address the program rejects. This came with the F13 seed change.

**Options**: (a) program: take `kind: u8` and `target_key: Pubkey` as instruction arguments, which the IDL can express, and check them against `target`; or (b) tooling: remove the `pda` entry for `primary` from the IDL before upload, document `primary_id_address` as the only derivation, and add a check that fails when the IDL seeds differ from the program's.

**Fix (0.2.0)**: option (a). `set_primary(target_kind: u8, target_key: Pubkey)` with seeds `["primary_id", target_kind, target_key]`; an unknown kind raises `InvalidTargetKind` (6028). The arguments are the same bytes as the Borsh `Target`. The generated IDL derives the address correctly (regression P16, P17).

#### 🟡 F26 (Low, open): the permanent Collection URI depends on the domain `ardrive.net`

`COLLECTION_URI` is fixed at `init` (F10), and clients use the same URL as the default URI of every ID. Arweave content is immutable by txid, but wallets trust whatever the domain serves. If `ardrive.net` lapses, is resold or is compromised at any time in the Collection's lifetime, it can serve its own JSON (phishing text, image, `collection.name`) for the official Collection, and nobody can ever correct the on-chain URI. Clients can switch the default for new IDs; the Collection cannot.

**Before mainnet `init`**: choose the dependency deliberately: re-test `arweave.net` (the txids have now been final for several days), or accept `ar://` for the Collection only (wallets do not display the Collection URI). Record the Arweave txids of the JSON and the image here so readers can re-resolve through any gateway.

#### 🟡 F27 (Low, open): a pre-funded Collection address breaks `admin-init-memo-id`

Anyone can compute the Collection PDA from the public program ID. A System transfer of about 0.0009 XNT creates a System-owned, empty account there. `admin-init-memo-id` sees an account, tries to verify it as the Collection, and exits with "unexpected MEMO ID Collection account" on every run, so a deploy stops after the program upgrade. The program itself is fine: a direct `init` sweeps the pre-fund (memo-home F09).

**Evidence**: S26-1 to S26-5. **Fix (tooling)**: treat a System-owned account with empty data as not yet created and send `init`; do the same in `admin-init-burn-guard`.

#### 🟡 F28 (Low, open): `deploy-mainnet.sh` with no arguments includes memo_id

`ALL_PROGRAMS` includes memo_id, and a deploy without program names deploys all of them. Today a routine mainnet release aborts, because no mainnet memo-id keypair exists; once it exists, a routine release would launch memo-id on mainnet, irreversibly. `PROD-DEPLOYMENT.md` still describes eight programs, and `verify-deployment` checks only those.

**Fix (tooling)**: keep memo_id out of the default set until its launch, or require an explicit confirmation for any first deployment on mainnet; add memo-id to `verify-deployment` and `PROD-DEPLOYMENT.md`.

#### ℹ️ Informational

- **F29 — A forged Borsh length prefix aborts instead of failing cleanly.** `parse_burn_memo` deserializes the attacker's bytes without checking length prefixes; Borsh pre-allocates up to the declared length, and the 32 KiB heap runs out. A payload prefix of 29,330 bytes or more, or a String prefix of `u32::MAX` inside the payload, ends in "memory allocation failed" instead of `InvalidMemoFormat`. Nothing changes and nothing is burned (S08-1 to S08-5); memo-burn guards this case on purpose. Optional program fix: check the outer prefix as memo-burn does, then build the expected payload and compare bytes instead of deserializing it (also cheaper).
- **F30 — Error numbers collide across programs.** memo-home's `InvalidCoreAccount`, raised by `owner_of` inside memo-id's frame, surfaces as 6009, memo-id's `NotInHolderVault` (a wallet as target Asset, a burned Asset, a burned holder). memo-burn errors surface under memo-id's instruction index: paying from another wallet's token account reads as 6008 `NotController`. The raising program is the first "failed" log line, not the last (S00, S11, S20). A pre-funded address given where Anchor expects a memo-id account reports `AccountOwnedByWrongProgram` 3007, not 3012 (found by the fuzzer). The regression suite now asserts the program and the error name. Optional program fix: map memo-home helper errors to memo-id codes, or give memo-id its own error range.
- **F31 — Registry bytes depend on history.** A Registry is allocated for a target; after `clear_target` Anchor writes the shorter serialization and leaves the last 33 bytes of the old one, while a Registry that never had a target has zeros there. Strict Borsh decoders (`try_from_slice`) fail on every target-less Registry. The fuzzer matched these bytes on every step. Rule: decode Registries by prefix; the bytes after `memo_id` are undefined. Optional before mainnet: a fixed-size target encoding, or zero the tail in `clear_target`. **Fixed in 0.2.0**: `target: Option<Target>` became `has_target: bool` + `target: Target` (zero when none), so the account is exactly its encoding and `clear_target` leaves the bytes of a Registry that never had a target (unit tests; every regression read checks the encoding; the fuzzer compares the bytes).
- **F32 — Held IDs cannot be verified from accounts readable by address.** The holder is supplied by the client and stored nowhere (Registry, Primary ID and Vault State do not record it; the Vault address is a one-way hash), so a reader using only `getAccountInfo` shows an ID held in a Vault as unverified. Finding the holder takes a Core `getProgramAccounts` search by owner. Optional before mainnet: record the holder used next to `target_set_by` and `set_by`; otherwise document the search. **Fixed in 0.2.0**: Registry `target_holder` and Primary ID `set_holder` (Asset targets only) record the holder; MEMO-ID-DESIGN.md §5 gives the address-only rule, which never verifies an invalid record and can miss a valid one after a deposit (the fuzzer checks the first property on every step).
- **F33 — Documented account filters return the wrong accounts.** A `memcmp` on `parent` alone also returns issued NFT SubIds, which belong to someone else forever (presenting F01's harm as part of a sale). Primary IDs have the same layout at offsets 8, 9 and 41, and anyone can create one naming any Registry. Rule: filter on the Registry discriminator, `kind` = 1 and `parent`, and compare the count with `record_subids`; list NFT SubIds through the SubId Index.
- **F34 — A memo is not evidence of a registration.** memo-burn does not record that a memo was used, so a memo-id-shaped BurnMemo can back a plain memo-burn burn with no memo-id instruction, or extra burns next to one. memo-id's state stays correct (one memo pays for at most one memo-id operation). Rule: count registrations from Registry and SubId Index state only (§2 reworded).
- **F35 — X1's Core build.** Log strings in X1's Core identify a pre-Groups upstream build with security merge #270 but without #285 (UpdateV1 shrink reallocated before the memmove) and #306 (LinkedAppData membership check). `update_uri` shrinking the URI of an Asset with plugins relies on the older code path. memo-home's `parse_asset` is also more lenient than Core's loader on bytes only Core can write (invalid UTF-8 names, a short plugin header); unreachable. Recorded for §8 and `MEMO-HOME-AUDIT.md`; re-run the regression suites when X1 upgrades Core.
- **F36 — Deploy-tool cross-checks.** The lookup table is filled and frozen from `Anchor.toml` values that nothing checks against the program's constants; prod-staging runs create a new frozen table on every memo-id deploy and the smoke test cannot use it; the `COLLECTION_URI` guard checks only that it is non-empty, and the idempotent admin check never compares the Collection's on-chain name and URI. Fix in the tooling.
- **F37 — Documentation drift.** A legacy `issue_subid` of a short SubId fits when the owner pays and signs alone (about 1,124 bytes for `a.b.c.memo` with a 63-byte URI), so F12's "every `issue_subid` below the top level" is too strong, and the legacy column of the testnet size table is inconsistent with the v0 column (the gap should be a constant 336 bytes); `MEMO-HOME-DESIGN.md` §6.4 is stale (invalid example IDs, suffix levels, a public sale that does not exist, rent); design status lines, account counts (18 and 24, not about 25), CU figures and "64-byte limit" comments are out of date. One design sentence, that an app can register a MEMO ID in the same transaction that creates its Asset, holds only for app instructions that need no BurnMemo of their own, since both would need index 0.
- **F38 — Test gaps (covered).** The 111-case suite never exercised: `create_subid`'s `HashMismatch`; the `WrongParent` constraints of `delete_subid` and `issue_subid`; an unsigned controller in any instruction; `issue_subid`'s URI check and `register`'s `UriTooLong` (a 257-byte URI fits as v0); the BurnMemo decoding errors; any reward other than 1 MEMO; memo-id at instruction index 1 (where real clients put it); several per-instruction call sites and success branches; `created_at`, bumps and events. V01's lapse check was vacuous; cases could skip silently; rejections matched by number only. All are now covered (Testing), except two defense-in-depth paths that need injected state: `ArithmeticOverflow` at u32 counters and `NotNft` inside `require_nft_controller`.
- **F39 — Integration notes (disputed by the skeptic, kept as rules).** (a) Any ID owner can drop IDs into any wallet (`issue_subid` needs no signature from `new_owner`) with JSON it controls, inside the official Collection; Collection membership is not curation, and wallets and apps should title the group by the on-chain Collection name and ignore JSON `collection` fields. (b) A transaction holds at most one paid memo-id operation, and none together with another paid MEMO app operation.

#### Decisions for the owner

Program changes (none applied; all need a testnet upgrade or redeployment):

| Finding | Change | Recommendation |
|---|---|---|
| F19 | `issue_subid` rejects a listed parent or holder | Make it: small, mirrors memo-home, closes the only Medium |
| F20 | holder must be a memo-home Asset | Make it before mainnet: adds the Collection Config account where a holder is used |
| F29 | check the BurnMemo prefixes, compare payload bytes | Optional: clean errors, fewer CU |
| F25 | `set_primary` takes `kind` and key as arguments | Optional; or fix the IDL in tooling |
| F30 | map memo-home helper errors, or an error offset | Optional |
| F31, F32 | canonical Registry encoding; record the holder | Optional; layout changes are only cheap before mainnet |
| F21 (a) | reject labels starting with `memo` | Not recommended; prefer the display rule (b) |

Not program changes: F26 (the `COLLECTION_URI` choice, before mainnet `init`); tooling fixes F27, F28, F36; documentation and integration rules F21 (b), F22, F23, F24, F33, F34, F37, F39.

---

## Security Analysis by Category

### 1. Name uniqueness ✅ SECURE
Every Registry is at `[b"memo_id", sha256(memo_id)]` and is created with `init`. The handler recomputes the full ID and requires the given hash to match (`HashMismatch`), so each ID has exactly one possible Registry address. The Asset address comes from the same hash, and memo-home checks it. `register` creates only second-level IDs (labels have no dot). SubIds are always deeper, so the two paths cannot collide. Labels are DNS hostname labels: `a-z`, `0-9` and `-`; no leading or trailing `-`; no `xn--`, which rules out punycode homographs. Full IDs are at most 253 bytes.

### 2. Payment binding ✅ SECURE
`register` and `issue_subid` require index 0 to be an SPL Memo. It must hold a Base64 Borsh BurnMemo whose `burn_amount` equals the computed price. Its payload must name the operation, the payer and the exact ID. The burn itself goes through memo-burn, which reads the same memo. Each `register` and `issue_subid` burns its own price, and one memo can pay for at most one memo-id operation. A memo does not prove that the operation happened, since memo-burn does not record its use (F34); registrations are read from Registry state. A forged length prefix aborts the transaction instead of failing with `InvalidMemoFormat`, with nothing burned (F29).

### 3. Collection integrity ✅ SECURE
Only the memo-id Authority PDA can sign memo-home `create_asset` and `update_asset` for the MEMO ID Collection. memo-id calls `create_asset` only:
- in `register`, for a freshly initialized Registry;
- in `issue_subid`, for an existing Record SubId of that parent.

`update_uri` never changes the name. Assets cannot be burned (memo-home Burn Guard). Their PDA addresses have no private key (memo-home F01).

### 4. Control ⚠️ SECURE, with open finding F22 (F19 and F20 fixed in 0.2.0)
Every controller action recomputes the controller from the Asset's Core owner on chain:
- it checks that the Asset is the Registry's own (`WrongAsset`);
- for a Record SubId, it checks the parent link (`WrongParent`);
- it follows the holder rule one level only (`NotInHolderVault`).

Forward and reverse records carry the controller that set them, so they lapse when control changes. Primary IDs are separate per target kind (F13). The regression suite covers lapsing on transfer and on sale, and every controller action through a holder (V01–V18, Y01–Y10), and a keypair Core Asset whose key holder and owner differ (K01–K05). The stateful fuzzer recomputed the controller of every accepted action independently of the holder the client passed (8,000 steps, no difference).

Fixed in 0.2.0: `issue_subid` refuses while the parent's Asset or the holder is listed, so a temporary controller cannot issue permanent NFT SubIds (F19; W01–W07); the holder must be a member of the memo-home Collection whose Config is passed (F20; Y01–Y10). The other controller actions still follow the Core owner alone, by design: their records lapse when control changes.

Open: when the owner is an Asset Signer PDA, records do not lapse on a sale of the controlling Asset (F22).

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

Their risks are recorded in their own audits (`MEMO-HOME-AUDIT.md` §9). X1's Core build was identified as a pre-Groups upstream build with security merge #270 but without #285 and #306 (F35). The permanent Collection URI depends on the domain `ardrive.net` (F26).

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
  - third review: lenders, rental and custody protocols record and recheck `nft_subids_issued` too (F19); never transfer an ID to a Core Asset address or a non-memo-home Vault (F20); always show `<label>.memo` in full, never cut the end (F21); Asset Signer and PDA Address targets (F22); payment destination for Asset targets (F23); use only bound helpers (F24); derive Primary IDs with `primary_id_address` (F25); decode errors by the first failing program (F30); decode Registries by prefix (F31); account filters with the discriminator (F33); count registrations from state, not memos (F34); Collection membership is not curation (F39).
- [x] Third review program decisions (2026-11-03): F19, F20, F25, F31, F32 made (version 0.2.0); F29, F30 and F21 (a) not made.
- [x] Testnet: 0.2.0 deployed as a new program on 2026-11-03: keypair `3U7nmWAr…` archived (`testnet/archive/memo_id_v3-keypair.json.gpg`), new program `CDbmi4gSZ47P5EMRH7GU7ztrPDrk9gn1uMgeZR7AXeLA` at slot 200198852; on-chain bytecode `a71e4c13…` (516,352 bytes) equals the testnet build; IDL `6gEoXbt7…`, authority = deploy admin. `init` (tx `3Zb18q6w…`, 56,751 CU): Collection `2QhWiKBsf6M7bsenTZBqsrFfJFmm3CqY82KLXVHt2NkW`, Config `FHMnkY2U…`. Lookup table `9cXwVzvg1HaeetqLo4shKEkncbnuUbxiu9v2fmaSakCH` frozen with 12 addresses. Unit tests 26/26 (testnet and mainnet features) and regression 202/202 at both supplies with the new ID before the deployment. `verify-deployment` passed (30/0); `smoke-test-memo-id` passed (`smoke261103083846.memo`: `register` 148,814 CU / 748 bytes v0; `set_primary` 10,272 CU; `create_subid` 15,017 CU; `issue_subid` 153,484 CU / 813 bytes v0).
- [x] Closed the 0.1.0 testnet program `3U7nmWAr…` on 2026-11-03 (IDL account `6cNv1qoS…` first, then the program; 3.44 XNT plus the IDL rent returned to the deploy admin); its Collection, Assets and registry accounts stay on chain but can no longer change.
- [ ] Choose the `COLLECTION_URI` gateway deliberately and record the JSON and image txids (F26).
- [ ] Deploy tooling: pre-funded Collection in `admin-init-memo-id` (F27); memo_id out of the default mainnet set until launch, and in `verify-deployment` and `PROD-DEPLOYMENT.md` (F28); lookup-table and Collection cross-checks, prod-staging table reuse (F36).
- [ ] Documentation: design §3, §5, §6 and §7 and `MEMO-HOME-DESIGN.md` §6.4 (F21–F24, F33, F37, F39).
- [ ] memo-app MEMO ID page: v0 + lookup table for `register` and `issue_subid`, including the Backpack path (which currently builds legacy `Transaction`s), and lookup-table parsing without `Buffer`.
- [x] Independent adversarial review of memo-id and its memo-home interaction (second review, 2026-10-28; fixes F13 and F14).
- [x] Testnet upgraded with the F13/F14 fixes on 2026-10-28 (slot 198692085, tx `4QewGuF8…`); on-chain bytecode `ce10b4fc…` equals the testnet build; IDL updated; Collection and lookup table unchanged. `smoke-test-memo-id` passed afterwards, including `set_primary` at the new kind-separated address (`smoke261028041132.memo`).
- [x] Full testnet rehearsal with the 2026-10-29 `COLLECTION_URI` (`QSwzOaPU…`, without the `collection` field): the first program's keypair archived (`testnet/archive/memo_id_v1-keypair.json.gpg`), new program `GBuh5nWVqztwvLnNzfoUCToqYJxLBLSXjEA7LqezkTYh` deployed at slot 199023254 (tx `4KRGdvMT…`); on-chain bytecode `966a7963…` equals the testnet build; IDL `BSDptgCf…`, authority = deploy admin. `init`: Collection `7YgyZPCaGf5v6cpYRjxXTGp18PQBGDK7pGAfXgX3XLfr` named `MEMO ID` with URI `https://ardrive.net/QSwzOaPU…` (read back from the account), Config `5C98dv2C…`. Lookup table `3vQa61rme77vyRnxEcfKyUS2DTVc93h2czUxp2j4bRaZ` frozen with 12 addresses. Unit tests 24/24 and regression 111/111 before the deployment. `smoke-test-memo-id` passed (`smoke261029124909.memo`: `register` 179,449 CU / 723 bytes v0; `issue_subid` 189,653 CU / 788 bytes v0); the smoke test now registers with the shared JSON as its URI.
- [x] Second full testnet rehearsal on 2026-10-30, so that the testnet Collection has the final, single JSON `Ff6UIKWr…`: keypair `GBuh5nWV…` archived (`testnet/archive/memo_id_v2-keypair.json.gpg`), new program `3U7nmWArstoRfkuGDsirEwiY6nngqjcooZxJ4Eh87MBt` deployed at slot 199245650 (tx `Zj3rWZRX…`); on-chain bytecode `c72d556f…` equals the testnet build; IDL `6cNv1qoS…`, authority = deploy admin. `init` (tx `5Pc7ZcAj…`, 56,882 CU): Collection `5rpxS7zHRc4tyzTTfx1zUBfvTgjoJn8KV7t8HXMywHwx` named `MEMO ID` with URI `https://ardrive.net/Ff6UIKWrVHgbQkj68w-VHc8GLoi4gKR690F8jJAXkBA` (read back from the account), Config `B6MWr8Fe…`. Lookup table `CcNJEzWdEFL3mEz67T1Fg5GAqi4rXzvZMrajqeD8tk57` frozen with 12 addresses. Unit tests 24/24 (testnet and mainnet features) and regression 111/111 before the deployment. `smoke-test-memo-id` passed (`smoke261030104437.memo`: `register` 182,998 CU / 749 bytes v0; `issue_subid` 188,392 CU / 813 bytes v0). The X1 wallet titled the new group `MEMO ID`. The retired programs `8FgBnyrx…` and `GBuh5nWV…` were closed the same day (IDL accounts first, then the programs; 6.86 XNT returned to the deploy admin); their Collections, Assets and registry accounts stay on chain but can no longer change.
- [ ] A third-party audit if IDs will carry significant market value.
- [ ] Lock order (F11): memo-mint and memo-burn, then memo-home, then memo-id.

---

## Testing

### Unit tests (`programs/memo-id/src/tests.rs`, 26 tests, both networks)
- **Labels and IDs**: label rules (including `memo`, F14), normal-form IDs, the 253-byte limit; Primary ID addresses per kind (F13).
- **Prices**: the price table; prices at a reward of 1 MEMO; prices along the curve, rounding up at its 1-unit tail; every price fits one burn.
- **BurnMemo**: valid memos, amount and version, malformed input, payload fields.
- **Other**: URIs; Registry sizing and the fixed offsets, a cleared forward record all zero (F31); other account sizes, `set_holder`'s offset; PDAs; target kinds (F25); controllers (owner, one-level Vault rule with the holder's Collection Config, `HolderNotHomeAsset`, NFT and Record IDs); forward lapsing; verification; Target controllers.

### Local-validator regression (`clients/regression/src/regression-memo-id.rs`, 202 cases)
`./scripts/run-regression-tests.sh id` loads X1 mainnet's Metaplex Core and Token-2022 builds, pinned by hash, together with memo-mint, memo-burn and memo-home. It runs the suite twice on fresh validators: at a supply of 5M (reward 1 MEMO) and at 2.5T (reward 8 units, where prices are fractions of a MEMO and the ceiling rounding shows). Each rejection asserts the error code, the program that raised it and, for Anchor programs, the error name, and that no account the transaction names changed except the fee payer's lamports. Successes assert exact account state, including `created_at`, bumps and events. A skipped case fails the run unless `MEMO_REGRESSION_ALLOW_SKIP=1`.

| Area | Cases |
|---|---|
| `init` and the Collection address | I01–I04 |
| `register`: labels, prices by length against an independent curve reference, BurnMemo and payload decoding, instruction index, URIs, pre-funded addresses, program and sysvar substitution, Registry size | R01–R54 |
| Record SubIds: hash binding, parent link, re-creation, separate payer, labels | S01–S22 |
| NFT SubIds, SubId Index and counters: URIs, payload, parent link, index, pre-funded entry, issued Record with a target | N01–N30 |
| Unsigned controller or signer in every gated instruction | G01–G08 |
| v0 transactions with the frozen lookup table, 257-byte URIs | A01–A05 |
| 253-byte IDs | L01–L05 |
| Forward records | T01–T09 |
| Primary IDs, including an unknown target kind and the IDL's seeds (F25) | P01–P17 |
| Primary IDs of a keypair Core Asset with a separate key holder (F13) | K01–K05 |
| Holder Vaults (every controller action through a holder, the recorded holder), transfers and sales | V01–V18 |
| Listings: `issue_subid` with a frozen or delegated parent Asset or holder (F19) | W01–W07 |
| Holder Collection Configs: missing, wrong or foreign Configs, a holder from a second memo-home Collection (F20) | Y01–Y10 |
| `update_uri` | U01–U10 |

Every Registry read asserts the canonical encoding (zero bytes after a cleared target, length 192 plus the ID; F31).

Full run on 2026-10-31 (`./scripts/run-regression-tests.sh`, with build; `target/deploy` hashes unchanged): curve 61/0, mint 48/0, burn 65/0, home 160/0, id 181/0 at 5M and 181/0 at 2.5T, admin-init 4/0. Version 0.2.0 on 2026-11-03: id 202/0 at 5M and 202/0 at 2.5T.

### Reproductions (`clients/regression/src/repro-memo-id-audit3.rs`, 52 cases)
Each case asserts today's behaviour of a third-review finding (F19, F20, F22, F27, F29, F30), so a fix shows up as a deliberate failure. In 0.2.0 the F19 (S21) and F20 (S17) cases assert the fixed behaviour, and cases made obsolete by the fixes were removed: 52/0 on a fresh validator (64/0 for 0.1.0); on a used ledger the five F27 cases skip, which fails the run unless skips are allowed. It uses a throwaway HOME and keypair for the admin client and deletes them afterwards.

### Mutation testing (third review)
71 mutants of `programs/memo-id/src/lib.rs`, one edit each (removed checks, flipped comparisons, `Signer` and `Program` types loosened, wrong seeds, sizes, prices, stamps, events), each built and run on fresh validators at both supplies with `run-regression-tests.sh --so-dir`. The unmutated build in the scratch copy was byte-identical to `target/deploy/memo_id.so`.

| | Mutants |
|---|---|
| Equivalent (identical binary, or no observable difference) | 7: M14 (off-chain `forward`; the unit tests catch it), M15, M40, M53, M54, M55, M56 |
| Caught by the 170-case suite | 53 of 64 (M08, M09 and M47 only at 2.5T) |
| Caught after 11 more cases (R50–R54, S22, N30, P15, U09, U10, I04) | 64 of 64 |

The 111-case suite missed, among others, the removal of `create_subid`'s `HashMismatch`, both `WrongParent` constraints, every `Signer` check and `issue_subid`'s URI check (F38).

Version 0.2.0 (2026-11-03): 14 mutants of the changes (each removed listing check, the holder membership check, the both-or-neither rule, each recorded holder field, each cleared field, swapped target kinds, kind-blind Primary ID seeds, `has_target` not set), run the same way. The 200-case suite caught 12; M05 (`create_subid` with a target does not record the holder) and M08 (an Address Primary ID records the holder) survived and are caught by V17 and V18. The unmutated build passed 200/0 at both supplies, and 202/0 with V17 and V18.

### Stateful fuzzing (`clients/regression/src/fuzz-memo-id.rs`)
`fuzz-memo-id --seed <n> --steps <n> [--edge]` drives four wallets through random sequences of every instruction plus Core transfers, memo-home withdrawals and pre-funding, with deliberately wrong variants (signers, holders, parents, Assets, indices, labels, URIs, memos, too little MEMO). Before each step a model predicts success or the exact error, raising program, error name and instruction index; after each step it compares every Registry byte for byte, Primary IDs, SubId Index entries, Vault States, lamports, Asset owners, names and URIs, MEMO balances, supply, burn statistics and events, and recomputes the counters and the name–Registry–Asset links from the chain alone.

| Run | Steps | Mismatches |
|---|---|---|
| Seeds 1–3, supply 5M | 3,000 | 0 |
| Seeds 4–5, supply 2.5T | 2,000 | 0 |
| Seeds 6–8, `--edge` (253-byte names, long nesting) | 3,000 | 0 (1 step skipped: over 1,232 bytes, F17) |

Version 0.2.0 (2026-11-03): the model adds holder Collection Configs (passed, missing, wrong), Core Freeze and Transfer delegates (add, approve, revoke; a transfer resets an owner-managed delegate), the listing gate, memo-home's refusal to issue an Asset to itself or its own Vault, the fixed layouts and their canonical encoding, and checks that every pair verified from accounts readable by address is also verified by the holder rule.

| Run (0.2.0) | Steps | Mismatches |
|---|---|---|
| Seeds 1–4, supply 5M | 4,000 | 0 |
| Seeds 7–8, supply 2.5T | 2,000 | 0 |
| Seeds 5–6, `--edge` | 2,000 | 0 |

Third review: 98 distinct (operation, outcome) pairs were reached. Against 17 non-equivalent mutants the fuzzer stopped at the first mismatch within 5–416 steps for all 17; on two equivalent mutants it ran 400 steps with no false alarm. Seed 1 re-ran identically.

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

The first medium finding (F01) is a marketplace concern with an exact, cheap check, like memo-home's Vault State nonce. The adversarial second review found no critical, high or medium issue; its two program findings (F13, F14) are fixed and covered by regression cases. Deployed to testnet on 2026-10-27 and upgraded with the fixes on 2026-10-28; the smoke test passes.

The third review found no critical or high issue. Its medium finding (F19) and the holder finding (F20) were program issues with small, local fixes; with F25, F31 and F32 they are fixed in version 0.2.0. The rest are integration rules, documentation, tooling and optional hardening (F21–F24, F26–F28 open). A review of the 0.2.0 changes found no program defect, every mutant of the changes is caught, and 8,000 stateful fuzz steps matched the model exactly. **Next: 0.2.0 on testnet as a new program; mainnet after the mainnet checklist.**
