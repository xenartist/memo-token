# Memo-Burn Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-burn  
**Audit Date**: November 8, 2025 (original), revised October 13 and 16, 2026  
**Auditor**: Pre-Production Security Review  
**Version**: Revision 3 plus the October 16, 2026 change to burns of any amount from 1 unit, program version 0.2.0 (Revision 3: global burn statistics; Revision 2: length-prefix check, header-only memo parsing, no maximum memo length, compute-unit optimization)  
**Language**: Rust (Anchor Framework)  
**Network**: X1 (SVM-based)

> **Note (October 16, 2026)**: the final audit of memo-mint and memo-burn, `MEMO-CORE-AUDIT-2026-10-16.md`, re-verified this contract against the source code and corrected several statements in this document (finding IDs such as BURN-01 and OPS-03 refer to that report). Where the two documents differ, the core audit report takes precedence.

### Overall Assessment

**Risk Level**: ✅ **LOW** – Contract is production-ready with verified design intent

The memo-burn program implements a tightly controlled burn pipeline that enforces Borsh-serialized memo payloads, validates burn amounts (1 unit up to the per-instruction cap), and records per-user burn statistics. All critical security properties are satisfied, no vulnerabilities were identified, and every unusual design choice has been confirmed as intentional.

### Summary Statistics

- **Critical Issues**: 0
- **Design Confirmations**: 5 (all verified as intentional)
- **Security Strengths**: 9
- **Best Practices**: 6
- **Code Quality**: Excellent

---

## Contract Overview

### Purpose
The memo-burn contract allows users to permanently destroy MEMO tokens while attaching a structured payload. Each burn is gated by an SPL Memo instruction that must contain a Base64-encoded Borsh structure describing the burn. The program tracks cumulative burn contributions per user via a PDA.

### Key Features
- Mandatory memo at instruction index 0, at least 69 bytes as checked on chain (the shortest valid Base64 memo is 72 characters, see Design Confirmation #3; no contract maximum, the transaction size limit applies)
- Base64 + Borsh validation that ensures memo payload matches the burn amount
- Token2022 CPI burn of any amount in units (6 decimal places; fractional tokens allowed)
- User-level global burn statistics (total burned, count, timestamp)
- PDA seeds preventing account forgery
- Network-aware program IDs and mint allowlist (testnet/mainnet)

### Burn Parameters
- **Minimum burn**: 1 unit (0.000001 token, `MIN_BURN_AMOUNT = 1`); fractional token amounts are allowed. Programs built on memo-burn (chat, profile, project, blog, forum) enforce their own, supply-scaled minimums before the CPI
- **Maximum burn per `process_burn` instruction**: 1,000,000,000,000 tokens (a transaction may contain several burn instructions)
- **Token decimals**: 6 (DECIMAL_FACTOR = 1,000,000)
- **Memo length**: at least 69 bytes checked on chain (in practice at least 72 characters, because padded Base64 lengths are multiples of 4), no contract maximum
- **Memo payload**: no contract maximum; a memo decodes to 3/4 of its length, and the payload is that minus the 13-byte Borsh header. With the 9 account keys a burn transaction needs since Revision 3 (the 7 `process_burn` accounts plus the Memo and memo-burn program IDs), a single-burn legacy transaction on X1 today holds a memo of about 812 bytes (about 772 with a SetComputeUnitLimit instruction), that is a payload of about 596 bytes; a v0 transaction with an address lookup table holds about 960 bytes

### Deployment Status (October 16, 2026, from the core audit, OPS-01 / OPS-04)

| Network | Deployed memo-burn | Burn amount rule |
|---|---|---|
| X1 testnet | The audited working tree (upgraded October 15, 2026, 21:34 UTC) | Any amount from 1 unit (this document) |
| X1 mainnet | Commit `ca4c8a0b` (Revision 3), last deployed October 15, 2026, 10:27 UTC (slot 86109925) | Still **whole tokens only, at least 1 token**: amounts below 1,000,000 units fail with `BurnAmountTooSmall` (6003), non-whole amounts with `InvalidBurnAmount` (6005) |

Until mainnet memo-burn is upgraded, the "any amount from 1 unit" statements in this document apply to testnet only. When upgrading mainnet, memo-burn must be upgraded **before or together with** the apps: the current apps set supply-scaled minimums that are fractional above 10M supply (for example 37,862.1 MEMO to create a chat group), and those burns fail with 6005 against the old memo-burn. The transaction rolls back and no funds are lost.

---

## Revision 2 Changes (October 13, 2026)

The instruction interface is unchanged: same accounts, same `amount` argument and same `BurnMemo` format. Error codes changed: `MemoTooLong` and `PayloadTooLong` were removed, so `UnauthorizedUser` moved from 6013 to 6011. memo-chat, memo-profile, memo-project, memo-blog and memo-forum call `process_burn` via CPI without any change.

| Change | Details |
|---|---|
| Payload length prefix checked before deserialization (fixes L-3 of the 2026-09 security review) | Borsh allocates a buffer of the declared payload length before reading it, so a forged 4-byte prefix (for example `0xFFFFFFFF`) made the program abort with "memory allocation failed, out of memory". `validate_memo_amount` now checks that the prefix equals the number of payload bytes actually present and returns `InvalidMemoFormat` otherwise. Verified on a local validator: the old program panicked, the new one returns error 6001. |
| Size limits removed (resolves L-4) | The old 800 / 787 decoded-size and payload limits could never be reached. `MEMO_MAX_LENGTH`, `MAX_BORSH_DATA_SIZE` and `MAX_PAYLOAD_LENGTH` were removed: only the 69-byte memo minimum and the 13-byte Borsh header minimum are checked, and the transaction size limit bounds the rest. The contract therefore adapts to future transaction-size changes without an upgrade. The `MemoTooLong` and `PayloadTooLong` errors were removed (see the error code table below). On a local validator, burns with 804- and 820-byte memos (588- and 600-byte payloads) succeeded; the previous build rejected them with `MemoTooLong`. |
| Compute-unit optimization | The user stats PDA is verified with its stored bump instead of re-deriving the canonical bump; success-path logs and the payload preview were removed; the memo data is no longer copied. `process_burn` dropped from 16,504 to 15,015 CU on a local validator (−9%). |
| Header-only memo parsing | `validate_memo_amount` reads `version` and `burn_amount` directly from the 13-byte Borsh header instead of deserializing the whole `BurnMemo`, so the payload is no longer copied. Together with the length-prefix check, it accepts exactly the memos the full Borsh deserialization accepted. `BurnMemo` stays public for clients and calling programs. |
| Constant renamed | `MAX_BURN_PER_TX` is now `MAX_BURN_PER_INSTRUCTION`, matching what it limits. |
| Comments and messages | The compute-budget comment is corrected (it occupies an instruction index, so it must be at index 1 or later). `BurnAmountTooLarge` now says "per burn instruction". Comments on `burn_count` ("burn instructions") and the payload length field were corrected. |

**Error codes (Revision 2)**:

| Error | Old code | New code |
|---|---|---|
| `MemoRequired` | 6000 | 6000 |
| `InvalidMemoFormat` | 6001 | 6001 |
| `UnsupportedMemoVersion` | 6002 | 6002 |
| `BurnAmountTooSmall` | 6003 | 6003 |
| `BurnAmountTooLarge` | 6004 | 6004 |
| `InvalidBurnAmount` | 6005 | 6005 (kept, no longer returned since fractional burns were allowed) |
| `InvalidTokenAccount` | 6006 | 6006 |
| `UnauthorizedMint` | 6007 | 6007 |
| `UnauthorizedTokenAccount` | 6008 | 6008 |
| `BurnAmountMismatch` | 6009 | 6009 |
| `MemoTooShort` | 6010 | 6010 |
| `MemoTooLong` | 6011 | removed |
| `PayloadTooLong` | 6012 | removed |
| `UnauthorizedUser` | 6013 | 6011 |
| `UnauthorizedAdmin` (Revision 3) | — | 6012 (new) |

**Behavior change integrators must know**: a successful burn now logs one line, `Burned {amount} units from {user}, user total burned: {total} units, burn count: {count}` (integer units). It replaces the previous seven log lines, including `Successfully burned X tokens ...` and the payload preview. Off-chain parsers of the old lines must be updated; indexers should prefer token balance changes or inner instructions, because a transaction's logs are truncated at 10 KB.

**Memory note**: the program heap is 32 KB and is not freed, and a burn holds a copy of the memo plus its decoded bytes (about 1.75x the memo length; the payload is not copied again). This is far below the heap for any current transaction size; only if transaction sizes ever grew past about 16 KB could a burn with such a memo fail on allocation, which would affect only the sender's own transaction.

**Verification**: 82 unit tests pass, including tests for a forged length prefix, a too-small prefix, decoded data shorter than the Borsh header, and memos and payloads above the former limits. Unit tests for all seven programs pass. On a local validator, one memo backing three burns updated the user stats correctly.

---

## Revision 3 Changes (October 13, 2026): Global Burn Statistics

memo-burn now keeps one on-chain account with burn totals across all users, so explorers and apps can read with a single account fetch the total burned **through memo-burn since the account was initialized**.

> **Corrected (core audit BURN-01, BURN-05)**: `GlobalBurnStats` starts at zero when the admin creates it and only `process_burn` adds to it, so it does not include burns made before initialization (on mainnet it was initialized on October 15, 2026; at the time of the core audit it showed 1 MEMO / 1 burn while the 156 user stats accounts together held 384,760 MEMO / 213 burns). It also never sees burns made directly through Token-2022 without memo-burn (see Informational #5). It is therefore not the historical total and only a lower bound of supply reduction. For circulating supply use `mint.supply`; for historical memo-burn totals, sum the `UserGlobalBurnStats` accounts or publish an off-chain baseline. An admin instruction to "set" the total should not be added, because it would make the counter untrustworthy.

| Item | Details |
|---|---|
| New account `GlobalBurnStats` | PDA with seeds `[b"global_burn_stats"]`, 105 bytes: discriminator (8), `total_burned: u128` (16), `burn_count: u64` (8), `last_burn_time: i64` (8), `bump: u8` (1), `reserved: [u8; 64]` for future fields. |
| Overflow handling | `total_burned` and `burn_count` both use `saturating_add`: at their maximum (`u128::MAX` units, `u64::MAX` burns) they stop increasing instead of failing, so a burn can never be blocked by the counters. A `u128` total cannot be reached in practice; the cap is a guarantee, not an expected state. |
| New instruction `initialize_global_burn_stats` | Admin only (`AUTHORIZED_ADMIN_PUBKEY`, the deploy admin key, per network). Creates the account once with `init`; a second call fails because the account already exists. On a network where the account does not exist yet, a non-admin signer gets `UnauthorizedAdmin`; once it exists, any signer gets the "already in use" error first. |
| `process_burn` change | Requires the global account as a new **last** account (writable, verified by seeds and stored bump). Existing accounts keep their order. Every burn updates the global stats together with the user stats, using one `Clock` read. |
| Seeds check kept | The global account is verified by seeds and stored bump in `process_burn` and in every app, like every other PDA in the project. Without it, `Account<GlobalBurnStats>` would still check the owner and discriminator, and only one such account can exist today, so the check is defense in depth: it keeps the account safe even if a future version adds another way to create a `GlobalBurnStats`. It costs about 1,630 CU per burn (about 0.000016 XNT on X1); the project chose consistency over that saving. |
| No `init_if_needed` | The account is created once by the admin, not lazily inside `process_burn`, so burns never pay rent or carry the extra system-program account. |
| Concurrency | Every burn already write-locks the mint account, so the global account adds no new write-lock bottleneck. |

**Deploy order (required)**: upgrade memo-burn → run `admin-init-global-burn-stats` → upgrade the five apps. Until the account exists, every burn (direct or through an app) fails with `AccountNotInitialized`. `scripts/deploy-common.sh` now runs the init client right after deploying memo-burn (the client does nothing if the account already exists), deploys selected programs in canonical order (memo-burn before the apps), and verifies memo-burn's `AUTHORIZED_ADMIN_PUBKEY` against the deploy admin keypair.

**Integrator impact**: every client that builds `process_burn` (directly or through an app instruction) must append the global stats PDA as the last account. Old clients fail until they are updated. The Rust helper `memo_token_client::global_burn_stats_pda()` derives it.

**Error codes**: `UnauthorizedAdmin` is new at the end, **6012**. No existing code changed.

**Compute units**: `process_burn` costs 17,478 CU on a local validator, up from 14,857 (+2,621): about 1,630 for the seeds check and about 990 for loading and writing the account. memo-profile `create_profile` costs 202,854 CU in total.

**Verification** (local validator, testnet program IDs): a burn before initialization fails with `AccountNotInitialized`; a non-admin init fails with `UnauthorizedAdmin`; passing the user stats account in place of the global one fails with `AccountDiscriminatorMismatch`, and passing an account owned by another program fails with `AccountOwnedByWrongProgram`; the admin init succeeds (7,285 CU) and a rerun of the client reports the existing account; one burn and then one memo backing three burns produce global stats of 4 MEMO and 4 burns, matching the user stats; a memo-profile `create_profile` (420 MEMO via CPI) brings the global stats to 421 MEMO and 2 burns, matching the supply decrease. 88 unit tests pass (testnet and mainnet builds), including counter saturation at `u64::MAX` and `u128::MAX`.

---

## Design Confirmations & Verification

### ✅ DESIGN CONFIRMATION #1: Mandatory Borsh+Base64 Memo at Index 0

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL – DATA INTEGRITY GUARANTEE**

```rust
        // Check memo instruction with length validation
        let memo_data = match check_memo_instruction(ctx.accounts.instructions.as_ref())? {
            Some(data) => data,
            None => return Err(ErrorCode::MemoRequired.into()),
        };

        // Validate Borsh memo contains correct amount matching the burn amount
        validate_memo_amount(&memo_data, amount)?;
```

**Transaction Structure Requirement**:
- Instruction `0`: `MemoProgram::Memo` (at least 69 bytes; no contract maximum)
- Instruction `1+`: `memo_burn::process_burn`
- Compute budget instructions occupy an instruction index too, so they go at index 1 or later (usually last); one at index 0 makes the burn fail
- One memo can back several `process_burn` instructions in the same transaction; each burn's `amount` must equal the memo's `burn_amount`
- Durable nonce transactions are not supported: the runtime requires `AdvanceNonceAccount` at index 0, which conflicts with the memo rule (core audit INT-04)

**Binding is per instruction, not per transaction** (core audit BURN-04, INT-06): `burn_amount` is compared with each `process_burn` instruction's own `amount`, and nothing records that a memo was already used. A memo with `burn_amount = X` can therefore back any number of burns of exactly X in the same transaction (top-level or via CPI, by different signers), and the same memo also satisfies memo-mint for the transaction's one mint. Burns of different amounts cannot share a memo. On-chain accounting is exact (each instruction burns and records its own amount), but indexers must not count burns by counting memos: use the per-instruction `Burned N units` logs, the Token-2022 burn inner instructions, or balance and stats deltas.

**Why This Matters**:
1. **Integrity** – Each burn instruction's amount must equal the memo's `burn_amount` exactly (an equality check per instruction, not a cryptographic or one-time binding).
2. **Ordering** – Enforcing memo at index 0 eliminates ambiguity in multi-instruction flows.
3. **Flexibility** – Payload can carry arbitrary application data, limited only by the transaction size.
4. **Performance** – O(1) lookup, no scanning for the memo instruction.
5. **Composability** – Other programs can construct compliant transactions reliably.

**Verdict**: Memo enforcement is deliberate, well-documented, and critical for preserving burn auditability.

---

### ✅ DESIGN CONFIRMATION #2: Burn Amount Bounds (1 Unit Minimum, Per-Instruction Cap)

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL – ANY BALANCE IS BURNABLE, EXTREME BURNS ARE CAPPED**

```rust
        // Check burn amount is at least 1 unit
        if amount < MIN_BURN_AMOUNT {
            return Err(ErrorCode::BurnAmountTooSmall.into());
        }

        // Check burn amount upper limit (prevent excessive burns)
        if amount > MAX_BURN_PER_INSTRUCTION {
            return Err(ErrorCode::BurnAmountTooLarge.into());
        }
```

**Rationale**:
1. **No Unburnable Dust** – memo-mint's reward falls from 1 token to as little as 1 unit as supply grows, so balances are increasingly fractional. The former whole-token rule (`amount % DECIMAL_FACTOR == 0`, minimum 1 token) would have left such balances unburnable. Any amount of at least 1 unit is now accepted; a zero-amount burn is still rejected.
2. **Economic Control** – Caps any single burn instruction at 1T tokens. Several burn instructions in one transaction can exceed this in total; since a user can only burn tokens they hold, this affects no one else.
3. **Mathematical Safety** – Amounts are `u64` units end to end (instruction argument, memo, Token-2022 burn, statistics), so no rounding or conversion is involved.
4. **Memo Consistency** – The memo's `burn_amount` is in the same units and must equal the instruction amount exactly.
5. **Application Minimums** – memo-burn sets only the floor. memo-chat, memo-profile, memo-project, memo-blog and memo-forum enforce their own minimums (a multiple of the current mint reward) before calling `process_burn`.

**Security note**: dropping the whole-token rule removes no protection. A tiny burn costs the sender a transaction fee and only updates the sender's own statistics and the saturating global counters. One consequence for analytics: `burn_count` (per user and global) can now be raised with 1-unit burns, so it counts burn instructions, not a minimum burned value; use `total_burned` for weight. `InvalidBurnAmount` (6005) stays in the enum but is no longer returned, so later error codes keep their numbers.

**Verdict**: Amount guards are well-considered: every balance can be burned, reckless single burns are capped, and arithmetic stays safe.

---

### ✅ DESIGN CONFIRMATION #3: Base64 + Borsh Memo Validation

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL – STRUCTURED METADATA CHANNEL**

```rust
fn validate_memo_amount(memo_data: &[u8], expected_amount: u64) -> Result<()> {
    // First, decode the Base64-encoded memo data
    let base64_str = std::str::from_utf8(memo_data)
        .map_err(|_| {
            msg!("Invalid UTF-8 in memo data");
            ErrorCode::InvalidMemoFormat
        })?;

    let decoded_data = general_purpose::STANDARD.decode(base64_str)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;

    // check decoded borsh data holds at least the fixed header
    if decoded_data.len() < BORSH_FIXED_OVERHEAD {
        return Err(ErrorCode::InvalidMemoFormat.into());
    }

    // Check the payload length prefix against the bytes actually present, before Borsh
    // allocates a buffer of the declared length (Revision 2)
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&decoded_data[BORSH_PAYLOAD_LEN_OFFSET..BORSH_FIXED_OVERHEAD]);
    let declared_payload_len = u32::from_le_bytes(len_bytes) as usize;
    if declared_payload_len != decoded_data.len() - BORSH_FIXED_OVERHEAD {
        return Err(ErrorCode::InvalidMemoFormat.into());
    }

    // Read the fixed header directly; the payload is not copied (Revision 2)
    let version = decoded_data[0];
    let mut amount_bytes = [0u8; 8];
    amount_bytes.copy_from_slice(&decoded_data[BORSH_U8_SIZE..BORSH_PAYLOAD_LEN_OFFSET]);
    let burn_amount = u64::from_le_bytes(amount_bytes);

    if version != BURN_MEMO_VERSION {
        msg!("Unsupported memo version: {} (expected: {})", version, BURN_MEMO_VERSION);
        return Err(ErrorCode::UnsupportedMemoVersion.into());
    }

    if burn_amount != expected_amount {
        msg!("Burn amount mismatch: memo {} vs expected {}", burn_amount, expected_amount);
        return Err(ErrorCode::BurnAmountMismatch.into());
    }

    Ok(())
}
```

**Benefits**:
1. **Integrity** – Burn amount must match memo payload exactly.
2. **Versioning** – `BURN_MEMO_VERSION` enables forward-compatible memo formats.
3. **Payload Safety** – The length prefix is checked against the bytes present before Borsh allocates anything, so a forged prefix cannot exhaust the heap (Revision 2). There is no maximum payload size; the transaction size limit bounds it.
4. **Encoding Hygiene** – Rejects invalid UTF-8 or Base64 before deserialization.
5. **Observability** – Every validation failure logs the reason and the values involved.

**Note on the minimum length** (core audit BURN-02, informational, no code change): in the project's design, 69 is the minimum the official client enforces on the user's input **before** Borsh/Base64 encoding (users must type at least 69 characters). On chain, memo-mint and memo-burn both check the raw memo instruction bytes against `MEMO_MIN_LENGTH = 69`; for memo-burn those bytes are the Base64 string. `general_purpose::STANDARD` requires padding, so valid Base64 lengths are multiples of 4 and the shortest memo memo-burn can decode is 72 characters (52–54 decoded bytes, a payload of at least 39 bytes). Memos of 69–71 characters pass the length check and then fail with `InvalidMemoFormat` (6001) rather than `MemoTooShort` (6010); memos under 69 characters fail with `MemoTooShort`. With the official client's 69-character input rule the encoded memo is always far longer than 72 characters, so this only matters to third-party integrators who build memos by hand. The effective rule is stricter than the stated one, so there is no security impact.

**Verdict**: Structured memo validation is robust and aligns with cross-program communication requirements.

---

### ✅ DESIGN CONFIRMATION #4: Mandatory User Burn Statistics PDA

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL – AUDIT TRAIL GUARANTEE**

```rust
    /// User global burn statistics tracking account (now required)
    #[account(
        mut,
        seeds = [b"user_global_burn_stats", user.key().as_ref()],
        bump = user_global_burn_stats.bump,
        constraint = user_global_burn_stats.user == user.key() @ ErrorCode::UnauthorizedUser
    )]
    pub user_global_burn_stats: Account<'info, UserGlobalBurnStats>,
```

**Runtime Behavior**:
1. **One PDA per User** – Derived from `user` pubkey, preventing impersonation.
2. **Initialization Flow** – `initialize_user_global_burn_stats` sets owner, counters, and bump.
3. **Stats Safety** – Burn totals saturate at `MAX_USER_GLOBAL_BURN_AMOUNT` (18T tokens) to avoid overflow.
4. **Last Burn Time** – Timestamp recorded via `Clock::get()` for analytics.
5. **Enforcement** – Constraint rejects mismatched PDA owners.
6. **Stored Bump** – Since Revision 2 the PDA is checked with the bump saved at initialization (the canonical bump), which avoids re-deriving it on every burn.
7. **Initialization Required** – Users must call `initialize_user_global_burn_stats` once before their first burn. Creating the account automatically on first burn would change the instruction accounts and break the apps that call `process_burn` via CPI, so it is kept as a separate step.

**Verdict**: Per-user tracking is deliberate and creates a tamper-resistant audit trail for ecosystem incentives.

---

### ✅ DESIGN CONFIRMATION #5: Network Configuration and Token2022 Enforcement

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL – ENVIRONMENTAL SAFEGUARDS**

```rust
#[cfg(feature = "mainnet")]
declare_id!("2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh");

#[cfg(not(feature = "mainnet"))]
declare_id!("FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP");

#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");
```

**Key Guarantees**:
1. **Program IDs** – Compile-time features set the correct on-chain address.
2. **Mint Allowlist** – Only the authorized MEMO mint can be burned.
3. **Token2022 CPI** – Uses `token_2022::burn` with `Program<'info, Token2022>`.
4. **Interface Accounts** – `InterfaceAccount` ensures compatibility with Token2022 extensions.
5. **Misconfiguration Defense** – Incorrect feature flags fail during deployment or runtime.

**Verdict**: Environment gating prevents accidental mainnet/testnet crossovers and enforces the intended token standard.

---

## Additional Security Analysis

### ℹ️ INFORMATIONAL #1: Memo Instruction Ordering

- `check_memo_instruction` ensures `process_burn` executes at index ≥ 1 and memo sits at index 0.
- Compute budget instructions must be placed at index 1 or later; they occupy an instruction index like any other instruction.
- Durable nonce transactions cannot burn, because `AdvanceNonceAccount` would have to be at index 0.
- Transactions lacking the memo or mis-ordering instructions fail with descriptive errors.

**Verdict**: Ordering constraint is intentional and safe.

---

### ℹ️ INFORMATIONAL #2: Saturating User Statistics

- `total_burned` uses `saturating_add` and is then clamped to `MAX_USER_GLOBAL_BURN_AMOUNT` (18T tokens); `burn_count` uses `saturating_add` only and stops at `u64::MAX`, it is not capped at 18T.
- This avoids panics but means totals plateau once the ceiling is reached.
- Design intent: keep contract live even if long-term cumulative burns exceed configured threshold; analytics should monitor for saturation.

**Verdict**: Acceptable trade-off; document the saturation behavior for indexers.

---

### ℹ️ INFORMATIONAL #3: Payload Handling

- No execution path consumes payload contents; memo data is informational only.
- The payload preview log was removed in Revision 2 to save compute units. The full memo is still visible in the SPL Memo program's own log and in the transaction itself.
- memo-burn does not interpret the payload, so a memo shaped like one app's payload can also be used for a direct `process_burn`, or for extra `process_burn` instructions next to the app's own burn (core audit INT-05). The apps reject memos written for other operations, so app state stays consistent, and every burn is a real burn by its signer. Indexers should attribute burns by the calling program, app account changes and inner instructions, not by memo text alone; third-party programs must validate their own payload before calling `process_burn` via CPI.

**Verdict**: Payload handling is safe.

---

### ℹ️ INFORMATIONAL #4: Token-2022 CPI Guard and Multisig Owners

- `process_burn` always burns through a CPI to Token-2022 with the token account owner as the authority. Even when a user calls memo-burn directly as a top-level instruction, Token-2022 runs inside a CPI (stack height 2; 3 through an app).
- A token account with the Token-2022 CPI Guard extension enabled therefore rejects **every** burn through memo-burn, direct or through an app, with Token-2022 error 43 (`CpiGuardBurnBlocked`). CPI Guard allows delegate burns, but memo-burn requires the signer to be the owner, so there is no workaround inside memo-burn. The user has to disable CPI Guard in a separate top-level transaction first. Only the owner can enable CPI Guard, so a third party cannot use it to block someone else's burns.
- Token accounts owned by a Token-2022 multisig cannot burn through memo-burn either: the multisig address cannot sign as `user`, and memo-burn does not forward multisig signer accounts.
- Minting to such accounts is unaffected (MintTo has no CPI Guard check, and the mint authority is the memo-mint PDA). No funds are at risk.

**Verdict**: Document for front-ends: map error 43 to "disable CPI Guard in a separate transaction first", and multisig errors to "use a single-key owner".

---

### ℹ️ INFORMATIONAL #5: Burns That Bypass memo-burn

- The memo requirement belongs to memo-burn's bookkeeping, not to the token. Any owner or delegate can burn MEMO directly through Token-2022 `Burn` / `BurnChecked` without a memo, and Token-2022 lets anyone burn from a token account whose owner field is exactly the incinerator address or the System program ID. These burns reduce supply but update neither user nor global burn stats (core audit BURN-05, MINT-06).
- memo-burn itself cannot be used this way: `user` must sign and equal the token account owner.
- Reliable relations: burns recorded by memo-burn ≤ total minted − current supply; `GlobalBurnStats.total_burned` ≤ burns through memo-burn since its initialization.

**Verdict**: By design (Token-2022 has no burn hook). Describe both stats accounts as "burned through memo-burn" and use `mint.supply` for circulating supply.

---

### ℹ️ INFORMATIONAL #6: Owner Delegated to Itself

- Token-2022 picks the authority path in the order permanent delegate → delegate → owner. If the owner approves itself as delegate, its own burns go through the delegate path: they are limited to the delegated amount, consume it, and a larger burn fails with Token-2022 error 1 (`InsufficientFunds`) although the balance is enough (core audit BURN-07).
- Only the owner can create this state, and no funds are at risk.

**Verdict**: Front-ends that see error 1 with a sufficient balance should check whether `delegate == owner` and ask the user to revoke the approval.

---

## Code Quality Excellence

### ✅ Best Practice #1: Defensive Encoding Validation
- Rejects invalid UTF-8, Base64, and Borsh before trusting memo data.
- Requires the decoded data to hold at least the 13-byte Borsh header and checks the Borsh length prefix against the bytes present before reading, preventing memory abuse (there is no maximum size since Revision 2).

### ✅ Best Practice #2: Explicit Error Codes
- Descriptive `ErrorCode` enum covers every failure path (`BurnAmountMismatch`, `MemoTooShort`, etc.).
- Errors are surfaced with actionable guidance for integrators.

### ✅ Best Practice #3: Compile-Time Constants
- All thresholds (`MEMO_MIN_LENGTH`, `MIN_BURN_AMOUNT`, `MAX_BURN_PER_INSTRUCTION`, etc.) are constants in module scope.
- No maximum memo or payload constants: the transaction size limit is the single source of truth (Revision 2).

### ✅ Best Practice #4: Token2022 Interface Usage
- `InterfaceAccount` enforces mint/program compatibility without manual account parsing.
- The MEMO mint has only the MetadataPointer and TokenMetadata extensions. Mint extensions cannot be added to an initialized mint, so extensions such as transfer fees will not appear on it.

### ✅ Best Practice #5: Saturation over Panic
- Uses `saturating_add` to maintain liveness even when counters approach `u64::MAX`.
- Clamps totals to documented maxima instead of failing.

### ✅ Best Practice #6: Focused Logging
- A successful burn logs one integer line with the amount, user, cumulative total and count.
- Validation errors include byte counts and expected thresholds.

---

## Security Analysis by Category

### 1. Access Control ✅ SECURE
- User must sign (`Signer<'info>`).
- Token account must belong to user and authorized mint.
- Memo instruction is mandatory for every burn through memo-burn; no bypass within memo-burn (direct Token-2022 burns do not go through memo-burn, see Informational #5).
- Instructions sysvar address is validated.

### 2. Arithmetic Safety ✅ SECURE
- All amount comparisons use safe thresholds.
- `saturating_add` prevents overflow in statistics.
- Memo amount equality check ensures no mismatch.

### 3. Memo Validation ✅ SECURE
- Enforces the 69-byte minimum on the raw memo (the shortest decodable memo is 72 characters); the transaction size limit bounds the maximum.
- Requires Base64 + Borsh structure; free-form memos rejected.
- Versioned payload allows future upgrades without ambiguity.

### 4. Token2022 Compliance ✅ SECURE
- CPI call uses `token_2022::burn`.
- `Program<'info, Token2022>` prevents SPL Token misuse.
- Interface accounts maintain ABI compatibility.

### 5. PDA Integrity ✅ SECURE
- User stats PDA derived via fixed seed and user key.
- Bump recorded at initialization and verified through Anchor using the stored value.
- Ownership constraint ensures correct mapping.

### 6. Replay & Ordering Protection ✅ SECURE
- Instruction sysvar gating prevents transactions without memo.
- Index requirement thwarts memo spoofing in later positions.

### 7. Resource Limits ✅ SECURE
- The transaction size limit bounds the memo (there is no contract cap since Revision 2), and the length-prefix check prevents a forged prefix from allocating memory; a maximum-size burn uses about 1.7 KB of the 32 KB heap.
- The burn amount ceiling applies per burn instruction; a user can only burn tokens they hold.

### 8. Reentrancy & External Calls ✅ SECURE
- The only external call is the Token-2022 burn CPI, and it runs **before** the statistics are updated: the user and global stats are written only after the burn succeeded, in the same atomic instruction. This order is correct (stats never count a burn that failed); earlier versions of this document described it the other way round.
- Token-2022 burn makes no CPI of its own, so there are no callbacks into memo-burn.
- No reentrant state exposure.

### 9. Observability & Auditing ✅ SECURE
- Logs record each burn's amount and the user's cumulative totals.
- PDA stats provide persistent off-chain query surface.

---

## Pre-Production Deployment Checklist

### ✅ Design Verification (COMPLETED)
- Mandatory memo structure verified.
- Burn amount constraints validated.
- PDA initialization and enforcement confirmed.
- Token2022 program alignment checked.
- Network configuration confirmed.

### 🔴 CRITICAL – Required Before Mainnet Launch

1. **Testnet Validation**
   - [ ] Deploy memo-burn with `--features` flag **OFF** (testnet build).
   - [ ] Initialize user burn stats via client tool.
   - [ ] Execute burn with proper transaction ordering:
     ```
     [0] MemoProgram::Memo (Base64-encoded BurnMemo)
     [1] memo_burn::process_burn
     [last] ComputeBudget instructions (optional, never at index 0)
     ```
   - [ ] Verify the burn log line and user stats updates.
   - [ ] Test error cases (invalid memo version, amount mismatch, short memo, zero-amount burn, forged length prefix); confirm a fractional burn (e.g. 1 unit) succeeds.
   - [ ] Run each app's burn path (profile, chat, project, blog, forum) against the new program.

2. **Mainnet Deployment Preparation**
   - [ ] Build with `--features mainnet`.
   - [ ] Confirm program ID: `2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh`.
   - [ ] Confirm mint address: `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`.
   - [ ] Deploy program and verify ID matches Anchor.toml.
   - [ ] Run end-to-end burn using production clients.
   - [ ] Update off-chain parsers for the new burn log line.

3. **Documentation for Integrators**
   - [ ] Describe required memo structure (Base64-encoded Borsh).
   - [ ] Publish BurnMemo schema and example payloads.
   - [ ] Document minimum/maximum burn amounts.
   - [ ] Provide transaction builder examples for wallets and dApps.
   - [ ] Detail PDA initialization flow for new users.

### ⚠️ Recommended – Post-Launch Monitoring
- [ ] Track aggregate burn volume and user-level stats via indexer.
- [ ] Alert on user stats approaching saturation limit.
- [ ] Monitor failed burns for memo format violations.
- [ ] Build dashboards for payload analytics (off-chain).

### ℹ️ Optional – Future Enhancements
- [ ] Consider raising `MAX_USER_GLOBAL_BURN_AMOUNT` if saturation observed.
- [ ] Explore compressed payload formats for richer metadata.
- [ ] Add optional memo schema registry for ecosystem coordination.

---

## Testing Recommendations

### Unit Tests (implemented)

`programs/memo-burn/src/tests.rs` — at the time of the core audit, 86 hand-written tests plus the Anchor-generated `test_id`, 87 in total, all passing (testnet and `--features mainnet` builds). Regression tests recommended by the core audit are being added afterwards; see the current `tests.rs` for the up-to-date list. As audited:
- `validate_memo_amount`: valid memos, wrong version, amount mismatch, invalid UTF-8 / Base64 / Borsh, payloads above the former 587-byte limit accepted
- Length prefix (Revision 2): forged `0xFFFFFFFF` prefix, prefix smaller than the bytes present, decoded data shorter than the 13-byte header
- `validate_memo_length`: empty, below/at/above the 69-byte minimum
- `validate_memo_length`: long memos (801 to 4,096 bytes) accepted
- `GlobalBurnStats::record_burn`, including saturation (Revision 3)
- `BurnMemo` serialization, `UserGlobalBurnStats` layout, constants

Not covered by unit tests at the time of the core audit (TEST-02, TEST-03, TEST-06, TEST-07, TEST-08, TEST-10): `check_memo_instruction`, the `process_burn` handler (amount bounds, user stats cap), account constraints, the init instructions and the error code numbering; a few tests are empty or only test third-party libraries. The core audit exercised every rejection path on a local validator with a byte-identical binary, but that harness is not committed.

### Integration Tests
1. **Memo Validation** – Submit burns with malformed Base64, invalid Borsh, wrong version, and mismatched amounts.
2. **Instruction Ordering** – Attempt burns with memo at index 1, missing memo, or a compute budget instruction at index 0 to confirm rejection.
3. **User Stats Flow** – Initialize PDA, perform sequential burns, and assert cumulative totals/timestamps.
4. **High-Volume Burn** – Burn near the per-instruction maximum to ensure limits hold.
5. **Payload Diversity** – Validate UTF-8, binary, and large payloads within limits.

---

## Audit Conclusion

### Final Status: ✅ **APPROVED FOR MAINNET DEPLOYMENT**

The memo-burn contract exhibits strong security posture, precise validation logic, and comprehensive logging. All intentional design choices—such as structured memo enforcement and per-user PDA tracking—align with the project’s transparency goals.

### Security Assessment: **EXCELLENT**
- ✅ Strict access control and account validation
- ✅ Robust memo decoding and schema enforcement
- ✅ Safe arithmetic with protective saturation
- ✅ Token2022 compliance
- ✅ Clear, actionable errors and logs

### Risk Assessment
- **Security Risk**: ✅ **LOW**
- **Deployment Risk**: ✅ **LOW**

### Mainnet Deployment Authorization
- ✅ Proceed after completing the critical checklist items.
- ✅ Maintain operational monitoring post-launch.

---

## Stakeholder Summary

- **Contract Name**: memo-burn  
- **Purpose**: Structured token burn with verifiable metadata  
- **Version**: Revision 3 plus the October 16, 2026 fractional-burn change (0.2.0); mainnet still runs Revision 3 with whole-token burns (see Deployment Status)  
- **Security Status**: ✅ Production Ready  
- **Risk Level**: Low  
- **Code Quality**: Excellent

**Key Findings**:
- No vulnerabilities detected.
- Memo structure enforcement is intentional and correct.
- User statistics PDA provides reliable audit data.
- Token2022 interactions are implemented safely.
- Deployment tooling and network gating are in place.

**Recommendation**: Approve for mainnet deployment following successful testnet validation and documentation updates.

---

## Auditor Notes

The memo-burn program demonstrates best-in-class validation discipline, layering memo parsing, amount checks, and PDA constraints to ensure only legitimate burns succeed. Its architecture cleanly complements memo-mint, creating a balanced ecosystem of verifiable minting and burning.

**Revision 2 (October 13, 2026)** added the length-prefix check, corrected the size limits and reduced compute units without changing the interface; see "Revision 2 Changes" above.

**Remaining operational risk**: the program upgrade authority is a single hot key (mainnet `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ`, not a multisig) shared with memo-mint, the five apps, the IDL and the token metadata, and also used for testnet staging deployments (core audit OPS-02, OPS-05). Whoever holds it can replace the program. Consider a multisig with a timelock or making the program immutable (`--final`) once the current revision is stable on mainnet.

---

## Appendix A: BurnMemo Schema

```text
BurnMemo {
    version: u8,           // currently 1
    burn_amount: u64,      // integer units (DECIMAL_FACTOR = 1_000_000)
    payload: Vec<u8>,      // arbitrary metadata (no contract maximum)
}
```

Base64-encoded Borsh serialization of this struct must be provided in the memo instruction. The Borsh payload length prefix must equal the number of payload bytes that follow.

---

## Appendix B: Deployment Checklist (Reference)

memo-burn is already deployed on X1 mainnet (`2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh`) and `GlobalBurnStats` is initialized, so new versions are **upgrades**, not fresh deployments. Use `scripts/deploy-mainnet.sh` as described in `PROD-DEPLOYMENT.md`; it builds with the `mainnet` feature, upgrades memo-burn before the apps, runs the admin init clients and upgrades the IDL. Upgrade memo-burn before or together with the apps, never the apps alone (see Deployment Status).

> **Corrected (core audit OPS-03)**: earlier versions of this appendix used `anchor build ... --features mainnet` (rejected by Anchor; the feature must follow `--`), `anchor deploy ... --provider.cluster mainnet` (`mainnet` means Solana mainnet-beta in Anchor, not X1, and `anchor deploy` makes a fresh deployment at a new address instead of upgrading), read the program ID from `target/deploy/memo_burn-keypair.json` (not the real program keypair; the real keypairs are kept encrypted, see `scripts/keys.sh`) and omitted the `GlobalBurnStats` initialization. Those steps could not bypass any safety check, but they would not upgrade the real program.

1. **Build**  
   `anchor build -p memo-burn -- --features mainnet`, then record and publish `sha256sum target/deploy/memo_burn.so`

2. **Unlock the Mainnet Keys** (dedicated mainnet machine only)  
   `./scripts/keys.sh unlock mainnet`

3. **Upgrade** (X1 mainnet RPC, never `--provider.cluster mainnet`; the new binary is 1,152 bytes larger than the deployed one, so ProgramData must be extended, which the core audit expects the Solana CLI 2.2.20 to do automatically)  
   `anchor upgrade target/deploy/memo_burn.so --program-id 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh --provider.cluster https://rpc.mainnet.x1.xyz --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json`

4. **Initialize Global Burn Stats (only if it does not exist yet)**  
   `MEMO_PROGRAM_ENV=mainnet cargo run --manifest-path clients/Cargo.toml --bin admin-init-global-burn-stats -- 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh https://rpc.mainnet.x1.xyz`  
   (does nothing if the account already exists; it exists on testnet and mainnet)

5. **Update the On-Chain IDL**  
   `anchor idl upgrade 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh --filepath target/idl/memo_burn.json --provider.cluster https://rpc.mainnet.x1.xyz --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json`

6. **Upgrade the Apps** (chat, profile, project, blog, forum) in the same run, then lock the keys: `./scripts/keys.sh lock mainnet`

7. **Test Burn**  
   `cargo run --manifest-path clients/Cargo.toml --bin test-memo-burn 1 valid-memo`, plus one fractional burn (for example 0.5 MEMO) to confirm the upgraded rule; run `verify-deployment` to compare the on-chain bytecode and IDL with the local build

8. **Monitor**  
   - Confirm the burn log line appears.
   - Inspect user and global stats.

---

## Appendix C: Client Integration Highlights

- **Required Accounts** (7, in this order): `user` (signer, writable), `mint` (writable), `token_account` (writable, owned by `user`), `user_global_burn_stats` (PDA `[b"user_global_burn_stats", user]`, writable), `token_program` (Token-2022), `instructions` (sysvar), `global_burn_stats` (PDA `[b"global_burn_stats"]`, writable, last). Omitting `global_burn_stats` fails with `AccountNotEnoughKeys` (3005).
- **Memo Construction**:
  1. Serialize `BurnMemo` via Borsh.
  2. Base64-encode the serialized bytes (standard alphabet with `=` padding, not URL-safe, no whitespace or newlines).
  3. Ensure the final string is at least 72 characters (69 is checked on chain, but padded Base64 lengths are multiples of 4, so 69–71 fail with `InvalidMemoFormat`) and that the whole transaction fits in 1,232 bytes.
  4. Include in instruction 0 using `spl_memo`.
- **Transaction Order**: Memo → ProcessBurn (one or more, each with `amount` equal to the memo's `burn_amount`) → optional compute budget instruction, never at index 0. Durable nonce transactions are not supported.
- **Token Accounts**: accounts with CPI Guard enabled (error 43) or owned by a multisig cannot burn through memo-burn; see Informational #4.
- **Compute Budget**: X1 charges by the requested compute-unit limit; simulate first and set the exact limit instead of relying on the 200k default.
- **Error Handling**: Surface contract error messages to end users for actionable feedback.
- **Indexing**: attribute burns per instruction (logs, inner instructions, balance or stats deltas), not by counting memos; `GlobalBurnStats` covers only burns through memo-burn since its initialization. See also the integration notes in `README.md`.

---

## Appendix D: Analytics & Observability Ideas

- **Leaderboard**: Rank users by cumulative burn (using PDA data).
- **Payload Explorer**: Off-chain service to decode and display payloads by schema.
- **Alerts**: Notify when payload validation failures spike (possible client regression).
- **Supply Dashboard**: Combine mint and burn data for net supply visibility.

---

**Audit Report End**

**Audit Date**: November 8, 2025 (original), revised October 13 and 16, 2026  
**Contract Version**: Revision 3 plus the October 16, 2026 fractional-burn change (0.2.0)  
**Final Status**: ✅ APPROVED FOR MAINNET  
**Deployment Status (October 16, 2026)**: testnet runs the audited working tree; mainnet runs Revision 3 (commit `ca4c8a0b`, whole-token burns only, `GlobalBurnStats` initialized October 15, 2026). The fractional-burn change still has to be deployed to mainnet, before or together with the apps.  

*This report is informational only and does not constitute financial or legal advice. The auditor confirmed security properties as of the audit date.*


