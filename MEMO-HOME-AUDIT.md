# Memo-Home Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-home (`programs/memo-home`)
**Audit Dates**: October 20, 2026 (first review of v1); October 20, 2026 (second, exhaustive review and fixes, this report)
**Auditor**: Pre-production security review. The second review was a multi-agent audit: 11 review dimensions (custody, mining, Core parsing, Core CPI and direct-Core paths, Anchor accounts, Token-2022 extensions, griefing, app composability, test adequacy, docs and deployment, free-form attacker), two adversarial verifiers per finding (one trying to refute it, one assessing impact with a reproduction), and two follow-up rounds from a completeness critic. Reproductions ran in LiteSVM against copies of the X1 mainnet Metaplex Core bytecode; every fix is covered by the local-validator regression suite.
**Version**: 0.1.0 (pre-deployment), 11 instructions. Program IDs: testnet `J1pxwy8V4xKvPgS5HMUn5jFo3u7At3MW4BvFWKg48Qcv`, mainnet `ArrrsGh5biKJYpiKMYJ6FD3AxCY7Fhd9Gv761ZCymzzs` (testnet deployed 2026-10-20; mainnet not yet deployed)
**Language**: Rust (Anchor 0.32.1)
**Network**: X1 (SVM-based)
**Design reference**: `MEMO-HOME-DESIGN.md`

### Overall Assessment

**Risk Level**: ✅ **LOW** after the fixes in this report. Ready for testnet deployment; mainnet after the checklist below.

The second review found one high-severity issue that the first review and its regression suite had missed: **a Collection's keypair was a hidden master key** (F01). Metaplex Core treats a Collection's own address as the update authority of every Asset in it, so whoever kept the keypair used in `create_collection` could rename any Asset, take it out of the Collection (permanently locking its Vault and lifting the Burn Guard) or move it into a collection they control. Collection and Asset addresses are now memo-home PDAs, for which no private key exists.

Other changes from this review:
- `create_collection` requires the Authority's signature.
- Each Asset has a **Vault State** whose `nonce` grows with every owner outflow. A market or bidder can now require that nothing left the Vault since listing, in every listing style, which addresses the listing limitation accepted in the first review (M-1).
- Names and URIs are limited to 64 and 256 bytes.
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

Tests after the fixes: 41 unit tests (`programs/memo-home/src/tests.rs`) and 160 local-validator regression cases (`clients/regression/src/regression-memo-home.rs`).

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
| `create_asset(seed, name, uri)` + `owner` account | Authority (+ payer) | Rejects at `max_assets` or when `owner` is the Core program. Then Core `CreateV1` at the PDA `[b"asset", collection, seed]`, signed by that PDA and the Program Authority, and creates the Vault State |
| `update_asset(name?, uri?)` | Authority | Core `UpdateV1` (name/URI only, update authority unchanged) |
| `mine(share_bps)` → `MineResult { reward, share }` | Authority (+ any caller) | Creates the Vault MEMO token account if missing and runs memo-mint `process_mint_to` into the Vault. It then pays `reward × share_bps / 10000` (rounded down, `share_bps ≤ max_share_bps`) to a MEMO token account other than the Vault's |
| `withdraw(amount)` | current owner, not listed | any SPL Token / Token-2022 token out of the Vault (`u64::MAX` = whole balance); transfer-hook extra accounts forwarded as non-signers |
| `withdraw_lamports(amount)` | current owner, not listed | native XNT out of the Vault, leaving 0 or at least the rent-exempt minimum |
| `close_token_account` | current owner, not listed | closes an empty (amount 0) Vault token account other than the MEMO one, rent to any address |
| `withdraw_asset` | current owner (+ payer), not listed | Core `TransferV1` of a Core Asset owned by the Vault; plugin accounts forwarded as non-signers |
| `burn(amount)` | current owner (+ payer), not listed | memo-burn `process_burn` from the Vault, the Vault signing as the user; first use initializes the Vault's `UserGlobalBurnStats` (the payer tops up the rent) |

Every owner outflow adds 1 to the Vault State `nonce` and rejects a destination equal to its source.

### Accounts

| Account | Seeds | Notes |
|---|---|---|
| Program Authority | `[b"authority"]` | Core update authority of every memo-home Collection; signs only Core `CreateV1`, `UpdateV1` and `UpdateCollectionV1` (unit test `signed_core_instructions_are_fixed`) |
| Burn Guard | `[b"burn_guard"]` | `OracleValidation::V1 { Pass, Pass, Rejected, Pass }` after the discriminator, plus bump (14 bytes) |
| Core Collection | `[b"collection", authority, seed]` | created by Core; memo-home signs with these seeds only inside `create_collection` |
| Collection Config | `[b"collection_config", collection]` | collection, authority, seed, max_share_bps, max_assets, creator, created_at, bump (152 bytes) |
| Core Asset | `[b"asset", collection, seed]` | created by Core; memo-home signs with these seeds only inside `create_asset` |
| Vault | `[b"vault", asset]` | system account; owns token accounts and Core Assets; signs only token transfers/closes, System transfers, Core `TransferV1` and memo-burn |
| Vault State | `[b"vault_state", asset]` | `nonce`, bump (17 bytes); created with the Asset, never closed |

### Compute units (local validator, X1 mainnet Core / Token-2022 / SPL Token / ATA builds)

| Instruction | CU (memo-home instruction) |
|---|---|
| `create_collection` | 34.7k |
| `create_asset` | 41–44k (PDA address, Vault State creation) |
| `mine` | 51–71k (64–84k when it creates the Vault MEMO token account); memo-mint's share depends on the supply tier |
| `burn` | 55–63k (77–80k with the burn-stats initialization) |

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

An Authority could write names or URIs large enough to push Core past its 32 KiB heap on every later instruction that deserializes the Asset or Collection, which blocked transfers in the whole Collection. Large strings also raise rent and break wallet rendering. **Fix**: `MAX_NAME_LEN = 64`, `MAX_URI_LEN = 256` (bytes), checked in all four create/update instructions. H06d, H06e, H11f, H11g, H15d, H18d. H11h shows that an Asset at the maximum lengths still transfers.

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
| F63 | (disputed) The listing parser failed open on Core values it did not know | **Fixed**: a plugin type above 18 (Groups) or an unknown authority tag counts as listed; unit test `unknown_values_fail_closed`. External plugin adapters stay ignored: anyone can attach a data section to an Asset on X1's Core, so failing closed there would let a third party freeze every Vault |
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
| F50 | (disputed) The exact X1 Core version is unknown | **Checklist**: the tested bytecode is pinned by hash. Before mainnet, ask the X1 operators for the Core version, or record the residual risk in §9 |
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
`parse_asset`, `parse_collection` and `asset_data_is_listed` match the Core Borsh layouts. They use bounds-checked reads and checked arithmetic, and fail closed on unknown keys, tags, plugin types and authority tags, as well as on truncation, oversized lengths and malformed registries. Any delegate authority other than Owner counts as listed.

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
- [ ] Verify the deployed bytecode against a reproducible `--features mainnet` build.
- [ ] Run `./scripts/local-validator.sh start --refresh-external` (or `run-regression-tests.sh` after deleting `target/local-validator/external`). If X1 upgraded a program, re-run the suites against the new build and update the pins (F26).
- [ ] Ask the X1 operators for the Core version and its upstream fixes (F50), or record the residual risk here.
- [ ] Check that memo-mint and memo-burn on-chain match the audited builds (ProgramData hash) (F58).
- [ ] Lock order (F57): memo-mint and memo-burn are made immutable first; memo-home last. Never finalize memo-home while either is upgradeable.
- [ ] Before locking, upload the final `--features mainnet` IDL and check its addresses, then `anchor idl erase-authority` (or build with `no-idl` and publish the IDL in the repo) (F15).
- [ ] Publish the integration rules for app and marketplace builders: design §4.4 "Integrating", §5.3 (Vault State nonce), §5.8 (wallet previews), §5.9 (deposits).

---

## Testing

### Unit tests (`programs/memo-home/src/tests.rs`, 41 tests, both networks)
- **Share, arguments and supply**: share rounding (including the 1-unit tail of the emission curve), royalty validation (including duplicates), name/URI limits, `max_assets` against `num_minted`.
- **Parsing**: Core Asset and Collection parsing (all update-authority variants, truncation including `seq`, bad tags, huge lengths and offsets). Listing detection for every plugin type and authority, with fixtures built from literal mpl-core values and fail-closed unknown values.
- **Helpers**: the exported account helpers over real `AccountInfo`s (Core, System and memo-home owners; owner-checked Config; Vault State address check).
- **Encodings and accounts**: exact Core instruction encodings; the fixed set of Core instructions memo-home signs; account sizes, PDAs (off curve) and network constants.

### Local-validator regression (`clients/regression/src/regression-memo-home.rs`, 160 cases)
`./scripts/run-regression-tests.sh home` loads X1 mainnet's Metaplex Core, Token-2022, SPL Token and ATA builds, pinned by hash, together with memo-mint, memo-burn and the two test-only programs in `clients/regression/test-programs/`. Every rejection asserts the exact error code and that the watched state did not change.

| Area | Cases |
|---|---|
| Burn Guard | H01–H02, H40–H40e |
| Collections, plugin authorities and limits | H03–H07b |
| Authority consent, Program Authority, PDA addresses and pre-funding | H08–H09d, H11–H11h |
| Core paths around memo-home | H14–H14e, H19–H19g |
| Updates and cross-app isolation | H15–H18f, H17b–c, H26b |
| Mining | H20–H29 |
| Withdrawals and the Vault State nonce | H30–H39b |
| Listing styles | H36–H38c, H80–H82b |
| Transfer hooks | H89–H91 |
| Vault burns | H50–H55 |
| `withdraw_asset`, nesting and forwarding | H60–H66b |
| XNT, closes, wrapped XNT, withdraw-all | H100–H114b |
| Correct key without its signature | H120–H129b |
| App program by CPI | H130–H136 |
| Non-memo-home Assets | H70 |

---

## Audit Conclusion

After the fixes, memo-home meets its design invariants for:
- Vault custody;
- mining routing;
- Collection membership and supply limits;
- burn prevention.

The high-severity finding (F01) came from a Core behavior that the first review did not model. It is fixed structurally: no key exists for any address memo-home creates. Listings remain a market-level concern, but the Vault State nonce now gives every market and bidder an exact, cheap check. **Approved for testnet deployment** after the testnet checklist; mainnet after the mainnet checklist.
