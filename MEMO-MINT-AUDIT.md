# Memo-Mint Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-mint  
**Audit Date**: October 27, 2025 (original), revised October 13, 14, 15 and 16, 2026, October 31, 2026 (Revision 5) and November 1, 2026 (Revision 5 re-audit)  
**Auditor**: Pre-Production Security Review  
**Version**: Revision 5, program version 0.5.0 (memo minimum counted in characters; Revision 4: emission curve steps down just above 10M; Revision 3: one mint per transaction; Revision 2: round-number emission curve, `process_mint_to` fix, compute-unit optimization)  
**Language**: Rust (Anchor Framework)  
**Network**: X1 (SVM-based)

> **Note (October 16, 2026)**: the final audit of memo-mint and memo-burn, `MEMO-CORE-AUDIT-2026-10-16.md`, re-verified this contract against the source code and corrected several statements in this document (finding IDs such as MINT-01 and OPS-03 refer to that report). Where the two documents differ, the core audit report takes precedence, except for Revision 5 and the Revision 5 Re-Audit (November 1, 2026), which are newer; the re-audit's findings MINT-07 to MINT-23 continue that report's MINT series.

### Overall Assessment

**Risk Level**: ✅ **LOW** - Contract is production-ready with confirmed design intent

The memo-mint contract implements a dynamic token minting mechanism with supply-based rewards (a round-number emission curve) for a fair-launch mining project. The contract demonstrates excellent security practices and all design decisions up to Revision 4 have been verified as intentional; the Revision 5 decisions were made by the owner on November 1, 2026, and Revision 5 is deployed on both networks (Revision 5 Re-Audit, Decisions).

### Summary Statistics

- **Critical Issues**: 0
- **Revision 5 Re-Audit (November 1, 2026)**: 0 critical, 0 high, 0 medium, 6 low, 11 informational (MINT-07 to MINT-23)
- **Design Confirmations**: 5 (all verified as intentional, up to Revision 4)
- **Security Strengths**: 8
- **Best Practices**: 5
- **Code Quality**: Excellent

---

## Contract Overview

### Purpose
The memo-mint contract enables users to mint MEMO tokens by submitting memo instructions. The minting amount decreases as circulating supply increases, stepping down just above every round number (10M, 20M, ..., 100M, 200M, ...) from 1 token to 1 lamport (0.000001 token) in the last step before the 10 trillion cap.

### Key Features
- Dynamic mint amounts based on a round-number emission curve
- 10 trillion token cap on circulating (outstanding) supply
- Mandatory memo requirement (at least 69 characters; no contract maximum)
- PDA-based mint authority
- Token2022 compatibility
- Dual network support (testnet/mainnet)

### Emission Curve
- **Up to and including 10M tokens**: 1 token per mint
- **Above 10M tokens**: supply in `(d × 10ⁿ, (d+1) × 10ⁿ]` (`d` = 1..9) pays `decade_base × (10 − d) / 10`, where `decade_base` is 1 token for 10M–100M, 0.1 for 100M–1B, 0.01 for 1B–10B, and so on. A mint at exactly a round number still pays the previous step; the first mint above it pays the next one.
- Each decade goes from its base down to a tenth of it: 10M → 100M: 1 → 0.1, 100M → 1B: 0.1 → 0.01, ...

| Supply before the mint | Reward per mint |
|---|---|
| up to and including 10M | 1 |
| above 10M / 20M / ... / 90M | 0.9 / 0.8 / ... / 0.1 |
| above 100M / 200M / ... / 900M | 0.09 / 0.08 / ... / 0.01 |
| above 1B | 0.009 |
| above 10B / 100B / 1T | 0.0009 / 0.00009 / 0.000009 |
| above 9T, up to the cap | 0.000001 (1 lamport) |

Every decade start pays exactly 10x less than the previous one, so the long-run slope matches the original 6-tier schedule; the single 10x cliff at each tier boundary is replaced by nine smaller steps.

---

## Revision 2 Changes (October 13, 2026)

| Change | Details |
|---|---|
| Round-number emission curve | Replaces the 6-tier `match` in `calculate_dynamic_mint_amount`. Integer math only, at most 5 loop iterations. Compile-time assertions: the curve ends exactly at `MAX_SUPPLY`, every step is a whole number of lamports, and the smallest reward is ≥ 1 lamport. |
| No maximum memo length | Only the 69-byte minimum is checked. The transaction size limit (1,232 bytes on X1 today, 4,096 bytes on Solana since September 2026) bounds the memo, so the contract adapts to future transaction-size changes without an upgrade. For most non-Latin scripts the 1.4M CU limit binds first (Revision 5 Re-Audit, MINT-08). The `MemoTooLong` error was removed. On a local validator, an 801-byte and an 812-byte memo minted successfully; a 900-byte memo failed only at the transaction size limit (1,279 > 1,232 bytes). |
| `process_mint_to` fixed | `recipient: Pubkey` is now a real handler argument, matching `#[instruction(recipient: Pubkey)]`. Previously the IDL showed no arguments, so IDL-generated clients and Anchor CPI calls always failed with `InstructionDidNotDeserialize`. The wire format (8-byte discriminator + 32-byte recipient) is unchanged, so existing hand-built clients keep working. |
| Compute-unit optimization | Removed the duplicate `find_program_address` (the `seeds = [b"mint_authority"], bump` constraint already verifies the PDA and canonical bump), success-path logs and floating-point log formatting (`calculate_token_count_safe` removed), and the memo data copy. memo-mint's own consumption dropped from 17,749 to 12,638 CU on a local validator (−29%). On X1 mainnet a single mint with a 70-byte memo is expected to drop from about 60,331 to about 55,220 CU (−8.5%). |
| Comments and messages | Corrected the compute-budget comment (it also occupies an instruction index, so it must be at index 1 or later). `UnauthorizedTokenAccount` now reads "Token account must be owned by the minting user or the declared recipient." |
| Error codes renumbered | `MemoTooLong` was removed, together with `InvalidMemoFormat` and `InvalidMintAuthority`, which were never returned (mint does not parse memo contents, and the mint authority PDA is checked by the account constraint). Every remaining error after them moved down (see the table below). |

**Error codes (Revision 2)**:

| Error | Old code | New code |
|---|---|---|
| `MemoRequired` | 6000 | 6000 |
| `InvalidMemoFormat` | 6001 | removed |
| `MemoTooShort` | 6002 | 6001 |
| `MemoTooLong` | 6003 | removed |
| `InvalidTokenAccount` | 6004 | 6002 |
| `UnauthorizedMint` | 6005 | 6003 |
| `UnauthorizedTokenAccount` | 6006 | 6004 |
| `InvalidMintAuthority` | 6007 | removed |
| `SupplyLimitReached` | 6008 | 6005 |
| `ArithmeticOverflow` | 6009 | 6006 |
| `UnauthorizedAdmin` (Revision 3) | — | 6007 (new) |
| `OneMintPerTransaction` (Revision 3) | — | 6008 (new) |

**Behavior changes integrators must know**:
1. **Success log format**: `Minted {amount} units to {owner}, supply before mint: {supply} units` (integer units, 1 token = 1,000,000 units). It replaces `Successfully minted X tokens ... current supply: Y tokens`. Off-chain parsers of the old line must be updated. Revision 5 drops the owner (decision D4, option O2b): `Minted {amount} units, supply before mint: {supply} units`.
2. **Boundary semantics**: the old schedule paid tier-1 rewards at exactly 100M (inclusive). The new curve pays 0.1 from exactly 100M on, matching the round-number rule.
3. **IDL**: `process_mint_to` now has one argument, `recipient: pubkey`. Upgrade the on-chain IDL after deploying.

**Verification**: 47 unit tests pass (every round number from 10M to 9T and one lamport below each, decade starts, monotonicity, cap edge cases, `process_mint_to` wire format). `process_mint` and `process_mint_to` were exercised on a local validator, including rejection of a mismatched recipient (`UnauthorizedTokenAccount`) and of a missing token account (`AccountNotInitialized`).

---

## Revision 3 Changes (October 14, 2026): One Mint per Transaction

A transaction can now mint at most once, whether it calls memo-mint directly, through chat/blog/forum, or through any other program via CPI. Before this revision one memo could back about 31 mints in one transaction, which made batch mining roughly 3.5x cheaper per token than single mints.

| Item | Details |
|---|---|
| Transaction fingerprint | The instructions sysvar holds the serialized top-level instructions followed by a 2-byte current instruction index. Everything except those last 2 bytes is identical for every instruction and every CPI of a transaction, cannot be modified by any program, and includes each instruction's program id, accounts (signers included) and data. Its SHA-256 (`sol_sha256` syscall) is the transaction fingerprint. |
| New account `MintState` | PDA with seeds `[b"mint_state"]`, 113 bytes: discriminator (8), `last_tx_fingerprint: [u8; 32]`, `last_slot: u64`, `bump: u8`, `reserved: [u8; 64]`. One account for the whole program. |
| Check | Before minting, `process_mint` and `process_mint_to` compute the fingerprint. If it equals the stored one in the same slot, the mint fails with `OneMintPerTransaction`; otherwise the fingerprint and slot are stored and the mint proceeds. State written by the first mint is visible to later instructions and CPIs of the same transaction, and every mint write-locks `MintState`, so no other transaction can run in between. A failed transaction rolls back, so rejected attempts never change the state. |
| Why one global account, not per user | Per-user records are bypassed by `process_mint_to` to many wallets or by several signers in one transaction. The fingerprint does not depend on who mints, so any second mint in the same transaction is rejected. Every mint already write-locks the mint account, so the extra global account adds no new write-lock contention. |
| No caller allowlist needed | The rule needs no list of allowed calling programs and no stack-height check: a program that calls memo-mint several times via CPI is rejected on its second call. A third-party program may still mint once per transaction. |
| New instruction `initialize_mint_state` | Admin only (`AUTHORIZED_ADMIN_PUBKEY`, the deploy admin key, per network; new in memo-mint). Creates `MintState` once; a second call fails because the account exists. |
| Accounts | `mint_state` is appended as the **last** account of `process_mint` and `process_mint_to` (writable, seeds and stored bump checked). Existing accounts keep their order. chat `send_memo_to_group`, blog `mint_for_blog` and forum `mint_for_post` pass it as their last account. |

**False positives** (corrected in the core audit, MINT-01): the fingerprint covers exactly the serialized top-level instruction list (program ids, account keys with their signer/writable flags, instruction data). It does not cover signatures, the recent blockhash, lookup table addresses, or a fee payer that no instruction references, and `record_mint` compares only with the last recorded mint. A mint is therefore rejected as a false positive only when its transaction's top-level instruction list is byte-for-byte identical to that of the **immediately preceding** recorded mint in the same slot; any other mint in between overwrites `MintState`. Example: a user re-signs the same instructions with a new blockhash or a different fee payer after an RPC timeout, and both transactions land in the same slot; the second fails with 6008 and still pays the fee. For direct calls and the built-in apps the minting user is a signer of a top-level instruction, so only the same user can collide with themselves; two different users with identical memos never collide. A third-party integration whose top-level instructions contain no signer (for example a permissionless crank calling `process_mint_to` with its own PDA) can be made to collide by anyone. Identical instructions in a later slot succeed. One mint per transaction is never affected.

How clients avoid false positives:
- Make every transaction's memo unique, for example with a random nonce. A timestamp in seconds is not enough: on mainnet, several memo-miner instances of one wallet collide within the same second (Revision 5 Re-Audit, MINT-14).
- Listing the fee payer as a memo signer helps only when the retry uses a different fee payer.
- An integration whose top-level instructions contain no signer must add one, for example the fee payer as a memo signer or a signer in its own instruction. A unique memo does not stop someone who copies its instruction list.

**Deploy order (required)**: upgrade memo-mint → run `admin-init-mint-state` → upgrade chat, blog and forum. Until `MintState` exists, every mint fails with `AccountNotInitialized`; until the apps are upgraded, their mint instructions fail with `AccountNotEnoughKeys`. `scripts/deploy-common.sh` runs the init client right after deploying memo-mint and verifies memo-mint's `AUTHORIZED_ADMIN_PUBKEY` against the deploy admin keypair.

**Integrator impact**:
1. Every `process_mint` / `process_mint_to` must append the `MintState` PDA (`memo_token_client::mint_state_pda()`) as the last account. Old clients fail with `AccountNotEnoughKeys`.
2. Batch mining (several mints in one transaction) is no longer possible. Each mint needs its own transaction and memo.
3. The extra account adds 33 bytes to every mint transaction. The longest memo that fits a single-signer `process_mint` transaction drops from 853 to 820 bytes (from 813 to 780 bytes with a ComputeBudget instruction); `process_mint_to` and app mint instructions lose the same 33 bytes.

**Economics**: the cheapest way to mint becomes the single-mint transaction again. On the local validator memo-mint itself uses 15,598 CU per mint, up from 12,483 (+3,115: about 1,600 for the seeds check on `MintState`, the rest for loading and writing the account, the clock read and hashing the sysvar). On X1 that is about +0.00003 XNT per mint (fee = 1,500 + 10 × requested CU). Together with the end of batching, the cost per token (and so the cost-based price ceiling) rises from about 0.00016 XNT (31-mint batches) to about 0.00058 XNT (single mints, mainnet Memo program CU).

**Verification**:
- 59 unit tests (testnet and mainnet builds), including fingerprints built with the runtime's own instructions sysvar serializer (`construct_instructions_data` / `store_current_index_checked`): identical for every instruction index of a transaction, different when any instruction, account, flag or signer differs.
- Local validator, full run from a fresh ledger, repeated twice with identical results: 3 pre-init checks (mint before init → `AccountNotInitialized`, non-admin init → `UnauthorizedAdmin`), 3 post-init checks, 36 core cases, all 7 smoke tests, 18 app cases and the 52-case `pen-test-memo-mint` suite.
- Rejected with `OneMintPerTransaction`, with balance, supply and `MintState` unchanged: 2, 3 and 31 top-level mints; `process_mint` + `process_mint_to` in both orders; two memos with two mints; memo/mint/memo/mint; mints separated by other instructions; `process_mint_to` to two recipients; three signers minting together; a test program calling memo-mint via CPI 2 and 30 times, through `process_mint_to`, nested two levels deep, and combined with a top-level mint; chat `send_memo_to_group` (zero-interval group), blog `mint_for_blog` and forum `mint_for_post` twice in one transaction, combined with a direct mint, and called twice via CPI; burn followed by two mints. Note (core audit TEST-01): the CPI test program used for these runs is not in the repository, and the committed tests (unit tests, `pen-test-memo-mint`, smoke tests) only build top-level mints, so this part cannot be reproduced from the repository today. The core audit re-verified the property with app CPI cases on a local validator and on testnet (read-only simulation) and with a LiteSVM CPI harness. Since then, `regression-memo-mint` (M04) covers the memo-blog and memo-forum CPI mints and `regression-memo-home` (H28) covers memo-home `mine` on a local validator. memo-chat `send_memo_to_group` twice in one transaction and the CPI loop program still have no committed test.
- Still allowed: single mints (11 in a row), `process_mint_to` to self and others, a single CPI mint from a third-party program, mint plus burn in one transaction, three burns in one transaction, two users with identical memos in the same slot, identical instructions in a later slot.
- Upgrade path on a local validator, both manually and with `deploy-common.sh`: the old program batches; after the upgrade, old clients fail with `AccountNotEnoughKeys` and new clients fail with `AccountNotInitialized` until the admin init; afterwards single mints succeed and batches are rejected.

---

## Revision 4 Changes (October 15, 2026): Curve Steps Down Just Above 10M

| Change | Details |
|---|---|
| Emission curve | `decade_base × (11 − d) / 10` → `decade_base × (10 − d) / 10`, and each step now takes effect just above its round number instead of at it (the step is located from `supply − 1`). Up to and including exactly 10M a mint pays 1; the first mint above 10M pays 0.9 (in Revision 3 the reward stayed 1 until 20M). Each decade goes from its base down to a tenth of it: above 10M … up to 100M pays 0.9, 0.8, ..., 0.1, then above 100M pays 0.09, ..., 0.01. Every step above 10M pays one tenth of the decade base less than in Revision 3. |
| Last step | Above 9T pays exactly 1 lamport (was 2), so supply can reach exactly 10T. The `new_supply > MAX_SUPPLY` check is kept as defense in depth. |
| Effect | Minting from 10M to 100M takes about 283M mints instead of about 193M (+47%). Mainnet supply was 8.73M when the change was made, so no mint at the new rates had happened. |
| Scope | Only `calculate_dynamic_mint_amount` changed. Accounts, instructions, error codes and the one-mint-per-transaction check are unchanged; chat, blog and forum mint through memo-mint and need no upgrade. |

**Follow-up (October 16, 2026): public curve function.** The curve calculation moved out of `calculate_dynamic_mint_amount` into a new public pure function, `mint_amount_at_supply(supply) -> u64`, which `calculate_dynamic_mint_amount` now calls before its cap checks. Mint behavior is unchanged. The function has no cap check: at or above `MAX_SUPPLY_LAMPORTS` it returns the last step (1 lamport), so its result is always ≥ 1. memo-chat, memo-profile, memo-project, memo-blog and memo-forum call it as a library function (no CPI) to scale their burn minimums with the current mint reward (`MINTS × mint_amount_at_supply(mint.supply)`). As a result, any future change to the curve also changes those burn minimums, and those programs must be rebuilt and redeployed to pick it up. Unit tests check that it equals `calculate_dynamic_mint_amount` below the cap and returns 1 at and above the cap (including `u64::MAX`).

---

## Revision 5 Changes (October 31, 2026): Memo Minimum Counted in Characters

The 69 minimum was meant as 69 characters but was checked as 69 bytes. A memo of multi-byte UTF-8 characters reached it with far fewer characters: a mainnet bot used memos such as `"æÀ" + "é" × 32 + "a"` (equivalent to `"é" × 34 + "R"`), which are 69 bytes but 35 characters. The SPL Memo program's cost depends on the characters it logs, so such a memo cost about 6,000 CU less than a 69-character ASCII memo. The reward does not depend on the memo, so this was a design gap, not a vulnerability.

| Change | Details |
|---|---|
| Memo minimum | At least 69 characters (Unicode code points) instead of 69 bytes. No contract maximum; the memo is bounded by the transaction size (780 ASCII characters) and, for most non-Latin scripts, first by the 1.4M CU limit (about 124 CJK characters or 79 emoji; MINT-08). |
| Counting | The SPL Memo program at index 0 runs first and rejects invalid UTF-8, so `validate_memo_length` does not decode the memo again. Every character takes 1 to 4 bytes and starts with exactly one byte that is not a continuation byte (`0b10xxxxxx`). The check takes three fast paths in order: fewer than 69 bytes is too short; 276 bytes or more (4 × 69) is always enough; an all-ASCII memo has as many characters as bytes. Only a memo of 69 to 275 bytes containing non-ASCII bytes is counted byte by byte. The final Revision 5 counts 8 bytes at a time and takes the length fast path from 273 bytes (decision D4, below). |
| Error | `MemoTooShort` (6001) keeps its code; its message now says "at least 69 characters". No error code was added or renumbered. |
| Scope | Only `validate_memo_length` changed (the owner's decisions D3–D5 later changed the logs, the PDA checks and two writable flags; below). Accounts, instructions and the other error codes are unchanged. memo-chat, memo-blog and memo-forum put base64 (ASCII) memos at index 0, where characters equal bytes, so they need no upgrade. memo-home `mine` mints through `process_mint_to` with the user's own memo at index 0, so the character rule applies to it too; memo-home needs no upgrade either. |

**Effect on memos**: ASCII memos behave exactly as before. `"é" × 34 + "R"` (69 bytes, 35 characters) is now rejected; `"é" × 69` (138 bytes) is accepted. In the Memo program a 69-character `é` memo costs more than a 69-character ASCII memo. ASCII is not the cheapest way to meet the minimum, though: 69 plane-2 ideographs (U+20000–U+2FA1D) cost less (Revision 5 Re-Audit, MINT-07).

**Compute units** (memo-mint's own consumption on a local validator, one `process_mint` after the memo; the Revision 4 build was dumped from testnet):

| Memo | Revision 4 | Revision 5 | Change |
|---|---|---|---|
| 69 ASCII characters (69 bytes) | 15,571 | 15,638 | +67 |
| `"é" × 69` (138 bytes) | 15,605 | 17,751 | +2,146 |
| `"é" × 137` (274 bytes) | 15,673 | 19,927 | +4,254 |
| 275 bytes with the non-ASCII bytes last, e.g. `"x" × 271 + "🌍"` (the slowest path) | 15,674 | 20,056 | +4,382 |
| 700 ASCII characters (700 bytes) | 15,886 | 15,886 | 0 |

Two earlier drafts were measured and replaced by the fast paths above. Decoding the memo with `core::str::from_utf8` and counting `chars()` cost +355 CU on the 69-byte ASCII memo. An early-exit byte loop cost +1,242 CU.

### Owner decisions folded into Revision 5 (November 1, 2026)

After the re-audit the owner decided D3, D4 (options O1b with `@ ConstraintSeeds`, O2b and O3) and D5 (Revision 5 Re-Audit, Decisions). The final Revision 5 (version 0.5.0, deployed on testnet and mainnet on November 1, 2026) therefore also changes:

| Change | Details |
|---|---|
| PDAs as constants (D4, O1b) | `mint_authority` and `mint_state` are checked against per-network constants (`MINT_AUTHORITY_PDA`, `MINT_STATE_PDA`) instead of being derived, and the signer seeds use `MINT_AUTHORITY_BUMP` (testnet 255, mainnet 254). A wrong account still fails with 2006 `ConstraintSeeds`. `test_pda_constants_per_network` checks the constants against `find_program_address`; `run-unit-tests.sh` now runs it for both builds (MINT-23). |
| Success log (D4, O2b) | `Minted {amount} units, supply before mint: {supply} units`. The recipient is no longer logged: it is the `user` of `process_mint`, or the `recipient` argument of `process_mint_to` (in the inner instructions for a CPI mint). |
| Character count (D4, O3) | Continuation bytes are counted 8 at a time with wrapping arithmetic. The length fast path starts at 273 bytes: 68 characters take at most 272 bytes. Only non-ASCII memos of 69–272 bytes are counted. |
| `MemoTooShort` log (D3, O4) | `Memo too short: {n} bytes` under 69 bytes, otherwise `Memo too short: {n} characters (minimum: 69)` (MINT-09). |
| Writable flags (D5) | `user` and `caller` are no longer `mut` (MINT-12). |

Instruction data, discriminators, the account list and the error codes are unchanged, so memo-sdk, memo-app, memo-miner and the deployed dependent programs need no change. Deployment notes:
- Deploy memo-mint before redeploying any dependent rebuilt against it (MINT-12).
- The IDL now lists `mint_authority` and `mint_state` with fixed addresses, which differ per network. The mainnet IDL must come from the mainnet build (`anchor build -p memo-mint -- --features mainnet`, as the deploy script does). The current mainnet IDL, fetched read-only, names the mainnet program, so the script's IDL follows the feature. Before `anchor idl upgrade` on mainnet, check that `mint_authority` is `C1r27YsPiY6wbVvRY2cVCsaRKK5HiXxdvWhxJd5wapSh` and `mint_state` is `EZheeAswQ9exUMpKFjHrQrczix5LSzjakroRFiMgJfWv` in `target/idl/memo_mint.json`.
- Builds: testnet `d9b4f75196e6721d056f4d216f40dfcebadd1791e9ffea17c67dd4398bbb67ac`, mainnet `1eb83794def585fbf570f3a4021be6acf28603cbe78cae19ad275840f2f59b9f`, 269,416 bytes each.
- Testnet deployment (November 1, 2026): upgrade signature `38yseJfkdtfMpjLPSpj56NV43GCD7NAwg1oUVk3fGErQpdN3jUnWyRHMnNF41hfoFRQJmwpLAAV8YHJd6mWKURjr`; the dumped program matches the testnet build hash (zero padding after 269,416 bytes); the on-chain IDL is 0.5.0 with the testnet PDA addresses. Mints with memo-sdk 0.2.0 landed for 69 Extension B ideographs (29,959 CU), 69 ASCII characters (35,866) and 69 CJK characters (777,316); `"é" × 34 + "R"` fails with 6001 and `Memo too short: 35 characters (minimum: 69)`. The memo-mint, memo-blog, memo-forum and memo-home smoke tests pass, so the deployed dependents mint through CPI unchanged.
- Mainnet deployment (November 1, 2026): a read-only sample just before the upgrade found 1,000 mints in about four minutes, all from one payer, every sampled memo the 35-character, 69-byte form (`"úà" + "é" × 32 + "a"`, about 37,070 CU, 0.9 MEMO each). Upgrade signature `uraGG7K3FJZrUpbphJKzE6PkTArDvp7qF5NYjZKGi4ndm54YkWZC5BCYKeea1oQ3kdnPHUVVqaugvuGcxJ7Hnc5` (slot 90,072,716); the dumped program matches the mainnet build hash (zero padding after 269,416 bytes in the 283,016-byte ProgramData); the on-chain IDL equals the mainnet build's IDL (0.5.0, PDA addresses `C1r27YsP…` and `EZheeAsw…`). The mint state account already existed. No mint from the sampled payer landed after the upgrade. A memo-sdk 0.2.0 mint with 69 Extension B ideographs landed (30,281 CU, limit 30,331, fee 304,810 lamports, `Minted 900000 units, supply before mint: 12342974600000 units`); `"é" × 34 + "R"` fails in simulation with 6001 and `Memo too short: 35 characters (minimum: 69)`.

An independent review of these changes (two reviewers, every finding checked by two verifiers trying to refute it) found no defect in the program. It recomputed the four PDAs and both bumps, confirmed on testnet and mainnet (read-only) that the MEMO mint's authority is `MINT_AUTHORITY_PDA` and that MintState stores bump 254, and checked the error codes, the 8-byte count and the CPI callers. It confirmed eight smaller issues, all fixed:
- a comment on `count_continuation_bytes` implied a plain `*` would only cost an overflow check; the multiply overflows by design and would panic (comment only: both builds' hashes are unchanged);
- the fuzzer's model still required a writable user and checked `mint_state`'s address before `mut` (Anchor checks `mut` first), and its low CU bounds predated the cheaper build;
- M24n no longer reached the ASCII fast path, and nothing pinned the 273-byte length fast path (now M24n and M24p, above);
- stale case counts and a unit test named for more than it checks (`test_fast_path_bound_is_sound`);
- `repro-memo-mint-audit`'s run recipe loaded the final build; it needs the audited one.

**Compute units of the final Revision 5** (memo-mint's own consumption, `[memo, process_mint]`, local validator at the mainnet supply of 11,963,090.9 MEMO; mainnet computed as in MINT-10: the final build's mint paths derive no PDA, so mainnet is the local figure − 54, against + 1,446 for the audited builds):

| Memo | Revision 4 (local) | Revision 5 as audited (local) | Final Revision 5 (local) | Final vs audited, mainnet |
|---|---|---|---|---|
| 69 ASCII characters (69 bytes) | 15,575 | 15,642 | 10,030 | 17,088 → 9,976 (−7,112) |
| `"é" × 69` (138 bytes) | 15,609 | 17,755 | 10,300 | 19,201 → 10,246 (−8,955) |
| `"é" × 137` (274 bytes) | 15,677 | 19,931 | 10,065 | 21,377 → 10,011 (−11,366) |
| 700 ASCII characters (700 bytes) | 15,890 | 15,890 | 10,278 | 17,336 → 10,224 (−7,112) |

`process_mint_to` costs 63 CU more in each row. Each row is 1–2 CU below the options stage's O5v2 build, which had the same options with the `MemoTooShort` log written inline. Rejected memos cost memo-mint 6,214 CU (68 ASCII bytes), 6,762 (`"é" × 34 + "R"`, 69 bytes) and 6,934 (68 `é`).

For memo-miner's transaction shape (no memo accounts, no account creation), the whole mint on the final build costs 35,976 CU locally with the released ASCII memo and 30,335 with 69 Extension B ideographs (MINT-07, follow-up). On mainnet that is about 30,281 CU against 42,994 for the released miner on Revision 4 today: a fee of about 304,810 lamports instead of 431,940 (−29%).

**Tests**:
- Unit tests cover:
  - 2-, 3- and 4-byte characters: 68 characters rejected and 69 accepted for each;
  - `"é" × 34 + "R"` rejected;
  - continuation bytes never counted;
  - long multi-byte memos accepted.
- New local-validator regression cases:
  - M06b: 69 bytes of `é`, too short;
  - M06c: 68 `é` characters, too short;
  - M07b: 69 `é` characters, accepted;
  - M07c: 69 three-byte characters, accepted.
- `regression-memo-mint` passed 52/52, including the memo-blog and memo-forum CPI mints (103/103 after the re-audit's new cases, 107/107 with the decisions' cases).

**Clients**:
- Count characters before sending:
  - Rust: `str::chars().count()`;
  - JavaScript: `[...memo].length`. Plain `.length` counts UTF-16 units, not characters;
  - Python: `len(memo)`.
- Build the memo instruction without signer accounts (unless the mint instruction has no signer; see MINT-01). A signer account makes the Memo program log `Signed by <address>`, which costs about 13,500 CU. memo-mint checks only the memo's program and data; the memo's accounts matter only as part of the one-mint fingerprint. The user already signs the transaction, and that signature covers the memo.
- Make every transaction's memo unique, for example with a random nonce (Revision 5 Re-Audit, MINT-14).

---

## Revision 5 Re-Audit (November 1, 2026)

Scope: the Revision 5 working tree (version 0.5.0; testnet build sha256 `8fc80212…`, mainnet build `2c9b3e31…`, 271,080 bytes; not deployed), compared with the deployed Revision 4 (testnet `A31a17…`, mainnet `8iq6zq…`). The review also covered:
- the programs that call memo-mint by CPI: memo-chat, memo-blog and memo-forum (`process_mint`) and memo-home `mine` (`process_mint_to`);
- the programs that link its curve: memo-chat, memo-profile, memo-project, memo-blog, memo-forum and memo-id;
- the clients: memo-sdk, memo-app with its agent skill file, and memo-miner;
- the deploy tools, the documents and the X1 runtime.

Method:
- **Static review**:
  - ten review dimensions: memo rule, one mint per transaction, accounts and Token-2022, curve, CPI and integration, attacker economics, on-chain operations, specification, test gaps, code quality;
  - an offline differential fuzz of the pure functions, and a completeness critic;
  - a skeptic and an impact verifier checked every finding: 71 verified (63 by both verifiers, 8 by one), 225 further ideas ruled out;
  - testnet and mainnet were used only through read-only RPC and `simulateTransaction` with `sigVerify: false`.
- **Dynamic review** on the local validator. It loads X1 mainnet's Token-2022, Token, ATA and Core builds and the SPL Memo program, whose ELF is byte-identical to X1's (sha256 `f520eaf0…`, asserted on every reproduction and options run). It ran in six stages:
  - reproduction of the chain-dependent findings on Revision 5 and on the deployed Revision 4 build (`repro-memo-mint-audit`);
  - the proposed program changes, built and measured in an isolated copy (`options-memo-mint-cu`);
  - new regression cases;
  - mutation testing;
  - a model-based stateful fuzzer (`fuzz-memo-mint`);
  - a review of the new tests.
- The local validator runs the testnet-ID build. memo-mint costs 1,446 CU more on mainnet, measured from mainnet transactions; local + 1,446 reproduced six independent mainnet simulations exactly. Mainnet figures below are computed this way unless they come from a mainnet simulation.
- The program was not changed. Program changes are proposed for the owner to decide (Decisions below). Findings continue the MINT series of the core audit.

No critical, high or medium issue was found. The one-mint-per-transaction check, the curve, the cap, the account checks and the Revision 5 character count hold, and 8,600 fuzz steps (about 9,400 transactions) found no divergence of the program from an independent model. The findings are about cost, clients, operations, tests and documents.

| ID | Severity | Status | Finding |
|---|---|---|---|
| MINT-07 | Low | Documented; memo-miner uses the cheapest memo | Revision 5 does not make ASCII the cheapest memo |
| MINT-08 | Low | Fixed in memo-sdk 0.2.0 and memo-miner 0.5.0; open in memo-app | A minimum memo in CJK, kana, Hangul or emoji cannot be minted with the official clients |
| MINT-14 | Low | Fixed in memo-miner 0.5.0 | MINT-01 false positives happen on mainnet now |
| MINT-19 | Low | Open (memo-app) | The agent skill file teaches stale rewards, burn amounts and memo rules |
| MINT-22 | Low | Fixed (tools) | `anchor build` in a checkout without `target/` rewrites the mainnet program IDs |
| MINT-23 | Low | Mostly fixed (tests) | No committed test script runs the mainnet build |
| MINT-09 | Info | Fixed (D3) | The `MemoTooShort` log reports bytes against a character minimum |
| MINT-10 | Info | Fixed (D4) | About 7,000 avoidable compute units per mint |
| MINT-11 | Info | Fixed (tools) | Revision 5 relies on the Memo program's UTF-8 check |
| MINT-12 | Info | Fixed (D5) | `user` and `caller` are writable but never written |
| MINT-13 | Info | Documented | Equivalent and dead code (extends MINT-05) |
| MINT-15 | Info | Cut over (November 1, 2026); memo-app release and announcement open | Revision 5 cut-over |
| MINT-16 | Info | Fixed (tools, docs) | Deployment tooling and procedure |
| MINT-17 | Info | Documented | Runtime changes since the core audit (INT-08, INT-12) |
| MINT-18 | Info | Open (memo-app) | Client release order and client defects |
| MINT-20 | Info | Fixed, except memo-app's SKILL.md (MINT-19) | Documentation drift |
| MINT-21 | Info | Fixed in the tests | Test gaps |

### 🟡 MINT-07 (Low, documented; owner decision): Revision 5 does not make ASCII the cheapest memo

The SPL Memo program logs the memo with Debug formatting (`msg!("Memo (len {}): {:?}", ...)`). Its cost per character depends on where the code point sits in libcore's printable and grapheme tables, not on how many bytes it takes. Assigned plane-2 ideographs (CJK Extension B–F and the compatibility supplement, U+20000–U+2FA1D) take a short path through the tables and are cheaper than printable ASCII. Escaped characters (TAB, quotes, backslash, LF, CR) are cheaper too.

| Memo (69 characters, no memo accounts) | Bytes | Memo CU | Whole mint, mainnet, Revision 5 |
|---|---|---|---|
| plane-2 ideographs (any mix; the cost is flat) | 276 | 20,097 | 37,393 |
| TAB | 69 | 24,035 | 41,294 |
| `"` | 69 | 24,104 | 41,363 |
| printable ASCII (`x`) | 69 | 26,037 | 43,296 |
| `é` | 138 | 37,702 | 57,075 |
| `中` (BMP CJK) | 207 | 767,024 | 787,535 |

Under Revision 5 the cheapest valid mint therefore uses 69 plane-2 ideographs: 37,393 CU on mainnet against 43,296 for an ASCII memo, a 13.6% lower fee (375,430 against 434,460 lamports). The statement in Revision 5 Changes that "ASCII is again the cheapest way to meet the minimum" was wrong and has been corrected.

Revision 5 still raises the floor a lot:
- Revision 4, deployed today, accepts `U+20000 × 17 + "x"` (69 bytes, 18 characters): 6,679 Memo CU and 23,871 CU for the whole mainnet mint, 45% cheaper than ASCII. Revision 5 raises the cheapest memo to 20,097 Memo CU (+13,418), about 69% of the way to ASCII.
- The mainnet bot's memo (`"é" × 32`-style, 69 bytes, 35 characters) costs 19,871 Memo CU, and its whole mint 37,063 CU at a fixed limit of 37,200. Revision 5 rejects it with 6001. With 69 plane-2 ideographs the same bot would pay 0.5% more (375,430 against 373,500 lamports). Revision 5 stops this memo, but not a minter who picks the cheapest valid one.

The rest cannot be closed in memo-mint at a reasonable cost: the cost belongs to another program and varies by more than 70x between characters. The reward does not depend on the memo, and the saving goes only to the minter, who pays the fee anyway.

**Evidence**: mainnet simulations (static review). On the local validator every Memo figure above reproduced to the unit on both revisions, including the flat cost for any mix of plane-2 code points (`repro-memo-mint-audit` S00, S01, S25).

**Options**: accept the gap (recommended); or restrict the memo to a character set, which would exclude most non-Latin text. A weighted minimum such as `bytes + characters ≥ 138`, suggested to lower the CJK cost (MINT-08), is worse: 28 plane-2 ideographs would pass at about 9,300 Memo CU.

**Follow-up (November 1, 2026)**: the owner asked that memo-miner users get the cheapest memo too, so that minting with the official miner costs no more than a hand-tuned client. memo-miner 0.5.0 mints with 69 random CJK Extension B ideographs (U+20000–U+2A6D6, 276 bytes). These have been assigned since Unicode 3.1, so any build of the Memo program logs them unescaped. Random characters also make every memo unique (MINT-14). memo-miner's transaction shape (no memo accounts, no account creation):

| Memo | Local validator, Revision 5 | Mainnet simulation, Revision 4 (today) |
|---|---|---|
| memo-miner's released memo (`{"action":"mint","platform":"memo-miner","chain":"X1","t":…}`) | 41,584 (Memo 25,775) | 42,994 (Memo 25,775) |
| 69 random Extension B ideographs | 35,943 (Memo 20,097) | 37,420 (Memo 20,097) |
| Difference | −5,641 (−13.6%) | −5,574 (−13.0%) |

All 300 random Extension B memos, both ends of the range, and random memos from Extension C and the compatibility supplement cost exactly the same. Sent on the local validator with the simulated CU + 50, all landed (35,943 CU executed, 35,970 simulated after a mint earlier in the slot). The mainnet figures are a read-only simulation (`sigVerify: false`, nothing signed or sent) for an existing MEMO account. At 10 lamports per requested CU the fee falls by about 56,000 lamports per mint. The transaction grows from 520 to 728 bytes, well below the 1,232-byte limit.

### 🟡 MINT-08 (Low, open in the clients; owner decision): a minimum memo in CJK, kana, Hangul or emoji cannot be minted with the official clients

For CJK, kana, Hangul and emoji the Memo program costs 10,000–20,000 CU per character (Cyrillic and Greek about 650, Arabic and Devanagari 1,000–1,500, Thai about 5,000). Under Revision 4 a Chinese memo could stop at 23 ideographs (69 bytes); under Revision 5 it needs 69.

| Script | Memo CU, 69 characters | Whole mint, mainnet | Most characters per mint (1.4M CU, 1,232-byte transaction) |
|---|---|---|---|
| ASCII | 26,037 | 43,296 | 780 (size) |
| `é`, Cyrillic, Greek | 37,702 / 45,430 / 49,225 | 57,075 (`é`) | 390 (size) |
| plane-2 ideographs | 20,097 | 37,393 | 195 (size) |
| hiragana | 735,422 | — | 129 (CU) |
| BMP CJK (`中`) | 767,024 | 787,535 | 124 (CU) |
| Hangul | 914,270 | — | 104 (CU) |
| emoji (`🌍`) | 1,194,339 | 1,211,635 | 79 (CU) |
| U+1FAD6, U+1FAF8 | 1,368,243 | 1,385,539 | 69 (CU) |

memo-app (`build_mint_transaction`) and memo-sdk (`SIMULATION_CU_LIMIT` in Rust, JS and Python) simulate with a 400,000 CU limit:
- Every 69-character memo in the last five rows fails simulation with `ProgramFailedToComplete` at instruction 0, which the clients show to the user raw.
- Mixed with ASCII, a 69-character memo within 400,000 CU holds at most:
  - 33 `中` in memo-sdk's transaction shape (399,916 CU on mainnet Revision 5, 84 to spare);
  - 32 `中` in memo-app's shape;
  - 20 `🌍` (21 under Revision 4).
- The memo-sdk comment that 400,000 is "high enough for any MEMO transaction" is wrong.

With a higher limit the mint works, but costs about 17x (kana) to 32x (U+1FAD6) an ASCII mint (BMP CJK 18x, `🌍` 28x). Above the last column a memo cannot be minted at all: 80 `🌍` or 70 U+1FAD6 fail at instruction 1 even at 1.4M CU.

**Fix (clients)**: simulate with 1,400,000 CU (the fee still follows the simulated units plus margin), show the fee before signing, and report a memo that exceeds the budget clearly. **Owner decision**: confirm that tripling the minimum cost of a CJK memo is intended (D2).

**Status**: memo-sdk 0.2.0 simulates with 1,400,000 CU in Rust, JS and Python, and checks the memo before any RPC read. memo-miner 0.5.0 uses it. memo-app still simulates with 400,000 CU and caps memos at 706 bytes (MINT-18).

### 🟡 MINT-14 (Low, fixed in memo-miner 0.5.0): MINT-01 false positives happen on mainnet now

In the latest 1,000 mainnet mints (2026-10-31 09:47–10:10 UTC), 29 (2.9%) failed with `[1, Custom(6008)]` `OneMintPerTransaction`. Each failing slot holds exactly one successful mint with the same instructions.

Example, slot 89,870,457:
- `4BX5jGqh…` minted 0.9 MEMO and `4iqgRFL5…` failed.
- Both were signed by the same wallet and carry the same memo, `{"action":"mint","platform":"memo-miner","chain":"X1","t":1793441412}`. Only the blockhash differs.
- Each paid 568,840 lamports.

The cause:
- The wallet sent 300 mints in 402 seconds, so it runs several memo-miner instances at once.
- memo-miner's memo changes once per second (`Utc::now().timestamp()`). Two instances that mint in the same second build identical instruction lists, and when both land in the same slot the second fails.
- Both MINT-01 mitigations are already in these transactions (a timestamp, and the user as memo signer, from an older memo-miner release), and neither helps.
- A failed mint pays the full X1 fee, 1,500 + 10 × the requested CU: 568,840 lamports (about 0.00057 XNT) here. The 5,000 lamports in MINT-01's reproduction was the local validator's fee.

The local validator reproduces it. Two transactions with the same instructions and different blockhashes in one slot give "ok, then 6008", and both pay the fee. The fuzzer saw the same outcome for all 69 identical pairs that landed in one slot, and every identical re-mint in a later slot succeeded.

**Fix (memo-miner)**: make every transaction's memo unique. memo-miner 0.5.0 draws each memo as 69 random Extension B ideographs, which are also the cheapest valid memo (MINT-07, follow-up). A client can then report `[1, 6008]` after a `process_mint` as "already minted by another instance". At a CPI caller the number is ambiguous, so the client must check which program failed (from the logs):
- at memo-home `mine`, 6008 is also memo-home's `ShareTooHigh`;
- at memo-chat it is also `MemoTooFrequent`;
- at memo-blog it is also `UnsupportedMemoVersion`;
- at memo-forum it is also `InvalidMemoFormat`.

MINT-01's guidance in Revision 3 Changes now asks for a unique memo per transaction.

### 🟡 MINT-19 (Low, open in memo-app): the agent skill file teaches stale rewards, burn amounts and memo rules

`memo-app/openclaw-skills/SKILL.md`:
- keeps a reward table from before Revision 2 that contradicts the program;
- hard-codes app burn amounts that do not scale with the curve, so an agent burns 11.1% more than the minimum at today's mainnet supply, and 10x more per decade;
- its W1 mint example uses a 50-character memo that memo-mint always rejects;
- gives "~750 bytes" and "69–800 bytes" as limits;
- states X1's default compute limit as 400,000 per instruction (it is 200,000).

**Fix**:
- compute rewards and burn minimums from `getTokenSupply` with the curve;
- use a memo of at least 69 characters;
- give the real size limits: 707 bytes for a first mint with that helper, 780 when the token account exists;
- regenerate the error tables from the current IDLs.

### 🟡 MINT-22 (Low, fixed in the tools): `anchor build` in a checkout without `target/` rewrites the mainnet program IDs

In a fresh clone, or after `cargo clean`, the first `anchor build` or `anchor idl build` (anchor-cli 0.32.1) behaves as follows:
- It prints "Found incorrect program id declaration" for all nine programs.
- It generates random keypairs in `target/deploy`.
- It rewrites the first `declare_id!` of each `lib.rs`, which is the `#[cfg(feature = "mainnet")]` one.
- Testnet builds are unaffected, so nothing looks wrong. A `--features mainnet` build from that tree carries random program IDs.

An existing `target/deploy` directory, even an empty one, prevents it. The re-audit hit this in its isolated copy. The O1 unit test caught it (the bump of the rewritten ID did not match), and the sources were restored.

`deploy-common.sh` builds with `anchor build -p <program> -- --features mainnet` (Step 2) and then re-reads the program ID from the code for the deployment (Step 3). It does not compare it with the ID it verified before the build. From a tree without `target/` it would see the program as absent and take the first-deployment path. It would also report the random keypair as "the real program keypair in plaintext". That path was not run.

**Fix**: `mkdir -p target/deploy` before any anchor command in the scripts, and stop when `git diff --quiet -- 'programs/*/src/lib.rs'` fails after the build.

**Status (fixed)**: `scripts/anchor-build.sh` defines `anchor_build`, which `deploy-common.sh`, `local-validator.sh` and `run-regression-tests.sh` use for every `anchor build`. It creates `target/deploy`, saves every `programs/*/src/lib.rs` before the build and compares them after it. A file comparison replaces `git diff`, because development trees have uncommitted program changes. If any file changed, it restores the files, names them and fails, and the deployment stops before Step 3. Checked with a stand-in `anchor` that rewrites two `declare_id!`: the guard restored both files and the calling script stopped under `set -e`. A build failure still returns anchor's exit code. A real `anchor build -p memo-mint` through the guard leaves the sources unchanged and gives the testnet hash `d9b4f751…`.

### 🟡 MINT-23 (Low, mostly fixed in the tests): no committed test script runs the mainnet build

`scripts/run-unit-tests.sh` never passes `--features mainnet`, and the local validator loads the testnet-ID builds. No committed script therefore tests anything selected by the `mainnet` feature: `declare_id!`, `AUTHORIZED_MINT_PUBKEY`, `AUTHORIZED_ADMIN_PUBKEY`, and the mainnet PDA bumps.

Example: hard-coding the `mint_authority` bump as testnet's 255 in `ProcessMintTo` (mutant NM15) passes all 74 unit tests and every regression suite. On mainnet, where the bump is 254, every `process_mint_to` and so every memo-home `mine` would fail with 2006. Applying option O1a with 255 in both account structs would break every `process_mint` as well.

Decision D4 (option O1b) makes this more pressing: the final Revision 5 hard-codes both PDAs and the `mint_authority` bump per network. `test_pda_constants_per_network` checks them against `find_program_address`, but the mainnet constants only when the tests run with `--features mainnet`. A wrong mainnet constant would make every mainnet mint fail with 2006.

**Fixed (November 1, 2026)**: `run-unit-tests.sh` now runs every program's unit tests twice, for the testnet build and with `--features mainnet`. All 9 programs pass both. A mutant with the mainnet bump set to 255 fails `test_pda_constants_per_network` in the mainnet run and passes the testnet run. Still open: asserting the constants against `Anchor.toml` `[programs.mainnet]` and `[tokens.mainnet]`.

`tests.rs` already has a mainnet assertion in `test_admin_pubkey_per_network`, but no committed script compiles it in. The 74 tests pass with `--features mainnet` when run by hand, as in this re-audit's options stage.

**Fix**:
- add `cargo test -p memo-mint --lib --features mainnet` to `run-unit-tests.sh`;
- assert the mainnet PDA bumps;
- assert that the constants match `Anchor.toml` `[programs.mainnet]` and `[tokens.mainnet]`.

### ℹ️ MINT-09 (Info, fixed by owner decision D3): the `MemoTooShort` log reports bytes against a character minimum

A rejected memo logs `Memo too short: 69 bytes (minimum: 69 characters)` for `"é" × 34 + "R"`, and `136 bytes (minimum: 69 characters)` for `"é" × 68`. The character count appears in no log line. The error code and message are right; only the log line is confusing.

Option O4 logs bytes when the memo has fewer bytes than the minimum, and characters when it has counted them (+3 to +7 CU). Two other versions of the same change made the success path 2 CU per byte slower (+281 CU for `"é" × 69`, +553 for `"é" × 137`), although they did not touch it in the source. Any later change to `validate_memo_length` should therefore be measured again.

**Fixed (November 1, 2026, owner decision D3)**: the final Revision 5 logs `Memo too short: 68 bytes` when the memo has fewer bytes than the minimum, and `Memo too short: 35 characters (minimum: 69)` otherwise. A separate `#[cold]`, never-inlined function writes the line and counts the characters again, so only a rejected memo pays for the count; the accepting path costs no more than with the line written inline (Revision 5 Changes, Owner decisions). M28a–c pin the three kinds of rejection (68 ASCII bytes; 69 bytes of `é`; 68 four-byte characters in 272 bytes).

### ℹ️ MINT-10 (Info, fixed by owner decision D4: options O1b, O2b, O3): about 7,000 avoidable compute units per mint

**Fixed (November 1, 2026, owner decision D4)**: O1b (with `@ ConstraintSeeds`), O2b and O3 are in the final Revision 5, together with O4 (D3). memo-mint's own cost for a minimum ASCII memo falls from 17,088 to about 9,976 CU on mainnet (−7,112); see Revision 5 Changes, Owner decisions.

memo-mint uses about 15,600–15,700 CU on the local validator (testnet-ID build) and 17,100–17,300 on mainnet for a minimum ASCII memo. Of that:
- `seeds = [b"mint_authority"], bump` calls `find_program_address`, which charges 1,500 CU for every bump it tries. The canonical bump is 255 on testnet and 254 on mainnet, so mainnet pays 3,000 CU and testnet 1,500. The local figures in this document therefore understate mainnet by 1,446 CU.
- `mint_state` costs one `create_program_address`, 1,500 CU.
- The success log line costs about 3,200 CU: about 2,500 for the base58 owner and about 700 for the rest of the line.
- The Revision 5 count loop costs about 15.5 CU per byte for non-ASCII memos of 69–275 bytes, a third of it an overflow check.
- memo-mint costs 27 CU more when another mint has already landed in the same slot: `record_mint` compares the 32-byte fingerprint only then. This explains the mainnet pairs of 37,063 and 37,090 CU for the same bot transaction.

The options were built in an isolated copy for both networks and passed the unit tests under both features. They ran the mint and memo-home regression suites as they stood before the re-audit's new cases (52 and 160 cases, `--so-dir`).

| Option | Change | memo-mint CU, mainnet, vs Revision 5 (`a`×69 / `é`×69 / `é`×137 / `a`×700) | Regression | Notes |
|---|---|---|---|---|
| O1a | `bump = MINT_AUTHORITY_BUMP`, a per-network constant | −1,476 on all four (testnet +24) | pass | New test: the constant equals the canonical bump |
| O1b | `address = MINT_AUTHORITY_PDA` and `MINT_STATE_PDA` constants, keeping `ConstraintSeeds` as the error | −4,652 on all four (testnet −3,152) | pass | IDL: the two accounts become fixed addresses. Without the `@ ConstraintSeeds` override, a wrong `mint_authority` gives 2012 instead of 2006 |
| O2a | `emit!(Minted { … })` instead of the log line | −2,858 | 17 cases fail on the log count only | IDL gains the event; every reader of the `Minted` line breaks |
| O2b | `Minted {amount} units, supply before mint: {supply} units` (no owner) | −2,460 | pass | Readers that take the recipient from the log (README integration notes) must read it from the instruction instead: the `user` account of `process_mint`, or the `recipient` argument of `process_mint_to` (from the inner instructions for CPI mints) |
| O2c | `no-log-ix-name` as a default feature | −106 | pass | Changes the dependents' bytecode by 4.9–12k bytes when they are rebuilt |
| O3 | count continuation bytes 8 at a time; length fast path from 273 bytes | +1 / −1,847 / −4,253 / +1 | pass | 4 new unit tests against `str::chars` |
| O4 | MINT-09 log | +3 / +7 / +7 / +3 | pass | |
| O1b + O2b + O3 + O4 | | −7,110 / −8,954 / −11,364 / −7,110 | mint 51/52, home 160/160 | Measured as O5v2, built without the `@ ConstraintSeeds` override, so M16f gets 2012 instead of 2006. With the override M16f is expected to pass; that build was not made or run. A 69-character ASCII mint: fee −16.4% |
| O1a + O2b + O3 + O4 | | −3,934 / −5,778 / −8,188 / −3,934 | pass | Fee −9.1% |

`process_mint_to` changes by the same amounts within ±3 CU. Accounts, discriminators and arguments do not change in any option, so memo-sdk, memo-app and the dependent programs need no change. The old builds of memo-blog, memo-forum and memo-home passed every suite against every variant, except O2a's log-count check.

### ℹ️ MINT-11 (Info, fixed in the tools): Revision 5 relies on the Memo program's UTF-8 check

`validate_memo_length` accepts 276 continuation bytes (`[0x80; 276]`, no characters) on its length fast path. This is safe:
- memo-mint only reads instruction 0 when it is the Memo program.
- The Memo program has already rejected invalid UTF-8 in the same atomic transaction.
- The Memo program on X1 is not upgradeable (owner `BPFLoader2111…`).

On the local validator, four invalid memos fail at instruction 0 with `InvalidInstructionData` from the Memo program ("Invalid UTF-8, from byte N"), and memo-mint never runs: `[0xFF; 69]`, `"x" × 68 + 0xC3`, `[0x80; 276]` and `[0xC3; 69]`.

Before Revision 5 memo-mint needed nothing from the Memo program but its presence; now the character rule depends on it. `repro-memo-mint-audit` and `options-memo-mint-cu` assert the Memo ELF hash. `local-validator.sh`, the regression harness, `fuzz-memo-mint` and `verify-deployment` do not.

**Fix**: pin the Memo ELF hash (`f520eaf096361abbb9639ea4dc3e5388a87b9330e121f476607b87c46ef67954`) in `local-validator.sh` and `verify-deployment`.

**Status (fixed)**: X1's Memo program dumps to `f520eaf0…` on both mainnet and testnet. On both it is owned by `BPFLoader2111…` and executable.
- `local-validator.sh` now dumps it from X1 mainnet with the other external programs and loads it instead of the bundled copy. If the hash differs from the pin, the start stops; the other external programs only print a warning. The regression harness and `fuzz-memo-mint` start the validator through this script, so they are covered too.
- A Memo file with one byte appended stopped the start with the error. With the pinned build, `regression-memo-mint` passes 107/107.
- `verify-deployment` checks the Memo program on the target network: owner `BPFLoader2111…`, executable, and the pinned sha256. The check passes on both networks.

### ℹ️ MINT-12 (Info, fixed by owner decision D5): `user` and `caller` are writable but never written

`process_mint`'s `user` and `process_mint_to`'s `caller` are `#[account(mut)]`, but memo-mint never writes them; the flag only forces a write lock. Removing it is backward compatible, since callers may still pass the account writable. The new M22 cases pin the current flags and would need updating.

In practice the user is usually the fee payer, which is writable anyway, so the gain is small. If the program is to be frozen, Revision 5 is the last chance. One verifier rated this as no issue.

**Fixed (November 1, 2026, owner decision D5)**: `user` and `caller` are now plain `Signer`s. M22c and M22f now pass a read-only user / caller (B signs, A pays the fee) and expect a normal mint. With this change the unit tests pass for the testnet and the mainnet build (74/74 each), and so do all seven regression suites (curve 61, mint 103, burn 65, home 162, id 181 twice, admin-init 11). Two consequences:
- Deployment order: a dependent program (memo-chat, memo-blog, memo-forum, memo-home) rebuilt against this memo-mint passes `user` / `caller` read-only through its CPI, which Revision 4 would reject with 2000 `ConstraintMut`. Upgrade memo-mint before redeploying any dependent. The deployed dependents keep passing the account writable, which Revision 5 accepts.
- The IDL no longer marks `user` / `caller` writable. The SDKs may keep passing them writable; the fee payer is writable in any case.

### ℹ️ MINT-13 (Info, documented; extends MINT-05): equivalent and dead code

- The cap check at the start of `calculate_dynamic_mint_amount` is redundant with the `new_supply > MAX_SUPPLY` check. `ArithmeticOverflow` (6006) is produced by no input: `test_overflow_protection_at_u64_max` gets 6005.
- `check_memo_instruction`'s `current_index < 1` check is redundant: at index 0, instruction 0 is memo-mint itself, not the Memo program.
- Mutants of these lines cannot be killed by any test. Mutation testing confirmed 13 equivalent mutants, 12 of them on chain:
  - the 11 predicted by the static review; one of them (MM43) compiles to the identical binary and was not run;
  - one where Anchor's `init` already zero-fills the account;
  - one that is equivalent only for the testnet build (MINT-23).

### ℹ️ MINT-15 (Info, cut over): Revision 5 cut-over

Mainnet traffic before and after the switch:
- From 02:46 to 04:40:49 UTC on 2026-10-31, all 29,000 sampled mainnet mints came from one fee payer (`9CRGtz…`). It used `"é" × 32`-style memos of 69 bytes and 35 characters, with a fixed limit of 37,200 CU; Revision 5 rejects them with 6001.
- Earlier samples include memo-app mints (02:38–02:42) and memo-miner mints until 21:30 on October 30.
- Since 04:40:49 every sampled mint uses memo-miner's 69-character ASCII memo, which Revision 5 accepts.
- If such a client returns, it must also raise its limit. Under the audited build, every valid memo needed more than 37,200 CU for the whole mint: 69 plane-2 ideographs 37,393, TAB 41,294, letters 43,296. The deployed final Revision 5 needs about 7,100 CU less: 69 Extension B ideographs used 30,281 CU on mainnet, and 69 ASCII characters 35,866 CU on testnet. A fixed limit is still fragile, so simulate first.

On the local validator:
- memo-miner's transaction shape costs 76 CU more on Revision 5 (its memo has a signer). A transaction simulated on Revision 4 with memo-miner's 50-CU margin and executed after the upgrade fails at the ComputeBudget instruction, 26 CU short, and pays the fee. Only transactions in flight during the upgrade are affected.
- A client whose limit equals its simulated CU also fails when another mint lands earlier in the same slot (+27 CU, MINT-10). The official clients keep 50 CU or more.

**Checklist**:
- sample live traffic again before the upgrade;
- announce the character rule;
- release the clients first (MINT-18);
- upgrade the on-chain IDL with the program (the `MemoTooShort` message changed). The final IDL names the two PDAs per network: upload the mainnet build's IDL to mainnet and check its `mint_authority` and `mint_state` addresses first (Revision 5 Changes, Owner decisions);
- watch memo-mint's failure rate after the upgrade.

**Status (cut over on November 1, 2026)**:
- The new sample before the mainnet upgrade found the 35-character memos back: 1,000 mints in about four minutes, all from `9CRGtz…`.
- The mainnet IDL is the mainnet build's IDL, and its two PDA addresses were checked first.
- In the first minutes after the upgrade, no mint from that payer landed, and an Extension B test mint succeeded.
- Open items:
  - memo-app has not been released yet (MINT-18);
  - the character rule still needs to be announced;
  - the failure rate still needs watching.
- Details: Owner decisions, deployment records.

### ℹ️ MINT-16 (Info, fixed in the tools and docs): deployment tooling and procedure

- `verify-deployment` compares all 8 deployed programs with `target/deploy/*.so`. The apps link the memo-mint crate, so after a memo-mint-only upgrade it fails for every app rebuilt from the Revision 5 tree:
  - The six dependent apps it checks rebuild (`--features mainnet`) at the same size, but differ from their mainnet builds by 1,739–13,060 bytes, although their source did not change. Most of the difference comes from the memo-mint crate version (0.5.0).
  - With the version set back to 0.4.0 they still differ by a few bytes (memo-blog: 5), from panic line numbers in the changed memo-mint code.
  - The check passes only with an undocumented mixed build: the apps from the commit they were deployed from, memo-mint from the Revision 5 tree.
  - The check needs a per-release hash manifest or a program filter.
- Revision 5 hashes: testnet `8fc80212ff94ddc844d84836456e5744796096f5c1d27f6603280312a520ed1f`, mainnet `2c9b3e3198c76ee44a34996140612e84db89cfb0b9c02653cf85de9459d41c0e`, 271,080 bytes. Both fit the existing ProgramData (mainnet 283,016 bytes). These are the audited builds. The final Revision 5 (D3–D5 applied) is testnet `d9b4f75196e6721d056f4d216f40dfcebadd1791e9ffea17c67dd4398bbb67ac` and mainnet `1eb83794def585fbf570f3a4021be6acf28603cbe78cae19ad275840f2f59b9f`, 269,416 bytes each; the repo build and a separate build in an isolated copy gave the same testnet hash.
- `PROD-DEPLOYMENT.md` tells operators to run `deploy-mainnet.sh` with no arguments ("all 8 programs"). The list now has 9 programs including memo-id, so the command aborts today, and would deploy memo-id once its keypair exists (same as MEMO-ID-AUDIT F28). Name the programs explicitly: `./scripts/deploy-mainnet.sh memo_mint`.
- Its first-deployment steps leave out memo-mint's `AUTHORIZED_ADMIN_PUBKEY`.
- See MINT-22 for `anchor build` in a tree without `target/`.

**Status (fixed)**:
- **Hash manifest.** `deployments/testnet.sha256` and `deployments/mainnet.sha256` record each deployed program's ELF sha256 and size. They were first recorded from the chain on November 1, 2026. The memo-mint lines equal the final Revision 5 builds (`d9b4f751…`, `1eb83794…`), and testnet memo-burn equals its local build. After each upgrade or first deploy, `deploy-common.sh` writes the program's line into the manifest of the network it deployed to. Prod staging (a mainnet build on testnet) is not recorded. A stand-in run replaced a line, appended a new program, and wrote nothing for prod staging.
- **`verify-deployment`.** It reads the ELF length from the ELF header, because the program data is padded with zeros. It then compares the on-chain hash with the manifest of the network the RPC URL names. For prod staging, and for programs the manifest does not list, it compares with the local build. It now also checks memo-id, and skips it where memo-id is not deployed. Results:
  - testnet: 30 passed, 0 failed;
  - mainnet: 23 passed, 0 failed, 3 skipped (the deploy admin holds no MEMO);
  - mainnet as a wallet that holds MEMO: the mint simulations pass;
  - a manifest entry with one changed hex digit fails.
- **`PROD-DEPLOYMENT.md`.** The deploy commands name the programs: the eight mainnet programs, or `memo_mint` alone. The guide explains why a run without arguments is wrong. The canonical order includes memo-id. The verification step describes the manifest and the Memo check. The authority section lists:
  - `AUTHORIZED_MINT_PUBKEY` for seven programs: memo-mint, memo-burn, memo-chat, memo-profile, memo-project, memo-blog and memo-forum. memo-home takes it from memo-mint and memo-id from memo-burn, and both assert the IDs per network at compile time.
  - `AUTHORIZED_ADMIN_PUBKEY` for memo-mint, memo-burn, memo-chat, memo-project and memo-forum.
- **README.** The deploy section was also corrected: it no longer says that running the scripts without arguments is the default.

### ℹ️ MINT-17 (Info, documented): runtime changes since the core audit

- SIMD-0459 (`syscall_parameter_address_restrictions`) has been active on X1 mainnet since slot 87,958,112 (2026-10-23), so testnet and mainnet now run the same program runtime.
  - The change is in the safe direction, and a mainnet simulation of memo-blog `mint_for_blog` succeeded after it.
  - INT-12's trigger, "re-run the CPI-tampering harness when this activates on mainnet", has fired, and that harness was never committed.
  - Seven other features activated on mainnet between October 16 and 26; none touches the instructions sysvar (MINT-03).
- INT-08's open item is closed: X1 allows 24,000,000 CU per writable account per block (SIMD-0306 active, 60M block limit), twice the figure assumed so far.

### ℹ️ MINT-18 (Info, open in memo-app): client release order and client defects

- memo-miner's working tree uses `MEMO_MIN_CHARS`, but it pinned memo-sdk `v0.1.0`, which has only `MEMO_MIN_BYTES`. Fixed on November 1, 2026: memo-sdk `v0.2.0` (commit `d836435`; 1.4M simulation, characters, no memo signer) is tagged, and memo-miner 0.5.0 is released with it.
- Of the released clients, only memo-app still counts bytes. memo-sdk 0.2.0 and memo-miner 0.5.0 count characters. Revision 5 was deployed before the memo-app release. A memo-app memo of at least 69 bytes but fewer than 69 characters fails on chain with `MemoTooShort` and pays the fee. Memos typed in a Latin script have one byte per character and are not affected.
- memo-app allows memos up to 800 bytes, but its mint transaction fits at most 706.
- memo-sdk on a lone surrogate: Python raises a raw `UnicodeEncodeError`, and JavaScript silently sends U+FFFD. Client and chain still agree on acceptance.
- memo-miner and memo-app show the reward predicted from a supply read before signing, not the minted amount. Mainnet supply is above 10M, so a step can fall in between.
- The curve now exists in at least nine source copies across three repositories (INT-03); memo-app's own copy has no tests.

### ℹ️ MINT-20 (Info, fixed except SKILL.md): documentation drift

Corrected in this document:
- the Revision 5 effect on memo cost (MINT-07), the Revision 5 scope (memo-home `mine`), the slowest path and the memo-signer guidance;
- the MINT-01 mitigation;
- the memo cost in Design Confirmation #1 and the `process_mint_to` use cases;
- the account order and the compute-limit advice in Appendix C;
- the upgrade steps in Appendix B (MINT-16, MINT-22);
- the memo maximum, which for most non-Latin scripts is set by the 1.4M CU limit (MINT-08);
- the Integration Tests list in Testing Recommendations, which still listed covered tests as needed;
- the test counts and the precedence note.

Corrected elsewhere on 2026-11-01:
- **core audit** (`MEMO-CORE-AUDIT-2026-10-16.md`): a new section, "Updates after this audit", covers:
  - the character rule (M-15, INT-06, BURN-02) and the Memo program's per-character cost;
  - MINT-01 recommendation 2 (MINT-14);
  - INT-03, INT-08 and INT-12 (MINT-17);
  - section 8.2.
  The affected findings point to it, and their original text is kept.
- **`README.md`**:
  - the reward is 0.9 MEMO per mint (12.34M supply on 2026-11-01), so `create_chat_group` needs 37,862.1 MEMO;
  - the advice for unique memos says a per-second timestamp is not enough;
  - the character rule is now current on both networks.
- **`MEMO-HOME-DESIGN.md`**: `mine` needs 69 characters, and burns need 69 bytes.
- **`MEMO-SECURITY-REVIEW.md`**: has a "Superseded" notice.
- **memo-sdk**: 0.2.0 replaced the 400,000 CU comment (MINT-08).
- **`CLAUDE.md`**: the regression counts. It is local and not committed.

Still open: memo-app's SKILL.md (MINT-19).

### ℹ️ MINT-21 (Info, fixed in the tests): test gaps

The static review found gaps that let defects through, and mutation testing confirmed them. The re-audit closed them in the local-validator suites. Nothing under `programs/` changed.

**Harness (`harness.rs`)**:
- The `MintState` fingerprint oracle now uses the runtime's own code: message sanitization with reserved-key write demotion, then `construct_instructions_data`. Before, a transaction that requested a reserved account writable, or a v0 transaction over a lookup table, would have been predicted wrongly.
- New helpers cover v0 transactions and lookup tables, simulation without signatures, block order within a slot, and exact builtin errors.
- `mint_ok` now asserts the exact success line: `Minted {amount} units to {owner}, supply before mint: {supply} units` during the re-audit, and `Minted {amount} units, supply before mint: {supply} units` since D4 (O2b), with the recipient checked by its balance. Before, a log with the supply after the mint passed every suite.

**`regression-memo-mint` (52 → 103 cases; 107 with the decisions)**:

| Cases | What they cover |
|---|---|
| M20 | `process_mint_to` account substitutions |
| M21 | A shadow `MintState` (an account with the right owner, discriminator and size at a non-PDA address, planted by `prepare-local-validator`) |
| M22 | Read-only accounts: the mint and the token account are rejected; since D5 a read-only user / caller mints |
| M23 | Framework rejections, with exact errors. A mint passed as the token account gives `UninitializedAccount`, not 3003 |
| M24 | Revision 5 encoding boundaries: 4-byte characters at 68/69 characters, combining accents at 68/69 code points, 3-byte and NUL memos at 69, invalid UTF-8, mostly-ASCII memos below the minimum, and relational CU bounds that catch a dropped fast path. Since D4 (O3) the length fast path starts at 273 bytes: M24a (272 bytes, 68 characters, counted) and M24b (272 bytes, 71 characters, counted) sit just below it, M24d (273 bytes, 69 characters) at it. M24n–p pin the fast paths by memo-mint's CU, since a dropped fast path changes no outcome: a counted 272-byte memo costs at least 300 CU more than 272 ASCII bytes (ASCII path; measured +532) and than a 273-byte memo (length path from 273 bytes; measured +783), and 701 bytes cost within 1,000 of 277 on the length path. A build without the ASCII path fails only M24n, and a build whose length path starts at 276 bytes fails only M24p; both pass all 79 unit tests |
| M25 | Reserved-key write demotion, and v0 transactions over a lookup table |
| M26 | Two different mints in one slot, both minting, with `MintState` holding the later one (every run landed in one slot on the first attempt). This covers concurrent minting (Testing Recommendations, Integration Tests). The MINT-01 false positive (identical instructions in one slot fail with 6008), which core audit 8.2 lists as not covered, is reproduced only by `repro-memo-mint-audit` and the fuzzer, not by a regression case |
| M27 | The same instructions minting again in a later slot (memo-miner's pattern) |
| M28 | D3: the exact `MemoTooShort` log line for a memo short in bytes (M06) and for two memos short in characters (M06b, M24a) |

**Other suites**:
- `regression-admin-init` (4 → 11 cases): the admin's `initialize_mint_state` is simulated without the key, plus a non-signer admin and the shadow `MintState` on a fresh deployment.
- `regression-memo-home` (160 → 162 cases): H29b and H29c, the character rule through `mine`.

**Assertion fixes**:
- Assertions at CPI callers that matched more than one program's error now name the failing program and the Anchor error: M04c/d/d2/g at memo-blog and memo-forum (6008); H20, H24–H26b and H28 at `mine` (6000, 6003, 6004, 6008); M17a.
- Two admin-init checks of account state after a failed simulation were vacuous, because the RPC returns every account as null when a simulation fails. They were removed.
- A lookup-table wait without a time limit and a retry race in M26 were fixed.

**Remaining**:
- MINT-23: `run-unit-tests.sh` now runs the mainnet build; the constants are not yet checked against `Anchor.toml`.
- The MINT-01 false positive (identical instructions in one slot, 6008) has no regression case; only `repro-memo-mint-audit` and the fuzzer reproduce it.
- A re-mint in exactly the next slot is checked only by unit tests and the fuzzer.
- `tests.rs` should get a valid-UTF-8, mostly-ASCII vector: today the only unit test that catches a partial ASCII check uses invalid UTF-8, which cannot reach memo-mint on chain.

### Decisions

Program changes are the owner's to decide. The owner decided D3, D4 and D5 on November 1, 2026, and they are applied in the final Revision 5 (Revision 5 Changes, Owner decisions).

| # | Decision | Recommendation |
|---|---|---|
| D1 | Memo rule: keep the Revision 5 character count and accept that plane-2 ideographs are 13.6% cheaper than ASCII (MINT-07), or restrict the character set | Keep Revision 5; accept the gap. **Owner: accepted** (Revision 5 deployed unchanged; memo-miner 0.5.0 uses the cheapest memo) |
| D2 | Accept that a minimum CJK, kana, Hangul or emoji memo costs about 17–32x an ASCII one (BMP CJK 18x, `🌍` 28x), and fix the clients to simulate at 1.4M (MINT-08) | Accept; fix the clients. **Owner: accepted**; memo-sdk 0.2.0 and memo-miner 0.5.0 simulate at 1.4M, memo-app is pending |
| D3 | Fold option O4 (log line, +3 to +7 CU) into Revision 5 before it is deployed | Yes. **Owner: yes.** Applied; a separate cold function writes the line (MINT-09) |
| D4 | Fold the CU options into Revision 5 before it is deployed. Folding avoids a second upgrade. Each option was measured. Except O2a (17 log-count failures), each passes the mint and memo-home suites as they stood before the re-audit's new cases (52 and 160) | **Owner: O1b with `@ ConstraintSeeds` + O2b + O3 (with O4 from D3).** Applied; the final build passes all suites (mint 107, home 162, admin-init 11) and saves 7,112 CU per ASCII mint on mainnet. Recommendation before the decision: the owner's call. If yes, O1b with `@ ConstraintSeeds` (O1b_2006) + O2b + O3 + O4: about −7,110 CU and −16.4% fee per ASCII mint. That figure was measured as O5v2, which lacks the override and so returns 2012 in M16f; O1b and O1b_2006 use the same CU. O1a + O2b + O3 + O4 (−3,934, −9.1%) keeps the IDL unchanged. Re-run the final suites (mint 103, home 162, admin-init 11) on the chosen build. Avoid O2a (breaks log readers). O2c saves only 106 CU and changes the dependents' bytecode by 4.9–12k bytes when they are rebuilt. O2b changes the documented log format |
| D5 | Remove `mut` from `user` / `caller` (MINT-12) | Recommended no change (the gain is small). **Owner: remove.** Applied to `programs/memo-mint`; M22c and M22f updated |
| D6 | Deploy Revision 5: testnet first, then mainnet, after the clients are released and live traffic is sampled again (MINT-15) | Yes, in that order. **Owner: deploy now.** Deployed on testnet, then mainnet, on November 1, 2026, after a new traffic sample; memo-app was not yet released (Owner decisions, deployment records) |

Actions that need no program change:
- memo-miner: 69 random Extension B ideographs per memo, unique and the cheapest valid memo (MINT-07, MINT-14); done, released as 0.5.0;
- memo-sdk: tag `v0.2.0`, then move memo-miner's pin (MINT-18; done: memo-sdk 0.2.0 tagged, memo-miner 0.5.0 released with it);
- memo-app: 706-byte limit, simulation at 1.4M (MINT-08, MINT-18), and SKILL.md (MINT-19);
- scripts:
  - `mkdir -p target/deploy` and a program-ID check after the build (MINT-22; done: `scripts/anchor-build.sh`);
  - mainnet-feature unit tests in `run-unit-tests.sh`, so that a committed script runs the mainnet build (MINT-23; done);
  - the Memo ELF hash in `local-validator.sh` and `verify-deployment` (MINT-11; done);
  - a hash manifest for `verify-deployment`, and explicit program names in `PROD-DEPLOYMENT.md` (MINT-16; done);
- documents: the items listed in MINT-20.

### Tests (re-audit)

Final state of the working tree:

| Suite | Result |
|---|---|
| memo-mint unit tests | 74/74 (testnet build; also 74/74 with `--features mainnet` when run by hand); `programs/` unchanged |
| `run-regression-tests.sh --no-build`, all 7 suites | curve 61/61, mint 103/103, burn 65/65, home 162/162, id 181/181 at 5M and 181/181 at 2.5T, admin-init 11/11 (run twice) |
| Final Revision 5 (D3–D5), November 1, 2026 | unit tests 79/79 for both builds (`run-unit-tests.sh`, all programs, both builds); `run-regression-tests.sh` (full build): curve 61/61, mint 106/106, burn 65/65, home 162/162, id 181/181 twice, admin-init 11/11, then mint 107/107 after M24n–p were reworked; `options-memo-mint-cu --variant FINAL --log no-owner --short-log o4 --count-check 300`: 30/30; `fuzz-memo-mint` (model updated for D4/D5, low CU bounds recalibrated): seed 101, 1,500 steps and seed 202, 1,200 steps at a substitution rate of 0.35, 0 mismatched (23 read-only users minted or failed exactly as predicted) |
| `repro-memo-mint-audit` | Revision 5: 181/181 at 5M and 63/63 at the mainnet supply. Revision 4 (testnet dump `d7fd1d34…`): 173/173 and 54/54 |
| `options-memo-mint-cu` | 37 runs, every option at 5M and at the mainnet supply, all passing |
| Revision 4 against the new suites (before the mutation-stage cases) | mint 92/96 and home 161/162: it fails only the Revision 5 cases (M06b, M06c, M24a, M24f, H29b). Through memo-home `mine` it still accepts `"é" × 34 + "R"` |

Mutation testing:
- 74 mutants from the static review, 23 new and 3 for the `mut` flags:
  - 31 of the static mutants were killed by the unit tests, and most of those were not run on the validator;
  - MM43 compiles to the identical binary;
  - every other mutant was built and run against every relevant suite.
- 13 mutants are equivalent for the testnet build (MINT-13). One of them, a hard-coded testnet bump, would break every `process_mint_to` on mainnet; no current test can see it (MINT-23).
- Every other mutant is killed. One, which rejects a re-mint in exactly the next slot, survives the regression suites but is killed by the unit tests and the fuzzer.

Stateful fuzzing (`fuzz-memo-mint`):
- **Model**: independent of the program. It counts characters with `str::chars`, uses its own curve table, and follows Anchor 0.32.1's check order.
- **Coverage**: four wallets and all five mint paths: `process_mint`, `process_mint_to` (including into a memo-home vault), blog, forum and `mine`.
- **Inputs**:
  - generated memos at the boundaries, invalid UTF-8, and memos moved, duplicated or replaced;
  - 35 account and data substitutions;
  - legacy and v0 messages, and compute limits from tiny to over the cap;
  - resends in the same and in later slots.
- **Prediction**: for every transaction, the exact error or the exact balances, supply, `MintState` and log line.
- **Results**: 8,600 steps (about 9,400 transactions) on Revision 5 (11 seeds, at 5M, just below 10M, just below 2T and up to the 10T cap). The first fuzzer version reported 2 mismatches, both from a bug in the fuzzer, which was fixed and re-run. No step showed the program diverging from the model. The same fuzzer caught the deployed Revision 4 and four mutants.

Testnet and mainnet were used only for read-only RPC and `simulateTransaction` with `sigVerify: false`. No transaction was sent, no key was used, and nothing was committed.

---

## Design Confirmations & Verification

### ✅ DESIGN CONFIRMATION #1: Fair-Launch Mining Model (Unrestricted Access)

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL**

**Implementation**:
```rust
pub struct ProcessMint<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    // No rate limits - open to all users
}
```

**Design Rationale**:
This is a **fair-launch mining project** where any user can mint tokens by submitting memos. This design is intentional for the following reasons:

1. **Equal Opportunity**: All users start from zero and can participate in mining
2. **Market-Driven Supply**: Total supply depends on community participation
3. **Competition Encouraged**: Users are expected to compete for early, higher rewards
4. **No Anti-Spam Needed**: High transaction volume is the desired behavior

**One Mint per Transaction (Revision 3)**:
- Revision 2 allowed one memo to back about 31 mints in one transaction. Revision 3 allows exactly one mint per transaction, enforced with a transaction fingerprint stored in `MintState` (see Revision 3 Changes). This covers top-level instructions, the apps and any CPI caller.
- Each mint therefore pays the full memo program cost (about 43,300 CU for a whole mint with a 69-character ASCII memo on X1 mainnet, 26,037 of it in the Memo program). The effective price ceiling is the single-mint cost.

**Security Analysis**:
- ✅ Transaction fees provide natural rate limiting (network-level)
- ✅ The emission curve naturally reduces rewards as supply increases
- ✅ Early miners get higher rewards (incentivizes early participation)
- ✅ No privilege escalation possible (everyone has equal access)

**Verdict**: This is a secure and well-designed fair-launch mechanism. The lack of artificial rate limits is a feature, not a bug.

---

### ✅ DESIGN CONFIRMATION #2: Fixed Memo Position Requirement

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL - PERFORMANCE OPTIMIZED**

**Implementation**:
```rust
/// IMPORTANT: This contract enforces memo at index 0:
/// - Index 0: SPL Memo instruction (REQUIRED)
/// - Index 1+: memo-mint::process_mint or memo-mint::process_mint_to (other instructions)
/// 
/// Compute budget instructions also occupy an instruction index, so they must be
/// placed at index 1 or later; a compute budget instruction at index 0 makes the mint fail.
fn check_memo_instruction(instructions: &AccountInfo) -> Result<bool> {
    let current_index = load_current_index_checked(instructions)?;
    
    // Current instruction (process_mint) must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        return Ok(false);
    }
    
    // Check that index 0 contains the memo instruction
    match load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH)
            } else {
                Ok(false)
            }
        },
        Err(_) => Ok(false)
    }
}
```

**Design Rationale**:
The strict position requirement has been **tested and validated** for the following benefits:

1. **Performance**: Checking a fixed position is O(1) vs scanning multiple positions
2. **Predictability**: Transaction structure is deterministic and easy to verify
3. **Gas Efficiency**: Reduces compute units by avoiding position scanning loops
4. **Security**: Prevents ambiguity about which memo applies to which instruction
5. **Compute Budget Placement**: Compute budget instructions occupy an instruction index like any other instruction, so they go at index 1 or later (usually last). Wallets or aggregators that prepend them at index 0 make the mint fail; this strict rule was re-confirmed in Revision 2.
6. **No durable nonce transactions** (core audit INT-04): the runtime treats a transaction as a durable nonce transaction only when instruction 0 is `AdvanceNonceAccount`, which conflicts with the memo-at-index-0 rule. `[AdvanceNonce, memo, process_mint]` fails with `MemoRequired` (the nonce still advances and the fee is charged), and `[memo, AdvanceNonce, process_mint]` is not recognized as a nonce transaction. Offline-signing, custody or multisig flows that depend on durable nonces cannot mint.

**Required Transaction Structure**:
```
Transaction:
  [0] MemoProgram::Memo (at least 69 characters) (REQUIRED)
  [1+] MemoMint::process_mint / process_mint_to
  [last] ComputeBudgetProgram::SetComputeUnitLimit (optional, never at index 0)
```

**Security Analysis**:
- ✅ Clear, documented transaction structure
- ✅ Efficient validation with minimal compute units
- ✅ Prevents memo injection attacks
- ✅ Works reliably with tested client implementations
- ✅ Simple, fixed rule that clients can follow without scanning

**Verdict**: This is an optimal design choice that prioritizes performance and clarity. Client integration guides should document the required transaction structure.

---

### ✅ DESIGN CONFIRMATION #3: PDA Mint Authority Transfer Process

**Design Intent**: ✅ **FULLY DOCUMENTED AND IMPLEMENTED**

**Implementation** (Revision 5, decision D4: the PDA is a per-network constant checked by address):
```rust
#[account(address = MINT_AUTHORITY_PDA @ anchor_lang::error::ErrorCode::ConstraintSeeds)]
pub mint_authority: AccountInfo<'info>,
```

**Authority Transfer Process**:
The project includes a dedicated tool for transferring mint authority: `admin-transfer-memo-token-mint-authority.rs`

**Transfer Procedure**:
1. **Load Keypairs**:
   - Mint keypair: `~/.config/solana/memo-token/<network>/authority/memo_token_mint-keypair.json`
   - Program keypair: `~/.config/solana/memo-token/<network>/programs/memo_mint-keypair.json`
   - Both are stored encrypted and available while `scripts/keys.sh unlock <network>` is in effect

2. **Calculate PDA**:
   ```rust
   let (mint_authority_pda, _bump) = Pubkey::find_program_address(
       &[b"mint_authority"],
       &program_id,
   );
   ```

3. **Verify Token-2022 Mint**:
   - Confirms mint account exists
   - Validates it's a Token-2022 mint
   - Checks current ownership

4. **Transfer Authority**:
   - Uses `token_instruction::set_authority`
   - Transfers `MintTokens` authority to PDA
   - Includes compute budget optimization
   - Confirms transaction on-chain

5. **Post-Transfer State**:
   - ✅ Mint authority is now the PDA
   - ✅ PDA can only mint via program CPIs
   - ✅ No private key controls the mint authority directly
   - ⚠️ The current program has no instruction that moves the authority away from the PDA, so the transfer cannot be undone with this program; a program upgrade could add one (see below)

**Security Analysis**:
- ✅ Well-documented transfer process
- ✅ Includes validation and error handling
- ✅ PDA-based authority is industry best practice
- ⚠️ The authority cannot be transferred back **as long as the deployed code is unchanged**. memo-mint is upgradeable: whoever holds the upgrade authority can deploy code that mints without the curve and cap, or that moves the mint authority with `SetAuthority` (core audit OPS-03, OPS-05)
- ⚠️ Mint logic is governed by the program rules only while the program is not upgraded; it becomes permanent only if the upgrade authority is removed (`--final`)

**Deployment Verification Steps**:
1. Run authority transfer tool on testnet first
2. Verify PDA owns mint authority: `spl-token display <mint>`
3. Test mint transaction to confirm functionality
4. Repeat on mainnet with production keys

**Verdict**: Comprehensive and secure mint authority management. Transferring the authority to the PDA is the correct design for a decentralized fair-launch token; it is final only once the program itself is immutable.

---

### ✅ DESIGN CONFIRMATION #4: Emission Curve Boundary Behavior

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL (Revision 2; step positions and last step changed in Revision 4)**

**Implementation** (in `mint_amount_at_supply`, which also returns 1 lamport at or above the cap; `calculate_dynamic_mint_amount` checks the cap and then calls it):
```rust
let amount = if current_supply <= CURVE_START_LAMPORTS {
    BASE_MINT_AMOUNT
} else {
    // Steps take effect just above each round number
    let below = current_supply - 1;
    // Find the decade: magnitude <= below < magnitude * 10
    let mut magnitude = CURVE_START_LAMPORTS;
    let mut decade_base = BASE_MINT_AMOUNT;
    while below / 10 >= magnitude {
        magnitude *= 10;
        decade_base /= 10;
    }
    // Leading digit, always 1..=9 here
    let digit = below / magnitude;
    decade_base * (10 - digit) / 10
};
```

**Behavior at Round Numbers** (each step starts just above the round number):
- Exactly 10M: **1 token**; 10M + 1 lamport: **0.9 token**
- Exactly 100M: **0.1 token**; 100M + 1 lamport: **0.09 token**
- Exactly 1B: **0.01 token**; 1B + 1 lamport: **0.009 token**
- Above 9T up to the cap: **1 lamport**; the last mint lands exactly on 10T

**Rationale**:
1. **Round numbers users can track**: every change happens just above 10M, 20M, ..., 100M, 200M, ...
2. **No 10x cliff**: the original schedule dropped the reward 10x in one step at each tier; the curve spreads the same drop over nine steps
3. **Same long-run slope**: each decade start pays exactly 10x less than the previous one, matching the original tiers
4. **Ends at the cap**: the last step (above 9T) pays exactly 1 lamport, so supply can reach exactly 10T

**Example Scenario**:
```
Current Supply: 9,999,999.000000 tokens
User A mints: Gets 1 token   → Supply becomes 10,000,000.000000 tokens
User B mints: Gets 1 token   → Supply becomes 10,000,001.000000 tokens (exactly 10M still pays 1)
User C mints: Gets 0.9 token → Supply becomes 10,000,001.900000 tokens
```

**Security Analysis**:
- ✅ Compile-time validation ensures the curve ends at `MAX_SUPPLY` and every reward is a whole number of lamports ≥ 1
- ✅ The decade loop runs at most 5 times and `magnitude * 10 <= current_supply < MAX_SUPPLY`, so it cannot overflow
- ✅ Behavior is deterministic and covered by unit tests at all 54 round numbers
- ✅ Race conditions prevented by account locking

**Verdict**: The curve boundary logic is correct and matches the intended economic model.

---

### ✅ DESIGN CONFIRMATION #5: Memo Content Validation

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL - ACCEPTS ANY TEXT** (Revision 5: the minimum counts characters)

**Implementation**:
```rust
fn validate_memo_length(memo_data: &[u8], min_length: usize) -> Result<bool> {
    // The memo program at index 0 has already rejected invalid UTF-8
    let len = memo_data.len();
    let enough = len >= min_length
        && (len >= 4 * min_length
            || memo_data.is_ascii()
            || memo_data.iter().filter(|&&b| b & 0xC0 != 0x80).count() >= min_length);
    if !enough {
        return Err(ErrorCode::MemoTooShort.into());
    }
    Ok(true)
}
```

**Design Rationale**:
The contract **intentionally accepts any content** and only validates length. The SPL Memo program only accepts valid UTF-8, so in practice every memo is UTF-8 text:

**Supported Memo Formats**:
1. **ASCII text**: Plain text messages
2. **UTF-8 text**: Unicode messages
3. **Base64 encoded data**: Used by other contracts calling memo-mint
4. **Borsh serialized data, base64-encoded**: Used for structured data from contracts
5. **Binary data**: only after encoding it as text (for example base64), since the Memo program rejects invalid UTF-8

**Contract Integration Example**:
Other contracts in the system (memo-chat, memo-blog, memo-forum) put structured data in the memo and call `process_mint` via CPI:
```rust
// In memo-chat contract:
let memo_data = borsh::to_vec(&structured_data)?;
let base64_memo = base64::encode(&memo_data);
// Pass base64_memo to SPL Memo program
```

**Why No Content Validation**:
1. **Flexibility**: Supports multiple use cases (human messages, machine data)
2. **Contract Composability**: Other contracts can encode structured data
3. **Simplicity**: Content validation is complex and subjective
4. **Gas Efficiency**: No UTF-8 decoding in memo-mint; the Memo program already validates it
5. **Off-Chain Filtering**: Content moderation handled by frontends/indexers

**Security Analysis**:
- ✅ Minimum length enforced (69 characters). No contract maximum; the memo is bounded by the transaction size (780 ASCII characters) and, for most non-Latin scripts, first by the 1.4M CU limit (about 124 CJK characters or 79 emoji; MINT-08). The memo program charges compute units per character
- ✅ No injection attacks possible (memo is just data)
- ✅ No execution of memo content
- ✅ Off-chain services can filter/validate as needed
- ✅ Invalid UTF-8 never reaches memo-mint (the Memo program rejects it), and continuation bytes never add to the character count

**Content Moderation Strategy**:
- On-chain: Accept any UTF-8 text (current implementation)
- Off-chain: Frontends/indexers filter inappropriate content
- User-level: Clients count characters before submission

**Verdict**: The permissive memo validation is the correct design choice for a composable system. Content moderation should be handled off-chain.

---

## Additional Security Analysis

### ℹ️ INFORMATIONAL #1: Max Supply Race Condition (Non-Issue)

**Analysis**: Potential concurrent minting at max supply boundary.

**Implementation**:
```rust
if current_supply >= MAX_SUPPLY_LAMPORTS {
    return Err(ErrorCode::SupplyLimitReached.into());
}
let new_supply = current_supply.checked_add(amount)?;
if new_supply > MAX_SUPPLY_LAMPORTS {
    return Err(ErrorCode::SupplyLimitReached.into());
}
```

**Theoretical Scenario**:
1. Supply at 9,999,999,999,999.999999 tokens (1 lamport below max; the last-step reward is 1 lamport)
2. Two users submit mint transactions simultaneously
3. Both read supply < MAX_SUPPLY

**Solana's Protection Mechanism**:
✅ **Race condition is IMPOSSIBLE due to Solana's account locking**:

1. **Account Locking**: Solana runtime locks all writable accounts during transaction execution
2. **Serialization**: Transactions touching the same writable accounts execute serially
3. **Atomic Reads**: Each transaction sees a consistent snapshot of account state

**Execution Order**:
```
Time | Transaction A              | Transaction B
-----|----------------------------|---------------------------
T0   | Locks mint account         | Waits for lock
T1   | Reads supply = MAX - 1     |
T2   | Mints 1 lamport            |
T3   | Supply = MAX               |
T4   | Unlocks mint account       | Acquires lock
T5   |                            | Reads supply = MAX
T6   |                            | Fails: SupplyLimitReached
```

Since Revision 4 the last step (above 9T) pays exactly 1 lamport, so a mint at 1 lamport below the cap succeeds and lands exactly on 10T; the next mint fails with `SupplyLimitReached` (observed on a local validator in the core audit, cases C03/C04). Because every reward below the cap is at most the remaining room, the `new_supply > MAX_SUPPLY_LAMPORTS` check and the `checked_add` overflow branch can never trigger; they are kept as defense in depth (core audit MINT-05).

**Verdict**: The double-check pattern is excellent defensive programming, but Solana's account locking already prevents race conditions. No changes needed.

---

### ℹ️ INFORMATIONAL #2: process_mint_to Recipient Flexibility

**Design**: `process_mint_to` allows anyone to mint for someone else (the caller pays the fee, the recipient receives the tokens).

**Implementation**:
```rust
pub fn process_mint_to(ctx: Context<ProcessMintTo>, recipient: Pubkey) -> Result<()>

#[derive(Accounts)]
#[instruction(recipient: Pubkey)]
pub struct ProcessMintTo<'info> {
    // ...
#[account(
    mut,
    constraint = recipient_token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
    constraint = recipient_token_account.owner == recipient @ ErrorCode::UnauthorizedTokenAccount
)]
pub recipient_token_account: InterfaceAccount<'info, TokenAccount>,
```

**Revision 2 fix**: `recipient` was declared in `#[instruction(...)]` but missing from the handler signature, so the IDL listed no arguments and every IDL-generated client or Anchor CPI call failed. It is now a handler argument; the wire format is unchanged.

**Validation Performed**:
- ✅ Token account belongs to correct mint
- ✅ Token account owner matches the declared recipient (a consistency check that catches a wrong account; the recipient does not sign, and does not need to). It is not access control and does not mean the recipient agreed to receive tokens (core audit MINT-06)
- ✅ Uses `InterfaceAccount` for Token2022 compatibility

**Flexibility by Design**:
The contract allows minting to **any valid token account**, including:
- User wallets (standard use case)
- Program-owned accounts (for contract integration)
- Multi-sig accounts
- Any address the caller specifies

**Use Cases**:
1. **Direct User Minting**: User mints to their own account
2. **Contract Integration**: memo-home `mine` mints into the Asset's Vault through `process_mint_to` (the Vault PDA is the recipient) and then forwards `share_bps` of the reward to its `recipient_token_account` (memo-chat, memo-blog and memo-forum call `process_mint` for the signing user)
3. **Airdrops**: Distribute tokens programmatically, one mint per transaction (since Revision 3 a transaction cannot mint to several recipients)

**Why No Recipient Restrictions**:
1. **Composability**: Other contracts need to call process_mint_to
2. **Flexibility**: Users should be free to mint to any valid account
3. **Simplicity**: No need for whitelist/blacklist management
4. **Permissionless**: Aligns with fair-launch philosophy

**Not Restricted**:
- ❌ No blacklist checking (would be centralized)
- ❌ No burn address prevention (user choice). Note: in Token-2022, a token account whose owner field is the incinerator address or the System program ID can be burned by anyone without authorization, and such burns do not go through memo-burn. Minting to such an account costs only the caller's own fee and one-mint slot; no security property is affected (core audit MINT-06)
- ❌ No program account restrictions (breaks composability)

**Verdict**: Permissionless recipient selection is the correct design for a fair-launch token. Restrictions would contradict the open-access philosophy.

---

## Code Quality Excellence

### ✅ Best Practice #1: Integer-Only Logging

**Implementation** (Revision 2; Revision 5 dropped the owner, decision D4):
```rust
msg!("Minted {} units, supply before mint: {} units", amount, current_supply);
```

**Why This is Excellent**: 
- ✅ No floating-point formatting on-chain (expensive in compute units)
- ✅ One log line per successful mint; error paths keep their detailed logs
- ✅ All math uses integer arithmetic

---

### ✅ Best Practice #2: Descriptive Error Messages

**Implementation**:
```rust
#[error_code]
pub enum ErrorCode {
    #[msg("Memo too short. Must be at least 69 characters.")]
    MemoTooShort,
    
    #[msg("Supply limit reached. Maximum supply is 10 trillion tokens.")]
    SupplyLimitReached,
    
    // ... all errors have clear descriptions
}

// Runtime messages include actual values (Revision 5, decision D3)
msg!("Memo too short: {} bytes", len);  // fewer bytes than the minimum
msg!("Memo too short: {} characters (minimum: {})", chars, min_length);
msg!("Minted {} units, supply before mint: {} units", amount, current_supply);
```

**Why This is Excellent**: 
- ✅ Error enums have clear descriptions
- ✅ Runtime logs include actual values
- ✅ Helps with debugging and user feedback
- ✅ Balance between static errors and dynamic context

---

### ✅ Best Practice #3: Compile-Time Constant Validation

**Implementation**:
```rust
const _: () = {
    assert!(MAX_SUPPLY_TOKENS <= u64::MAX / DECIMAL_FACTOR, "MAX_SUPPLY_TOKENS too large");
    assert!(CURVE_START_LAMPORTS * 10u64.pow(CURVE_DECADES) == MAX_SUPPLY_LAMPORTS, "Curve must end at max supply");
    assert!(BASE_MINT_AMOUNT % 10u64.pow(CURVE_DECADES) == 0, "Base mint amount must be divisible across all decades");
    assert!(BASE_MINT_AMOUNT / 10u64.pow(CURVE_DECADES) >= 1, "Minimum mint amount must be positive");
};
```

**Why This is Excellent**: 
- ✅ Catches configuration errors at compile-time
- ✅ Prevents deployment of misconfigured contract
- ✅ Zero runtime cost (checked at compilation)
- ✅ Self-documenting invariants
- ✅ Industry best practice for Rust/Anchor contracts

---

## Security Analysis by Category

### 1. Access Control ✅ SECURE

**Strengths**:
- Mint address is hardcoded and validated
- PDA-based mint authority prevents external minting
- Token account ownership validated
- Memo requirement enforced (prevents unauthorized minting)

**Design Intent**:
- No rate limiting (intentional - fair-launch model)
- No user-level restrictions (intentional - permissionless)
- Anyone can call mint functions (intentional - equal opportunity)

**Verdict**: Secure within stated design. The fair-launch model with unrestricted access is intentional and verified as secure.

---

### 2. Arithmetic Safety ✅ SECURE

**Strengths**:
```rust
// Checked addition prevents overflow
let new_supply = current_supply.checked_add(amount)
    .ok_or(ErrorCode::ArithmeticOverflow)?;

// Compile-time validation
assert!(MAX_SUPPLY_TOKENS <= u64::MAX / DECIMAL_FACTOR);
```

**Analysis**:
- The cap addition uses `checked_add` ✓
- The curve (`mint_amount_at_supply`) uses plain integer operations (`-`, `*`, `/`), not checked ones; the core audit proved they cannot overflow or panic for any `u64` input (`below = supply - 1` only runs when supply > 10M, `magnitude * 10 <= below`, the digit is 1..=9), and `[profile.release] overflow-checks = true` turns any unexpected overflow into a failed transaction instead of a wrapped value ✓
- Compile-time constant validation ✓
- No floating-point math in critical paths ✓
- Proper error handling ✓

**Verdict**: Excellent arithmetic safety implementation.

---

### 3. PDA Validation ✅ SECURE

**Implementation** (Revision 5, decision D4, option O1b):
```rust
// per network (cfg feature "mainnet"); test_pda_constants_per_network checks them
pub const MINT_AUTHORITY_PDA: Pubkey = pubkey!("C1r27YsPiY6wbVvRY2cVCsaRKK5HiXxdvWhxJd5wapSh"); // mainnet
pub const MINT_AUTHORITY_BUMP: u8 = 254;                                                        // mainnet
pub const MINT_STATE_PDA: Pubkey = pubkey!("EZheeAswQ9exUMpKFjHrQrczix5LSzjakroRFiMgJfWv");     // mainnet

/// CHECK: PDA serving as mint authority (seeds = [b"mint_authority"])
#[account(address = MINT_AUTHORITY_PDA @ anchor_lang::error::ErrorCode::ConstraintSeeds)]
pub mint_authority: AccountInfo<'info>,

#[account(mut, address = MINT_STATE_PDA @ anchor_lang::error::ErrorCode::ConstraintSeeds)]
pub mint_state: Account<'info, MintState>,

// in the handler: sign with the canonical bump
&[&[b"mint_authority".as_ref(), &[mint_authority_bump]]]   // MINT_AUTHORITY_BUMP
```

**Analysis**:
- Any account other than the PDA is rejected, with the same error as before (2006 `ConstraintSeeds`) ✓
- `mint_state` keeps the owner and discriminator checks of `Account<MintState>`; the address check replaces `seeds` + `bump = mint_state.bump` ✓
- The constant bump is the canonical one; a wrong bump would make the Token-2022 `mint_to` fail its signer check, not mint ✓
- `test_pda_constants_per_network` compares the constants with `find_program_address`; `run-unit-tests.sh` runs it for the testnet and the mainnet build (MINT-23) ✓
- Revision 2 removed a second, manual `find_program_address`; Revision 5 removes the remaining derivations (1,500 CU per hash, MINT-10) ✓

**Verdict**: Robust PDA validation.

---

### 4. Account Validation ✅ SECURE

**Strengths**:
```rust
#[account(
    mut,
    constraint = mint.key() == AUTHORIZED_MINT_PUBKEY @ ErrorCode::UnauthorizedMint
)]
pub mint: InterfaceAccount<'info, Mint>,

#[account(
    mut,
    constraint = token_account.mint == mint.key() @ ErrorCode::InvalidTokenAccount,
    constraint = token_account.owner == user.key() @ ErrorCode::UnauthorizedTokenAccount
)]
pub token_account: InterfaceAccount<'info, TokenAccount>,
```

**Analysis**:
- Mint address hardcoded and validated ✓
- Token account ownership checked ✓
- Token account mint verified ✓
- Instructions sysvar address validated ✓
- Uses `InterfaceAccount` for Token2022 ✓

**Verdict**: Comprehensive account validation.

---

### 5. Reentrancy Protection ✅ SECURE

**Analysis** (wording corrected in the core audit, MINT-04):
- `MintState` is checked and updated in memory (`record_mint`) before the only external call, the Token-2022 `mint_to` CPI ✓
- The CPI is the last action of the handler; Anchor writes the updated `MintState` back to the account in its exit step, i.e. after the CPI returns. During the CPI the stored `MintState` still holds the old value ✓
- This is safe today: Token-2022 `mint_to` makes no CPI of its own, `MintState` is not passed to it, the runtime forbids A→B→A reentrancy, and a calling app cannot write back a stale copy of `MintState` (Anchor only writes accounts owned by the executing program) ✓
- If a future version CPIs into a program that could call back into memo-mint, or passes `MintState` to a CPI, it should persist `MintState` (`mint_state.exit(&crate::ID)?`) right after `record_mint`

**Verdict**: Not vulnerable to reentrancy attacks.

---

### 6. Supply Cap Enforcement ✅ SECURE

**Implementation**:
```rust
if current_supply >= MAX_SUPPLY_LAMPORTS {
    return Err(ErrorCode::SupplyLimitReached.into());
}

let new_supply = current_supply.checked_add(amount)?;

if new_supply > MAX_SUPPLY_LAMPORTS {
    return Err(ErrorCode::SupplyLimitReached.into());
}
```

**Analysis**:
- Double-check prevents overflow ✓
- The cap is enforced on circulating (outstanding) supply ✓
- Account locking serializes mints at the boundary (see Informational #1) ✓

**Design note**: the 10T limit is a cap on **circulating** supply, not on total issuance (core audit MINT-05). While supply equals the cap, every operation that mints fails with `SupplyLimitReached` (6005), including chat `send_memo_to_group`, blog `mint_for_blog` and forum `mint_for_post`, which roll back as a whole until someone burns. At the current supply this is far away. The cap and the curve both read the live `mint.supply`. Burns lower it, so they free room under the cap and can move the reward back up a step. Total tokens ever minted are therefore not capped; this is intentional, so minting never stops permanently. The burn minimums of chat, profile, project, blog and forum, and memo-id's ID burn price, use the same curve value (`mint_amount_at_supply`), so near a step boundary a burn can raise them by one step as well.

**Verdict**: Secure; behavior at the boundary is verified by unit tests.

---

### 7. Memo Validation ✅ SECURE

**Implementation**:
```rust
fn check_memo_instruction(instructions: &AccountInfo) -> Result<bool> {
    let current_index = load_current_index_checked(instructions)?;
    
    // Current instruction (process_mint) must be at index 1 or later
    // to leave index 0 available for memo
    if current_index < 1 {
        return Ok(false);
    }
    
    // Check that index 0 contains the memo instruction
    match load_instruction_at_checked(0, instructions) {
        Ok(ix) => {
            if ix.program_id == MEMO_PROGRAM_ID {
                validate_memo_length(&ix.data, MEMO_MIN_LENGTH)
            } else {
                Ok(false)
            }
        },
        Err(_) => Ok(false)
    }
}
```

**Strengths**:
- Memo program ID verified ✓
- Length constraints enforced (69 characters, Revision 5) ✓
- Empty memo rejected ✓
- Fixed position (index 0) for performance ✓
- Clear transaction structure ✓

**Verdict**: Secure and optimized design with fixed memo position for performance.

---

### 8. Token2022 Compatibility ✅ SECURE

**Implementation**:
```rust
use anchor_spl::token_2022::{self, Token2022};

pub struct ProcessMint<'info> {
    pub mint: InterfaceAccount<'info, Mint>,
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Program<'info, Token2022>,
}

token_2022::mint_to(
    CpiContext::new_with_signer(
        token_program.to_account_info(),
        token_2022::MintTo {
            mint: mint.to_account_info(),
            to: token_account.to_account_info(),
            authority: mint_authority.to_account_info(),
        },
        &[&[b"mint_authority".as_ref(), &[mint_authority_bump]]]
    ),
    amount
)?;
```

**Analysis**:
- Uses `InterfaceAccount` for Token2022 compatibility ✓
- Uses `token_2022::mint_to` CPI ✓
- Program type is `Token2022` ✓

**Verdict**: Proper Token2022 implementation.

---

### 9. Network Configuration ✅ SECURE

**Implementation**:
```rust
#[cfg(feature = "mainnet")]
declare_id!("8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX");

#[cfg(not(feature = "mainnet"))]
declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");

#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");
```

**Analysis**:
- Feature flags separate testnet/mainnet ✓
- Program IDs match Anchor.toml ✓
- Mint addresses match Anchor.toml ✓
- Compile-time configuration prevents misuse ✓

**Verification**:
- ✅ Testnet program ID: A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy
- ✅ Mainnet program ID: 8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX
- ✅ Testnet mint: HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1
- ✅ Mainnet mint: memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick

**Verdict**: Proper network configuration management.

---

## Best Practices Observed

### 1. ✅ Code Documentation
- Clear comments explaining instruction ordering
- Function documentation
- Constant explanations

### 2. ✅ Error Messages
- Descriptive error messages with context
- Custom error codes
- Helpful debug logging

### 3. ✅ Defensive Programming
- Compile-time constant validation
- Double-checks for critical operations
- Explicit error handling

### 4. ✅ Code Organization
- Shared logic extracted (`execute_mint_operation`)
- Clear separation of concerns
- Modular validation functions

### 5. ✅ Integer Arithmetic
- Integer-only math; the cap addition is checked, and the curve uses plain operations proven not to overflow (see Arithmetic Safety)
- No unchecked conversions
- `overflow-checks = true` in the release profile as a backstop

---

## Pre-Production Deployment Checklist

### ✅ Design Verification (COMPLETED)

All design decisions up to Revision 4 have been confirmed as intentional; Revision 5 decisions D1–D6 were made by the owner on November 1, 2026 (Revision 5 Re-Audit, Decisions):
- ✅ Fair-launch unrestricted minting model
- ✅ Fixed memo position requirement
- ✅ PDA mint authority transfer process
- ✅ Emission curve boundary behavior (Revision 4)
- ✅ Permissive memo content validation
- ✅ One mint per transaction (Revision 3; Revision 2 allowed several)

### 🔴 CRITICAL - Required Before Mainnet Launch

#### 1. Testnet Validation
- [ ] Deploy to testnet with `--features` flag **OFF**
- [ ] Transfer mint authority to PDA using transfer tool
- [ ] Execute test mint transactions with exact instruction structure:
  ```
  [0] MemoProgram::Memo (69+ characters) (REQUIRED)
  [1] MemoMint::process_mint
  [last] ComputeBudgetProgram::SetComputeUnitLimit (optional, never at index 0)
  ```
- [ ] Verify logs show correct curve amounts
- [ ] Test `process_mint_to` (to another wallet, mismatched recipient, missing token account)
- [ ] Measure compute units of a real mint transaction
- [ ] Test error cases (short memo, no memo, compute budget at index 0, two mints in one transaction); a long memo is bounded by the transaction size (780 ASCII characters) and, for most non-Latin scripts, first by the 1.4M CU limit (about 124 CJK characters or 79 emoji; MINT-08)

#### 2. Mainnet Deployment Preparation
- [ ] Compile with `--features mainnet` flag
- [ ] Verify program ID: `8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX`
- [ ] Verify mint address: `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- [ ] Deploy to mainnet
- [ ] Transfer mint authority to PDA
- [ ] Verify PDA authority: `spl-token display memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- [ ] Execute test mint with small compute budget
- [ ] Verify a mint succeeds and gives the expected curve amount (0.9 MEMO at today's mainnet supply, above 10M)
- [ ] Upgrade the on-chain IDL (`process_mint_to` now takes `recipient`)

#### 3. Documentation for Users/Integrators
- [ ] Transaction structure requirements (instruction ordering)
- [ ] Minimum memo length: 69 characters (not bytes; Revision 5)
- [ ] No contract maximum memo length; the memo is bounded by the transaction size (780 ASCII characters) and, for most non-Latin scripts, first by the 1.4M CU limit (about 124 CJK characters or 79 emoji; MINT-08)
- [ ] Emission curve explanation
- [ ] Example transactions for wallets
- [ ] CPI integration guide for contracts

### ⚠️ RECOMMENDED - Post-Launch Monitoring

#### 4. Operational Monitoring
- [ ] Track current supply and the current curve step
- [ ] Monitor mint transaction rate
- [ ] Alert on supply approaching the next round number
- [ ] Track failed transactions (memo too short, `OneMintPerTransaction`, `SupplyLimitReached`)

#### 5. Community Resources
- [ ] User guide: How to mint tokens
- [ ] Wallet integration guide
- [ ] API/indexer for supply metrics
- [ ] Block explorer integration

### ℹ️ OPTIONAL - Future Enhancements

#### 6. Analytics & Insights
- [ ] Mint rate dashboard
- [ ] Emission curve visualization
- [ ] Memo content analysis (off-chain)
- [ ] User participation metrics

---

## Testing Recommendations

### Unit Tests (implemented, Revision 4)

`programs/memo-mint/src/tests.rs` — at the time of the core audit, 60 hand-written tests plus the Anchor-generated `test_id`, 61 in total, all passing (testnet and `--features mainnet` builds). Regression tests recommended by the core audit are being added afterwards; see the current `tests.rs` for the up-to-date list. As audited:
- Emission curve: below the curve start, the current mainnet supply, every round number from 10M to 9T and one lamport below each (checked against an independent implementation of the formula), decade starts, monotonicity, minimum reward, `mint_amount_at_supply` equal to `calculate_dynamic_mint_amount` below the cap and 1 lamport at or above it
- Max supply: exactly at the cap, above it, the last 1-lamport mint landing exactly on the cap, values near `u64::MAX`
- Memo length: empty, below/at/above the 69-byte minimum, long memos, binary and text content (Revision 5 counts characters; see Revision 5 Changes for the added tests)
- One mint per transaction: fingerprints built with the runtime's instructions sysvar serializer; error code numbering
- `process_mint_to` wire format: 8-byte discriminator + 32-byte recipient

Not covered by unit tests at the time of the core audit (TEST-01, TEST-03, TEST-09): `check_memo_instruction`, the instruction handlers, account constraints and `initialize_mint_state`. `test_cpi_loop_mints_once` calls `record_mint` repeatedly with the same input, so it shows that `record_mint` rejects a repeated fingerprint, not that a CPI sees the same sysvar bytes. These paths were exercised on a local validator during the core audit, but that harness is not committed.

Since then the local-validator suites cover these paths: `regression-memo-mint` (107 cases) and `regression-admin-init` (11 cases) exercise `check_memo_instruction`, both instruction handlers, the account constraints and `initialize_mint_state`. memo-mint has 79 unit tests in the final Revision 5 (74 as audited; Revision 5 Re-Audit, MINT-21).

### Integration Tests (covered since the core audit)

The integration tests this section used to list as needed now exist on the local validator:

1. **Concurrent minting**: two different mints in one slot (`regression-memo-mint` M26) and the same instructions in a later slot (M27); curve steps and the cap in `regression-memo-curve` and `fuzz-memo-mint` (four wallets, up to the 10T cap).
2. **Transaction structure**: instruction orderings, compute budget and additional instructions in `regression-memo-mint` M01–M25.
3. **Error paths**: memo lengths, wrong token accounts, wrong mints and the supply cap in `regression-memo-mint` M01–M25 and `regression-memo-curve`.

Still open: the items under MINT-21 **Remaining**. MINT-23 is mostly fixed: `run-unit-tests.sh` runs the mainnet build.

---

## Audit Conclusion

### Final Status: ✅ **APPROVED FOR MAINNET DEPLOYMENT**

The memo-mint contract has passed comprehensive security review with **all design decisions up to Revision 4 verified as intentional** (the Revision 5 decisions were made by the owner on November 1, 2026, and Revision 5 is deployed on both networks; see Revision 5 Re-Audit).

### Security Assessment: **EXCELLENT**

**Critical Security Strengths**:
- ✅ **Arithmetic Safety**: Checked cap addition, curve arithmetic proven overflow-free for every `u64` input, compile-time constant validation, `overflow-checks = true`
- ✅ **Access Control**: PDA-based mint authority with proper validation
- ✅ **Account Security**: Comprehensive constraint validation for all accounts
- ✅ **Token2022 Compatibility**: Proper interface implementation
- ✅ **Reentrancy Protection**: `MintState` is checked and updated before the single Token-2022 CPI; the CPI cannot call back
- ✅ **Supply Cap Enforcement**: Double-check pattern on circulating supply
- ✅ **Error Handling**: Descriptive errors with runtime context
- ✅ **Network Configuration**: Proper testnet/mainnet separation

**Confirmed Design Features**:
- ✅ **Fair-Launch Model**: Unrestricted minting is intentional (equal opportunity for all)
- ✅ **Performance Optimized**: Fixed memo position at index 0 reduces compute units
- ✅ **Flexible Integration**: any UTF-8 text, including base64-encoded binary or Borsh data, enables contract composability
- ✅ **Emission Curve**: Round-number steps confirmed as economically sound (Revision 2, adjusted in Revision 4)
- ✅ **Permissionless**: No artificial restrictions align with decentralization goals
- ✅ **One Mint per Transaction**: Batch minting was removed in Revision 3; any second mint in a transaction fails with `OneMintPerTransaction`

**Code Quality**: **EXCELLENT**
- Clean, well-documented code
- Defensive programming throughout
- Industry best practices followed
- Minimal complexity, maximum clarity

### Risk Assessment

**Security Risk**: ✅ **LOW**
- No critical vulnerabilities identified
- No critical, high or medium issue is open; the Revision 5 re-audit's open low findings are in memo-app (MINT-08, MINT-19), and MINT-23 is mostly fixed
- Design intent confirmed for all decisions up to Revision 4; Revision 5 decisions D1–D6 made by the owner (November 1, 2026)

**Deployment Risk**: ✅ **LOW**
- Clear deployment procedure documented
- Authority transfer tool implemented
- Testnet validation path defined

### Mainnet Deployment Authorization

**The memo-mint contract is APPROVED for mainnet deployment**, subject to completing the pre-deployment checklist and, for Revision 5, the MINT-15 cut-over checklist in the order of D6:

### Required Actions Before Launch:
1. ✅ Complete testnet validation cycle
2. ✅ Verify all program IDs and addresses
3. ✅ Transfer mint authority to PDA
4. ✅ Execute test mint transaction on mainnet
5. ✅ Document transaction structure for integrators

### Post-Launch Recommendations:
- Monitor supply as it approaches each round number
- Track mint transaction success rate
- Provide integration guides for wallets/dApps
- Set up analytics dashboard

---

## Summary for Stakeholders

**Contract Name**: memo-mint  
**Purpose**: Fair-launch token minting with dynamic rewards  
**Security Status**: ✅ Production Ready  
**Risk Level**: LOW  
**Code Quality**: Excellent  

**Key Findings**:
- Zero critical security issues
- All design decisions up to Revision 4 verified as intentional; Revision 5 decisions made by the owner and Revision 5 deployed (November 1, 2026)
- Excellent code quality and safety practices
- Proper Token2022 implementation
- Clear deployment procedure

**Recommendation**: **APPROVED FOR MAINNET** after testnet validation

---

## Auditor Notes

This audit confirms that the memo-mint contract implements a well-designed fair-launch token mechanism with:
- Strong security foundations
- Clear economic incentives
- Excellent code quality
- Proper testing and deployment procedures

All initial questions were answered satisfactorily, confirming that design choices that initially appeared unusual are in fact intentional and aligned with the project's fair-launch philosophy.

**Revision 2 (October 13, 2026)** changed the emission curve, fixed `process_mint_to`, and reduced compute units; see "Revision 2 Changes" above.

**Remaining operational risk**: the program upgrade authority, `AUTHORIZED_ADMIN_PUBKEY`, the IDL authority and the token metadata update authority are the same hot key (mainnet `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ`, not a multisig), and the same key has also been used for testnet staging deployments (core audit OPS-02, OPS-05). Whoever holds the upgrade authority can replace the program and mint without these rules. Consider moving the upgrade authority to a multisig with a timelock or making the program immutable (`--final`) once the current revision is stable on mainnet.

---

## Appendix A: Code Quality Metrics

- **Lines of Code**: 461 in `lib.rs` (Revision 4 with the October 16 follow-up)
- **Functions**: 3 instructions (`process_mint`, `process_mint_to`, `initialize_mint_state`), 2 public helpers (`transaction_fingerprint`, `mint_amount_at_supply`), 6 private helpers
- **Complexity**: Moderate
- **Test Coverage**: 61 unit tests at the time of the core audit (60 hand-written + `test_id`; see Testing Recommendations); 79 in the final Revision 5, plus 107 `regression-memo-mint` and 11 `regression-admin-init` cases on a local validator
- **Documentation**: Good
- **Error Handling**: Excellent

---

## Appendix B: Mainnet Deployment Procedure

memo-mint is already deployed on X1 mainnet (`8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX`), `MintState` is initialized and the mint authority is already the PDA. New versions are **upgrades** of that program, not fresh deployments. Use `./scripts/deploy-mainnet.sh memo_mint` (name the programs explicitly; see MINT-16; a run without arguments would include memo-id). The script builds with the `mainnet` feature, upgrades the named programs in canonical order (memo-mint, memo-burn, then the apps), runs the admin init clients and upgrades the IDL.

> **Corrected (core audit OPS-03)**: earlier versions of this appendix used `anchor build --features mainnet` (rejected by Anchor; the feature must follow `--`), `anchor deploy ... --provider.cluster mainnet` (`mainnet` means Solana mainnet-beta in Anchor, not X1, and `anchor deploy` makes a fresh deployment at a new address instead of upgrading), read the program ID from `target/deploy/memo_mint-keypair.json` (that file is not the real program keypair; the real keypairs are kept encrypted, see `scripts/keys.sh`) and omitted the `MintState` initialization. Following those steps could not bypass any safety check (a binary at the wrong address fails with `DeclaredProgramIdMismatch`), but it would not upgrade the real program.

### Step-by-Step Upgrade Guide (manual equivalent of the script)

**1. Build for Mainnet**
```bash
mkdir -p target/deploy                          # without it, anchor build rewrites the mainnet declare_id! (MINT-22)
anchor build -p memo-mint -- --features mainnet
git diff --quiet -- 'programs/*/src/lib.rs'     # on the committed release tree this must succeed; otherwise stop (MINT-22)
                                                # deploy-mainnet.sh does both through anchor_build (scripts/anchor-build.sh)
sha256sum target/deploy/memo_mint.so   # record and publish the hash
```

**2. Unlock the Mainnet Keys** (on the dedicated mainnet machine only)
```bash
./scripts/keys.sh unlock mainnet
```

**3. Upgrade the Program** (X1 mainnet RPC, never `--provider.cluster mainnet`)
```bash
anchor upgrade target/deploy/memo_mint.so \
  --program-id 8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX \
  --provider.cluster https://rpc.mainnet.x1.xyz \
  --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json
```

**4. Initialize `MintState` (only if it does not exist yet)**
```bash
MEMO_PROGRAM_ENV=mainnet cargo run --manifest-path clients/Cargo.toml --bin admin-init-mint-state -- \
  8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX https://rpc.mainnet.x1.xyz
# Does nothing if the account already exists (it exists on testnet and mainnet)
```

**5. Verify PDA Authority** (unchanged by upgrades; the one-time transfer to the PDA was done at launch with `admin-transfer-memo-token-mint-authority`)
```bash
spl-token display memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick --program-id TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb --url https://rpc.mainnet.x1.xyz
# "Mint authority" must be C1r27YsPiY6wbVvRY2cVCsaRKK5HiXxdvWhxJd5wapSh (the memo-mint PDA)
```

**6. Upgrade the On-Chain IDL**
```bash
anchor idl upgrade 8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX --filepath target/idl/memo_mint.json --provider.cluster https://rpc.mainnet.x1.xyz --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json
```

**7. Lock the Keys**
```bash
./scripts/keys.sh lock mainnet
```

**8. Test Mint Transaction**
```bash
cargo run --manifest-path clients/Cargo.toml --bin test-memo-mint valid-memo
# Should succeed and mint the curve amount (0.9 MEMO at today's mainnet supply, above 10M)
```
Then run `cargo run --bin verify-deployment -- <WALLET_WITH_MEMO>` with `program_env = "mainnet"` and the mainnet cluster. It compares every program with `deployments/mainnet.sha256`, which the deploy script updated for memo-mint (Revision 5: `1eb83794…`, 269,416 bytes). The dependents keep their recorded hashes, so a memo-mint-only upgrade passes (MINT-16).

**9. Verify Contract Constants**
- Program ID: `8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX`
- Mint address: `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- Token decimals: `6`
- Max supply: `10,000,000,000,000` tokens (circulating)
- Curve start: `10,000,000` tokens
- Memo length: at least `69` characters (no contract maximum)

**10. Monitor After the Upgrade**
- Track the first mint transactions
- Verify curve amounts are correct
- Monitor for any failed transactions
- Check PDA authority remains unchanged

---

## Appendix C: Transaction Structure Reference

### Required Instruction Order

All mint transactions MUST follow this exact structure:

```
Transaction Instructions:
  [0] MemoProgram::Memo
      - Data: at least 69 characters of UTF-8 text (no contract maximum)
      - Program: MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr
      - Required: YES
  
  [1+] MemoMint::process_mint OR MemoMint::process_mint_to (exactly one per transaction)
      - Mints tokens based on the emission curve
      - Validates memo at index 0
      - Executes token mint CPI
      - A second mint in the same transaction (top-level or via any CPI) fails
        with OneMintPerTransaction (Revision 3)
  
  [last] ComputeBudgetProgram::SetComputeUnitLimit (optional)
      - Sets compute budget for transaction; X1 charges for the requested limit,
        so simulate first and set the simulated units plus a margin
        (at least 50 CU: memo-mint uses 27 CU more when another mint lands
        earlier in the same slot; see MINT-10, MINT-15)
      - Must NOT be at index 0
```

### Example: Successful Mint Transaction

```rust
// Instruction 0: Memo (69+ characters) - REQUIRED at index 0
spl_memo::build_memo(
    b"This is a valid memo with at least 69 characters of content for token minting...",
    &[]
)

// Instruction 1: Mint
memo_mint::process_mint(
    user: Signer (writable),
    mint: Pubkey (writable),
    mint_authority: Pubkey (PDA [b"mint_authority"]),
    token_account: Pubkey (writable, the user's MEMO token account),
    token_program: Token2022,
    instructions: Sysvar,
    mint_state: Pubkey (PDA [b"mint_state"], writable, last)
)

// Last instruction: Compute Budget (optional, never at index 0)
ComputeBudgetInstruction::set_compute_unit_limit(simulated_units + margin)
```

### Common Integration Errors

❌ **Wrong instruction order** - Memo not at index 0
❌ **process_mint at index 0** - Must be at index 1 or later
❌ **Memo too short** - Fewer than 69 characters (a 69-byte memo of multi-byte characters is too short)
❌ **Transaction too large** - The memo and instructions together exceed the 1,232-byte transaction size limit
❌ **Wrong token program** - Using SPL Token instead of Token2022
❌ **Compute budget at index 0** - Pushes the memo out of index 0
❌ **Durable nonce transaction** - `AdvanceNonceAccount` must be at index 0, which conflicts with the memo rule; not supported
❌ **Two mints in one transaction** - The second fails with `OneMintPerTransaction`
❌ **Missing `mint_state` account** - Must be the last account (`AccountNotEnoughKeys` otherwise)
❌ **Old IDL for process_mint_to** - Missing the `recipient` argument

See also the integration notes in `README.md`.

---

## Appendix D: Economic Model Reference

### Emission Curve (Revision 4)

| Supply before the mint | Reward per mint |
|---|---|
| 0 – 10M (including 10M) | 1 |
| (10M – 20M] ... (90M – 100M] | 0.9 ... 0.1 |
| (100M – 200M] ... (900M – 1B] | 0.09 ... 0.01 |
| (1B – 2B] | 0.009 |
| (10B – 20B] | 0.0009 |
| (100B – 200B] | 0.00009 |
| (1T – 2T] | 0.000009 |
| (9T – 10T) | 0.000001 |

Rule: above 10M, for supply in `(d × 10ⁿ, (d+1) × 10ⁿ]`, reward = `decade_base × (10 − d) / 10`. By decade: 10M → 100M: 1 → 0.1, 100M → 1B: 0.1 → 0.01, ..., 1T → 10T: 0.00001 → 0.000001.

### Economic Incentives

**Early Miner Advantage**: 
- The first 10M tokens (and the mint at exactly 10M) are minted at 1 token per mint
- Rewards then fall one step at every round number
- Predictable, gradual reward decay with no 10x cliffs

**Cost Anchor**:
- Minting costs the transaction fee, which X1 charges by requested compute units
- Minting is worthwhile only when the market price exceeds the cost per token, which keeps the price roughly anchored relative to XNT
- Since Revision 3 each transaction can mint only once, so every mint pays the full memo and transaction cost; batching is no longer possible

**Supply Cap**:
- Maximum of 10 trillion tokens in circulation, enforced on every mint
- Burns free room under the cap, so minting can continue indefinitely
- Changeable only through a program upgrade by the upgrade authority

---

**Audit Report End**

**Audit Date**: October 27, 2025 (original), revised October 13, 14, 15 and 16, 2026, October 31, 2026 (Revision 5) and November 1, 2026 (Revision 5 re-audit)  
**Contract Version**: Revision 5 (0.5.0)  
**Final Status**: ✅ APPROVED FOR MAINNET  
**Revision 5 (October 31, 2026)**: memo minimum counted in characters. Deployed on testnet and mainnet on November 1, 2026 (Owner decisions, deployment records). The deployment status below comes from the core audit and describes Revision 4.  
**Revision 5 Re-Audit (November 1, 2026)**: no critical, high or medium issue; 6 low and 11 informational findings (MINT-07 to MINT-23). The owner decided D3, D4 and D5 on the same day; they are applied in the final Revision 5 (Revision 5 Changes, Owner decisions).  
**Deployment Status (October 16, 2026, from the core audit, OPS-04)**: Revision 4 (the curve change) is deployed on X1 mainnet since October 15, 2026, 12:38:02 UTC (slot 86131302); the mainnet bytecode matches the `--features mainnet` build of commit `ca4c8a0b`. The October 16 follow-up (`mint_amount_at_supply` extracted, mint behavior unchanged) is deployed on testnet and, since **October 17, 2026**, on mainnet (commit `c47f1eb9`, slot 86507050, bytecode sha256 `2423c17f…` = the audited `--features mainnet` build). Both networks run the audited code.  

*This audit report is provided for informational purposes and does not constitute financial or legal advice. The auditor has conducted a thorough review of the smart contract code and design, confirming its security and correctness as of the audit date.*

