# Memo-Home Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-home (`programs/memo-home`)
**Audit Dates**: October 20, 2026 (first review of v1); October 20, 2026 (second, exhaustive review and fixes); November 2, 2026 (re-audit and 0.2.0, see "Re-audit")
**Auditor**: Pre-production security review. The second review was a multi-agent audit: 11 review dimensions (custody, mining, Core parsing, Core CPI and direct-Core paths, Anchor accounts, Token-2022 extensions, griefing, app composability, test adequacy, docs and deployment, free-form attacker), two adversarial verifiers per finding (one trying to refute it, one assessing impact with a reproduction), and two follow-up rounds from a completeness critic. Reproductions ran in LiteSVM against copies of the X1 mainnet Metaplex Core bytecode; every fix is covered by the local-validator regression suite.
**Version**: 0.2.0, 12 instructions (0.1.0 with 11 instructions deployed 2026-10-20, 253-byte names deployed 2026-10-22 at `85832fd8`). Program IDs: testnet `J1pxwy8V4xKvPgS5HMUn5jFo3u7At3MW4BvFWKg48Qcv`, mainnet `ArrrsGh5biKJYpiKMYJ6FD3AxCY7Fhd9Gv761ZCymzzs` (both deployed 2026-10-20, upgradeable)
**Language**: Rust (Anchor 0.32.1)
**Network**: X1 (SVM-based)
**Design reference**: `MEMO-HOME-DESIGN.md`

### Overall Assessment

**Risk Level**: ✅ **LOW** after the fixes in this report. Ready for testnet deployment; mainnet after the checklist below.

The second review found one high-severity issue that the first review and its regression suite had missed: **a Collection's keypair was a hidden master key** (F01). Metaplex Core treats a Collection's own address as the update authority of every Asset in it, so whoever kept the keypair used in `create_collection` could rename any Asset, take it out of the Collection (permanently locking its Vault and lifting the Burn Guard) or move it into a collection they control. Collection and Asset addresses are now memo-home PDAs, for which no private key exists.

Other changes from this review:
- `create_collection` requires the Authority's signature.
- Each Asset has a **Vault State** whose `nonce` grows with every owner outflow. A market or bidder can now require that nothing left the Vault since listing, in every listing style, which addresses the listing limitation accepted in the first review (M-1).
- Names and URIs are limited to 64 and 256 bytes (names raised to 253 bytes on 2026-10-22, `85832fd8`, for MEMO ID).
- The Vault's MEMO token account can no longer be closed.
- Parser hardening, self-transfer and owner checks, and a separate payer for Core and memo-burn fees.
- A compile-time network guard, `mine` return data and withdraw-all.
- About 80 new regression cases, including an app program that calls memo-home by CPI.

### Summary Statistics (second review)

67 findings after deduplication: 51 confirmed, 14 disputed (the verifiers disagreed, usually on severity or on whether a documentation gap is a finding), 2 refuted. Severities below are the final ones after verification.

| Severity | Count | Status |
|---|---|---|
| Critical | 0 | — |
| High | 1 | Fixed (F01) |
| Medium | 3 | Fixed (F02, F04); mitigated by the Vault State and documented (F59) |
| Low | 20 | 8 fixed, 1 mitigated, 5 documented, 6 deployment-checklist items |
| Informational | 43 | Fixed, documented or checklist; 2 refuted, 1 not adopted (F40), 1 not applicable (F56) |

Tests after the fixes: 41 unit tests (`programs/memo-home/src/tests.rs`) and 160 local-validator regression cases (`clients/regression/src/regression-memo-home.rs`). After the re-audit (0.2.0): 43 unit tests and 198 regression cases.

---

## Contract Overview

### Purpose

memo-home is a primitive like memo-mint and memo-burn. It covers three things, and apps (chat, blog, MEMO ID, MEMO Web, consoles) build on it by CPI:
- **identity**: Core Collections and Assets;
- **custody**: one Vault per Asset;
- **routing**: a bounded share of freshly mined MEMO.

It has no admin key, no fees, no app data and no memo parsing.

### Instructions

| Instruction | Signer | Effect |
|---|---|---|
| `init_burn_guard` | anyone, once | creates the global Burn Guard Oracle account with fixed content (burn Rejected, everything else Pass) |
| `create_collection(seed, name, uri, max_share_bps, max_assets?, royalty?)` | Authority (+ payer) | Core `CreateCollectionV2` at the PDA `[b"collection", authority, seed]`, signed by that PDA. It sets memo-home's Program Authority as update authority and adds the Burn Guard Oracle, plus the optional MasterEdition (`max_supply`) and Royalties plugins, all with plugin authority None. It also creates the Collection Config. `authority`, `max_share_bps` and `max_assets` are immutable |
| `update_collection(name?, uri?)` | Authority | Core `UpdateCollectionV1` (name/URI only) |
| `create_asset(seed, name, uri)` + `owner` account | Authority (+ payer) | Rejects at `max_assets`, or when `owner` is the Core program, the new Asset or its own Vault (0.2.0). Then Core `CreateV1` at the PDA `[b"asset", collection, seed]`, signed by that PDA and the Program Authority, and creates the Vault State |
| `update_asset(name?, uri?)` | Authority | Core `UpdateV1` (name/URI only, update authority unchanged) |
| `advance_nonce` (0.2.0) | Authority | adds 1 to the Asset's Vault State `nonce` when the app changes state bound to the Asset; event `NonceAdvanced` |
| `mine(share_bps)` → `MineResult { reward, share }` | Authority (+ any caller) | Creates the Vault MEMO token account if missing and runs memo-mint `process_mint_to` into the Vault. It then pays `reward × share_bps / 10000` (rounded down, `share_bps ≤ max_share_bps`) to a MEMO token account other than the Vault's |
| `withdraw(amount)` | current owner, not listed | any SPL Token / Token-2022 token out of the Vault (`u64::MAX` = whole balance); transfer-hook extra accounts forwarded as non-signers |
| `withdraw_lamports(amount)` | current owner, not listed | native XNT out of the Vault, leaving 0 or at least the rent-exempt minimum |
| `close_token_account` | current owner, not listed | closes a Vault token account with token amount 0, other than the MEMO one; all its lamports to any address |
| `withdraw_asset` | current owner (+ payer), not listed | Core `TransferV1` of a Core Asset owned by the Vault; plugin accounts forwarded as non-signers |
| `burn(amount)` | current owner (+ payer), not listed | memo-burn `process_burn` from the Vault, the Vault signing as the user; first use initializes the Vault's `UserGlobalBurnStats` (the payer tops up only the missing rent, 0.2.0) |

Every owner outflow adds 1 to the Vault State `nonce` and rejects a destination equal to its source (since 0.2.0 also for `withdraw_asset`, and for `withdraw` to another token account of the same Vault).

### Accounts

| Account | Seeds | Notes |
|---|---|---|
| Program Authority | `[b"authority"]` | Core update authority of every memo-home Collection; signs only Core `CreateV1`, `UpdateV1` and `UpdateCollectionV1` (unit test `signed_core_instructions_are_fixed`) |
| Burn Guard | `[b"burn_guard"]` | `OracleValidation::V1 { Pass, Pass, Rejected, Pass }` after the discriminator, plus bump (14 bytes) |
| Core Collection | `[b"collection", authority, seed]` | created by Core; memo-home signs with these seeds only inside `create_collection` |
| Collection Config | `[b"collection_config", collection]` | collection, authority, seed, max_share_bps, max_assets, creator, created_at, bump (152 bytes) |
| Core Asset | `[b"asset", collection, seed]` | created by Core; memo-home signs with these seeds only inside `create_asset` |
| Vault | `[b"vault", asset]` | system account; owns token accounts and Core Assets; signs only token transfers/closes, System transfers, Core `TransferV1` and memo-burn |
| Vault State | `[b"vault_state", asset]` | `nonce` (owner outflows and `advance_nonce`), bump (17 bytes); created with the Asset, never closed |

### Compute units (0.2.0, local validator and LiteSVM, X1 mainnet Core / Token-2022 / SPL Token / ATA builds)

| Instruction | CU (memo-home instruction, including its CPIs) |
|---|---|
| `create_collection` | 30.1k |
| `create_asset` | 34.1k (PDA address, Vault State creation) |
| `mine` | 31–37k (51–68k when it creates the Vault MEMO token account); memo-mint's share depends on the supply tier |
| `withdraw` / `withdraw_lamports` / `close_token_account` | 15.3k / 11.1k / 14.5–17.5k |
| `burn` | 36.1k (51.0k with the burn-stats initialization) |
| `advance_nonce` | 6.4k |

0.1.0 figures and the comparison: see "Re-audit".

---

## Re-audit (November 2, 2026)

**Scope**: `programs/memo-home` at commit `85832fd8` (0.1.0 with 253-byte names, the build deployed on both networks), its unit tests, the regression suite and its two test programs, the smoke test, this report, `MEMO-HOME-DESIGN.md`, and how memo-id and future apps call it by CPI. It followed the memo-mint (Revision 5) and memo-burn (Revision 4) re-audits, to the same depth, so that the three core programs are reviewed together before locking.

**Method**: independent review lenses (Vault custody, mining, Core parsing and Core CPI, Anchor accounts, Token-2022 extensions, griefing and DoS, app composability, compute units, tests, docs and deployment, free-form attacker). Every finding was checked by two reviewers who tried to refute it, one on correctness and one on impact, and a completeness critic looked for areas no lens covered. Reproductions ran in LiteSVM 0.13.1 with X1 mainnet's Metaplex Core, Token-2022, SPL Token, ATA and Memo dumps, and on the local validator.

**Result**: no critical, high or medium findings. 40 findings survived verification; after merging duplicates, 17 low and 9 informational remain (HOME-R01 to HOME-R26, below). The owner accepted every recommended change on November 2, 2026, and memo-home 0.2.0 implements them.

**Verified properties** (selection):
- Both deployed binaries reproduce bit for bit from `85832fd8`: testnet `b111033a…` and mainnet `1db77c55…` (`deployments/*.sha256`). That upgrade (253-byte names) was deployed to both networks but not recorded here before (HOME-R24).
- Every owner outflow re-reads the Core Asset (owner, Collection, listing) at the time it runs; no stale ownership or listing state exists.
- The Vault PDA signs only Token / Token-2022 transfers and closes, System transfers, Core `TransferV1` and memo-burn; forwarded remaining accounts never get a signature.
- The Program Authority signs only Core `CreateV1`, `UpdateV1` and `UpdateCollectionV1`, with the update authority unchanged.
- On X1's Core, only the Collection's update authority (memo-home's Program Authority, which never signs it) can add external plugin adapters or permanent delegates to a memo-home Asset, and a foreign Asset cannot join a memo-home Collection. These are now pinned by regression cases (HOME-R15).
- Every account memo-home passes to memo-mint and memo-burn without its own check is checked there; substitutions through `mine` and `burn` now fail in regression (HOME-R16).

### Findings

| ID | Severity | Status | Title |
|---|---|---|---|
| HOME-R01 | Low | Fixed in 0.2.0 | `withdraw_asset` accepted the Vault itself as new owner; `withdraw` accepted another Vault token account |
| HOME-R02 | Low | Fixed in 0.2.0 | `create_asset` accepted the new Asset or its own Vault as owner |
| HOME-R03 | Low | Fixed in 0.2.0 | An owner could orphan its own Vault with a frozen FreezeDelegate held by nobody |
| HOME-R04 | Low | Fixed in 0.2.0 | `mine` called ATA `CreateIdempotent` on every mine |
| HOME-R05 | Low | Fixed in 0.2.0 | Core parsers built an Anchor error on every read |
| HOME-R06 | Info | Fixed in 0.2.0 | Program Authority and Burn Guard re-derived on every call |
| HOME-R07 | Low | Fixed in 0.2.0 | The first Vault burn took an extra rent minimum from the payer |
| HOME-R08 | Low | Fixed in 0.2.0 | The Vault State nonce could not cover app state bound to an Asset |
| HOME-R09 | Info | Documented | A `mine` transaction leaves room for only about 450 memo bytes |
| HOME-R10 | Info | Corrected | Design §5.3 said nested Vaults need their own nonce checks |
| HOME-R11 | Info | Documented | CPI depth of `withdraw` with a transfer hook |
| HOME-R12 | Low | Documented | `close_token_account` moves all lamports, not only the rent |
| HOME-R13 | Low | Documented | Lamports above rent in the Vault MEMO token account are locked |
| HOME-R14 | Info | Documented | Withheld transfer fees block `close_token_account` |
| HOME-R15 | Low | Fixed (tests) | No regression case pinned the Core rejections the Burn Guard and membership rely on |
| HOME-R16 | Low | Fixed (tests) | No regression case substituted the accounts passed to memo-mint and memo-burn |
| HOME-R17 | Low | Open | No test runs a Vault burn through an app CPI or with a program-owned owner |
| HOME-R18 | Low | Open | No regression case pins Token-2022's signer de-escalation for a hostile transfer hook |
| HOME-R19 | Low | Open | Token-2022 extension behavior described for Vaults is untested beyond transfer hooks |
| HOME-R20 | Info | Open | No regression bound on compute units or on the largest transaction that fits |
| HOME-R21 | Info | Partly fixed | Instruction paths and error codes no case exercised |
| HOME-R22 | Info | Corrected | F63's rationale was wrong |
| HOME-R23 | Info | Recorded (F50) | X1's Core matches no upstream release; the UpdateV1 shrink fix is unknown |
| HOME-R24 | Low | Corrected | Stale records: 64-byte name limit, deployment hashes, test counts |
| HOME-R25 | Low | Recorded | HEAD no longer builds the deployed memo-home (memo-mint and memo-burn changed) |
| HOME-R26 | Info | Updated | Compute-unit figures predated memo-mint Revision 5 and memo-burn Revision 4 |

#### 🟡 HOME-R01 (Low, fixed): self-destinations in `withdraw_asset` and `withdraw`

The design and F30 say that every owner outflow rejects a destination equal to its source. `withdraw_asset` did not compare `new_owner` with the Vault, and Core's `TransferV1` has no self-transfer check, so the call succeeded: nothing moved, the nonce rose by 1 and `AssetWithdrawn { new_owner: vault }` was logged. `withdraw` had the same gap for a destination token account owned by the same Vault. Funds were never at risk (only the owner can call either, and a higher nonce only cancels the owner's own listings), but an indexer that trusted the event would record an outflow that never happened. **0.2.0**: both reject with `InvalidDestination` (regression H62b, H62c, H39c).

#### 🟡 HOME-R02 (Low, fixed): self-locking owners in `create_asset`

`create_asset` accepted the Asset's own address or its own Vault as `owner`. Such an Asset is locked from birth: its owner is a PDA memo-home never signs for as an owner, so neither the Asset nor anything mined into its Vault could ever leave. memo-id's `issue_subid` passes a caller-chosen recipient to `create_asset`. **0.2.0**: both are rejected with `InvalidOwner` (H11i, H11j).

#### 🟡 HOME-R03 (Low, fixed): an owner could orphan its own Vault

Any delegate whose authority was not Owner counted as listed, and a frozen FreezeDelegate always did. An owner could add a FreezeDelegate with authority None (or UpdateAuthority), freeze it, and never be able to thaw it: memo-home would then treat the Asset as listed forever and reject every outflow, while `mine` kept paying into the Vault. This contradicted invariant 4. **0.2.0**: only a delegate a third party holds (plugin authority `Address`) counts as listed; permanent delegates and unknown values still fail closed. A frozen Asset still cannot be transferred, but its owner can empty the Vault (H38b, H38d, H38e; unit tests `third_party_delegate_after_unlisted_ones_is_found` and the updated listing tests). Every market style in design §5.3 uses an `Address` delegate or escrow, so listing protection is unchanged.

#### 🟡 HOME-R04, HOME-R05 (Low, fixed) and HOME-R06 (Info, fixed): compute units

- `mine` invoked ATA `CreateIdempotent` on every call (8.6k–17.6k CU). It now does so only when the Vault MEMO token account is not yet owned by Token-2022 (H21b, H22b).
- The Core parsers built an Anchor error value (with its file and line) on every byte they read and then dropped it (`ok_or` instead of `ok_or_else`): about 8.5k CU per owner outflow and 4.3k per `mine`.
- The Program Authority and Burn Guard PDAs were found by `find_program_address` on every call. They are now address constants per network (`PROGRAM_AUTHORITY_PDA` with its bump, `BURN_GUARD_PDA`), checked by `address = … @ ConstraintSeeds`, pinned by the unit test `test_pda_constants_per_network`; substitutions are rejected in H11k–H11m. memo-id benefits through its CPIs.

See the table under "Compute units" for the measured effect.

#### 🟡 HOME-R07 (Low, fixed): first Vault burn top-up

On the first Vault burn the payer topped the Vault up to the stats rent plus the Vault's own rent minimum (0.00089 XNT), which the owner could withdraw at once. **0.2.0**: the payer pays only what is missing: the stats rent when the Vault is empty (it ends at 0), otherwise the stats rent plus the Vault's rent minimum, less what it holds (H50, H55).

#### 🟡 HOME-R08 (Low, fixed): app state and the nonce

Apps on memo-home keep value outside the Vault (room settings, moderators, SubIds, files). A seller can change it before a sale without any outflow, so the nonce a buyer recorded stays the same. VaultState has no spare bytes, so this could only be addressed before locking. **0.2.0** adds `advance_nonce`, signed by the Collection's Authority: it adds 1 to the Asset's nonce and emits `NonceAdvanced { asset, nonce }`. Apps call it whenever they change state bound to an Asset (design §5.3, §6.1). It moves nothing and needs no listing check (H140–H142b).

#### 🟡 HOME-R12, HOME-R13, HOME-R14 (documented)

- `close_token_account` checks only that the token amount is 0. Lamports sent straight to a token account (or to wrapped XNT without `SyncNative`) leave with the rent. The owner signs and the nonce rises, so custody holds; the code comment and the design now say "all its lamports".
- The Vault MEMO token account cannot be closed (F02), and memo-home has no `WithdrawExcessLamports`, so lamports above its rent stay there for good; the same holds for lamports sent to memo-home PDAs. Design §5.9 now says never to send XNT there. Adding an owner-gated `withdraw_excess_lamports` before locking remains possible; the owner did not request it.
- Token-2022 refuses to close an account with withheld transfer fees; the permissionless `HarvestWithheldTokensToMint` must run first. Design §4.3 and §5.9 and the code comment now say so.

#### 🟡 HOME-R15, HOME-R16 (Low, fixed in tests)

New regression cases:
- **Core rejections**: the owner's `AddPluginV1` of PermanentBurn, PermanentTransfer and PermanentFreeze delegates, UpdateDelegate, Attributes and AddBlocker on a memo-home Asset; `AddExternalPluginAdapterV1` (Oracle by the owner and by a stranger, AppData by the owner); a stranger's `UpdateV2` moving their own Asset into a memo-home Collection. Each is rejected by Core, and the Asset's data and the Collection count are unchanged (H40f–H40q). After an X1 Core upgrade, these cases show whether the Burn Guard and membership assumptions still hold.
- **Dependency accounts**: `mine` with the shadow MintState or a wrong mint authority, `burn` with another user's stats, an empty non-PDA account as the Vault stats (memo-home's initialization branch) and the shadow GlobalBurnStats. Each is rejected by memo-mint or memo-burn (`ConstraintSeeds`) and nothing changes (H27e, H27f, H54b–H54d).

#### Open items (HOME-R17 to HOME-R21)

These are test gaps; the behavior was checked by reading the code and, where noted in the findings file, in LiteSVM:
- **R17**: a Vault burn through an app CPI, and with a program-owned Asset owner (the case the separate payer of F31 serves). Needs a burn path in the `home-app` test program.
- **R18**: a hostile transfer hook that tries to use the Vault's signature. Token-2022 de-escalates the source owner to a non-signer before invoking the hook; Vault custody against such hooks depends on that.
- **R19**: transfer-fee, MemoTransfer, non-transferable, permanent-delegate and default-frozen mints in a Vault, and a hook that makes its own CPI (depth).
- **R20**: upper bounds on CU per instruction and maximum-size `mine` and Vault-burn transactions (memo-burn has the same gap, BURN-R15).
- **R21**: fixed: `withdraw_asset` to the Vault (H62b), a burn with another Asset's Vault State (H54e), `update_asset` with a 257-byte URI (H15e), `update_collection` with a 254-byte name (H18g). Still open: `close_token_account` with the closed account as destination; error codes never asserted on-chain (`NotHomeCollection`, `InvalidCoreAccount`, `ArithmeticOverflow`, `InvalidVaultState`). `AddressInUse` is defense in depth: the System program's "account in use" fires first on every path the suite can reach.

#### HOME-R22 (Info, corrected): F63 rationale

F63 said external plugin adapters are ignored because anyone can attach a data section to an Asset on X1's Core. That is wrong: Core rejects `DataSection` outright, and Oracle, LifecycleHook and AppData adapters on an Asset need its update authority, which for a memo-home Asset is the Program Authority (it never signs that). The external registry of a memo-home Asset is therefore always empty, and ignoring it is safe; H40l–H40n pin this.

#### HOME-R23 (Info, recorded): X1's Core build

Strings in the pinned `mpl_core.so` (`e471b0c9…`) show features of upstream 0.10.0 and the 0.12.0 FreezeExecute hardening, but none of the Groups strings of 0.11.0: it is a custom or backported build. Whether it contains upstream #285 (0.15.1, `UpdateV1` reallocating before the memmove when a name or URI shrinks on an Asset with plugins) cannot be told from strings. `update_asset` takes that path when the owner has added a delegate. In LiteSVM a 253-to-1-byte rename of such an Asset kept the plugin registry intact. Added to the F50 checklist item.

#### HOME-R24, HOME-R25 (corrected and recorded)

- The summary and design gave a 64-byte name limit; the code allows 253 since `85832fd8`. The deployment records named mainnet `7be70db1…` and 160 regression cases. Corrected below.
- With memo-mint 0.5.0 and memo-burn 0.3.0 in the workspace, the HEAD build of memo-home 0.1.0 already differed from the deployed bytecode, although memo-home's own source had not changed: memo-home links both crates for its CPIs. The behavior is equivalent. A deploy script run without a program name would have upgraded memo-home too; always name the program. memo-id embeds memo-home and is rebuilt and redeployed with it.

### Revision 0.2.0 changes

- `withdraw_asset` rejects `new_owner == vault`; `withdraw` rejects a destination owned by the Vault (`InvalidDestination`).
- `create_asset` rejects `owner` equal to the Asset or its Vault (`InvalidOwner`).
- Listing rule: only third-party (`Address`) delegates count; `require_owner_not_listed` parses the Asset once.
- `mine` skips ATA `CreateIdempotent` once the Vault MEMO token account exists.
- Core parsers build errors lazily (`ok_or_else`).
- `PROGRAM_AUTHORITY_PDA`, `PROGRAM_AUTHORITY_BUMP` and `BURN_GUARD_PDA` are address constants per network.
- First Vault burn: the payer tops up only what is missing.
- New instruction `advance_nonce` (Authority) and event `NonceAdvanced`.
- No account layout changed: Collection Config, Vault State and Burn Guard are as in 0.1.0, so existing accounts keep working.

### Compute units, 0.1.0 → 0.2.0

LiteSVM 0.13.1 with X1 mainnet's Core, Token-2022, ATA and Memo builds and the same memo-mint build; the same transactions run against both memo-home binaries. memo-home's figure includes its CPIs. The two values are two Asset seeds (PDA bump searches differ).

| Instruction | memo-home 0.1.0 | memo-home 0.2.0 | Change |
|---|---|---|---|
| `mine` (Vault token account exists, 30% share) | 53,065 / 44,065 | 34,184 / 31,184 | −36% / −29% |
| `mine`, whole transaction (memo of 80 bytes, memo-mint, Token-2022) | 79,252 / 70,252 | 60,371 / 57,371 | −24% / −18% |
| `mine` (first, creates the Vault token account) | 64,500 / 55,500 | 60,269 / 51,269 | −7% / −8% |
| `withdraw` | 24,067 | 15,284 | −37% |
| `withdraw_lamports` | 19,882 | 11,107 | −44% |
| `close_token_account` | 26,238 / 23,238 | 17,463 / 14,463 | −33% / −38% |

Local validator (regression suite, one run each, random keys): `create_collection` 37.7k → 30.1k, `create_asset` 41–44k → 34.1k (it now derives `vault_of(asset)` once, about 3k, for HOME-R02), Vault `burn` 52.7k → 36.1k, first Vault `burn` 69.5k → 51.0k, `advance_nonce` 6.4k.

The size check confirmed HOME-R09: a legacy `[memo, mine, SetComputeUnitLimit]` transaction with two signers is 1,232 bytes with a 450-byte memo and does not fit at 480.

---

## Findings of the Second Review

### 🔴 F01 (High, fixed): the Collection keypair was a hidden master key

**Where**: `CreateCollection.collection` was a `Signer` (a fresh keypair held by the caller of `create_collection`). Core's `AssetV1::validate_update` approves an update when the signer equals `update_authority.key()`, and for `UpdateAuthority::Collection(c)` that key is the Collection's own address. Upstream mpl-core keeps and tests this behavior (F53), so a Core upgrade would not remove it.

**Scenario**: reproduced twice against the X1 Core bytecode. The holder of the Collection keypair (creator, app operator, or anyone who obtained the key file) could:
- rename or re-point any Asset, bypassing the Authority-only `update_asset`;
- take an Asset out of the Collection with `UpdateV2 { new_update_authority: Address }`. All five owner outflows then fail with `WrongCollection`, so the Vault is locked forever. The Burn Guard no longer applies and the owner can burn the Asset;
- move an Asset into a collection whose update authority is the old Collection address, which Core creates without that address's signature, and attach a transfer-rejecting Oracle (hostage).

This broke invariants 1, 4, 5 and 6. It was not direct theft, but it froze every Vault in a Collection, and it was present in every Collection made through the only supported path.

**Fix**: Collection addresses are PDAs `[b"collection", authority, seed]`, and memo-home signs with those seeds only for `CreateCollectionV2` (F51). Asset addresses are PDAs too (F64). Because anyone can pre-fund a PDA, and Core's System `create_account` then fails, both create instructions first move any lamports at the address to the payer (F09). The seeds bind the signing Authority, so nobody can claim another Authority's addresses.

**Regression**:
- Core behavior: H19 and H19b (controls showing the Collection-address signer on this Core build).
- Core `UpdateV1` and `UpdateV2` on a memo-home Asset signed by the creator, the Authority or a stranger are rejected, and the Asset is unchanged: H19c–H19g.
- Addresses: PDA and off curve (H03, H11); bound to the Authority (H09b); pre-funded addresses (H09d, H11e); reused seeds (H09c, H11c); a non-PDA address (H11d).

### 🟠 F02 (Medium, fixed): an owner could collect the Vault MEMO token-account rent from every mine caller

`mine` creates the Vault's MEMO token account idempotently at the caller's expense, and `close_token_account` let the owner close it and take the rent. A bot could withdraw and close after every `Mined` event, earning about 0.002 XNT from each app user's next message. **Fix**: `close_token_account` rejects the Vault's MEMO ATA (`MemoAccountNotClosable`), so it is created once per Asset and its rent stays with the Asset. H113, H113b.

### 🟠 F04 (Medium, fixed): unbounded names and URIs

An Authority could write names or URIs large enough to push Core past its 32 KiB heap on every later instruction that deserializes the Asset or Collection, which blocked transfers in the whole Collection. Large strings also raise rent and break wallet rendering. **Fix**: `MAX_NAME_LEN = 64`, `MAX_URI_LEN = 256` (bytes), checked in all four create/update instructions. H06d, H06e, H11f, H11g, H15d, H18d. H11h shows that an Asset at the maximum lengths still transfers. *Update 2026-10-21*: `MAX_NAME_LEN` raised to 253 (the DNS hostname limit) so that every MEMO ID fits as an Asset name; 253 + 256 bytes stay far below Core's heap, and H11h passes at the new maximum.

### 🟠 F59 (Medium, mitigated): listings did not protect buyers

In one transaction a seller can end any listing, empty the Vault and list again at the same address and price:
- escrow and frozen listings, through the market's own delist;
- unfrozen delegate listings, by revoking the delegate (first review M-1).

Offers, bids and pool trades are filled by a transaction the seller builds (F60). memo-home cannot see listings, and Core transfers bypass it. **Mitigation**: the Vault State nonce (F62). A market or bidder records `vault_nonce_of(...)` and requires it unchanged at the sale, so any outflow in between is detected (H80b). The design (§5.1 invariant 3, §5.3) now says what listings do and do not protect, and how to value Vault contents (F61).

### 🟡 Low findings

| ID | Finding | Resolution |
|---|---|---|
| F03 | `create_collection` recorded any address as Authority without its signature, so an app's PDA could be named in Collections it never created | **Fixed**: the Authority signs (an app PDA by CPI); H09, H120, H131 |
| F05 | `deploy-common.sh` expects `AUTHORIZED_MINT_PUBKEY` in every program, so a deploy that includes memo_home aborts | **Fixed**: memo_home's mint is checked as `memo_mint::AUTHORIZED_MINT_PUBKEY` with the mainnet cascade |
| F07 | A destination or share recipient that requires incoming memos (Token-2022 MemoTransfer) makes `withdraw` / `mine` fail | **Documented** (design §5.9): unsupported; apps fall back to share 0 |
| F08 | `create_asset` with owner = Core program ID gave the Asset to the payer | **Fixed**: `InvalidOwner`; H11b |
| F10 | Transfer-hook mints whose extra accounts need a signer can never leave a Vault (forwarded accounts are non-signers by design) | **Documented** (§5.9) |
| F11 | Tokens sent to a nested ATA (an ATA of a Vault token account) are lost: there is no `RecoverNested` | **Documented** (§5.9) |
| F12 | The design claimed "nothing sent to a Vault is ever stuck" | **Corrected**: soulbound and other unsupported types are listed (§2, §5.9) |
| F14 | An app could build memo-home for another network than memo-mint/memo-burn, and the helpers then return wrong-network PDAs | **Fixed**: compile-time assertion on memo-mint, memo-burn and MEMO mint IDs per feature (fails with E0080); unit test `constants` per network |
| F15 | The Anchor IDL account keeps an authority that can rewrite the published interface after locking, and IDL creation is first-come | **Checklist** |
| F20 | Rejections enforced by Core accepted any error and had no positive controls | **Fixed**: exact Core codes (`InvalidAuthority` 9, `NoApprovals` 26, `MissingExternalPluginAdapterAccount` 34) in H14–H14c, H40–H40c, H81–H81d; controls H14d, H14e, H19, H19b |
| F25 | The placeholder program ID is the key of a plaintext build keypair, shared by both networks | **Checklist**: real per-network keys from `keys.sh` |
| F27 | Helper-crate docs were incomplete; integration requirements (owner-checked Config, `mainnet` cascade) were missing | **Fixed**: design §4.4 helper table and "Integrating" |
| F28 | The deployment checklist missed tooling a memo_home deploy needs | **Fixed**: deploy tooling, Burn Guard client, verification and smoke test (checklist) |
| F55 | A wallet owner's signature on any program can empty all their Vaults by CPI, and wallet previews do not show it | **Documented** (design §5.8) with the front-end rule. A top-level-only owner check was rejected: it would not stop a malicious program from transferring the Asset itself, and it would break program-owned Assets |
| F57 | No lock order between memo-home and memo-mint/memo-burn | **Checklist**: lock memo-mint and memo-burn before memo-home (design §5.1 invariant 6) |
| F58 | (disputed) The memo-mint/memo-burn versions memo-home depends on are not pinned by the deploy tooling | **Checklist**: compare their ProgramData hashes before deploying or locking memo-home |
| F60 | (disputed) Offers, bids, AMM and loan fills are built by the seller | **Mitigated** by the Vault State nonce; design §5.3 |
| F62 | memo-home had no Vault-state commitment, and one cannot be added after locking | **Fixed**: Vault State `[b"vault_state", asset]`, created with the Asset, `nonce` +1 on every owner outflow, reported in the outflow events, read with `vault_nonce_of`. Tests: H11, H21 (mining leaves it unchanged), H30 (and other outflow cases), H33b (a failed outflow does not change it), H80b, H129b |
| F63 | (disputed) The listing parser failed open on Core values it did not know | **Fixed**: a plugin type above 18 (Groups) or an unknown authority tag counts as listed; unit test `unknown_values_fail_closed`. External plugin adapters stay ignored. *Corrected 2026-11-02 (HOME-R22)*: the original reason (anyone can attach a data section) was wrong; on X1's Core only the Program Authority could add an external adapter to a memo-home Asset and it never does, so the external registry is always empty (H40l–H40n) |
| F64 | An Asset keypair was a lifelong key for the NFT's address: its generator controlled tokens sent to that address and could sign "as the NFT" in other programs | **Fixed**: Asset addresses are PDAs `[b"asset", collection, seed]` |

### ℹ️ Informational findings

| ID | Finding | Resolution |
|---|---|---|
| F06 | (refuted) `is_listed` ignores FreezeExecute / PermanentFreezeExecute | No change: FreezeExecute only guards Core's `ExecuteV1`, not transfers, and Core resets it on transfer. Pinned as "no listing effect" in unit tests |
| F09 | (disputed) Pre-funded Collection/Asset addresses can never be created by Core | **Fixed** with F01: lamports are swept to the payer first (H09d, H11e) |
| F13 | (disputed) `is_home_asset` trusted a `CollectionConfig` of unknown origin; the owner gate was private | **Fixed**: `is_home_asset` takes `&Account<CollectionConfig>`; `require_owner_not_listed` is exported; raw parsers are documented as trusted-bytes-only; unit tests `home_asset_and_owner_gate`, `config_must_be_owned_by_memo_home` |
| F16 | No test passed the correct Authority/Owner key unsigned | **Fixed**: H120–H129b |
| F17 | (disputed) The memo-burn program account was guarded only by its type, untested | **Fixed** (tests): H54, and H27d for memo-mint |
| F18 | The Program Authority check and "a Config only with a fresh Collection" were untested | **Fixed**: H08–H08c |
| F19 | No cross-app case | **Fixed**: a second app Authority; H17b, H17c, H18e, H18f, H26b |
| F21 | Unit fixtures reused the program's own Core constants | **Fixed**: fixtures use literal mpl-core values; `program_constants_match_core` |
| F22 | Exported helpers untested | **Fixed**: unit tests over `AccountInfo`s with Core, System and memo-home owners |
| F23 | `withdraw_asset` forwarding untested; hook tests skipped without an out-of-repo program | **Fixed**: `clients/regression/test-programs/` (transfer hook, test app) built by `run-regression-tests.sh`; H66, H66b; H89/H130 fail (not skip) when a program is missing |
| F24 | `max_assets` must use `num_minted`, untested | **Fixed**: `check_max_assets` unit test with `num_minted ≠ current_size` |
| F26 | The Core bytecode used by the suite was neither pinned nor refreshed | **Fixed**: `local-validator.sh` pins the sha256 of Core, Token-2022, SPL Token and ATA and warns on a mismatch; `--refresh-external`; the suite prints the loaded hashes |
| F29 | `close_token_account` closed non-empty wrapped-XNT accounts | **Fixed**: amount must be 0 (`TokenAccountNotEmpty`); H110, H112b |
| F30 | Self-transfers accepted, events reporting value that never moved | **Fixed**: `InvalidDestination` in `withdraw`, `withdraw_lamports`, `mine`; H29, H32b, H102c |
| F31 | The first Vault burn needed a System transfer from the owner, which fails for program-owned owners with data | **Fixed**: separate `payer` in `burn` (and `withdraw_asset`); H50, H55, H61 |
| F32 | The production CPI path (an app PDA as Authority) was untested | **Fixed**: test app; H131–H136 (create, mine through app → memo-home → memo-mint → Token-2022, program-signed owner outflow) |
| F33 | Raw parsers: unchecked offset addition, truncated `seq` accepted | **Fixed**: checked arithmetic, `seq` length checked; unit tests |
| F34 | (refuted) Shortening renames untested on X1's Core | No change |
| F35 | Plugin authority None does not stop the Collection's update authority (memo-home) from removing the Burn Guard | Holds because memo-home never signs such an instruction; pinned by `signed_core_instructions_are_fixed` |
| F36 | Duplicate royalty creators not rejected (Core rejects them later) | **Fixed**: H06c, unit test |
| F37 | Dust could block `close_token_account` | **Fixed**: `withdraw(u64::MAX)` = whole balance, so withdraw-all and close fit in one transaction; H112c |
| F38 | Invariants 1 and 3 overstated Vault isolation (token-level powers) | **Corrected** (design §5.1, §5.9) |
| F39 | All mines and Vault burns share the MEMO mint's write lock | Documented: inherent to memo-mint; apps keep `mine` separable from their core action |
| F40 | (disputed) Events are program logs (spoofable by naive indexers, truncatable) | **Not adopted**: `emit_cpi!` would add two accounts and a self-CPI to every instruction. Events are documented as hints, not proof (design §4.5); outflow events now include the nonce |
| F41 | (disputed) `mine` returned nothing | **Fixed**: returns `MineResult { reward, share }`; H133b |
| F42 | Instructions that call Core cannot run under Core's `ExecuteV1`; the MEMO ID depth count was off by one | **Corrected** (design §5.4, §5.6, §6.4) |
| F43 | First-burn lamport handling untested | **Fixed**: H50 (exact top-up), H55 (funded Vault) |
| F44 | Backup checks and error codes never pinned | **Fixed**: H27c, H34c, H54 |
| F45 | URI path, empty-argument checks, Config fields, events and on-chain rounding unexercised | **Fixed**: H03, H11, H15b, H15c, H23b, H30b, H102d |
| F46 | The wallet claim in §2 exceeded Appendix B; the production layout was never wallet-tested | **Done**: the deployed program's Assets tested in the X1 wallet (iOS, web) and Backpack (Appendix B) |
| F47 | The cost table called 0.00375 XNT "rent" (it includes Core's 0.0015 XNT fee) | **Corrected** (design §3, §7) |
| F48 | (disputed) Stale numbers | **Corrected** |
| F49 | Minor wording mismatches | **Corrected** |
| F50 | (disputed) The exact X1 Core version is unknown | **Checklist**: the tested bytecode is pinned by hash. Before mainnet, ask the X1 operators for the Core version, or record the residual risk in §9. HOME-R23: the build has 0.10.0 features and the 0.12.0 FreezeExecute hardening but not 0.11.0 Groups (a custom build); ask specifically about upstream #285 (`UpdateV1` shrink with plugins) |
| F51 | The F01 fix must bind a signer and a caller seed, and clear pre-funded lamports | **Done** as part of F01 |
| F52 | (disputed) Granting Vault access to Assets outside the Collection would widen trust | **Agreed**: not done; Vault access requires Collection membership. The Vault State's existence also marks memo-home provenance |
| F53 | An X1 Core upgrade will not fix F01 | Recorded (F01) |
| F54 | Design and audit statements invalidated by F01 | **Corrected** |
| F56 | (disputed) A top-level-only owner rule with `is_on_curve` would abort on-chain | Not applicable: no such rule was adopted (F55) |
| F61 | (disputed) A buyer-side check cannot bind a Vault completely without a commitment | **Documented** (§5.3); the nonce is the commitment |
| F65 | (disputed) XNT sent to an Asset address can be collected by anyone for Metaplex (Core `Collect`) | **Documented**: pay `vault_of(asset)`, never the Asset address (§4.4, §5.9) |
| F66 | The suite ran the test validator's bundled SPL Token / ATA instead of X1's builds | **Fixed**: X1 mainnet Token-2022, SPL Token (p-token) and ATA are loaded with pinned hashes. X1 testnet's Token-2022 build is not covered |
| F67 | security.txt misidentifies X1's Token-2022 version; no token-program version or hash recorded | **Recorded** below |

---

## Findings of the First Review (updated)

- **M-1** (Medium, accepted): unfrozen delegate listings can be emptied by revoke → withdraw → re-approve in one transaction (H80). **Now mitigated** by the Vault State nonce (H80b); see F59.
- **L-1** (Low, fixed): transfer-hook tokens could not leave a Vault. `withdraw` forwards the hook's extra accounts as non-signers (H90, H91).
- **L-2** (Low, fixed): XNT and token-account rent were locked in Vaults. `withdraw_lamports` and `close_token_account` were added. Since F02, the MEMO token account is excluded from closing.
- **I-1**: anyone can create a Core Collection naming the Program Authority as update authority. memo-home cannot adopt such a Collection (addresses are PDAs, H08b), and without a Config nothing can be created in it (H08c).
- **I-2**: `is_listed` is sound only for memo-home Assets (it reads the Asset's own plugin registry).
- **I-3**: Vault MEMO does not prove mining (`process_mint_to` is permissionless).
- **I-4**: self-nesting and cycles of Assets lock them forever (plain Core transfers).
- **I-5**: documentation and placeholder IDs; see the checklist.

---

## Security Analysis by Category

### 1. Vault custody ✅ SECURE
Every outflow (`withdraw`, `withdraw_lamports`, `close_token_account`, `withdraw_asset`, `burn`) runs `require_owner_not_listed`. It checks that the Asset account is owned by Core, parses as `AssetV1`, belongs to the Collection of the given Config (whose seeds are re-derived), has a live `owner` equal to the signer, and is not listed. Token accounts are constrained to `token::authority = vault` (or the Vault ATA). The mint is pinned to MEMO wherever memo-mint or memo-burn is involved, and the token program is Token-2022 or restricted by `TokenInterface`. The Vault State is bound by seeds, so another Asset's Vault State is rejected (H34c). Regression: H30–H39b, H52, H60–H66b, H70, H100–H114b, H125–H129b.

### 2. Mining ✅ SECURE
The reward is the Vault MEMO balance change around a single memo-mint CPI. The share is computed in u128, rounded down and capped by the immutable `max_share_bps`, and it never goes back to the Vault itself. memo-mint's one-mint-per-transaction rule holds (H28). Regression: H20–H29, H133.

### 3. Collection membership and supply ✅ SECURE
No private key exists for any Collection or Asset address. The Program Authority signs only `CreateV1` (Authority signature and `num_minted < max_assets` required), and `UpdateV1` / `UpdateCollectionV1` with the update authority unchanged. Core rejects direct creates, updates and plugin additions by everyone else (H14–H14c, H19c–H19g). MasterEdition, Royalties and the Oracle carry plugin authority None, verified on-chain (H07b). `num_minted` never decreases.

### 4. Burn Guard ✅ SECURE
The Collection-level Oracle rejects every Asset burn, with or without the guard account, by the owner or by a BurnDelegate (H40–H40e). Permanent burn delegates, whose force-approve could override it, cannot be added after creation. Since F01, no key can take an Asset out of its Collection to escape the Guard.

### 5. Core parsing ✅ SECURE
`parse_asset`, `parse_collection` and `asset_data_is_listed` match the Core Borsh layouts. They use bounds-checked reads and checked arithmetic, and fail closed on unknown keys, tags, plugin types and authority tags, as well as on truncation, oversized lengths and malformed registries. Since 0.2.0, a Freeze, Transfer or Burn Delegate counts as listed only when a third party (`Address`) holds it (HOME-R03); permanent delegates and unknown values count always.

### 6. CPI and signatures ✅ SECURE
The Vault signature reaches only these instructions, and forwarded remaining accounts are always non-signers:
- Token-2022 / SPL Token transfers and closes;
- System transfers;
- memo-burn;
- Core `TransferV1` of an Asset the Vault owns.

The Collection and Asset PDA signatures reach only the Core create instructions, and the System transfer that sweeps pre-funded lamports. Core does not call other programs (Oracles are read, not invoked), so there is no reentrancy. The stack depth of `app → memo-home → memo-mint → Token-2022` is 4 of the allowed 5 (H133).

### 7. Arithmetic ✅ SECURE
Checked arithmetic throughout; u128 share computation; `overflow-checks = true` in release builds.

### 8. Access control ✅ SECURE
There is no admin key, no global configuration and no upgrade-sensitive state. The Authority can create and rename Assets and mine. It cannot:
- move Vault holdings, change owners, freeze or burn;
- add plugins;
- raise `max_share_bps` or change `max_assets`;
- hand over the Collection.

A Collection naming an app as Authority was created by that app.

### 9. External dependencies ⚠️ ACCEPTED
Tested builds (X1 mainnet, dumped 2026-10-20):

| Program | Upgrade authority | sha256 of the ELF |
|---|---|---|
| Metaplex Core `CoREENxT…` | `X1CoGMN7…` (X1) | `e471b0c92e1f70eab2e7e6ee9e39968b6506d3abf4ea86c6a8a1d876308f098c` |
| Token-2022 `TokenzQd…` (v8.0.0) | `2sXr5THr…` | `d32c6364ed5d2a0ad68a8f4b04baff9b24d2b7e60df1bcdd1894071d07d69de6` |
| SPL Token `Tokenkeg…` (p-token v1.0.0) | none (immutable) | `8190d3f7ceb6cb7a7a8d8924bff89f9f611e15ce1f806f2b6237f3311a98f697` |
| ATA `ATokenGP…` | BPF Loader 2 (immutable) | `9ba8d4dfec9ca6ba8480f7f4edcc1a90184271db44e238ea0f09bba47ad2d8a9` |

- **Upgradeable programs**: Core and Token-2022 can be upgraded by third parties. memo-home's guarantees hold for the builds above, and every Core NFT on X1 shares this dependency.
- **Version labels**: the Token-2022 `security.txt` `source_release` is not a reliable version label (it reports v7.0.0 on mainnet).
- **memo-mint and memo-burn**: memo-home calls both, and memo-burn receives the Vault's signature. Invariants 1 and 6 are therefore unconditional only once both are immutable.

---

## Pre-Deployment Checklist

### 🔴 Required before testnet deployment
- [x] Real program keypairs per network (`keys.sh new <env> programs memo_home`, backed up offline); `declare_id!` and `Anchor.toml` set to the testnet / mainnet IDs; placeholder key deleted (F25).
- [x] Deploy tooling (F05, F28): memo_home is in `setup-keypairs.sh` and `deploy-common.sh` (after memo_burn); the mint check accepts `MEMO_MINT = memo_mint::AUTHORIZED_MINT_PUBKEY` with the mainnet cascade; the deploy runs the idempotent `admin-init-burn-guard` client and checks the IDL authority after a first deploy; `verify-deployment` checks the Burn Guard; `smoke-test-memo-home` covers the main path (verified on the local validator).
- [x] Deployed to testnet 2026-10-20 (slot 196824766, commit b383351e): IDL authority is the deploy admin, Burn Guard `2YdkLa2r…` initialized, `verify-deployment` passes for memo_home and the Burn Guard, `smoke-test-memo-home` passes on testnet.
- [x] Wallet tests with production Collections and PDA addresses (2026-10-20, design Appendix B): X1 wallet iOS and web and Backpack display and transfer memo-home Assets; none offers a burn; Vault control follows the transfer (F46).

### 🔴 Required before mainnet
- [x] Deployed to mainnet 2026-10-20 (slot 87306447, commit 073fd001), before locking, to keep testnet and mainnet on the same code: memo-mint and memo-burn on-chain matched the audited builds (`2423c17f…`, `39d06125…`) beforehand (F58); bytecode `7be70db1…` matches the `--features mainnet` build; IDL authority is the deploy admin; Burn Guard `2xwsdHfP…` initialized and verified. The items below remain before locking memo-home.
- [ ] Verify the deployed bytecode against a reproducible `--features mainnet` build.
- [ ] Run `./scripts/local-validator.sh start --refresh-external` (or `run-regression-tests.sh` after deleting `target/local-validator/external`). If X1 upgraded a program, re-run the suites against the new build and update the pins (F26).
- [x] Deployed 2026-10-22 to both networks at `85832fd8` (253-byte names): testnet `b111033a…`, mainnet `1db77c55…`; both reproduce bit for bit from that commit (re-audit, 2026-11-02).
- [ ] memo-home 0.2.0: deploy to testnet with `./scripts/deploy-testnet.sh memo_home` (always name the program, HOME-R25), then rebuild and redeploy memo-id, which embeds memo-home; smoke tests; then mainnet.
- [ ] Ask the X1 operators for the Core version and its upstream fixes (F50), in particular #285 (HOME-R23), or record the residual risk here.
- [ ] Check that memo-mint and memo-burn on-chain match the audited builds (ProgramData hash) (F58).
- [ ] Lock order (F57): memo-mint and memo-burn are made immutable first; memo-home last. Never finalize memo-home while either is upgradeable.
- [ ] Before locking, upload the final `--features mainnet` IDL and check its addresses, then `anchor idl erase-authority` (or build with `no-idl` and publish the IDL in the repo) (F15).
- [ ] Publish the integration rules for app and marketplace builders: design §4.4 "Integrating", §5.3 (Vault State nonce), §5.8 (wallet previews), §5.9 (deposits).

---

## Testing

### Unit tests (`programs/memo-home/src/tests.rs`, 43 tests, both networks)
- **Share, arguments and supply**: share rounding (including the 1-unit tail of the emission curve), royalty validation (including duplicates), name/URI limits, `max_assets` against `num_minted`.
- **Parsing**: Core Asset and Collection parsing (all update-authority variants, truncation including `seq`, bad tags, huge lengths and offsets). Listing detection for every plugin type and authority, with fixtures built from literal mpl-core values and fail-closed unknown values.
- **Helpers**: the exported account helpers over real `AccountInfo`s (Core, System and memo-home owners; owner-checked Config; Vault State address check).
- **Encodings and accounts**: exact Core instruction encodings; the fixed set of Core instructions memo-home signs; account sizes, PDAs (off curve) and network constants.

### Local-validator regression (`clients/regression/src/regression-memo-home.rs`, 198 cases)
`./scripts/run-regression-tests.sh home` loads X1 mainnet's Metaplex Core, Token-2022, SPL Token and ATA builds, pinned by hash, together with memo-mint, memo-burn and the test-only programs in `clients/regression/test-programs/` (`home-app`, an app that calls memo-home by CPI; `transfer-hook`, a hook program for Token-2022 mints; `home-web`, the prototype behind the `home://` extension). Every rejection asserts the exact error code and that the watched state did not change.

| Area | Cases |
|---|---|
| Burn Guard and the Core rejections it relies on | H01–H02, H40–H40q |
| Collections, plugin authorities and limits | H03–H07b |
| Authority consent, Program Authority, PDA addresses, owners and pre-funding | H08–H09d, H11–H11m |
| Core paths around memo-home | H14–H14e, H19–H19g |
| Updates, limits and cross-app isolation | H15–H18g, H17b–c, H26b |
| Mining, including dependency-account substitution | H20–H29c |
| Withdrawals and the Vault State nonce | H30–H39c |
| Listing styles and the third-party listing rule | H36–H38e, H80–H82b |
| Transfer hooks | H89–H91 |
| Vault burns, including dependency-account substitution | H50–H55 |
| `withdraw_asset`, nesting and forwarding | H60–H66b (H62b–c: the Vault as new owner) |
| XNT, closes, wrapped XNT, withdraw-all | H100–H114b |
| Correct key without its signature | H120–H129b |
| App program by CPI | H130–H136 |
| Non-memo-home Assets | H70 |
| `advance_nonce` | H140–H142b |

---

## Audit Conclusion

After the fixes, memo-home meets its design invariants for:
- Vault custody;
- mining routing;
- Collection membership and supply limits;
- burn prevention.

The high-severity finding (F01) came from a Core behavior that the first review did not model. It is fixed structurally: no key exists for any address memo-home creates. Listings remain a market-level concern, but the Vault State nonce now gives every market and bidder an exact, cheap check. **Approved for testnet deployment** after the testnet checklist; mainnet after the mainnet checklist.

**Re-audit (November 2, 2026)**: no critical, high or medium findings. memo-home 0.2.0 fixes the low findings that needed code (self-destinations, self-locking owners, self-orphaned Vaults, the first-burn top-up), saves 18–44% of compute units per instruction, and adds `advance_nonce` so app state is covered by the same nonce check before the program is locked. 0.2.0 is approved for testnet deployment, together with a rebuilt memo-id; mainnet after the testnet smoke tests.
