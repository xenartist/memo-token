# Memo-Mint Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-mint  
**Audit Date**: October 27, 2025 (original), revised October 13 and 14, 2026  
**Auditor**: Pre-Production Security Review  
**Version**: Revision 3, program version 0.3.0 (one mint per transaction; Revision 2: round-number emission curve, `process_mint_to` fix, compute-unit optimization)  
**Language**: Rust (Anchor Framework)  
**Network**: X1 (SVM-based)

### Overall Assessment

**Risk Level**: ✅ **LOW** - Contract is production-ready with confirmed design intent

The memo-mint contract implements a dynamic token minting mechanism with supply-based rewards (a round-number emission curve) for a fair-launch mining project. The contract demonstrates excellent security practices and all design decisions have been verified as intentional.

### Summary Statistics

- **Critical Issues**: 0
- **Design Confirmations**: 5 (all verified as intentional)
- **Security Strengths**: 8
- **Best Practices**: 5
- **Code Quality**: Excellent

---

## Contract Overview

### Purpose
The memo-mint contract enables users to mint MEMO tokens by submitting memo instructions. The minting amount decreases as circulating supply increases, stepping down at every round number (10M, 20M, ..., 100M, 200M, ...) from 1 token to 1 lamport (0.000001 token) at the 10 trillion cap.

### Key Features
- Dynamic mint amounts based on a round-number emission curve
- 10 trillion token cap on circulating (outstanding) supply
- Mandatory memo requirement (at least 69 bytes; no contract maximum)
- PDA-based mint authority
- Token2022 compatibility
- Dual network support (testnet/mainnet)

### Emission Curve
- **Below 10M tokens**: 1 token per mint
- **From 10M tokens**: supply in `[d × 10ⁿ, (d+1) × 10ⁿ)` (leading digit `d` = 1..9) pays `decade_base × (11 − d) / 10`, where `decade_base` is 1 token for 10M–100M, 0.1 for 100M–1B, 0.01 for 1B–10B, and so on

| Supply reaches | Reward per mint |
|---|---|
| below 20M | 1 |
| 20M / 30M / ... / 90M | 0.9 / 0.8 / ... / 0.2 |
| 100M | 0.1 |
| 200M / ... / 900M | 0.09 / ... / 0.02 |
| 1B | 0.01 |
| 10B / 100B / 1T | 0.001 / 0.0001 / 0.00001 |
| 9T – 10T | 0.000002 (2 lamports) |

Every decade start pays exactly 10x less than the previous one, so the long-run slope matches the original 6-tier schedule; the single 10x cliff at each tier boundary is replaced by nine smaller steps.

---

## Revision 2 Changes (October 13, 2026)

| Change | Details |
|---|---|
| Round-number emission curve | Replaces the 6-tier `match` in `calculate_dynamic_mint_amount`. Integer math only, at most 5 loop iterations. Compile-time assertions: the curve ends exactly at `MAX_SUPPLY`, every step is a whole number of lamports, and the smallest reward is ≥ 1 lamport. |
| No maximum memo length | Only the 69-byte minimum is checked. The transaction size limit (1,232 bytes on X1 today, 4,096 bytes on Solana since September 2026) bounds the memo, so the contract adapts to future transaction-size changes without an upgrade. The `MemoTooLong` error was removed. On a local validator, an 801-byte and an 812-byte memo minted successfully; a 900-byte memo failed only at the transaction size limit (1,279 > 1,232 bytes). |
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
1. **Success log format**: `Minted {amount} units to {owner}, supply before mint: {supply} units` (integer units, 1 token = 1,000,000 units). It replaces `Successfully minted X tokens ... current supply: Y tokens`. Off-chain parsers of the old line must be updated.
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

**False positives**: only two transactions of the same user with exactly the same instructions (same memo, same accounts) in the same slot collide; the second fails. Two different users with identical memos never collide, because their signer accounts differ. Identical instructions in a later slot succeed.

**Deploy order (required)**: upgrade memo-mint → run `admin-init-mint-state` → upgrade chat, blog and forum. Until `MintState` exists, every mint fails with `AccountNotInitialized`; until the apps are upgraded, their mint instructions fail with `AccountNotEnoughKeys`. `scripts/deploy-common.sh` runs the init client right after deploying memo-mint and verifies memo-mint's `AUTHORIZED_ADMIN_PUBKEY` against the deploy admin keypair.

**Integrator impact**:
1. Every `process_mint` / `process_mint_to` must append the `MintState` PDA (`memo_token_client::mint_state_pda()`) as the last account. Old clients fail with `AccountNotEnoughKeys`.
2. Batch mining (several mints in one transaction) is no longer possible. Each mint needs its own transaction and memo.
3. The extra account adds 33 bytes to every mint transaction. The longest memo that fits a single-signer `process_mint` transaction drops from 853 to 820 bytes (from 813 to 780 bytes with a ComputeBudget instruction); `process_mint_to` and app mint instructions lose the same 33 bytes.

**Economics**: the cheapest way to mint becomes the single-mint transaction again. On the local validator memo-mint itself uses 15,598 CU per mint, up from 12,483 (+3,115: about 1,600 for the seeds check on `MintState`, the rest for loading and writing the account, the clock read and hashing the sysvar). On X1 that is about +0.00003 XNT per mint (fee = 1,500 + 10 × requested CU). Together with the end of batching, the cost per token (and so the cost-based price ceiling) rises from about 0.00016 XNT (31-mint batches) to about 0.00058 XNT (single mints, mainnet Memo program CU).

**Verification**:
- 59 unit tests (testnet and mainnet builds), including fingerprints built with the runtime's own instructions sysvar serializer (`construct_instructions_data` / `store_current_index_checked`): identical for every instruction index of a transaction, different when any instruction, account, flag or signer differs.
- Local validator, full run from a fresh ledger, repeated twice with identical results: 3 pre-init checks (mint before init → `AccountNotInitialized`, non-admin init → `UnauthorizedAdmin`), 3 post-init checks, 36 core cases, all 7 smoke tests, 18 app cases and the 52-case `pen-test-memo-mint` suite.
- Rejected with `OneMintPerTransaction`, with balance, supply and `MintState` unchanged: 2, 3 and 31 top-level mints; `process_mint` + `process_mint_to` in both orders; two memos with two mints; memo/mint/memo/mint; mints separated by other instructions; `process_mint_to` to two recipients; three signers minting together; a test program calling memo-mint via CPI 2 and 30 times, through `process_mint_to`, nested two levels deep, and combined with a top-level mint; chat `send_memo_to_group` (zero-interval group), blog `mint_for_blog` and forum `mint_for_post` twice in one transaction, combined with a direct mint, and called twice via CPI; burn followed by two mints.
- Still allowed: single mints (11 in a row), `process_mint_to` to self and others, a single CPI mint from a third-party program, mint plus burn in one transaction, three burns in one transaction, two users with identical memos in the same slot, identical instructions in a later slot.
- Upgrade path on a local validator, both manually and with `deploy-common.sh`: the old program batches; after the upgrade, old clients fail with `AccountNotEnoughKeys` and new clients fail with `AccountNotInitialized` until the admin init; afterwards single mints succeed and batches are rejected.

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
- Each mint therefore pays the full memo program cost (about 40k CU for a 70-byte memo on X1). The effective price ceiling is the single-mint cost.

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

**Required Transaction Structure**:
```
Transaction:
  [0] MemoProgram::Memo (at least 69 bytes) (REQUIRED)
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

**Implementation**:
```rust
#[account(
    seeds = [b"mint_authority"],
    bump
)]
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
   - ✅ No private key controls the mint authority
   - ⚠️ Authority transfer is **irreversible** (by design)

**Security Analysis**:
- ✅ Well-documented transfer process
- ✅ Includes validation and error handling
- ✅ PDA-based authority is industry best practice
- ✅ Authority cannot be transferred back (prevents recentralization)
- ✅ Mint logic is permanently governed by program rules

**Deployment Verification Steps**:
1. Run authority transfer tool on testnet first
2. Verify PDA owns mint authority: `spl-token display <mint>`
3. Test mint transaction to confirm functionality
4. Repeat on mainnet with production keys

**Verdict**: Comprehensive and secure mint authority management. The irreversible transfer to PDA is the correct design for a decentralized fair-launch token.

---

### ✅ DESIGN CONFIRMATION #4: Emission Curve Boundary Behavior

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL (Revision 2)**

**Implementation**:
```rust
let amount = if current_supply < CURVE_START_LAMPORTS {
    BASE_MINT_AMOUNT
} else {
    // Find the decade: magnitude <= current_supply < magnitude * 10
    let mut magnitude = CURVE_START_LAMPORTS;
    let mut decade_base = BASE_MINT_AMOUNT;
    while current_supply / 10 >= magnitude {
        magnitude *= 10;
        decade_base /= 10;
    }
    // Leading digit, always 1..=9 here
    let digit = current_supply / magnitude;
    decade_base * (11 - digit) / 10
};
```

**Behavior at Round Numbers** (each step starts exactly at the round number):
- 20M − 1 lamport: **1 token**; exactly 20M: **0.9 token**
- 100M − 1 lamport: **0.2 token**; exactly 100M: **0.1 token**
- Exactly 1B: **0.01 token**
- 9T up to the cap: **2 lamports**

**Rationale**:
1. **Round numbers users can track**: every change happens at 10M, 20M, ..., 100M, 200M, ...
2. **No 10x cliff**: the original schedule dropped the reward 10x in one step at each tier; the curve spreads the same drop over nine steps
3. **Same long-run slope**: each decade start pays exactly 10x less than the previous one, matching the original tiers
4. **Ends at the cap**: the curve would reach exactly 1 lamport at 10T

**Example Scenario**:
```
Current Supply: 19,999,999.000000 tokens
User A mints: Gets 1 token   → Supply becomes 20,000,000.000000 tokens
User B mints: Gets 0.9 token → Supply becomes 20,000,000.900000 tokens
```

**Security Analysis**:
- ✅ Compile-time validation ensures the curve ends at `MAX_SUPPLY` and every reward is a whole number of lamports ≥ 1
- ✅ The decade loop runs at most 5 times and `magnitude * 10 <= current_supply < MAX_SUPPLY`, so it cannot overflow
- ✅ Behavior is deterministic and covered by unit tests at all 54 round numbers
- ✅ Race conditions prevented by account locking

**Verdict**: The curve boundary logic is correct and matches the intended economic model.

---

### ✅ DESIGN CONFIRMATION #5: Memo Content Validation

**Design Intent**: ✅ **CONFIRMED AS INTENTIONAL - ACCEPTS ANY BINARY DATA**

**Implementation**:
```rust
fn validate_memo_length(memo_data: &[u8], min_length: usize) -> Result<bool> {
    // Only validates the minimum length, not content; the transaction size limit bounds the maximum
    if memo_data.is_empty() {
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    if memo_length < min_length {
        return Err(ErrorCode::MemoTooShort.into());
    }
    
    Ok(true)
}
```

**Design Rationale**:
The contract **intentionally accepts any binary data** and only validates length:

**Supported Memo Formats**:
1. **ASCII text**: Plain text messages
2. **UTF-8 text**: Unicode messages
3. **Base64 encoded data**: Used by other contracts calling memo-mint
4. **Borsh serialized data**: Used for structured data from contracts
5. **Binary data**: Any arbitrary bytes

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
4. **Gas Efficiency**: No expensive UTF-8 validation needed
5. **Off-Chain Filtering**: Content moderation handled by frontends/indexers

**Security Analysis**:
- ✅ Minimum length enforced (69 bytes); the transaction size limit bounds the maximum, and the memo program charges compute units per byte
- ✅ No injection attacks possible (memo is just data)
- ✅ No execution of memo content
- ✅ Off-chain services can filter/validate as needed
- ✅ Binary data cannot break on-chain logic

**Content Moderation Strategy**:
- On-chain: Accept any data (current implementation)
- Off-chain: Frontends/indexers filter inappropriate content
- User-level: Wallets can validate UTF-8 before submission

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
1. Supply at 9,999,999,999,999.999998 tokens (2 lamports below max; the last-step reward is 2 lamports)
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
T1   | Reads supply = MAX - 2     |
T2   | Mints 2 lamports          |
T3   | Supply = MAX              |
T4   | Unlocks mint account       | Acquires lock
T5   |                           | Reads supply = MAX
T6   |                           | Fails: SupplyLimitReached
```

At 1 lamport below the cap the 2-lamport reward would overshoot, so that mint fails with `SupplyLimitReached`; the cap is never exceeded.

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
- ✅ Token account owner matches the declared recipient (a consistency check that catches a wrong account; the recipient does not sign, and does not need to)
- ✅ Uses `InterfaceAccount` for Token2022 compatibility

**Flexibility by Design**:
The contract allows minting to **any valid token account**, including:
- User wallets (standard use case)
- Program-owned accounts (for contract integration)
- Multi-sig accounts
- Any address the caller specifies

**Use Cases**:
1. **Direct User Minting**: User mints to their own account
2. **Contract Integration**: memo-chat/memo-project mint rewards to users
3. **Airdrops**: Distribute tokens programmatically
4. **Batch Operations**: Mint to multiple recipients

**Why No Recipient Restrictions**:
1. **Composability**: Other contracts need to call process_mint_to
2. **Flexibility**: Users should be free to mint to any valid account
3. **Simplicity**: No need for whitelist/blacklist management
4. **Permissionless**: Aligns with fair-launch philosophy

**Not Restricted**:
- ❌ No blacklist checking (would be centralized)
- ❌ No burn address prevention (user choice)
- ❌ No program account restrictions (breaks composability)

**Verdict**: Permissionless recipient selection is the correct design for a fair-launch token. Restrictions would contradict the open-access philosophy.

---

## Code Quality Excellence

### ✅ Best Practice #1: Integer-Only Logging

**Implementation** (Revision 2):
```rust
msg!("Minted {} units to {}, supply before mint: {} units", amount, token_account.owner, current_supply);
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
    #[msg("Memo too short. Must be at least 69 bytes.")]
    MemoTooShort,
    
    #[msg("Supply limit reached. Maximum supply is 10 trillion tokens.")]
    SupplyLimitReached,
    
    // ... all errors have clear descriptions
}

// Runtime messages include actual values
msg!("Memo too short: {} bytes (minimum: {})", memo_length, min_length);
msg!("Minted {} units to {}, supply before mint: {} units", amount, token_account.owner, current_supply);
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
- All arithmetic uses checked operations ✓
- Compile-time constant validation ✓
- No floating-point math in critical paths ✓
- Proper error handling ✓

**Verdict**: Excellent arithmetic safety implementation.

---

### 3. PDA Validation ✅ SECURE

**Implementation**:
```rust
/// CHECK: PDA serving as mint authority
#[account(
    seeds = [b"mint_authority"],
    bump
)]
pub mint_authority: AccountInfo<'info>,

// in the handler: sign with the bump Anchor verified
&[&[b"mint_authority".as_ref(), &[ctx.bumps.mint_authority]]]
```

**Analysis**:
- Anchor derives the PDA with the canonical bump and rejects any other account ✓
- The verified bump is passed to the signer seeds ✓
- Revision 2 removed a second, manual `find_program_address` that repeated the same check (compute-unit optimization) ✓

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

**Analysis**:
- No external calls before state changes ✓
- CPI call is the final action (mint_to) ✓
- No callbacks or external program invocations ✓
- No state changes after CPI ✓

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

**Design note**: the cap and the curve both read the live `mint.supply`. Burns lower it, so they free room under the cap and can move the reward back up a step. Total tokens ever minted are therefore not capped; this is intentional, so minting never stops permanently.

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
- Length constraints enforced ✓
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
- All operations use checked math
- No unchecked conversions
- Proper overflow prevention

---

## Pre-Production Deployment Checklist

### ✅ Design Verification (COMPLETED)

All design decisions have been confirmed as intentional:
- ✅ Fair-launch unrestricted minting model
- ✅ Fixed memo position requirement
- ✅ PDA mint authority transfer process
- ✅ Emission curve boundary behavior (Revision 2)
- ✅ Permissive memo content validation
- ✅ Multiple mints per transaction allowed (Revision 2)

### 🔴 CRITICAL - Required Before Mainnet Launch

#### 1. Testnet Validation
- [ ] Deploy to testnet with `--features` flag **OFF**
- [ ] Transfer mint authority to PDA using transfer tool
- [ ] Execute test mint transactions with exact instruction structure:
  ```
  [0] MemoProgram::Memo (69+ bytes) (REQUIRED)
  [1] MemoMint::process_mint
  [last] ComputeBudgetProgram::SetComputeUnitLimit (optional, never at index 0)
  ```
- [ ] Verify logs show correct curve amounts
- [ ] Test `process_mint_to` (to another wallet, mismatched recipient, missing token account)
- [ ] Measure compute units of a real mint transaction
- [ ] Test error cases (short memo, long memo, no memo)

#### 2. Mainnet Deployment Preparation
- [ ] Compile with `--features mainnet` flag
- [ ] Verify program ID: `8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX`
- [ ] Verify mint address: `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- [ ] Deploy to mainnet
- [ ] Transfer mint authority to PDA
- [ ] Verify PDA authority: `spl-token display memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- [ ] Execute test mint with small compute budget
- [ ] Verify a mint succeeds and gives the expected curve amount (1 token below 20M)
- [ ] Upgrade the on-chain IDL (`process_mint_to` now takes `recipient`)

#### 3. Documentation for Users/Integrators
- [ ] Transaction structure requirements (instruction ordering)
- [ ] Minimum memo length: 69 bytes
- [ ] No maximum memo length (bounded by the transaction size limit)
- [ ] Emission curve explanation
- [ ] Example transactions for wallets
- [ ] CPI integration guide for contracts

### ⚠️ RECOMMENDED - Post-Launch Monitoring

#### 4. Operational Monitoring
- [ ] Track current supply and the current curve step
- [ ] Monitor mint transaction rate
- [ ] Alert on supply approaching the next round number
- [ ] Track failed transactions (memo too short/long)

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

### Unit Tests (implemented, Revision 2)

`programs/memo-mint/src/tests.rs` — 47 tests, all passing:
- Emission curve: below the curve start, the current mainnet supply, every round number from 10M to 9T and one lamport below each (checked against an independent implementation of the formula), decade starts, monotonicity, minimum reward
- Max supply: exactly at the cap, above it, the last 2-lamport mint landing exactly on the cap, a mint that would overshoot, values near `u64::MAX`
- Memo length: empty, below/at/above both limits, binary and text content
- `process_mint_to` wire format: 8-byte discriminator + 32-byte recipient

### Integration Tests Needed

1. **Concurrent Minting**
   - Multiple users minting simultaneously
   - Behavior at curve steps
   - Behavior at max supply

2. **Transaction Structure**
   - Various instruction orderings
   - With/without compute budget
   - With additional instructions

3. **Error Paths**
   - Invalid memo lengths
   - Wrong token accounts
   - Invalid mint addresses
   - After max supply reached

---

## Audit Conclusion

### Final Status: ✅ **APPROVED FOR MAINNET DEPLOYMENT**

The memo-mint contract has passed comprehensive security review with **all design decisions verified as intentional**.

### Security Assessment: **EXCELLENT**

**Critical Security Strengths**:
- ✅ **Arithmetic Safety**: All operations use checked math, compile-time validation prevents overflow
- ✅ **Access Control**: PDA-based mint authority with proper validation
- ✅ **Account Security**: Comprehensive constraint validation for all accounts
- ✅ **Token2022 Compatibility**: Proper interface implementation
- ✅ **Reentrancy Protection**: No external calls before state changes
- ✅ **Supply Cap Enforcement**: Double-check pattern on circulating supply
- ✅ **Error Handling**: Descriptive errors with runtime context
- ✅ **Network Configuration**: Proper testnet/mainnet separation

**Confirmed Design Features**:
- ✅ **Fair-Launch Model**: Unrestricted minting is intentional (equal opportunity for all)
- ✅ **Performance Optimized**: Fixed memo position at index 0 reduces compute units
- ✅ **Flexible Integration**: Binary memo support enables contract composability
- ✅ **Emission Curve**: Round-number steps confirmed as economically sound (Revision 2)
- ✅ **Permissionless**: No artificial restrictions align with decentralization goals
- ✅ **Batch Minting**: Multiple mints per transaction allowed by design (Revision 2)

**Code Quality**: **EXCELLENT**
- Clean, well-documented code
- Defensive programming throughout
- Industry best practices followed
- Minimal complexity, maximum clarity

### Risk Assessment

**Security Risk**: ✅ **LOW**
- No critical vulnerabilities identified
- All potential issues investigated and resolved
- Design intent confirmed for all decisions

**Deployment Risk**: ✅ **LOW**
- Clear deployment procedure documented
- Authority transfer tool implemented
- Testnet validation path defined

### Mainnet Deployment Authorization

**The memo-mint contract is APPROVED for mainnet deployment**, subject to completing the pre-deployment checklist:

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
- All design decisions verified as intentional
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

**Remaining operational risk**: the program upgrade authority and the token metadata update authority are the same hot key. Whoever holds the upgrade authority can replace the program and mint without these rules. Consider moving the upgrade authority to a multisig or making the program immutable (`--final`) once Revision 2 is stable on mainnet.

---

## Appendix A: Code Quality Metrics

- **Lines of Code**: 326 (Revision 2)
- **Functions**: 2 instructions, 4 private helpers
- **Complexity**: Moderate
- **Test Coverage**: 47 unit tests (see Testing Recommendations)
- **Documentation**: Good
- **Error Handling**: Excellent

---

## Appendix B: Mainnet Deployment Procedure

### Step-by-Step Deployment Guide

**1. Build for Mainnet**
```bash
anchor build --features mainnet
```

**2. Verify Program ID**
```bash
solana address -k target/deploy/memo_mint-keypair.json
# Expected: 8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX
```

**3. Deploy Program**
```bash
anchor deploy --program-name memo-mint --provider.cluster mainnet
```

**4. Transfer Mint Authority to PDA**
```bash
cargo run --bin admin-transfer-memo-token-mint-authority
```

**5. Verify PDA Authority**
```bash
spl-token display memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick --program-id TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
# Verify "Mint authority" shows the PDA address
```

**6. Upgrade the On-Chain IDL**
```bash
anchor idl upgrade 8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX --filepath target/idl/memo_mint.json --provider.cluster mainnet
```

**7. Test Mint Transaction**
```bash
cargo run --bin test-memo-mint valid-memo
# Should succeed and mint the curve amount (1 token while supply is below 20M)
```

**8. Verify Contract Constants**
- Program ID: `8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX`
- Mint address: `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick`
- Token decimals: `6`
- Max supply: `10,000,000,000,000` tokens (circulating)
- Curve start: `10,000,000` tokens
- Memo length: at least `69` bytes (no contract maximum)

**9. Monitor Initial Launch**
- Track first 100 mint transactions
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
      - Data: at least 69 bytes (any binary format; no contract maximum)
      - Program: MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr
      - Required: YES
  
  [1+] MemoMint::process_mint OR MemoMint::process_mint_to (one or more)
      - Mints tokens based on the emission curve
      - Validates memo at index 0
      - Executes token mint CPI
      - Up to about 31 mints fit in one transaction (64-entry instruction trace limit)
  
  [last] ComputeBudgetProgram::SetComputeUnitLimit (optional)
      - Sets compute budget for transaction; X1 charges for the requested limit,
        so simulate first and set the exact value
      - Must NOT be at index 0
```

### Example: Successful Mint Transaction

```rust
// Instruction 0: Memo (69+ bytes) - REQUIRED at index 0
spl_memo::build_memo(
    b"This is a valid memo with at least 69 bytes of content for token minting...",
    &[]
)

// Instruction 1: Mint
memo_mint::process_mint(
    mint: Pubkey,
    mint_authority: Pubkey (PDA),
    token_account: Pubkey,
    user: Signer,
    token_program: Token2022,
    instructions: Sysvar
)

// Last instruction: Compute Budget (optional, never at index 0)
ComputeBudgetInstruction::set_compute_unit_limit(simulated_units)
```

### Common Integration Errors

❌ **Wrong instruction order** - Memo not at index 0
❌ **process_mint at index 0** - Must be at index 1 or later
❌ **Memo too short** - Less than 69 bytes
❌ **Transaction too large** - The memo and instructions together exceed the 1,232-byte transaction size limit
❌ **Wrong token program** - Using SPL Token instead of Token2022
❌ **Compute budget at index 0** - Pushes the memo out of index 0
❌ **Old IDL for process_mint_to** - Missing the `recipient` argument

---

## Appendix D: Economic Model Reference

### Emission Curve (Revision 2)

| Supply range | Reward per mint |
|---|---|
| 0 – 20M | 1 |
| 20M – 30M ... 90M – 100M | 0.9 ... 0.2 |
| 100M – 200M | 0.1 |
| 200M – 300M ... 900M – 1B | 0.09 ... 0.02 |
| 1B – 2B | 0.01 |
| 10B – 20B | 0.001 |
| 100B – 200B | 0.0001 |
| 1T – 2T | 0.00001 |
| 9T – 10T | 0.000002 |

Rule: in `[d × 10ⁿ, (d+1) × 10ⁿ)` from 10M on, reward = `decade_base × (11 − d) / 10`.

### Economic Incentives

**Early Miner Advantage**: 
- The first 20M tokens are minted at 1 token per mint
- Rewards then fall one step at every round number
- Predictable, gradual reward decay with no 10x cliffs

**Cost Anchor**:
- Minting costs the transaction fee, which X1 charges by requested compute units
- Minting is worthwhile only when the market price exceeds the cost per token, which keeps the price roughly anchored relative to XNT
- Batching several mints in one transaction lowers the cost per token (the memo is paid once)

**Supply Cap**:
- Maximum of 10 trillion tokens in circulation, enforced on every mint
- Burns free room under the cap, so minting can continue indefinitely
- Changeable only through a program upgrade by the upgrade authority

---

**Audit Report End**

**Audit Date**: October 27, 2025 (original), revised October 13 and 14, 2026  
**Contract Version**: Revision 3 (0.3.0)  
**Final Status**: ✅ APPROVED FOR MAINNET (Revision 3 pending testnet validation)  

*This audit report is provided for informational purposes and does not constitute financial or legal advice. The auditor has conducted a thorough review of the smart contract code and design, confirming its security and correctness as of the audit date.*

