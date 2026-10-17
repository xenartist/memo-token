# MEMO Token Core Programs — Final Security Audit Report (memo-mint / memo-burn)

| Item | Details |
|---|---|
| Audit target | `memo-mint`, `memo-burn` (Anchor 0.32.1, Token-2022, decimals = 6, X1 chain) |
| Code version | Branch `dev`, HEAD `ca4c8a0b` ("re-design memo emission curve") plus uncommitted working-tree changes. This report audits the **working tree** |
| Report date | 2026-10-16 |
| Audience | Project owner; third-party developers building on MEMO |
| Severity definitions | Critical: unauthorized minting, burning other users' assets, breaking the supply cap, multiple mints in one transaction, theft of funds. High: a core invariant broken under realistic conditions, or permanent denial of service for minting or burning. Medium: an invariant broken under edge conditions, real harm to other users, or accounting errors with real impact. Low: minor accounting/logging/UX inconsistencies, defense-in-depth gaps. Info: observations, design notes, centralization and trust assumptions |

---

## 1. Executive summary

### 1.1 Overall conclusion

| Program | Conclusion |
|---|---|
| **memo-mint** | Within the scope of this audit (see 1.3–1.6), **no critical, high, medium or low program defects were found, and the program code does not need to change**. At most one mint per transaction, the 10T circulating-supply cap, the reward curve, the mint authority and the account bindings are all proven from the code (together with dependency source), and the expected results were observed in local-validator attack tests and in read-only simulations on X1 testnet and mainnet. All confirmed program-related findings are Info. They are design notes, documentation errors, test coverage gaps and runtime trust assumptions. |
| **memo-burn** | Within the scope of this audit, **no critical, high, medium or low program defects were found, and the program code does not need to change**. Proven: only tokens of the authorized mint held by the signer can be burned; the memo and the amount are bound per instruction; the statistics arithmetic is correct and atomic with the burn CPI. Every rejection path was observed on a local validator running a binary byte-identical to the testnet deployment. **Note: mainnet currently runs the HEAD `ca4c8a0b` version, which still requires "whole tokens, at least 1 token". This report's conclusion that "any amount from 1 base unit can be burned" holds for mainnet only after mainnet is upgraded to the working tree (OPS-01, OPS-04).** |

This report **does not claim "100% security"**. The conclusions hold only within these limits:
- The audit covers the working-tree source and the dependency versions listed in section 2.2.
- Runtime semantics are taken from X1 `solana-core 4.0.3` (tachyon branch). Its program-runtime / transaction-context / syscalls / bpf_loader source was checked to be byte-identical to the agave 4.0.0 crates (INT-12).
- Token-2022 and SPL Memo are taken as the programs actually deployed on chain.
- The trust assumptions in section 1.6 hold.

The audit found no way to bypass the core security properties. The remaining risk comes mainly from **upgrade authority and key management (centralization)** and from **the upgradeability of the platform runtime and Token-2022**, not from the program logic itself.

**Owner response on key centralization (2026-10-16):** the owner accepts the key-centralization risk (OPS-02, OPS-05) for now. Once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (the programs will be made immutable). This report therefore treats OPS-02 and OPS-05 as **accepted risks with a planned mitigation**, not as open actions that block the mainnet upgrade. Severities are unchanged.

### 1.2 Finding counts (confirmed findings only)

| Final severity | Count | IDs |
|---|---|---|
| Critical | 0 | — |
| High | 0 | — |
| Medium | 0 | — |
| Low | 1 | OPS-02 (operations: the mainnet upgrade/admin hot key is also used for testnet staging deployments. Not a program code issue. Accepted risk with a planned mitigation, see 8.3) |
| Info | 43 | Core programs and runtime 19, integration 4, operations and on-chain 8, test coverage 9, existing documentation 3 (details in section 4) |
| **Total confirmed** | **44** | |

**Findings judged invalid or non-issues (not counted above): 4**

| ID | Original title (short) | Conclusion |
|---|---|---|
| BURN-08 | Vaults owned by data PDAs can "never" be burned through memo-burn | Core claim is invalid: Anchor `init` skips the payer transfer when the PDA is already prefunded; verified by local case B35 |
| TEST-04 | No test reaches the `token_account.mint == mint` constraint | Core claim is invalid: local cases M12b (6002) and B28b (6006) reach it. The only remaining gap is the recipient branch of `process_mint_to` |
| TEST-11 | Tests run on three different Token-2022 binaries | Invalid: the Token-2022 built into the local validator is byte-identical to the X1 mainnet deployment (sha256 `d32c6364…`) |
| OPS-06 | X1 Token-2022 is upgraded by "a single external private key" | Original claim is invalid: the mainnet upgrade authority is a Squads v4 multisig vault (3/6, no timelock). The underlying platform trust assumption still holds and has been merged into 1.6 and section 6 |

**Severity adjustments**: the following 11 findings were first reported as Low and set to Info after review by two reviewers (all are documentation, test or operations issues and do not affect on-chain security properties): MINT-01, MINT-02, BURN-01, BURN-02, BURN-03, OPS-01, OPS-03, TEST-01, TEST-02, TEST-03, TEST-05.

### 1.3 What has been proven (code proof: line-by-line reading of the programs, combined with dependency source and offline harnesses)

1. **At most one successful mint per transaction**, on every path: multiple top-level instructions, `process_mint` + `process_mint_to`, CPI from any program, multiple CPIs within one instruction, nested CPI, and mixed top-level and CPI.
   - Basis: fingerprint = SHA-256(instructions sysvar data minus the last 2 bytes) (`programs/memo-mint/src/lib.rs:111-117`). It is recorded together with the slot before the mint CPI (`lib.rs:185-186`) and compared by `record_mint` (`lib.rs:98-106`).
   - The runtime builds the sysvar data only once per transaction. During execution it only rewrites the trailing u16 current index (`solana-instructions-sysvar-2.2.2/src/lib.rs:68-74, 99-129, 171-181`; `solana-transaction-context-2.3.13/src/lib.rs:378-391`).
   - The sysvar address is a reserved key. It is demoted to read-only and no program can write it.
   - Only memo-mint can write `MintState`, and transactions are atomic.
2. **The reward curve** matches the specification exactly for all 2^64 inputs: monotonically non-increasing, minimum 1 lamport, and each step happens exactly "after" every integer d·10^k tokens.
   - Basis: 57 constant intervals were derived from the control flow. All interval endpoints were checked against two independent references (an exact rational Python implementation and a u128 breakpoint table), plus 40 million random/boundary samples. Total 41,809,355 points, 0 failures.
3. **Circulating supply can never exceed 10T tokens** (1e19 units).
   - `calculate_dynamic_mint_amount` returns `SupplyLimitReached` (6005) when supply ≥ MAX. Otherwise it guarantees supply + amount ≤ MAX (`lib.rs:303-322`).
   - The mint authority is the memo-mint PDA `[b"mint_authority"]`. The mint has no freeze authority and no dangerous extensions.
   - The curve and cap arithmetic do not overflow or panic for any u64.
4. **memo-burn can only burn Token-2022 accounts of the authorized mint held by the signer**, and records the statistics under that signer (`programs/memo-burn/src/lib.rs:381-418`).
5. **memo-burn memo parsing** (`lib.rs:228-279`) accepts the same set as `BurnMemo::try_from_slice`: differential testing with 2,000,000 + 300,000 cases, 0 divergences. Base64 is strictly canonical and decodes reversibly and uniquely (3,000,000 cases).
6. **Statistics arithmetic**:
   - User statistic `total_burned` is always ≤ 18T; `burn_count` is a saturating counter.
   - Global statistics are u128 saturating counters.
   - Statistics are written only after the Token-2022 burn CPI succeeds.
7. **CPI caller tampering has no effect**: on runtime code identical to X1 4.0.3, a CPI caller cannot tamper with any account the callee reads (sysvar, mint, MintState, token accounts, statistics accounts) and cannot escalate privileges (INT-12: 140 runs, 0 bypasses).

### 1.4 What has been tested

- **Unit tests**: memo-mint 61/61 and memo-burn 87/87 pass (60/86 hand-written respectively, plus the Anchor-generated `test_id`). Both the testnet build and the `--features mainnet` build pass.
- **Local validator** (solana-test-validator 2.2.20, loading `.so` files byte-identical to the testnet deployment; the built-in Token-2022 is byte-identical to the X1 mainnet deployment):
  - All 131 dynamic cases pass (see 5.4).
  - The existing pen-test passes 52/52 with 0 warnings.
  - All 7 smoke tests pass.
  - The threshold matrix and cap tests all pass.
- **X1 testnet** (all 7 programs upgraded to this working tree):
  - Smoke tests for mint, burn, profile, blog and forum pass.
  - Burns of 0.5, 0.000001 and 1.234567 tokens succeed; 0 is rejected.
  - Read-only simulation confirms that combinations such as `[memo, mint, mint]` return 6008 at the second mint.
- **X1 mainnet** (HEAD bytecode): read-only simulation confirms that `mint+mint` and `mint_to+mint` return 6008 at the 3rd instruction.
- **Reproduction**: of 26 findings, 25 were reproduced dynamically and 1 (OPS-10) was checked statically only.

### 1.5 What was not tested, or is supported only by reasoning

- **Double mint through two levels of nested CPI**: code reasoning only. "Multiple CPIs within one top-level instruction" was observed to return 6008 in the control case of the LiteSVM harness (INT-12), but no CPI test program is committed to the repository (TEST-01).
- **End-to-end burns on the mainnet runtime + mainnet Token-2022**: not executed, including small-amount burns (mainnet rejects small amounts before the upgrade anyway). However, the testnet runtime version is the same as mainnet, and the local Token-2022 is the same as mainnet.
- **The mainnet upgrade itself**: not executed. By rule, no mainnet transactions are sent.
- **Deployment scripts**: not actually run. They need encrypted keys and act on real clusters. OPS-09 and OPS-10 were checked statically only.
- **The exact X1 per-writable-account block CU limit**: not verified, because the cost-model source was not available locally (INT-08).
- **There are no repeatable regression tests in the repository** covering memo-burn rejection paths, CPI double mint, or `check_memo_instruction`. The dynamic harnesses used in this audit are temporary files and were not committed (TEST-01/02/03). **Status: addressed by committed regression tests — see section 8.2.**

### 1.6 Residual trust assumptions

1. **Project upgrade authority (the main remaining risk)**
   - On mainnet, memo-mint, memo-burn and all 5 application programs are upgradeable. The upgrade authority is a single System-owned hot key `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ` (not a multisig). This key is also `AUTHORIZED_ADMIN_PUBKEY`, the IDL authority, and the update authority of the token metadata.
   - Anyone who holds or steals it can replace memo-mint and mint without limit through the PDA, or replace any program and abuse users' signer privileges (OPS-05).
   - This key is also used for testnet staging deployments (OPS-02).
   - **Owner response (2026-10-16):** accepted risk for now. Once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable).
2. **X1 platform**
   - Token-2022 (`TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`) is upgradeable on X1. The mainnet upgrade authority is the Squads v4 multisig vault `2sXr5THr5ZAwSfuJAc1KogPKZ2uKkhfNUAtEvNw5Q6Z7` (multisig `89PksQjFwnpUNu3Rc3GLdnZnBCbUNLNruDHcjdU1zPfs`, threshold 3/6, time_lock 0). On testnet it is a single key.
   - MEMO's supply cap, mint authority control and balances all assume that Token-2022 behaves according to its public code.
3. **Runtime semantics**
   - The one-mint guarantee depends on the instructions sysvar layout (MINT-03).
   - CPI account synchronization depends on the runtime's byte comparison that enforces "a non-owner must not modify an account". SIMD-0459 pointer checks are not yet active on mainnet, so this is the only barrier (INT-12).
   - Re-verify after any X1 validator upgrade.
4. **SPL Memo** (`MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr`) is loaded by BPFLoader2 on both networks and is not upgradeable. No extra trust is needed here.
5. **Key management**: private keys are stored GPG-encrypted and are used only after being unlocked to a RAM disk. The testnet and mainnet key trees are on the same machine.

---

## 2. Scope and method

### 2.1 Scope

| Category | Files |
|---|---|
| Core programs | `programs/memo-mint/src/lib.rs` (461 lines), `programs/memo-mint/src/tests.rs`; `programs/memo-burn/src/lib.rs` (469 lines), `programs/memo-burn/src/tests.rs` |
| CPI callers (included only where they could break core invariants) | `programs/memo-chat`, `memo-blog`, `memo-forum` (CPI to `process_mint` and `process_burn`; note: the apps call `process_mint`, not `process_mint_to`); `programs/memo-profile`, `memo-project` (CPI to `process_burn` only) |
| Clients and scripts (read-only) | `clients/` (`clients/src/mint_curve.rs`, `clients/mint/src/pen-test-memo-mint.rs`, `clients/smoke-test/src/*`, admin-init clients); `scripts/deploy-*.sh`, `scripts/keys.sh` |
| Existing documentation (checked item by item, not assumed correct) | `MEMO-MINT-AUDIT.md`, `MEMO-BURN-AUDIT.md` (relevant sections of `MEMO-PROJECT-AUDIT.md` and `MEMO-CHAT-AUDIT.md` were also checked) |
| On chain (read-only RPC) | `https://rpc.testnet.x1.xyz`, `https://rpc.mainnet.x1.xyz` |

### 2.2 Versions

| Component | Version |
|---|---|
| Framework | anchor-lang / anchor-spl / anchor-syn 0.32.1, borsh 0.10.4 (used inside Anchor) |
| Token | spl-token-2022 8.0.1 (actually linked by the programs, pulled in by anchor-spl); 9.0.0 (used only by the clients crate, semantics compared); spl-token 8.0.0 |
| Other dependencies | spl-memo 6.0.0, solana-program 2.3.0, solana-instructions-sysvar 2.2.2, solana-transaction-context 2.3.13, solana-message 2.4.0, solana-sha256-hasher 2.3.0, base64 0.22.1 |
| Toolchain | anchor-cli 0.32.1, cargo-build-sbf 2.2.20, platform-tools v1.48 (SBF rustc 1.84.1), host rustc 1.93.1 |
| X1 runtime | Both testnet and mainnet run `solana-core 4.0.3`, feature-set 2137973216. Source is x1-labs/tachyon v4.0.3 = agave v4.0.3 + 14 commits. None of these commits touch program-runtime, syscalls, transaction-context, bpf_loader, svm-feature-set or svm |
| Local validator | solana-test-validator 2.2.20 (Agave, feature-set 3073396398) |

### 2.3 Review perspectives

10 independent perspectives in total:
1. memo-mint one-mint mechanism
2. Reward curve and supply cap
3. memo-mint accounts and authorities
4. memo-burn memo validation
5. memo-burn accounts and statistics
6. CPI composition and integration
7. Runtime and dependency semantics
8. Operations and on-chain configuration
9. Test coverage
10. Second-round addition: CPI tampering, using a malicious caller program and a LiteSVM harness

### 2.4 Dynamic test environment

- **Local validator**: started with `scripts/local-validator.sh`, loading `target/deploy/*.so` (testnet IDs). First, `memo_mint.so` and `memo_burn.so` were checked to be byte-identical across a fresh build of the working tree, the testnet on-chain programs, and the programs loaded by the local validator (ENV-01). 7 rounds in total, each with a different initial supply:
  - 5T: functional and attack suite
  - 10M / 20M / 100M / 1B / 9T: curve steps
  - 10T: cap
  - 18,446,744,073,709 tokens: statistics saturation
  - Plus dedicated runs for CPI Guard / v0 + lookup tables / durable nonce
- **Test method**:
  - Rejection cases are sent with `skip_preflight`, so the transaction lands on chain and pays fees; the full logs are then fetched with `getTransaction`.
  - Before and after each case, token balances, supply, `MintState` (fingerprint, slot), and user and global statistics are read.
  - A rejection case passes only if both "the error code is correct" and "all observed values are unchanged" hold.
  - For successful mint cases, the minted amount must equal the value of an independently implemented specification curve, and `MintState` must equal the transaction fingerprint and slot recomputed by the client.
- **X1 testnet**: real on-chain smoke tests (run by the project team; results used as input evidence), plus read-only `simulateTransaction` by reviewers (`sigVerify=false`, not signed, not sent).
- **X1 mainnet**: read-only RPC, and read-only simulation against the HEAD bytecode.
- **Offline**:
  - Curve harness: 41,809,355 points
  - Curve fuzz: 4,000,861 points
  - memo-burn differential fuzz: 2,000,000 and 300,000 cases
  - Base64 strictness: 3,000,000 cases
  - Offline fingerprint demo: fp-demo, using the real decompile and serialization pipeline
  - LiteSVM CPI tampering harness: 140 runs
  - Mutation testing: 19 mutants built in a temporary copy

### 2.5 Review process

1. Reviewers for each perspective raise findings independently. Each finding includes `file:line` and a failure scenario.
2. Each finding is given to **2 independent reviewers for adversarial review**; the reviewers try to refute it. Each decides independently whether it holds and gives a severity, reasoning, evidence and a recommendation, verified against dependency source or on-chain data.
3. When the two disagree, the final severity follows the side with more specific evidence (consensus severity), and the disagreement is noted in section 4. BURN-01, BURN-02 and OPS-03 were split between Low and Info; OPS-06 and TEST-11 were split between valid and invalid.
4. A further round of dynamic reproduction: 25 of 26 findings were reproduced on the local validator or via read-only RPC; OPS-10 was checked statically only. Mutants were also built to test whether the existing tests catch regressions.

### 2.6 Disclosure of actions taken during the audit

- No transactions were sent to testnet or mainnet, and `programs/`, `scripts/` and the documentation were not modified.
- One reviewer used unsigned `simulateTransaction` against X1 testnet and mainnet (no state change). This goes beyond the read-only query types listed in the task and is disclosed here.
- To force recompilation, two reviewers ran `touch` on `programs/memo-mint/src/lib.rs` (and `programs/memo-burn/src/lib.rs`). **Only the modification time changed, not the content.** In one case the original time was restored. `git diff` output is unchanged, and the next `anchor build` produces identical bytes.
- The local validator was used only during the dynamic testing and reproduction phases and has now been stopped.

---

## 3. Core invariants

Evidence types:
- **Code proof**: line-by-line reading of the programs with reasoning over dependency source, including offline harnesses and fuzzing.
- **Unit test**: `tests.rs` in the repository.
- **Local on-chain attack test**: local-validator dynamic cases, pen-test, reproduction.
- **Testnet live test**: on-chain or read-only simulation on testnet; parts done as mainnet read-only simulation are marked.

### 3.1 memo-mint

| # | Invariant | Status | Evidence type | Main basis |
|---|---|---|---|---|
| M-1 | At most one successful mint per transaction (multiple top-level, mint + mint_to, CPI from any program, multiple CPIs in one instruction, nested CPI, top-level + CPI) | Holds (nested CPI by code proof only) | Code proof; unit test; local on-chain attack test; testnet live test (including mainnet read-only simulation) | `memo-mint/src/lib.rs:98-106, 111-117, 185-186, 196-207, 357-362, 399-404`; unit tests `tests.rs:658-769` (use the runtime serializer; `tests.rs:771-781` is tautological); local M01–M04g, M03c, X2b, PEN-instruction/dos/mint-to; testnet simulation `[memo,mint_to,mint_to]`→`[2,6008]`, `[memo,CU,mint_to,memo,mint_to]`→`[4,6008]`, app CPI combinations→IX2 6008; mainnet simulation→`[3,6008]`; LiteSVM control S2→6008 (INT-12) |
| M-2 | Within one transaction, all top-level instructions and CPIs compute the same fingerprint, and no program can influence it | Holds | Code proof; testnet live test | `solana-instructions-sysvar-2.2.2/src/lib.rs:68-74, 99-129, 138-161, 171-181`; `solana-transaction-context-2.3.13/src/lib.rs:378-391`; `solana-reserved-account-keys-2.2.2/src/lib.rs:172`; address constraints `lib.rs:353, 395`; tachyon 4.0.3 runtime source identical (INT-12) |
| M-3 | The slot used by `record_mint` does not change within a transaction | Holds | Code proof | `Clock::get()` reads the bank's sysvar cache (`solana-sysvar-2.3.0 clock.rs:133-134`) |
| M-4 | Dropping the last 2 bytes removes exactly the current-index field, for every sysvar shape | Holds | Code proof | `lib.rs:111-117` matches the handling in `load_current_index` and `store_current_index_checked` |
| M-5 | `MintState` cannot be forged, replaced, re-initialized, reset or closed; only the admin can initialize it, once | Holds | Code proof; local on-chain attack test | `lib.rs:357-362, 399-404, 407-426`; Anchor checks owner, discriminator, seeds and the stored bump; local M16, M16b (3002), M16c (3007), M16d (2000), M17a; non-admin initialization on a fresh ledger→6007 (reproduces TEST-09); on-chain bump 254 is canonical |
| M-6 | `MintState` is persisted only at exit, but there is no reentrancy window | Holds | Code proof | Token-2022 `process_mint_to` makes no CPI (`spl-token-2022 processor.rs:973-1058`); `MintState` is not passed to the CPI; the runtime forbids A→B→A reentrancy (MINT-04) |
| M-7 | A caller (app or any program) cannot write back a stale `MintState` | Holds | Code proof; local on-chain attack test | `anchor-lang account.rs:252-265` (exit does not write when the owner is not the current program); INT-12 B1/B4→`ExternalAccountDataModified` |
| M-8 | Fails closed when `MintState` is not initialized; prefunding cannot block initialization | Holds | Code proof; local on-chain attack test | `account.rs:302-309`; `process_mint` on a fresh ledger→3012 (reproduces TEST-09) |
| M-9 | Only memo-mint can mint MEMO: the mint authority is a PDA, there is no freeze authority, no dangerous extensions, and the PDA signature cannot be reused for other purposes | Holds | Code proof; local on-chain attack test; testnet live test (read-only RPC) | On-chain mint authority testnet `HDz8d6Uj…` (bump 255) and mainnet `C1r27YsP…` (bump 254) match the PDA derivation; `invoke_signed` appears only at `lib.rs:196-207`; local M11, M11b, M11c, M16f, PEN-pda |
| M-10 | The reward curve matches the spec exactly for all u64; monotonically non-increasing; ≥1 lamport; steps exactly after each integer d·10^k | Holds | Code proof; unit test; local on-chain attack test; testnet live test | `lib.rs:275-300`; harness 41,809,355 points and fuzz 4,000,861 points, both 0 failures; independent reference in unit tests `tests.rs:23-38`; local S-10M/20M/100M/1B/9T; chat thresholds on testnet and locally |
| M-11 | Curve and cap arithmetic do not overflow or panic for any u64; loop runs ≤ 5 times | Holds | Code proof | `lib.rs:286-299, 313`; harness with overflow-checks on; `Cargo.toml:9-10` `overflow-checks = true` |
| M-12 | Circulating supply never exceeds 10T; returns 6005 when supply ≥ MAX, otherwise supply + amount ≤ MAX; the overflow branch is unreachable | Holds | Code proof; local on-chain attack test | `lib.rs:303-322`; local C01, C01b, C03, C04, T01 |
| M-13 | Minting alone, starting from supply 0, stops exactly at MAX | Holds | Code proof | Closed-form simulation of 2,857,543,690,884,166,656 mints in total; the end point is exactly 1e19 |
| M-14 | 10T caps **circulating supply**, not cumulative issuance | Holds (by design) | Code proof; local on-chain attack test | `lib.rs:192, 305`; local C02→C03→C04 (MINT-05) |
| M-15 | Memo rule: index 0 must be SPL Memo v3 (`MemoSq4g…`) and ≥ 69 bytes; the mint instruction is at index ≥ 1 (also applies under CPI) | Holds | Code proof; local on-chain attack test | `lib.rs:223-268`; local M05–M10b, X3a, PEN-memo; offline harness tests a `check_memo_instruction` matrix with a real sysvar (no such unit test in the repository, see TEST-03) |
| M-16 | The mint amount is determined only by on-chain supply and the curve; the caller cannot choose it | Holds | Code proof; local on-chain attack test | `lib.rs:125-155, 192-193`; local M19c, PEN-account (appended `u64::MAX` data is ignored) |
| M-17 | Minted tokens can only go to a Token-2022 account of this mint; `process_mint` can only mint to the signer's own account | Holds | Code proof; local on-chain attack test | `lib.rs:343-348, 385-390`; local M12, M12b, M13, M13b. The recipient mint constraint branch of `process_mint_to` (`lib.rs:386-387`) is not yet reached by any test |
| M-18 | `token_program` is the real Token-2022, the sysvar is the real sysvar, and the mint equals `AUTHORIZED_MINT` | Holds | Code proof; local on-chain attack test | Local M14, M14b, M15, PEN-account |
| M-19 | A third party cannot make another user's mint fail with 6008 | Direct calls and built-in apps: holds; third-party integrations without a signer: does not hold | Code proof; local on-chain attack test | Local M20.1/M20.2/M20c; reproduction: third-party crank (MINT-01) |
| M-20 | 6008 rejects only "a second mint in the same transaction" | Does not hold (benign false positive, design trade-off) | Code proof; local on-chain attack test | Another transaction adjacent in the same slot with byte-identical instruction list is also rejected (MINT-01) |

### 3.2 memo-burn

| # | Invariant | Status | Evidence type | Main basis |
|---|---|---|---|---|
| B-1 | Only Token-2022 accounts of the authorized mint held by the signer can be burned; delegates, permanent delegates and third parties cannot; statistics are recorded under the signer | Holds | Code proof; local on-chain attack test | `memo-burn/src/lib.rs:381-418`; local B24, B25, B25b (6008), B26 (2006), B28 (6007), B28b (6006), B29 (3008), B32 (3010), B32b (2000) |
| B-2 | Each `process_burn` burns exactly `amount`, with 1 ≤ amount ≤ 1e18, amount equal to burn_amount in the memo, and version 1; errors are returned in a fixed order | Holds | Code proof; local on-chain attack test; testnet live test | `lib.rs:164-193, 228-279`; local B01–B09, B04b; testnet 0 / 0.5 / 0.000001 / 1.234567 |
| B-3 | The memo is at index 0, is SPL Memo v3, and is ≥ 69 bytes (for the Base64 memo the shortest decodable length is 72, see BURN-02); `process_burn` is at index ≥ 1 | Holds | Code proof; local on-chain attack test | `lib.rs:283-299, 311-338`; local B19 (6010), B19b, B20–B22b, X3c |
| B-4 | Base64 is strictly canonical and decodes reversibly and uniquely | Holds | Code proof | `base64-0.22.1` with `RequireCanonical` and no trailing bits allowed (`general_purpose/mod.rs:252-262, 332, 347`; `decode_suffix.rs:94-141`); fuzz 3,000,000 cases; local B16–B18b |
| B-5 | The header-only parser accepts the same set as `BurnMemo::try_from_slice` and reads the same version and amount | Holds | Code proof; local on-chain attack test | `borsh-0.10.4/src/de/mod.rs:45-52, 374-399`; differential 2,000,000 and 300,000 cases, 0 divergences; local B10–B14 |
| B-6 | The total burned in a transaction equals burn_amount in the memo | Does not hold (by design: per-instruction binding) | Code proof; local on-chain attack test | BURN-04; local B23, B23c, B37, T02 |
| B-7 | Statistics are exact: user total = min(cumulative, 18T), `burn_count` saturating, global u128 saturating, atomic with the burn CPI | Holds | Code proof; unit test (covers `record_burn` only); local on-chain attack test | `lib.rs:196-217, 109-113`; local B02, B02b, B03, T02, T04; smoke test asserts the user statistic increment |
| B-8 | Only the user can create their own `UserGlobalBurnStats`; it cannot be front-run or blocked, and cannot be reset | Holds | Code proof; local on-chain attack test | `lib.rs:341-356`; local B33, B35 (initialization still works after prefunding) |
| B-9 | `GlobalBurnStats` is unique, can be created only once by the admin, and cannot be reset | Holds | Code proof; local on-chain attack test | `lib.rs:359-377`; local B34; non-admin on a fresh ledger→6012 (reproduces TEST-09) |
| B-10 | A third party cannot stop others from burning (freeze, pause, statistics PDA) | Holds | Code proof; testnet live test (read-only RPC) | Freeze authority is null and cannot be set again (`spl-token-2022-9.0.0 processor.rs:767-773`); extensions cannot be added afterwards; only the owner can enable CPI Guard (BURN-06) |
| B-11 | Heap use and sizes are bounded | Holds | Code proof; local on-chain attack test | The heap use for the largest memo (about 812 bytes legacy, about 960 bytes v0) is about 1.7KB, far below 32KB; harness verified with a 1KB memo |
| B-12 | Error codes are stable (6005 `InvalidBurnAmount` kept as a placeholder) | Holds | Code proof | `lib.rs:421-462`; the offline harness pins the error codes (no such test in the repository, see TEST-10) |
| B-13 | A third-party program cannot tamper with statistics through CPI, nor burn on behalf of a user who did not sign | Holds | Code proof; local on-chain attack test | INT-12 D1–D4 |
| B-14 | Every decrease of MEMO supply goes through memo-burn | Does not hold (Token-2022 semantics, by design) | Code proof; local on-chain attack test | BURN-05; reproduction A-burn05 |
| B-15 | `GlobalBurnStats` equals all burns ever made through memo-burn | Does not hold (counts only after initialization) | Testnet live test (read-only RPC on both networks) | BURN-01 |

### 3.3 Cross-program and runtime

| # | Invariant | Status | Evidence type | Main basis |
|---|---|---|---|---|
| X-1 | The callee always reads the real transaction-level account state; a CPI caller's modified copy never reaches the callee | Holds | Code proof; local on-chain attack test (LiteSVM) | tachyon 4.0.3 `serialization.rs:543-565, 647-656`, `cpi.rs:1255-1264`, `instruction_accounts.rs:91-111, 120-128, 181-192, 339-362`; INT-12, 140 runs in total, 0 bypasses |
| X-2 | A CPI caller cannot escalate a sysvar, `MintState` or a PDA to writable or signer | Holds | Code proof; local on-chain attack test (LiteSVM) | `invoke_context.rs:331-341, 396-421`; INT-12 E1–E7, D4→`PrivilegeEscalation` |
| X-3 | There is no reentrancy; current CPI depth is sufficient (deepest is app→core→Token-2022, 3 levels, limit 5) | Holds | Code proof; testnet live test | `transaction-context lib.rs:372-374` |
| X-4 | blog/forum mint memos cannot authorize burns, and burn memos cannot authorize app mints | Holds | Code proof | `memo-blog lib.rs:964` and `memo-forum lib.rs:788` require burn_amount == 0; memo-burn requires amount ≥ 1 |
| X-5 | Network constants switch consistently; no mainnet app links testnet core IDs | Holds | Code proof; unit test | The two sets of `declare_id!` in the 7 programs match `Anchor.toml`; apps cascade `mainnet = ["memo-mint/mainnet","memo-burn/mainnet"]`; unit tests pass under both features |
| X-6 | On-chain syscalls and off-chain unit tests use the same SHA-256 | Holds | Code proof | `solana-sha256-hasher-2.3.0 lib.rs:32-54` |
| X-7 | Testnet test evidence applies to mainnet runtime semantics | Holds (error codes may differ, see INT-12) | Testnet live test | Both networks run `solana-core 4.0.3` with the same feature-set; SIMD-0459 is active only on testnet |

### 3.4 Deployment and operations

| # | Invariant | Status | Evidence type | Main basis |
|---|---|---|---|---|
| D-1 | The bytecode deployed on testnet is the audited working tree | Holds | Local on-chain attack test; testnet live test (program dump) | ENV-01: sha256 `caa266ae…` / `eecad893…` |
| D-2 | The bytecode deployed on mainnet is the audited working tree | **Does not hold** (mainnet runs HEAD `ca4c8a0b`) | Testnet live test (mainnet read-only dump) | OPS-04 |
| D-3 | `MintState` and `GlobalBurnStats` exist on both networks, use canonical bumps and are rent-exempt | Holds | Read-only RPC | See section 6 |
| D-4 | Deployment scripts never auto-confirm a mainnet deployment; mainnet builds always use `--features mainnet` | Holds (network detection relies on a URL substring, see OPS-09) | Code proof | `scripts/deploy-common.sh:449-456`; `scripts/deploy-mainnet.sh:31-40, 56-60` |
| D-5 | A binary built for the wrong network cannot run at the other network's program address | Holds | Code proof | `anchor-syn-0.32.1/src/codegen/program/entry.rs:51-53` (`DeclaredProgramIdMismatch`) |
| D-6 | SPL Memo is not upgradeable | Holds | Read-only RPC | Owned by BPFLoader2 on both networks |
| D-7 | No single private key can mint outside the curve and cap or change core rules | **Does not hold** (trust assumption; accepted risk with a planned mitigation) | Read-only RPC | OPS-05 |
| D-8 | The mainnet upgrade key is used only on mainnet | **Does not hold** (accepted risk with a planned mitigation) | Read-only RPC; code proof | OPS-02 |

---

## 4. Findings in detail

### 4.1 Low

#### OPS-02 The mainnet upgrade/admin hot key is also used for testnet staging deployments
- **Final severity**: Low (reported Low; both reviewers set Low)
- **Category**: Operations (not a program code issue)
- **Location**: `scripts/deploy-prod-staging-testnet.sh:13-14`; `scripts/deploy-common.sh:207-219, 542, 555-560, 573-577`; `scripts/deploy-mainnet.sh:51`
- **Description**:
  - `deploy-prod-staging-testnet.sh` sets `ENV=mainnet` and `FEATURE_FLAG=mainnet`, while forcing the cluster to testnet (lines 23-44).
  - As a result, `deploy_to_env` signs with `~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json` (`deploy-common.sh:542`). `verify_configuration` also requires this mainnet key to be unlocked (`deploy-common.sh:207-219`).
  - On chain, copies of the mainnet program IDs exist on testnet, and their upgrade authority is the mainnet key `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ`: `8iq6…` was last deployed at testnet slot 195610675 (2026-10-15T12:29:19Z), and `2sb3…` at slot 195559033 (07:24:05Z).
  - This key is an ordinary System-owned keypair, not a multisig. Its balance is 91.19 XNT on mainnet and 480.88 XNT on testnet.
  - The mainnet memo-mint upgrade (2026-10-15T12:38:02Z) happened only about 9 minutes after the testnet staging deployment. The key trees for both networks are on the same workstation. This contradicts the checklist item "This is NOT the same machine as testnet?" in `deploy-mainnet.sh:51`.
- **Failure scenario**: while the mainnet key is unlocked (after `keys.sh unlock mainnet`, the plaintext is on the RAM disk), the machine or terminal session used for testnet staging is compromised. The attacker upgrades mainnet memo-mint (`8iq6…`) to malicious code and mints at will through the PDA `C1r27YsPiY6wbVvRY2cVCsaRKK5HiXxdvWhxJd5wapSh`, bypassing the curve and the 10T cap.
- **Evidence**: two reviewers independently verified this with read-only `solana program show`, ProgramData header decoding, `getBlockTime` and `getBalance`. The local `.pub` file confirms that `FVvew…` is the mainnet deploy_admin. The keys are currently locked (`/Volumes/MemoTokenKeys` is not mounted).
- **Recommendation**:
  1. On testnet, use `solana program set-upgrade-authority` to transfer the upgrade authority of the mainnet ID copies (`8iq6…`, `2sb3…` and the 5 apps) to a testnet key used only for staging, and switch `deploy-prod-staging-testnet.sh` to a separate key tree. Both state accounts are already initialized on testnet, and the admin key is needed only for one-time initialization, so this loses no staging capability.
  2. Unlock the mainnet key only on a dedicated machine or session, and run `keys.sh lock mainnet` immediately after use.
  3. For the structural fix, see OPS-05: use a multisig or timelock, or freeze the core programs with `--final` once the code is stable.
- **Owner response (2026-10-16)**: the owner accepts this risk for now. Once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable). The recommendations above remain valid but are not treated as open actions blocking the mainnet upgrade. Severity stays Low. See 8.3.

### 4.2 Info: core programs and runtime

#### MINT-03 The one-mint guarantee depends on the X1 runtime's instructions sysvar layout
- **Final severity**: Info (reported Info)
- **Program**: memo-mint
- **Location**: `programs/memo-mint/src/lib.rs:73-117, 179-186, 223-249`; `solana-instructions-sysvar-2.2.2/src/lib.rs:68-74, 99-129, 138-181`; `solana-transaction-context-2.3.13/src/lib.rs:378-391`; `solana-reserved-account-keys-2.2.2/src/lib.rs:172`; `clients/smoke-test/src/verify-deployment.rs:208-216`
- **Description**: "one mint per transaction" depends on these runtime facts:
  1. The sysvar data is built once per transaction. It contains the serialized top-level instructions plus a trailing u16 index.
  2. During execution the only writer is `TransactionContext::push`, which rewrites only the last 2 bytes on each top-level or CPI push.
  3. The account is a reserved key. It is demoted to read-only and owned by Sysvar1111; users and programs can neither supply nor modify it.
  4. Instructions and CPIs in the same transaction share state, and any CPI failure rolls back the whole transaction.
  5. Clock.slot does not change within a transaction.
  6. Write locks guarantee that transactions touching `MintState` do not interleave.

  `load_current_index_checked` also reads those last 2 bytes, so the memo position check and the fingerprint depend on the same layout. The local dependencies are 2.x crates, while X1 runs 4.0.3. INT-12 checked that the relevant tachyon 4.0.3 source is byte-identical to the agave 4.0.0 crates.
- **Failure scenario**: if a future X1 or Agave runtime re-serializes the sysvar per instruction, or adds data that changes per instruction or per CPI outside the trailing u16 (for example a wider index or the stack height), two mints in one transaction would get different fingerprints and both would succeed (Critical). Conversely, a layout change could also break `load_current_index`, making all mints fail.
- **Evidence and reproduction**:
  - **Testnet read-only simulation (4.0.3)**:
    - `[memo, mint_to]` succeeds
    - `[memo, mint_to, mint_to]` → `[2, 6008]`
    - `[memo, CU, mint_to, memo, mint_to]` → `[4, 6008]`
    - `[CU, memo, mint_to]` → `[2, 6000]`
  - **Mainnet simulation** (HEAD bytecode, mainnet Token-2022): `mint+mint` and `mint_to+mint` both give `[3, 6008]`.
  - **Reproduction** (`A-mint03.log`): after every mint, `MintState.last_tx_fingerprint` equals the fingerprint rebuilt by the client, in three cases: top-level, with ComputeBudget, and via a memo-blog CPI (stack height 2).
  - **Mutant U1**: XORs the fingerprint with `get_stack_height()`, simulating "sysvar data that changes with CPI depth". On U1, unit tests 61/61, pen-test 0 failures, smoke 7/7 and the functional checks in verify-deployment all pass, while `[blog mint memo, mint_for_blog, process_mint]` mints twice in one transaction (+2,000,000 units). This shows that none of the existing tests would catch such a runtime regression.
- **Review correction**: the original finding said the secp256k1 and ed25519 precompiles depend on this layout. That is wrong: precompiles read instruction data directly from the message, not from the sysvar account. What really depends on this layout is the large number of on-chain programs that do instruction introspection through `load_instruction_at_checked` and `load_current_index_checked`. Because so many programs depend on it, a silent layout change is unlikely, and such a change would need a feature gate and a SIMD.
- **Recommendation**:
  1. Treat it as a monitored dependency: after every X1 validator upgrade or feature activation, re-run `verify-deployment` (read-only simulation; expect `[memo, mint, mint]` to return `(2, 6008)`) and the double-mint simulation through app CPI, and track SIMDs that touch the instructions sysvar or `TransactionContext`.
  2. Optional defense in depth, so that a layout change causes "mint rejected" rather than "multiple mints": in `transaction_fingerprint`, first parse the sysvar header, walk the offset table and each instruction, and require that parsing ends exactly at `len-2`; otherwise return an error. The cost is one linear scan over about 1232 bytes, roughly a few thousand CU.

#### INT-12 No CPI tampering bypass exists on X1 4.0.3; on mainnet the runtime byte comparison is the only barrier (SIMD-0459 pointer checks are not active on mainnet)
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-mint/src/lib.rs:185-193, 223-249, 343-362`; `programs/memo-burn/src/lib.rs:311-338, 390-418`; tachyon-4.0.3 `program-runtime/src/cpi.rs:302-433, 1157-1174, 1255-1264`, `program-runtime/src/serialization.rs:543-565, 647-656`, `transaction-context/src/instruction_accounts.rs:91-111, 120-145, 181-192, 339-362`
- **Description**:
  - All core invariants assume that memo-mint and memo-burn deserialize the real account state. These invariants include: memo at index 0, one mint, the 10T cap, only the owner can burn, and statistics cannot be forged.
  - The callee's input is always serialized by the runtime from the transaction-level `BorrowedInstructionAccount` (`serialization.rs:543-565`, per call via `vm.rs:217-224`), never taken from the caller's buffer.
  - The caller's changes can enter transaction state only through `set_lamports`, `set_data_from_slice` and `set_owner`, which require the account to be writable and owned by the current program. Any other difference is rejected at CPI entry (copy mode, `cpi.rs:1255-1264`) and when the caller exits (`serialization.rs:652-655`).
  - Read-only RPC shows that `account_data_direct_mapping` (`6xynfKa…`) and `virtual_address_space_adjustments` (`Dww8cW…`) are not active on either network, so both run in copy mode. `syscall_parameter_address_restrictions` (SIMD-0459, `5Tx7ZWg…`) is active only on testnet (since slot 185455980).
  - So on mainnet, `check_account_info_pointer` (`cpi.rs:321-334, 345-356, 379-385`) is skipped: a malicious caller can point the AccountInfo key, lamports and data pointers at forged heap memory, and only the content byte comparison stops it in the end.
- **Failure scenario**: no bypass found. 35 attack and control scenarios, each run in 4 ABI modes, for 140 runs in total. All attacks (108 runs) were rejected and state was unchanged.
  - **Examples in mainnet-replica mode**:
    - A1 (forge a Memo at index 0 of the sysvar copy) → `ReadonlyDataModified`
    - B1 (set `MintState.last_slot` to 0 between two `process_mint` CPIs) → `ExternalAccountDataModified`
    - C1 (supply already at 10T; change the copy to 0) → `ExternalAccountDataModified`
    - D1 (change the victim's `token_account.owner` to the attacker) → `ExternalAccountDataModified`
  - **Testnet-replica mode**: the heap-pointer variants fail one step earlier with "Invalid account info pointer" → `ProgramFailedToComplete`.
  - **Control cases** (the result of the program's own checks when they do not rely on runtime checks): S2→6008, A0→6000, C0→6005, D1c→6008.
- **Review additions**:
  - For "in-place tampering" attacks (A1, A2, B1, C1, C2, D1–D3), SIMD-0459 has no effect even on testnet; they are also stopped by the byte comparison.
  - So the effective security boundary is the same on mainnet and testnet; only the error codes and the point of failure differ.
  - This barrier is the standard account ownership/writability model that every SVM program relies on. It is not a weakness specific to MEMO.
- **Recommendation**:
  1. Commit the malicious caller program and the LiteSVM harness as a regression suite, pinned to agave crate versions that match X1.
  2. Re-run after every X1 upgrade, and also when `syscall_parameter_address_restrictions`, `virtual_address_space_adjustments` or `account_data_direct_mapping` is activated on mainnet.
  3. Assert only "the transaction fails and state is unchanged". Do not assert a specific `InstructionError` variant, because the error codes differ between the two clusters.

#### MINT-01 The one-mint fingerprint covers only the top-level instruction list: another transaction in the same slot, immediately next, with byte-identical instructions is rejected with 6008; the wording in `MEMO-MINT-AUDIT.md:110` was too broad
- **Final severity**: Info (reported Low → Info; both reviewers agree)
- **Program**: memo-mint
- **Location**: `programs/memo-mint/src/lib.rs:98-106, 111-117, 185-186`; `MEMO-MINT-AUDIT.md:110`; `solana-instructions-sysvar-2.2.2/src/lib.rs:68-74, 99-129`; `solana-message-2.4.0/src/sanitized.rs:288-312`
- **Description**:
  - The fingerprint covers exactly the serialized top-level instruction list: program id, account keys with the message-level is_signer/is_writable flags, and instruction data.
  - It **does not include** signatures, the recent blockhash, lookup table addresses, or a fee payer that no instruction references.
  - `record_mint` compares only with "the previous record" (`lib.rs:98-106`). So another transaction is rejected only when both conditions hold: its top-level instruction list is byte-identical to the mint just recorded, and it is the **immediately next** mint in the same slot. Any other mint in between overwrites `MintState`.
  - Direct calls to `process_mint`/`process_mint_to`, and the built-in apps (blog `lib.rs:1217-1218`, chat `lib.rs:1120`, forum `lib.rs:1029`), all put the minter in the top-level instruction as a signer. So a user can only collide with themselves; a third party cannot use this to interfere with them.
  - But for third-party integrations with **no signer at all** in the top-level instruction, anyone can create a collision (for example, a permissionless crank that calls `process_mint_to` with its own PDA as the caller).
- **Failure scenario**:
  - (a) User U sends T1 = `[memo M, process_mint(U)]`. After an RPC timeout, U re-signs with a different blockhash, or with a relayer paying the fee, and sends T2. Both land in the same slot. T2 fails with `InstructionError(1, Custom 6008)`, and U still pays the fee.
  - (b) Third-party crank: Alice and Mallory each send the same instruction list. The later one fails and loses a fee; the mint it wanted has in fact already been done by the earlier transaction.
  - In no case is "at most one mint per transaction" affected.
- **Evidence and reproduction**:
  - Offline fp-demo: with a different blockhash, a different fee payer, or legacy vs v0 messages, the fingerprint is the same; with a different signer, it differs; for a crank with no signer, changing the fee payer keeps the same fingerprint.
  - Local M20.1 (slot 768) and M20.2 (slot 769): one succeeds, one returns 6008, same fingerprint. Control M20c: different users, same memo, both succeed.
  - Reproduction `A-mint01a.log`: 4 pairs of "re-signed" or "relayer-paid" transactions each gave one success and one failure; the failing side was charged 5000 lamports. In the slot right after T1 was processed, T2's preflight simulation already returned 6008.
  - Reproduction `A-crank.log`: a third-party crank with no signer was deployed; Alice's and Mallory's transactions landed in the same slot, with one success and one failure.
- **Recommendation**: the program does not need to change; it cannot see signatures, the blockhash or an unreferenced fee payer anyway.
  1. Change `MEMO-MINT-AUDIT.md:110` to the exact condition: a mint is rejected only when the top-level instruction list (including account flags) is byte-identical to the **immediately preceding** recorded mint in the same slot; for direct calls and built-in apps, different users never collide.
  2. Clients: add a nonce or timestamp to the memo (the smoke tests already do this), or use `spl_memo::build_memo(.., &[payer])` to list the fee payer as a memo signer.
  3. Third-party integrations: list the end user as a signer in the top-level instruction, or require unique memos.
- **Status**: fixed in MEMO-MINT-AUDIT.md (2026-10-16) (recommendation 1, documentation wording).

#### MINT-04 `MintState` is written back at Anchor exit, after the Token-2022 `mint_to` CPI: safe today, but the documentation's "No state changes after CPI" is inaccurate
- **Final severity**: Info (reported Info)
- **Program**: memo-mint
- **Location**: `programs/memo-mint/src/lib.rs:185-207`; `anchor-lang-0.32.1/src/accounts/account.rs:252-265, 354-359`; `MEMO-MINT-AUDIT.md:684-693, 972`
- **Description**:
  - `record_mint` only modifies the deserialized in-memory `Account<MintState>`. Anchor serializes it back in `exit()` after the handler returns, that is, after `token_2022::mint_to` (`lib.rs:196-207`). So during the CPI the on-chain `MintState` still holds the old value.
  - This cannot be exploited today:
    1. Token-2022 `process_mint_to` makes no CPI (`processor.rs:973-1058` in 8.0.1 and 9.0.0).
    2. The runtime forbids A→B→A reentrancy.
    3. `MintState` is not passed to Token-2022.
    4. Apps declare `MintState` as `Account<memo_mint::MintState>`. Because `T::owner()` ≠ the app ID, exit does not write back a stale copy (`account.rs:258`).
  - "No state changes after CPI" and "No callbacks or external program invocations" in `MEMO-MINT-AUDIT.md:690-691, 972` are literally inaccurate: the state is decided before the CPI but saved after it, and there is in fact one external call (to Token-2022).
- **Failure scenario**: hypothetical only. If a future version makes a CPI to a program that may call back into memo-mint, or passes `MintState` to a CPI, a nested mint would be compared against stale state.
- **Evidence**: local M01–M04g and M03c reject the second mint in every order.
- **Recommendation**: optional: add a comment at `lib.rs:185` saying "CPIs before exit must not receive `MintState` and must not call back into memo-mint". If a CPI target that can call back is added in the future, call `mint_state.exit(&crate::ID)?` right after `record_mint`. Also fix the documentation wording.

#### MINT-05 The 10T limit caps circulating supply, not cumulative issuance; the overflow branch is unreachable; when supply equals 10T, app operations that rely on CPI minting are unavailable
- **Final severity**: Info (reported Info)
- **Program**: memo-mint (affects apps)
- **Location**: `programs/memo-mint/src/lib.rs:192-193, 303-322`; `programs/memo-chat/src/lib.rs:574`; `programs/memo-blog/src/lib.rs:734`; `programs/memo-forum/src/lib.rs:615`
- **Description**:
  - The cap compares against the live `mint.supply`. Burns free up room, and cumulative issuance is bounded only by "10T + cumulative burns". This is intended and is already described in `MEMO-MINT-AUDIT.md:35, 714, 717, 1217`.
  - While supply < MAX, the reward never crosses the cap: in the (9T, 10T) range the reward is 1 lamport. So the `new_supply > MAX` branch (`lib.rs:317-319`) and the `checked_add` overflow branch (`lib.rs:313-314`) are unreachable and serve only as defense in depth.
  - Exact issuance simulation: going from 0 to the cap by minting alone takes about 2.86e18 mints ([0,10M] 10,000,001 mints, 10M–100M 282,896,824, 100M–1B 28,289,682,538, and about 100 times more for each following decade). At 1,000 mints per second, reaching 1B takes about 0.9 years and reaching 10B about 90 years.
  - When supply == MAX, chat `send_memo_to_group`, blog `mint_for_blog` and forum `mint_for_post` all roll back entirely with 6005 until someone burns to free up room.
- **Evidence and reproduction**:
  - Local C01/C01b (6005 at 10T), C02 (burn 1 unit), C03 (then mints exactly 1 unit; supply back to 10T), C04 (6005 again).
  - Reproduction `D-cap5.log`: at 10T, the mint operations of all three apps return `InstructionError(1, Custom 6005)` with no state change.
  - Current mainnet supply is 8,730,757 tokens; in practice the cap is far out of reach.
- **Recommendation**: the program needs no change. In the documentation, call it "maximum circulating supply" and note that app operations that rely on minting are unavailable when supply == MAX. If a cumulative issuance cap is wanted, a cumulative mint counter would need to be kept in `MintState.reserved`; that would change the economic design.

#### MINT-06 The `recipient` parameter of `process_mint_to` is only a consistency check; any token account of this mint can receive, including accounts owned by the incinerator address or the System program
- **Final severity**: Info (reported Info)
- **Program**: memo-mint
- **Location**: `programs/memo-mint/src/lib.rs:141-155, 366-390`; `spl-token-2022 processor.rs:1119-1121`
- **Description**:
  - `recipient` is only compared with `recipient_token_account.owner` (`lib.rs:388`). It is not an authorization and does not mean the recipient consents. This matches the design "anyone can mint for anyone".
  - Any initialized, unfrozen, non-native Token-2022 account of this mint can receive. Among these, token accounts whose owner field is the incinerator `1nc1nerator1111…` or the System program ID can be **burned by anyone without authorization** in Token-2022 (`processor.rs:1119-1121`), and such burns do not go through memo-burn.
  - `process_mint` is not affected: it requires `owner == user` and user must be a signer.
- **Failure scenario**: a caller mints to the incinerator's ATA; afterwards anyone can burn those tokens directly through Token-2022, with no memo and no `GlobalBurnStats` update. No security property is broken: the caller pays for it and uses up the transaction's only mint.
- **Evidence and reproduction**: reproduction `A-mint06.log`: 1,000,000 units were minted to each of the incinerator ATA and the System program ATA, then burned directly through Token-2022 by an unrelated new key; supply decreased and statistics were unchanged. Local M13/M13b confirm that a recipient mismatch returns 6004.
- **Recommendation**: no change needed. In the IDL and documentation, describe `recipient` as a consistency check against typos, not as access control.

#### INT-01 Within one transaction, "burn then mint" can push supply back to a step boundary and collect the reward of a higher tier; the gain is bounded; the 1-unit minimum burn makes the memo-burn path viable for the first time, while burning directly through Token-2022 was always possible
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-burn/src/lib.rs:52, 164-193`; `programs/memo-mint/src/lib.rs:179-193, 223-268, 275-300`
- **Description**:
  - The reward depends on the live `mint.supply` read when the memo-mint instruction starts. The same index-0 memo can satisfy both programs (INT-06).
  - After this change, memo-burn accepts any amount ≥ 1 unit. So when supply S = B + ε (B is an integer step point, ε < the step's drop δ), one can burn ε first and then mint, getting δ − ε more than a plain mint. δ per decade: 0.1 token for 10M–100M, one tenth of that for each following decade, and 1 lamport for 1T–10T, where the method is not profitable.
  - The old memo-burn minimum burn was 1 whole token, larger than δ ≤ 0.1 token, so this path through memo-burn did not pay. But burning directly through Token-2022 and then calling `process_mint` was always possible (BURN-05), so this change adds no new capability.
  - After the arbitrage, supply becomes B + f(B) ≥ B + 2δ and the window closes. It reopens only if other users' burns push supply back into (B, B+δ).
  - If supply only increases, the total extra issuance over all 54 step points is at most 999,945 units (less than 1 token).
  - The cap and the one-mint rule are not affected.
- **Failure scenario**: at supply 10,000,000.05 tokens, `[BurnMemo{burn_amount:50000}, process_burn(50000), process_mint]` mints 1.0 token instead of 0.9 token. Net gain +0.95 token, 0.05 token more than a normal mint.
- **Evidence and reproduction**:
  - Reproduction `B-int01.log`: reproduces the scenario above exactly; the log reads "Minted 1000000 units … supply before mint: 10000000000000 units".
  - The direct-burn variant (text memo + Token-2022 Burn of 1 token + `process_mint`) also skipped the 0.9 tier.
  - On the HEAD memo-burn, the 0.05-token burn is rejected with 6003, confirming that the 1-unit minimum burn is what makes the memo-burn path viable.
  - Exact Python model: 54 step points in total, δ ∈ {1, 10, 100, 1000, 10000, 100000} units, minimum f(B)/δ = 2.0, cumulative upper bound 999,945.
- **Recommendation**: accept the design and document it: the reward follows net supply, and state the per-event gain bound. Only if the project wants the curve to ignore burns does it need to index on a monotonic high-water mark or cumulative minted amount; that is an economic design change and would also affect the apps' burn thresholds.

#### INT-06 memo-mint accepts any memo ≥ 69 bytes (including memos written for memo-burn or apps), so one memo can back one mint and several burns at the same time
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-mint/src/lib.rs:223-268`; `programs/memo-burn/src/lib.rs:228-279, 311-338`
- **Description**: memo-mint only checks that index 0 is the Memo v3 program and that the data is ≥ 69 bytes. The Memo program has already executed that instruction successfully, which means the data is valid UTF-8 and all listed accounts have signed. Beyond that, the memo content has nothing to do with the mint. memo-burn requires each burn amount to equal burn_amount in the memo, so N burns of the same amount can share one memo (BURN-04). This breaks no invariant: `MintState` still limits each transaction to one mint, and every counted burn is a real burn.
- **Failure scenario**: `[Memo(Base64 BurnMemo{amount=X}), burn X, burn X, process_mint]` succeeds: it burns 2X, counts 2 burns, and mints once along the curve.
- **Evidence and reproduction**: reproduction `A-int06.log`: the combination above (X = 3 tokens) succeeds, burning 6,000,000 units, counting 2 burns, and minting 1,000,000 units once. Local B36 also passes.
- **Recommendation**: no change needed. Warn indexers in the documentation: one memo does not correspond to one action; amounts and counts should be taken from program logs or from balance and supply changes.
- **Status**: documented in README integration notes (2026-10-16).

#### INT-07 `InterfaceAccount<Mint>` and `InterfaceAccount<TokenAccount>` also accept accounts owned by legacy SPL Token, but this cannot be exploited because the mint address is fixed and Token-2022 does its own checks
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-mint/src/lib.rs:330-348, 372-390`; `programs/memo-burn/src/lib.rs:384-395`; `anchor-spl-0.32.1/src/token_interface.rs:12, 29-33, 55-59`; `anchor-lang-0.32.1/src/accounts/interface_account.rs:219-226`; `spl-token-8.0.0/src/processor.rs:113-117`
- **Description**:
  - TokenInterface's `owners()` is `[spl_token::ID, spl_token_2022::ID]`, so Anchor alone would accept accounts owned by legacy SPL Token.
  - Three things make this path unexploitable:
    1. The mint address is fixed to an account owned by Token-2022 (verified via RPC), and it cannot be reassigned or closed: there is no MintCloseAuthority and supply > 0.
    2. Legacy spl-token refuses to initialize accounts for a mint it does not own (`processor.rs:113-117`), and the MEMO mint is 455–456 bytes long, not the 82 bytes legacy requires.
    3. Token-2022 MintTo and Burn check the account owner again, and the runtime does not let Token-2022 write accounts it does not own.
- **Failure scenario**: no reachable scenario.
- **Evidence**: local M15, B29 (3008 when the legacy Token program is passed); M11/M11b/B28 (`UnauthorizedMint` when a fake mint is passed).
- **Review note**: the two reviewers cited different line numbers for the owner check in Token-2022 (around `processor.rs` lines 1037–1046, and around 1172–1174 for Burn), but agree on the semantics.
- **Recommendation**: optional defense in depth: add `owner = token_2022::ID` to the mint and token accounts, or add a `token::token_program = token_program` constraint.

#### INT-08 All mints and burns write-lock the same set of global accounts, which limits network-wide throughput and allows paid congestion; `MintState` and `GlobalBurnStats` add no new contention
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-mint/src/lib.rs:330-334, 357-362, 372-375, 399-404`; `programs/memo-burn/src/lib.rs:384-388, 412-418`
- **Description**:
  - Every mint write-locks the MEMO mint and `MintState`; every burn write-locks the mint and `GlobalBurnStats`.
  - Token-2022 MintTo/Burn already require the mint to be writable, so the lock sets of `MintState` and `GlobalBurnStats` are subsets of the mint's lock set and add no new serialization point.
  - Any transaction can list these accounts as writable and use up the account's per-block CU budget, that is, paid congestion. This only causes delay; it does not corrupt state.
  - `MintState` adds about 3.1k CU per mint, and the hashing cost is paid only by the minter.
  - The exact X1 per-writable-account block CU limit could not be verified from local source.
- **Evidence**: local M19b/M19d: memo-mint uses about 15.6k–16k CU per call.
- **Recommendation**: no code change needed. Clients should set a tight SetComputeUnitLimit (at index ≥ 1) and add a priority fee under congestion; document the shared write locks.

#### BURN-04 A BurnMemo binds each `process_burn` instruction, not the whole transaction: one memo can back any number of burns of the same amount (top-level, CPI, or different signers)
- **Final severity**: Info (reported Info)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:54-56, 175-193, 228-279, 311-337`
- **Description**:
  - `check_memo_instruction` always reads top-level instruction 0. `validate_memo_amount` only checks version == 1, burn_amount equal to this instruction's amount, and a consistent length prefix. It does not record whether the memo has been used.
  - So k `process_burn(X)` instructions in the same transaction all pass and burn k·X in total. All burns must be the same amount; different amounts cannot be added up.
  - On-chain accounting is exact: each instruction burns and records exactly its own amount.
  - A program authorized by the user's signature can burn k·X while the memo says X, but this is not a new power: the same signer privilege can already call Token-2022 Burn or Transfer directly.
- **Failure scenario**: `[Memo(BurnMemo{1, 1_000_000, p}), process_burn(1_000_000) ×3]` burns 3 tokens, statistics +3,000,000 and +3 burns, while the only memo in the transaction says 1 token. An indexer that counts by memo will undercount.
- **Evidence and reproduction**:
  - Local B23: two burns of 1.234567 tokens, statistics +2.
  - Local B23b: second burn with a different amount returns 6009 and the whole transaction rolls back.
  - Local B23c: signers A and B share one memo.
  - Local B37: one blog memo backs both an app CPI burn and a direct burn.
  - Local T02: a 1T memo backs 6T of burns in one transaction.
  - Reproduction `A-burn04.log`: three burns, two signers, and two blog CPI burns all happen as expected.
- **Review correction**: the design is stated explicitly in `MEMO-BURN-AUDIT.md:138` (Design Confirmation #1) and `:593`; INFORMATIONAL #3 (`:325-329`) is about reusing a memo across apps.
- **Recommendation**: keep the design. Tell integrators that burn_amount applies to a single instruction; indexers should sum the per-instruction "Burned N units" logs, the Token-2022 Burn inner instructions, or statistics increments, not memo amounts. Commit executable regression tests covering "one memo, three burns" and "mismatched amount rolls back".
- **Status**: documented in README integration notes (2026-10-16).

#### BURN-05 memo-burn only records burns and cannot force every burn to carry a memo: holders can burn directly through Token-2022 without a memo, and anyone can burn balances owned by the incinerator or the System program, so burn statistics are only a lower bound on supply reduction
- **Final severity**: Info (reported Info)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:88-114, 196-217`; `spl-token-2022-9.0.0/src/processor.rs:1062-1166` (`:1119-1121`); `pod.rs:97-100`; `state.rs:135-138`
- **Description**:
  - The memo requirement is part of memo-burn's accounting logic, not a rule of the token itself. Any owner or delegate can call Token-2022 Burn/BurnChecked directly.
  - For token accounts whose owner field is **exactly** the System program ID or the incinerator, Token-2022 skips the authority check. Note that this does not mean ordinary wallets: an ordinary wallet's token account owner is the wallet public key, which is protected.
  - memo-burn itself cannot reach this path, because user must sign and equal the owner (`lib.rs:382, 393`).
  - On both networks the mint has only the MetadataPointer and TokenMetadata extensions and no freeze authority; nothing on the token side can restrict burning.
  - The only reliable relationships are: burns recorded by memo-burn ≤ cumulative minted − current supply; `GlobalBurnStats` ≤ burns since initialization.
  - There is no equivalent bypass for minting: the only mint authority is the memo-mint PDA.
- **Failure scenario**: a holder burns 1,000 tokens with `spl-token burn`. Supply decreases but statistics are unchanged. A dashboard that computes circulating supply as "cumulative minted − GlobalBurnStats.total_burned" will overstate it.
- **Evidence and reproduction**: reproduction `A-burn05.log`: a direct burn of 1,000 tokens without a memo leaves statistics unchanged; 5 tokens sent to the incinerator ATA were burned by an unrelated new key, statistics unchanged.
- **Recommendation**: in the documentation, describe both statistics accounts as "burns made through memo-burn"; compute circulating supply and total burns from `mint.supply`. The program cannot and need not change: Token-2022 has no burn hook.

#### BURN-01 `GlobalBurnStats` counts only burns through memo-burn after initialization (on mainnet, since 2026-10-15), while `MEMO-BURN-AUDIT.md` described it as an all-time total
- **Final severity**: Info (reported Low; reviewers said Low / Info; Info taken)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:88-114, 148-159, 217`; `MEMO-BURN-AUDIT.md:93`; mainnet PDA `73xgyJWmEocTF5BguwZ4bJAP8irpSyFZBsooL4yoDUFa`; testnet PDA `HY5VzmHtf3braVAz7ihJWsAfp3hyPHiycfcUbA3en26j`
- **Description**:
  - `GlobalBurnStats` starts at 0 when the admin creates it (`lib.rs:151-152`); after that only `process_burn` adds to it (`lib.rs:217`). So all burns before initialization are missing.
  - Read-only checks:
    - Mainnet global account: total_burned 1,000,000 units (1 token), burn_count 1, last_burn_time 1792061814 (2026-10-15T10:56:54Z), bump 251, reserved all zero.
    - The 156 mainnet `UserGlobalBurnStats` accounts sum to 384,760,000,000 units (384,760 tokens), 213 burns.
    - Testnet global 89,363.234568 tokens / 39 burns; 34 user statistics sum to 723,252.234568 tokens / 139 burns.
  - `MEMO-BURN-AUDIT.md:93` said apps can "get the total burned amount with one account read instead of summing all user statistics". This does not currently hold on either network. The comment at `lib.rs:89`, "Counts every burn made through memo-burn", lacks the "since initialization" qualifier.
  - The program arithmetic itself is correct.
- **Failure scenario**: an explorer that follows the documentation and shows `GlobalBurnStats.total_burned` as "total MEMO burned" would show 1 token on mainnet, while 384,760 tokens have actually been burned through memo-burn, a difference of about 5 orders of magnitude.
- **Evidence and reproduction**: reproduction `B-burn01-rpc.log`. The review also found that the earliest mainnet program signature is at 2025-11-09 21:23:07 UTC (the original finding said 2025-11-10 because of the UTC+8 time zone).
- **Recommendation**:
  1. Change the documentation to "burns through memo-burn since initialization (2026-10-15 on mainnet)".
  2. If an all-time total is needed, publish an off-chain baseline at a snapshot time: sum(user stats) − global. On mainnet this is currently about 384,759 tokens and 212 burns.
  3. Do not add an admin "set total" instruction; that would make this counter untrustworthy.
- **Status**: fixed in MEMO-BURN-AUDIT.md (2026-10-16) (recommendation 1).

#### BURN-02 The "69" minimum is a client-side input rule; on chain the raw memo length is compared against 69, so 69–71-character memo-burn memos return `InvalidMemoFormat` instead of `MemoTooShort`
- **Final severity**: Info (reported Low; reviewers said Low / Info; Info taken)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:33, 236-240, 283-299, 454`; `base64-0.22.1/src/engine/general_purpose/decode_suffix.rs:89-100`
- **Owner clarification (2026-10-16)**: in the design, "69" is the minimum that the official client enforces on user input before Borsh/Base64 encoding (users must type at least 69 characters). It is not meant as the encoded length.
- **Description**:
  - On chain, both programs compare the raw memo bytes against 69. For memo-burn, those bytes are the Base64 string.
  - `general_purpose::STANDARD` requires canonical padding, so the length of any valid string is a multiple of 4. The shortest memo-burn memo that passes the 69-byte check and also decodes is therefore 72 characters (52–54 bytes after decoding, payload ≥ 39 bytes).
  - Memos of 69–71 characters pass the length check but cannot be decoded, so they return `InvalidMemoFormat` (6001) instead of `MemoTooShort`. The error message "Memo too short (minimum 69 bytes)" does not describe this case.
  - Memos produced by the official client are always far longer than 72 characters, because the user input alone is at least 69 characters before encoding. This only affects third-party integrators who build memos by hand.
  - The effective on-chain rule is stricter than the stated one, so there is no security impact. memo-mint does not decode the memo, and its 69-byte on-chain minimum is exact.
- **Failure scenario**: an integrator builds a memo by hand to the "69 bytes" rule and gets 6001 instead of a length error, which may lead them to misdiagnose the problem.
- **Evidence and reproduction**:
  - Local B19 (68 bytes → 6010), B19b (72 bytes succeeds), B18 (no padding → 6001).
  - Reproduction `A-burn02.log`: 68 bytes → 6010; 69/70/71 bytes → 6001; both 72-byte forms succeed.
- **Recommendation**: document the effective on-chain rule for integrators: a memo-burn memo must be ≥ 72 Base64 characters (payload ≥ 39 bytes), and the "69" minimum is a client-side input rule. Optional: reword the error message at the next coordinated upgrade (keep the error code), or change the memo-burn constant to 72; the latter does not change behavior.
- **Status**: documented in README integration notes (2026-10-16).

#### BURN-06 Token accounts with CPI Guard enabled, or owned by a Token-2022 multisig, can never be burned through memo-burn (directly or via an app); minting to these accounts still works
- **Final severity**: Info (reported Info)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:183-193, 343-352, 380-395`; `spl-token-2022-9.0.0/src/processor.rs:1106-1116, 1937-1973`; `extension/cpi_guard/mod.rs:40-46`; `anchor-spl-0.32.1/src/token_2022.rs:104-118`
- **Description**:
  1. **CPI Guard**: `in_cpi()` means stack height > 1. When a user calls memo-burn directly, Token-2022 already runs at stack height 2, and at 3 via an app. When `lock_cpi` is on and the authority is the owner, Token-2022 returns `CpiGuardBurnBlocked` (43). CPI Guard allows a delegate to burn, but memo-burn requires the signer to equal the owner (`lib.rs:393`), so there is no workaround. Only the owner can enable CPI Guard, so a third party cannot use it to stop others from burning.
  2. **Multisig owner**: the anchor-spl burn passes an empty multisig signer list, and memo-burn does not forward remaining accounts. Review correction: in practice it usually fails earlier — the multisig address cannot sign as `user` (3010), or a data account owned by Token-2022 cannot pay rent for the statistics PDA (3012). Only in a deliberately constructed case (the multisig keypair was kept and the statistics PDA was prefunded) does it reach `MissingRequiredSignature`.
  - memo-mint is not affected: MintTo has no CPI Guard check, and the mint authority is a PDA.
  - `MEMO-BURN-AUDIT.md:335-340` mentions only CPI Guard, does not say that direct calls are also affected, and does not mention multisig.
- **Evidence and reproduction**:
  - Local X1a: returns Custom 43 with the log "CPI Guard is enabled, and a program attempted to burn user funds without using a delegate"; state unchanged.
  - Local X1b: minting to the same account mints exactly along the curve.
  - Reproduction `A-burn06-logs.log`: chat `burn_tokens_for_group` also returns 43; a direct top-level Token-2022 burn succeeds; a 1-of-1 multisig account returns `MissingRequiredSignature` when the multisig key signs and statistics are initialized.
- **Recommendation**: front ends should map error 43 to "please turn off CPI Guard first in a separate top-level transaction" and map multisig-related errors to "please use a single-key owner". Do not relax the owner constraint. Multisig support is optional and not a security matter.
- **Status**: documented in README integration notes (2026-10-16).

#### BURN-07 When an owner sets itself as delegate, it can only burn up to the delegated amount
- **Final severity**: Info (reported Info)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:189, 393`; `spl-token-2022-9.0.0/src/processor.rs:1121-1166`
- **Description**: Token-2022 picks the authority path in the order "permanent delegate → delegate → owner". After owner U approves U itself as delegate, burns signed by U take the delegate path: they require `delegated_amount ≥ amount`, reduce the delegated amount, and clear the delegation when it reaches zero. Only U can cause this state, and funds are not at risk.
- **Evidence and reproduction**: reproduction `A-burn07.log`: U holds 100 tokens and self-delegates 1 token; `process_burn(50)` returns Custom 1 (InsufficientFunds) with state unchanged; burning 1 token succeeds and uses up the delegation; after that a 50-token burn succeeds. Also, a third-party delegate burning from U's account is rejected with 6008.
- **Recommendation**: no change needed. When a front end gets error 1 although the balance is sufficient, it should check `delegate == owner` and prompt the user to revoke the delegation.

#### INT-04 The index-0 memo rule makes durable nonce transactions unusable and excludes wallets that put ComputeBudget first; comments in 5 apps wrongly say compute budget instructions can go anywhere
- **Final severity**: Info (reported Info)
- **Program**: both
- **Location**: `programs/memo-mint/src/lib.rs:223-249` (229, 235); `programs/memo-burn/src/lib.rs:311-338` (317, 323); `solana-message-2.4.0/src/sanitized.rs:23, 339-364`; `solana-nonce-2.2.1/src/lib.rs:7`; comments at `programs/memo-chat/src/lib.rs:864`, `programs/memo-blog/src/lib.rs:991`, `programs/memo-forum/src/lib.rs:815`, `programs/memo-profile/src/lib.rs:474`, `programs/memo-project/src/lib.rs:960`
- **Description**:
  - Both core programs require top-level instruction 0 to be SPL Memo v3, while the runtime recognizes a durable nonce transaction only when "instruction 0 is System AdvanceNonceAccount" (`NONCED_TX_MARKER_IX_INDEX = 0`). The two rules are mutually exclusive, so offline signing, hardware wallet, custody and multisig flows that rely on durable nonces cannot mint or burn.
  - Multisig flows that do not rely on nonces (for example, Squads-style execute at index 1) still work.
  - The core program comments (mint `lib.rs:221-222`, burn `lib.rs:307-308`) are correct; the comments in the 5 apps are wrong. Neither audit document mentions the nonce limitation.
- **Evidence and reproduction**:
  - Local X3a: `[AdvanceNonce, memo, process_mint]` lands on chain; the mint returns 6000, the nonce advances and the fee is charged.
  - Local X3c: the burn also returns 6000.
  - Local X3b: `[memo, AdvanceNonce, process_mint]` is not recognized as a nonce transaction and returns BlockhashNotFound.
  - Local M05, B20: ComputeBudget at index 0 returns 6000.
  - Reproduction `A-int04-logs.log`: all of the above reproduced; chat returns 6010 (MemoRequired), blog returns 6005 (MemoRequired).
- **Recommendation**: state in the integration documentation that durable nonces are not supported and that compute budget instructions must be at index ≥ 1, and fix the 5 app comments. If nonce support is wanted later, it needs a separate review: accept a memo at index 1 only when index 0 is exactly AdvanceNonceAccount (System program, discriminator 4, data length 4).
- **Status**: documented in README integration notes (2026-10-16).

#### INT-10 The spl-memo entrypoint is disabled only because Cargo feature unification happens to turn on `no-entrypoint`
- **Final severity**: Info (reported Info)
- **Program**: both (build)
- **Location**: `programs/memo-mint/Cargo.toml:26`; `programs/memo-burn/Cargo.toml:26`; `spl-memo-6.0.0/src/entrypoint.rs:3`
- **Description**: both programs use only `spl_memo::ID`, but the dependency `spl-memo = "6.0"` is declared without `no-entrypoint`. `cargo tree -e features -i spl-memo` shows that this feature is turned on only by spl-token-2022 8.0.1. If a future dependency upgrade stops turning it on, spl-memo will generate a second entrypoint and global allocator, and the build will **fail outright at compile or link time**; it will not produce a silently broken program.
- **Recommendation**: declare `spl-memo = { version = "6.0", features = ["no-entrypoint"] }`, or hard-code the Memo program ID and compare it in a test. Re-check the `.so` hashes after the change.

#### INT-11 The crate-level `#![allow(deprecated)]` currently has no effect, and `#![allow(unexpected_cfgs)]` also hides feature-name typos
- **Final severity**: Info (reported Info)
- **Program**: both (build)
- **Location**: `programs/memo-mint/src/lib.rs:1-2, 12-30`; `programs/memo-burn/src/lib.rs:1-2, 12-30`
- **Description**:
  - Compiling the default, mainnet and idl-build feature combinations with `--force-warn deprecated` produces no deprecation warnings in either crate.
  - With `--force-warn unexpected_cfgs`, only the `target_os = "solana"` warning generated by `#[program]` appears.
  - But the crate-level allow also hides typos such as `feature = "mainet"`.
- **Failure scenario**: if the cfg on a pair of admin constants were misspelled the same way, the code would compile silently and the mainnet build would carry the testnet admin. In practice, the exact grep in the deployment script (`deploy-common.sh:145-195`, abort logic at 355-376) catches this; a wrong `declare_id!` fails closed with `DeclaredProgramIdMismatch`, and a wrong mint constant fails closed with `UnauthorizedMint`.
- **Evidence**: demonstrated with a temporary crate: no warning with the allow; with the allow removed, "unexpected cfg condition value: mainet" is reported; the recommended `[lints.rust]` configuration keeps the typo warning and removes the solana noise.
- **Recommendation**: remove `#![allow(deprecated)]` and replace `#![allow(unexpected_cfgs)]` with this configuration:
  ```toml
  [lints.rust]
  unexpected_cfgs = { level = "warn", check-cfg = ['cfg(target_os, values("solana"))'] }
  ```

### 4.3 Info: integration (apps and third-party developers)

#### INT-02 When a single mint or a dust-sized burn crosses a step boundary, app burn thresholds jump one tier and in-flight transactions fail; at the d = 9 step points the jump is 2x
- **Final severity**: Info (reported Info)
- **Program**: integration
- **Location**: `programs/memo-chat/src/lib.rs:53-55, 437, 606`; `programs/memo-project/src/lib.rs:57-58, 472, 577, 697`; `programs/memo-profile/src/lib.rs:41-42, 289, 356`; `programs/memo-blog/src/lib.rs:48-49, 484, 556, 637`; `programs/memo-forum/src/lib.rs:54-55, 419, 512`; `clients/src/mint_curve.rs:38-57`
- **Description**:
  - Each app computes `min_required = MINTS × mint_amount_at_supply(supply)` from the real pre-burn supply, in the first statement of the handler, before any CPI.
  - Within a decade the tiers decrease linearly, so at 90M, 900M, 9B, 90B, 900B and 9T the relative jump is 2x, and at the start of a decade it is −10% (+11.1% looking downward).
  - A single mint crosses at most one step point, and that lowers everyone's threshold. Conversely, anyone can now push supply back to a step point with a burn of ε tokens, raising all app thresholds by one tier; in-flight transactions built for the old threshold then fail with `BurnAmountTooSmall`.
  - No funds are lost: the transaction rolls back entirely, and the attacker's own burn is real and permanent.
- **Failure scenario**:
  - At supply 90,000,000.05 tokens, the threshold to create a chat group is 42,069 × 0.1 = 4,206.9 tokens. The attacker first burns 0.05 tokens; supply becomes 90M, the threshold becomes 8,413.8 tokens, and the user's transaction fails.
  - The reverse: at supply 90M − 0.1 tokens, `[memo, process_mint, create_chat_group]` can create a group at half price.
- **Evidence and reproduction**: reproduction `C-int02.log`: a pre-signed transaction returns `Custom 6020 BurnAmountTooSmall` (log "minimum: 8413800000 units") after a front-running burn; the reverse combination successfully creates a group for 4,206.9 tokens.
- **Recommendation**: the program needs no change. Clients should quote the higher tier when supply is close to a step point, or re-read supply and retry after `BurnAmountTooSmall` (the comment at `clients/src/mint_curve.rs:43-44` already says this).

#### INT-03 The reward curve logic exists in several copies: two in source (memo-mint and a client mirror) and seven in compiled artifacts (memo-mint, 5 apps, client); they may diverge after a memo-mint upgrade
- **Final severity**: Info (reported Info)
- **Program**: integration
- **Location**: `programs/memo-mint/src/lib.rs:275-300`; `programs/memo-*/Cargo.toml` (memo-mint pulled in as a path dependency, `features = ["cpi"]`); `clients/src/mint_curve.rs:6-34`
- **Description**:
  - Apps call `memo_mint::mint_amount_at_supply` through static linking, not CPI, so each app binary contains the curve as it was at build time.
  - `clients/src/mint_curve.rs` is a hand-written copy; `clients/Cargo.toml` does not depend on memo-mint.
  - Today all three agree for every u64 (proven by the harness, including all 57 intervals, the labels of all 55 curve values, and round-trip conversion).
  - They diverge only if the deployer performs an incomplete upgrade (the project has upgraded only memo-mint before). A divergence would not affect core invariants.
- **Recommendation**: make the client depend on memo-mint and re-export the function, or add a cross-check test; enforce "a curve change must redeploy the 5 apps" in the deployment script (today it is only text in `MEMO-MINT-AUDIT.md:139`).

#### INT-05 A burn memo is not bound to the calling program or the operation; a memo written for one app can be reused by a direct burn or an extra burn
- **Final severity**: Info (reported Info)
- **Program**: integration
- **Location**: `programs/memo-burn/src/lib.rs:116-129, 258-276`; `programs/memo-chat/src/lib.rs:357-380, 762-818`
- **Description**:
  - memo-burn validates only the 13-byte header and never reads the payload. So an app's memo is also accepted by a direct `process_burn`, by an extra `process_burn` alongside it, or by a third-party program that does not validate the payload.
  - Apps themselves reject memos written for other operations (for example, `ChatGroupBurnData::validate` checks category, operation, group_id and burner), so app state stays consistent.
  - Every burn is a real burn by the signer; nothing is stolen. Only attribution "by memo text" goes wrong.
  - The `BurnMemo` layout in the 5 apps matches memo-burn, so there is no parsing difference between them.
- **Evidence and reproduction**:
  - Reproduction `A-int05.log`: `[chat group burn memo, burn_tokens_for_group(X), process_burn(X)]` burns 4 tokens in total, but the group records only 2.
  - Using only "create-group memo + `process_burn(42,069)`" burned 42,069 tokens, but no group was created.
  - Local B37 is consistent with this.
- **Recommendation**: indexers should attribute burns by calling program, app account changes and inner instructions, not by memo text; third-party programs must validate their own payload before making a CPI to `process_burn`.
- **Status**: documented in README integration notes (2026-10-16).

#### INT-09 Example app memo-chat: rate limiting per group rather than per sender lets one sender take almost every window (affects the app only; the core is not affected)
- **Final severity**: Info (reported Info)
- **Program**: integration (app only)
- **Location**: `programs/memo-chat/src/lib.rs:550-557, 576-578`
- **Description**: `send_memo_to_group` compares the group-level `last_memo_time` with `min_memo_interval` (default 60 seconds, cannot be changed after the group is created). A bot that submits as soon as each window opens can win most windows and get the mint reward each time; other users' transactions in the same window fail with `MemoTooFrequent` and still pay fees. This is a race, not a full lock-out. Groups with `min_memo_interval = 0` are not affected. Each mint is still bound by the core "one mint per transaction" rule.
- **Evidence and reproduction**: reproduction `A-int09.log`: in both windows the attacker's message lands first, and the victim gets `Custom 6008 MemoTooFrequent`.
- **Recommendation**: if chat is meant to be a reference implementation, rate-limit per (group, sender), or let the group creator change the interval; at least document it in `MEMO-CHAT-AUDIT.md`.

### 4.4 Info: operations and on-chain

#### OPS-01 Mainnet memo-burn is still the HEAD whole-token version, and there is no cross-program version check: if working-tree apps are deployed without memo-burn, small-amount burns fail
- **Final severity**: Info (reported Low → Info; both reviewers agree)
- **Category**: operations
- **Location**: `scripts/deploy-common.sh:26, 409-437, 505-516, 546-618`; `programs/memo-burn/src/lib.rs:162-171` (working tree) compared with mainnet program `2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh`; `programs/memo-chat/src/lib.rs:53-55`
- **Description**:
  - Mainnet memo-burn (last deployed at slot 86109925) is byte-identical to the `--features mainnet` build of HEAD `ca4c8a0b` (sha256 `a675b8646a45966736a2ae1a066e5c4b6c694dd9b5c335ee49f4520f7d15cc22`, 292,256 bytes). The binary and IDL still contain "Must burn at least 1 token" and "whole tokens only": amount < 1,000,000 returns 6003 and non-whole amounts return 6005. The error code layout is the same in both versions.
  - The working-tree apps no longer check for whole tokens themselves. Once supply exceeds 10M the thresholds are fractional (for example 37,862.1 tokens to create a group), and users may also choose fractional amounts above the threshold.
  - `deploy-common.sh` only sorts the programs selected in the same invocation into the canonical order; it does not check the memo-burn version on the target cluster. A full deployment is safe: `ALL_PROGRAMS` is ordered mint → burn → apps, and `set -e` aborts on error.
  - The opposite direction (upgrade memo-burn first, leave old apps unchanged) is safe. Upgrading memo-mint is not required for this issue: the working-tree memo-mint is only a refactor, with unchanged on-chain behavior.
  - The upgrade requires extending memo-burn's ProgramData by 1,152 bytes. CLI 2.2.20 extends it automatically, and ExtendProgram has already succeeded on X1 mainnet before (transaction `FSbdqoAPm7hdgyNLh8DvVLjJFuoUX4VgxRvDg3o56wtVtTinFaR75DQv5VQFtfyoquzr631EeLhr6yDikpiS993`).
- **Failure scenario**: running only `./scripts/deploy-mainnet.sh memo_project` (without memo_burn); afterwards `burn_for_project(420.5)` reaches the HEAD memo-burn via CPI, returns 6005, and the whole transaction rolls back. No funds are lost.
- **Evidence and reproduction**: reproduction `E-ops01.log` (local HEAD memo-burn plus working-tree apps, supply 10,000,000.000001):
  - Creating a group (threshold 37,862.1 tokens) returns `Custom 6005 InvalidBurnAmount`.
  - A direct burn of 0.5 tokens returns 6003, 420.5 tokens returns 6005, and 420 tokens succeeds.
  - A project burn of 420.5 tokens returns 6005, and 420 tokens succeeds.
- **Recommendation**: for the mainnet upgrade, upgrade memo-burn first or in the same deployment as the apps; never deploy the apps alone. Add a check to `deploy-common.sh`: when apps are selected but memo_burn is not, compare the on-chain memo-burn ProgramData with a local build using the same feature (truncated to the ELF length), and abort on mismatch. After the upgrade, verify hashes and simulate one small-amount burn.
- **Owner note (2026-10-16)**: mainnet currently has very few users, so the upgrade is low-impact. The recommended order (memo-burn and memo-mint first, then the apps) still applies.

#### OPS-04 Deployed version matrix: testnet runs the audited working tree, mainnet runs HEAD `ca4c8a0b`; until the mainnet upgrade, this audit does not cover mainnet bytecode
- **Final severity**: Info (reported Info)
- **Category**: operations
- **Description**:
  - Builds use cargo-build-sbf 2.2.20, platform-tools v1.48 and rustc 1.84.1. The output does not depend on the build path and is reproducible.
  - Testnet memo-mint and memo-burn ProgramData = working-tree testnet build + zero padding (sha256 `caa266aeae1ed3518e357c045372ed9ac49e5dc949a69d3bf4a6cd2ea7dbd1bb` / `eecad89398ed9c2b4a17c7a8f176c15813eaefbaa685c233c58282ab4fa018ee`), upgraded at slot 195702768 / 195702899 (2026-10-15T21:33Z).
  - The prefix of mainnet memo-mint (283,016 bytes, slot 86131302) equals the HEAD `--features mainnet` build (`b62083db27480ab8…`, 270,616 bytes). Mainnet memo-burn is exactly equal to the HEAD build (`a675b864…`). Neither equals the working-tree mainnet build.
  - Compared with HEAD, the working-tree memo-mint only extracts `mint_amount_at_supply`; minting behavior is unchanged.
  - The on-chain IDLs match the respective deployed versions.
- **Review tip**: when reusing a target directory that already holds other build output, cargo may skip recompilation because of mtimes and produce stale artifacts. Hash checks must use a clean target directory, or touch the sources first.
- **Recommendation**: state this matrix clearly in the final report (see section 6). Commit the working tree before the mainnet upgrade. The expected working-tree `--features mainnet` hashes are:
  - `memo_mint.so`: `2423c17f850df4d41d32db57c7743b6fd15488851be92c52f3ef7c66079be5c7` (270,624 B)
  - `memo_burn.so`: `39d06125fca59a581b19fb9ab2925f79166b6d15dcfb4155de19b4e6faac3abf` (293,408 B)

  After the upgrade, use `verify-deployment` (with the mainnet build in target/deploy) to check bytecode and IDL, and do one small-amount burn as a smoke check.
- **Owner note (2026-10-16)**: mainnet currently has very few users, so the upgrade is low-impact. The recommended order (memo-burn and memo-mint first, then the apps) still applies.

#### OPS-05 Centralization: one System-owned hot key is at the same time the upgrade authority, admin, IDL authority and metadata authority of all memo programs; it could mint without limit and abuse users' signer privileges
- **Final severity**: Info (reported Info; by definition this is centralization and a trust assumption)
- **Category**: operations and trust
- **Location**: `programs/memo-mint/src/lib.rs:26-30, 196-207, 337-341`; `programs/memo-burn/src/lib.rs:26-30, 183-193, 380-382`; on-chain ProgramData
- **Description**:
  - On mainnet, memo-mint, memo-burn and the 5 apps are all upgradeable, and the upgrade authority of all of them is `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ` (a System-owned, on-curve ordinary key, not a multisig). It is also `AUTHORIZED_ADMIN_PUBKEY` (`lib.rs:27`), the IDL authority, the MetadataPointer authority and the TokenMetadata update authority. On testnet, `Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn` plays the same roles.
  - The mint itself has no hidden powers: the mint authority is a canonical PDA, there is no freeze authority, and the only extensions are the two metadata ones; dangerous extensions cannot be added later, and a null freeze authority cannot be set again. So under the current code, supply can only grow along the curve.
  - But whoever can replace the memo-mint code fully controls minting: they can mint at will, or move the mint authority away with SetAuthority.
  - Replacing memo-mint, memo-burn or any app also exposes users' signer privileges. Review correction: the realistic theft path has two steps: first a CPI `Approve` to a PDA that is already in the transaction and controlled by the malicious program, then a transfer with `invoke_signed`. Transferring directly to an attacker account within the same instruction would require the client to pass that account in.
  - CPI Guard blocks owner actions in such CPIs, but it also blocks memo-burn itself (BURN-06), so it is not a workable mitigation for burners.
  - The metadata URI points to a mutable GitHub branch (`main` on mainnet, `dev` on testnet).
  - The existing documents (`MEMO-MINT-AUDIT.md:1053`, `MEMO-BURN-AUDIT.md:540`) already acknowledge the upgrade-authority trust.
- **Recommendation**:
  1. Transfer the upgrade authority of memo-mint and memo-burn to a multisig with a timelock, or freeze them with `solana program set-upgrade-authority <id> --final` once stable. After freezing, `AUTHORIZED_ADMIN_PUBKEY` has no remaining power, because both state accounts are already initialized.
  2. Use different keys for the metadata authority and the app upgrade authority, and pin the metadata URI to a specific commit or immutable storage.
  3. Publish verifiable build hashes (OPS-07).
  4. Make `deploy-common.sh:273-284` recognize final states such as "immutable" or "multisig".
- **Owner response (2026-10-16)**: the owner accepts this risk for now. Once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable, matching recommendation 1). This is an accepted risk with a planned mitigation and does not block the mainnet upgrade. Severity is unchanged. See 8.3.

#### OPS-07 Builds are neither version-pinned nor verifiable, so users cannot check that deployed bytecode matches the source
- **Final severity**: Info (reported Info)
- **Location**: `Anchor.toml:1` (`[toolchain]` is empty); no rust-toolchain file in the repository; `scripts/deploy-common.sh:509, 515`
- **Description**: the repository does not pin the anchor, solana or platform-tools versions, does not use `anchor build --verifiable` or solana-verify, and does not publish hashes at deployment. The installation docs use unpinned curl install scripts.
  - Partial mitigations: Cargo.lock is committed, so crate dependencies are pinned; `verify-deployment.rs:311-321` compares on-chain bytecode with the local build (this only protects against deployment mismatch, not a compromised build machine); the `.so` contains only relative paths, so a third party who knows the toolchain versions can very likely reproduce the build.
  - This audit verified that the testnet bytecode equals the local build and the mainnet bytecode equals the HEAD build (OPS-04), but external users currently cannot do such checks themselves.
- **Recommendation**: set `anchor_version = "0.32.1"` and `solana_version = "2.2.20"` (or the X1 target version) in `Anchor.toml` and record the platform-tools version; use verifiable builds for mainnet; publish the commit, build command and sha256 of each `.so` with each release.

#### OPS-08 The token mint configuration is verified safe, but the deployment tooling never checks the freeze authority or an extension allow-list
- **Final severity**: Info (reported Info)
- **Location**: `scripts/deploy-common.sh:198-385`; `clients/smoke-test/src/verify-deployment.rs:139-160`
- **Description**:
  - Raw TLV decoding shows that the mint on both networks is: owned by Token-2022, decimals 6, initialized, freeze authority None, and only the extensions MetadataPointer(18) and TokenMetadata(19). The mint authority equals the memo-mint PDA.
  - None of this can be changed afterwards.
  - The tooling does not check any of it: `verify_configuration` does not read the mint at all, `verify-deployment` only checks the owner and mint authority, and no script calls `verify-deployment`.
  - Side note: the mainnet mint holds 104,064,640 lamports, 0.1 XNT more than the rent-exempt minimum (4,064,640). Withdrawing it would need a signature from the mint authority (the PDA), and memo-mint offers no such function, so this amount is locked permanently. The metadata update authority is the admin key, which can change the name, symbol and URI, but the impact is limited to display.
- **Recommendation**: make `verify-deployment` assert that `freeze_authority` is None, the extension set is exactly {MetadataPointer, TokenMetadata}, decimals is 6, and the metadata address points to the mint itself; call it from `deploy-common.sh` after deployment and abort on failure.

#### OPS-09 Deployment scripts detect the network by an RPC URL substring and do not check the genesis hash (`ASSUME_YES` cannot affect mainnet)
- **Final severity**: Info (reported Info)
- **Location**: `scripts/deploy-mainnet.sh:20, 31`; `scripts/deploy-testnet.sh:13, 22, 33`; `scripts/deploy-prod-staging-testnet.sh:34`; `scripts/deploy-common.sh:398-407, 448-456`
- **Description**: `AUTO_CONFIRM` is set only when the URL does not contain "mainnet"; `deploy-mainnet.sh` always asks for interactive confirmation. The network environment and feature are hard-coded in each script. The only weak point on this path is "network detection relies on a URL regex".
- **Corrected failure scenario (review)**: if a private RPC URL contains "testnet" but actually serves mainnet, `deploy-testnet.sh` does not fail because of "a different upgrade authority". Instead, since the testnet program IDs do not exist on mainnet, the script takes the first-deployment branch (`deploy-common.sh:571-578`) and uses the testnet deploy admin's balance of about 4 XNT on mainnet to deploy orphan programs and a `MintState` at the testnet IDs. These binaries hard-code the `HLCoc…` mint, and their PDA `HDz8…` is not the mainnet mint authority, so they cannot mint real MEMO. The real mainnet programs are not affected.
- **Recommendation**: first check with `solana genesis-hash -u $CLUSTER` (testnet `C7ucgdDEhxLTpXHhWSZxavSVmaNTUJWwT5iTdeaviDho`, mainnet `4SvBP3omtvcCVWdxq1zBY5cDp4wndjsThb6nEMn6iMdN`) and decide `AUTO_CONFIRM` and labels from that; refuse mainnet deployments from a working tree with uncommitted changes; optionally move the testnet deploy admin's balance off mainnet.

#### OPS-10 The admin initialization clients are compiled only after the program upgrade, so a client build failure leaves the deployment half done
- **Final severity**: Info (reported Info; checked statically only, deployment script not run)
- **Location**: `scripts/deploy-common.sh:546-618` (mint 588-597, burn 601-610)
- **Description**: `cargo run --manifest-path clients/Cargo.toml --bin admin-init-mint-state` starts compiling only after `anchor upgrade` has replaced memo-mint. A compile error or a missing toolchain causes `exit 1`, and memo_burn and the apps that follow are not upgraded.
  - Today the state accounts exist on both networks and the clients are idempotent, so there is no practical impact. On a fresh network, minting and burning fail closed until initialization succeeds; initialization is admin-only and cannot be front-run.
  - Step 1 already checks that the deploy_admin key is unlocked, so the "key is locked" trigger basically does not apply.
  - Side issue: the initialization client aborts with an error on a prefunded PDA (System-owned), while the program itself handles that case.
- **Evidence**: on a fresh ledger, `process_mint` and `process_burn` both return 3012 (reproduction `G-test09fresh.log`).
- **Recommendation**: in Step 2, before any on-chain action, run `cargo build` for both clients; in Step 3 call the compiled binaries directly.

#### OPS-11 `admin-init-mint-state` matches memo-burn's `UnauthorizedAdmin` error code (0x177c), while memo-mint's is 0x1777
- **Final severity**: Info (reported Info)
- **Location**: `clients/mint/src/admin-init-mint-state.rs:143`
- **Description**: 0x177c = 6012 is memo-burn's error code; memo-mint's `UnauthorizedAdmin` is 6007 = 0x1777 (`tests.rs:813`). The line was copied from the burn client. The review found that the client simulates first, and the non-admin case already prints logs and returns during simulation, so line 143 is never reached; the branch is in fact unreachable.
- **Recommendation**: change the constant to 0x1777 and move the message into the simulation-failure branch.

### 4.5 Info: test coverage

> **Status (all TEST findings in this section)**: addressed by committed regression tests — see section 8.2.

#### TEST-01 No test in the repository can reproduce the CPI double-mint evidence: the CPI test program cited by the audit document is not in the repository, the pen-test covers only top-level paths, and the CPI unit test is tautological
- **Final severity**: Info (reported Low → Info; both reviewers agree)
- **Program**: memo-mint
- **Location**: `programs/memo-mint/src/tests.rs:771-781`; `clients/mint/src/pen-test-memo-mint.rs:352-545`; `clients/smoke-test/src/verify-deployment.rs:203-216`; `MEMO-MINT-AUDIT.md:124`
- **Description**:
  - `MEMO-MINT-AUDIT.md:124` claims that a test program was used for tests with 2 and 30 CPIs, two-level nesting, and mixing with top-level calls, but the program is not in the repository (including git history).
  - The pen-test and `verify-deployment` build only top-level mints.
  - `test_cpi_loop_mints_once` calls `record_mint` 30 times on the same data. It only proves that `record_mint` rejects identical input; it does not prove that the runtime shows the same sysvar bytes under CPI.
  - This audit's app CPI evidence (local M04b–M04g, testnet simulation) lives in an uncommitted harness marked "audit temporary".
  - The invariant itself is proven, but nothing in the repository would catch a regression.
- **Evidence and reproduction**: under mutant U1 (fingerprint XORed with stack height), unit tests 61/61, the 8 pen-test categories with 0 failures, smoke 7/7 and the functional checks in `verify-deployment` all pass, while `MUT.cpi+top` mints twice in one transaction, balance +2,000,000 (`F-oc1-mutant.log`).
- **Recommendation**:
  1. Add app CPI cases to the pen-test (simulation mode), expecting 6008 with supply and `MintState` unchanged:
     - `[blog memo, mint_for_blog, process_mint]`
     - `[memo, process_mint_to, mint_for_blog]`
     - `[memo, mint_for_blog ×2]`
     - `[forum memo, mint_for_post, process_mint]`
     - two consecutive sends in a chat group with interval 0
  2. Commit a minimal CPI test program outside the Anchor workspace, covering "multiple CPIs in one instruction" and "nested CPI".
  3. Correct or remove the claim in `MEMO-MINT-AUDIT.md:124`.
  4. Rename `test_cpi_loop_mints_once`, or add a comment explaining what it actually tests.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-02 memo-burn has no adversarial on-chain test suite committed to the repository
- **Final severity**: Info (reported Low → Info; the review noted that the original title, "rejections were never observed on chain", is refuted by this audit's dynamic results)
- **Program**: memo-burn
- **Location**: `programs/memo-burn/src/lib.rs:164-193, 228-279, 379-419`; `clients/smoke-test/src/smoke-test-memo-burn.rs:92-266`; `clients/burn/src/test-memo-burn.rs`
- **Description**:
  - memo-mint has a 52-case pen-test; memo-burn has no equivalent suite.
  - Unit tests cover only helper functions. The smoke test covers only success paths. `test-memo-burn.rs` prints "EXPECTED FAILURE" for any error and does not check error codes.
  - This audit observed every rejection path one by one on a byte-identical binary: B01–B37, T02/T04, C02, covering 6000–6010, 2000/2006/2012 and 3002/3007/3008/3010/3012. The claim in `MEMO-BURN-AUDIT.md:113` is also confirmed by B31/B31b.
  - But this harness (`clients/examples/audit_dynamic.rs`) was not committed and is marked "delete after audit".
- **Evidence and reproduction**:
  - Mutant B1 (owner constraint at `lib.rs:393` removed): unit tests 87/87 and smoke 7/7 pass, while delegate D burns 1 token from U's account and it is recorded in D's statistics (`F-oc1-mutant.log`). On the audited version the same transaction returns 6008.
  - Mutant B2 (statistics seed changed): unit tests pass, but the smoke test catches it with 2006. The original finding said "all 7 smoke tests would pass", which does not hold for the seed change.
  - Mutant B3 (memo amount rounded to whole tokens before comparison): unit tests pass.
- **Recommendation**: turn B01–B37 into a committed `pen-test-memo-burn` binary that asserts exact error codes and that balances, supply and both statistics are unchanged, and wire it into `run-smoke-tests.sh`. Change `test-memo-burn.rs` to check specific error codes.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-03 `check_memo_instruction`, which enforces the memo position rule, has no unit tests in either program, although it can be unit-tested
- **Final severity**: Info (reported Low → Info)
- **Location**: `programs/memo-mint/src/lib.rs:223-249`; `programs/memo-burn/src/lib.rs:311-338`; `programs/memo-mint/src/tests.rs:587-630`; `solana-instructions-sysvar-2.2.2/src/lib.rs:151-155, 246-251`
- **Description**: both load functions only check that the AccountInfo key is the sysvar ID, so test accounts can be built with `construct_instructions_data`. The memo-mint tests already build a real sysvar, but only for fingerprint tests; memo-burn lacks the matching dev-dependencies. The offline harness covered the full matrix on a byte-for-byte copy of the programs, and every result was as expected (69 bytes passes; 68 bytes or an empty memo → `MemoTooShort`; no memo, ComputeBudget at index 0, legacy Memo v1, or the caller at index 0 → rejected; wrong key → error; and so on).
- **Evidence and reproduction**: all of the following mutants leave the unit tests passing (61/61, 87/87, `F-mutants-unit.log`): U4 (memo taken from current_index − 1), U5 (accepts Memo v1), U6 (index rule changed to < 2), B4, B5 (lenient Base64 engine). These paths are covered on the local chain (M05–M10b, B16–B22b), but there is no regression protection in the repository.
- **Recommendation**: add the `solana-instructions-sysvar` / `solana-instruction` dev-dependencies to memo-burn and move the harness matrix tests into both `tests.rs` files; add negative Base64 cases and a differential test against `BurnMemo::try_from_slice`.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-05 The curve "reference" used by the pen-test and smoke tests is not independent, and the smoke tests do not fail on a wrong amount
- **Final severity**: Info (reported Low → Info)
- **Location**: `clients/src/mint_curve.rs:16-33, 85-121`; `clients/mint/src/pen-test-memo-mint.rs:238-263`; `clients/smoke-test/src/smoke-test-memo-mint.rs:145-157`; `clients/smoke-test/src/smoke-test-memo-burn.rs:239-260`; `clients/smoke-test/src/smoke-test-memo-forum.rs:524-551`
- **Description**:
  - `expected_mint_amount` is copied line by line from the program, so checking the program with it means checking the program against itself.
  - The smoke test only checks that the minted amount is one of the 55 valid values. On a mismatch it only prints a warning and still prints "SMOKE TEST PASSED".
  - The burn smoke test compares balances with f64 and only warns; the statistics, however, are compared as exact integers and return Err on failure.
  - The forum and blog smoke tests do not check the amount of CPI mints.
  - The curve itself is proven independently: by the independent reference `tests.rs:23-38` in the unit tests and by the harness.
- **Evidence and reproduction**: with mutant U7 (pay 1.0 for everything below 20M), at supply 10,000,001 `smoke-test-memo-mint` mints 1,000,000 (correct value 900,000) and still prints PASSED; the forum smoke test also passes. The pen-test has 3 failures, only because the client copy was not changed in step. The unit tests catch U7 (5 failures).
- **Recommendation**: the smoke test should read supply before minting, assert `minted == oracle(supply)` and fail on a mismatch; the client reference should use an independent implementation; the burn smoke test should compare balances as integers and fail on a mismatch.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-06 The user statistics cap and saturation logic is inlined in the handler and untested; no test checks the `GlobalBurnStats` increment
- **Final severity**: Info (reported Info)
- **Location**: `programs/memo-burn/src/lib.rs:196-217`; `programs/memo-burn/src/tests.rs:789-812, 1098-1134`; `clients/smoke-test/src/smoke-test-memo-burn.rs:283-325`
- **Description**:
  - The 18T cap, `burn_count` saturation and the `last_burn_time` update are all inlined in `process_burn`. The two "saturating" tests only test the standard library, and the comment in one of them contradicts its assertion.
  - `GlobalBurnStats::record_burn` has unit tests.
  - The smoke test checks user statistics exactly, but no test checks the global increment.
  - The user cap is unreachable in practice.
- **Evidence and reproduction**:
  - Mutants B6 (`>` changed to `>=`; the review noted this is an equivalent mutant), B7 (`burn_count` not updated), B8 (`record_burn` removed) and B12 (`last_burn_time` not updated) all leave the unit tests passing.
  - With B8, the burn, chat and project smoke tests all pass on chain, while supply decreased by 84,139 tokens and global statistics stayed unchanged (`F-oc3-smoke.log`).
  - B7 and B12 would be caught by the smoke tests (not actually run).
  - Local T02/T04 verified the cap behavior of the audited version: user statistics stop at 18T, count +1, global +1.
- **Recommendation**: extract `UserGlobalBurnStats::record_burn` and unit-test it at MAX−1, MAX and `u64::MAX`; have the smoke test assert the global increment; delete or rewrite tests that only test the standard library.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-07 The small-amount burn change has no unit-level evidence; the amount upper bound was verified only in this audit's local dynamic tests
- **Final severity**: Info (reported Info)
- **Location**: `programs/memo-burn/src/lib.rs:164-171`; `programs/memo-burn/src/tests.rs:41-119, 380-409, 823-826`
- **Description**: the relevant unit tests only check constants; all positive tests use whole-token amounts, and `test_valid_memo_minimum_burn` still treats 1 token as the minimum. Review correction: "there is no evidence at all for the upper bound" no longer holds; local B03 (exactly 1T, succeeds), B04 (1T + 1 → 6004), B04b (`u64::MAX` → 6004), B01 (0 → 6003), B02 (1 unit) and B02b (1.234567) all verified it. But these cases are in the uncommitted harness.
- **Evidence and reproduction**: mutants B9 (`>` at `lib.rs:169` changed to `>=`), B10 (whole-token check restored) and B11 (minimum restored to 1 token) all leave unit tests 87/87 passing.
- **Recommendation**: extract a pure function `check_burn_amount(amount)` and unit-test it with 0, 1, 999_999, 1_234_567, MAX, MAX+1 and `u64::MAX`; add small amounts to the positive cases.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-08 Several unit tests are empty, test only third-party libraries, or are tautological, and the coverage summary overstates coverage
- **Final severity**: Info (reported Info)
- **Location**: `programs/memo-burn/src/tests.rs:650-765, 790-807, 931-948, 951-974, 976-1019, 1026-1075`; `programs/memo-mint/src/tests.rs:550-581, 771-781`
- **Description**:
  - `test_memo_size_boundaries` asserts `len >= MIN || len < MIN`, which is always true.
  - The 4 memos in `test_various_burn_scenarios` are only 36–40 bytes, so the `>= 69` branch never runs and the test makes no assertions.
  - The two "saturating" tests only test the standard library.
  - The 7 `burn_memo_structure_tests` test borsh/base64. The review noted that the 3 of them that pin `BORSH_FIXED_OVERHEAD` have regression value and should be kept.
  - The summary claims "110+/115+" tests, "covers all functions" and "all error paths"; in fact there are 60 and 86 hand-written tests. `check_memo_instruction`, the handlers, the Accounts constraints and the initialization instructions are not exercised by any unit test.
- **Recommendation**: fix or remove the empty tests (give `test_various_burn_scenarios` a payload of ≥ 39 bytes and remove the if); change the summary to accurate numbers, and list explicitly what cannot be unit-tested and which on-chain tests cover it.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-09 Admin-only initialization has no negative test that can be reproduced from the repository
- **Final severity**: Info (reported Info)
- **Location**: `programs/memo-mint/src/lib.rs:408-426`; `programs/memo-burn/src/lib.rs:358-377`; both admin-init clients
- **Description**: the `UnauthorizedAdmin` constraint is tested only through the constant value. Both clients run only as the admin. The "non-admin is rejected" claims in `MEMO-MINT-AUDIT.md:123` and `MEMO-BURN-AUDIT.md:113` cannot be reproduced from the repository. On a network where the state accounts already exist, Anchor's `init` runs before the admin constraint, so a non-admin gets "already in use".
- **Evidence and reproduction**:
  - On a fresh ledger (`G-test09fresh.log`): non-admin calls to `initialize_mint_state` and `initialize_global_burn_stats` return 6007 and 6012 respectively, and the accounts still do not exist; before initialization, `process_mint` and `process_burn` both return 3012.
  - Mutants U8 and B13, which remove the admin constraint, still pass all unit tests.
  - The practical risk is zero; the state accounts exist on both networks.
- **Recommendation**: commit a fresh-ledger script covering 3012 before initialization, 6007/6012 for non-admin, admin success, and "already in use" on repeated initialization; add a unit assertion for error code 6012 in memo-burn.
- **Status**: addressed by committed regression tests — see section 8.2.

#### TEST-10 No test pins memo-burn's error code numbers
- **Final severity**: Info (reported Info)
- **Location**: `programs/memo-burn/src/lib.rs:421-462`
- **Description**: the unused `InvalidBurnAmount` (6005) is kept exactly so that later error codes do not shift, and clients match error codes by number. memo-mint already has `test_error_codes_unchanged_and_appended` (`tests.rs:803-815`); memo-burn does not. The current numbering runs from 6000 `MemoRequired` to 6012 `UnauthorizedAdmin`; local B01/B04/B06/B19/B24/B28/B28b confirmed the numbers on chain.
- **Recommendation**: add a unit test that asserts the numbers of all 13 variants (including 6005), and note next to the enum "append only".
- **Status**: addressed by committed regression tests — see section 8.2.

### 4.6 Info: existing documentation

> The following three findings concern outdated or wrong statements in `MEMO-MINT-AUDIT.md`, `MEMO-BURN-AUDIT.md` and `MEMO-PROJECT-AUDIT.md`. This report re-derives everything from the code and **does not rely on** those statements. These documents should be corrected, or marked "superseded by this report", before public release. **Status (2026-10-16): the corrections have been made in `MEMO-MINT-AUDIT.md` and `MEMO-BURN-AUDIT.md`; see the status line of each finding.**

#### MINT-02 Statements in `MEMO-MINT-AUDIT.md` about the curve, the cap, batch minting, arithmetic and test counts are outdated or wrong
- **Final severity**: Info (reported Low → Info)
- **Location**: `MEMO-MINT-AUDIT.md:441, 458, 465, 509, 623, 852, 868, 933, 935, 968, 983, 1059, 1062, 1213`
- **Content** (verified against the working tree):
  1. Informational #1 (441, 458, 465) and 935 still describe "the last tier is 2 lamports, and minting at MAX−1 fails with `SupplyLimitReached`". In fact the last tier is 1 lamport, and minting at MAX−1 succeeds and lands exactly on MAX (observed in local C03). Lines 133 and about 330 of the same document already say 1 lamport, so the document contradicts itself.
  2. 623, 852 and 968 say "all arithmetic uses checked operations". In fact the curve uses plain operations (`lib.rs:286, 293-294, 298-299`); it is only proven not to overflow for any u64, and `overflow-checks = true` is on.
  3. 868 and 983 say "multiple mints per transaction are allowed (Revision 2)", 1213 says batch minting lowers the unit cost, and 509 lists "Batch Operations". All of these contradict the one-mint rule of Revision 3: batch mints all return 6008 (M01, M03c, PEN).
  4. 933 and 1062 say 47 tests, and 1059 says 326 lines of code. In fact there are 61 tests (60 + `test_id`), and `lib.rs` has 461 lines.
  5. Review correction: lines 89, 92 and 122 are in the Revision 2/3 change log; they are history, not wrong statements. The bound at line 353 is loosely worded but not wrong.
- **Recommendation**: rewrite Informational #1, fix the arithmetic description, batch minting and test counts, and mark the Revision 2/3 subsections "superseded by Revision 4".
- **Status**: fixed in MEMO-MINT-AUDIT.md (2026-10-16).

#### BURN-03 `MEMO-BURN-AUDIT.md` contains outdated or inaccurate statements, including an Appendix C account list that omits `global_burn_stats`
- **Final severity**: Info (reported Low → Info)
- **Location**: `MEMO-BURN-AUDIT.md:8, 46, 93, 141, 317, 348, 359-360, 377, 405, 409, 475, 478, 486, 518, 587, 611-612`
- **Content**:
  1. Appendix C (587) lists only 6 accounts and omits `global_burn_stats`, which has been required in the last position since Revision 3 (`lib.rs:412-418`). An integration that follows it fails every burn with `AccountNotEnoughKeys` (3005), i.e. it fails closed.
  2. 348 and 405 mention a "decoded byte cap" and a "Base64 memo size cap", but there is no longer a maximum length limit (`lib.rs:44-45`).
  3. 141 says the memo and the amount are "cryptographically linked". In fact it is only a per-instruction equality check, and one memo can back several burns.
  4. 317 says `burn_count` is capped at 18T. In fact only `total_burned` is capped; `burn_count` saturates at `u64::MAX` (`lib.rs:210`).
  5. 475 says 85 tests; in fact there are 87. 87 and 113 are history for the version at that time. Review correction: line 466 is not a test count.
  6. 46 says a single burn memo is about 840 bytes. With the 9 accounts of Revision 3, a legacy transaction allows at most about 812 bytes (about 772 with SetComputeUnitLimit), and v0 + lookup tables about 960.
  7. 93 calls `GlobalBurnStats` the total burned amount (BURN-01, BURN-05); 377 says "no bypass", which holds only for burns through memo-burn.
  8. 359-360 say future extensions (such as transfer fees) "remain compatible". In fact mint extensions cannot be added to an already initialized mint.
  9. 409 says "no external CPI before state updates". In fact the Token-2022 CPI runs first (`lib.rs:183`) and statistics are updated afterwards. This order is correct, but it is the opposite of what the document says.
  10. 8, 518 and 611-612 still say Revision 2; 478 says "two limits" (there is only a lower limit); 486 says "per-transaction" (it should be per-instruction).
- **Recommendation**: fix each item; at a minimum, change Appendix C to 7 accounts (user, mint, token_account, user_global_burn_stats, token_program, instructions, global_burn_stats).
- **Status**: fixed in MEMO-BURN-AUDIT.md (2026-10-16).

#### OPS-03 The manual deployment steps in the audit documents target Solana mainnet-beta, perform a fresh deployment instead of an upgrade, and omit state initialization; the status and authority descriptions are also outdated
- **Final severity**: Info (reported Low; reviewers said Low / Info; Info taken)
- **Location**: `MEMO-MINT-AUDIT.md:294-295, 1072-1102, 1213, 1226`; `MEMO-BURN-AUDIT.md:558-575, 612`; `MEMO-PROJECT-AUDIT.md:1230-1241`
- **Content**:
  1. In Anchor, `--provider.cluster mainnet` means `https://api.mainnet-beta.solana.com`, not X1.
  2. `anchor build --features mainnet` is rejected; the correct form is `-- --features mainnet`.
  3. `target/deploy` holds random keypairs (for example `CpHseydW…` for memo_mint), because `deploy-common.sh:496-534` removes the links; the real keypairs exist only in encrypted storage.
  4. `anchor deploy` performs a fresh deployment at a random address, not an upgrade.
  5. Appendix B omits the two steps `admin-init-mint-state` and `admin-init-global-burn-stats`.
  6. `MEMO-MINT-AUDIT.md:1226` says "Revision 3 deployed, Revision 4 pending testnet verification". In fact mainnet already runs HEAD (the Revision 4 curve), deployed at 2026-10-15T12:38:02Z (slot 86131302). `MEMO-BURN-AUDIT.md:612` says "Revision 2 pending", which is also outdated.
  7. `MEMO-MINT-AUDIT.md:294-295` says the mint authority "cannot be transferred back" and the mint logic is "permanently constrained by program rules". Neither holds while the program is upgradeable, as line 1053 of the same document admits.

  Following the wrong steps only leads to a failed build or an orphan program deployment; `DeclaredProgramIdMismatch` makes it fail closed, and no security mechanism is bypassed.
- **Recommendation**: replace the manual steps with references to `scripts/deploy-mainnet.sh` and `PROD-DEPLOYMENT.md`, or with explicit X1 commands (`anchor build -p memo-mint -- --features mainnet`, `anchor upgrade … --program-id 8iq6… --provider.cluster https://rpc.mainnet.x1.xyz`), and add the initialization steps; update the status lines from on-chain facts; reword lines 294-295 so they are conditional on the upgrade authority.
- **Status**: fixed in MEMO-MINT-AUDIT.md / MEMO-BURN-AUDIT.md (2026-10-16).

### 4.7 Findings judged invalid or non-issues

| ID | Original claim | Review conclusion and reasoning |
|---|---|---|
| **BURN-08** (reported Info) | Burning requires the owner's signature and the statistics PDA requires the owner to pay rent, so "a vault owned by a data-holding PDA can never burn" | **Invalid** (both reviewers agree). Anchor 0.32.1 `init` (`anchor-syn constraints.rs:1638-1676`) computes only the difference when the target address already has a balance, transfers from the payer only if the difference is > 0, and then allocates and assigns with the program seeds. As long as the rent-exempt amount for 65 bytes is first sent to the statistics PDA address (any wallet can do this in the same transaction), a data-holding PDA can complete initialization and burning through `invoke_signed`. Local B35: after prefunding 0.01 SOL, initialization succeeds and the payer is not charged. The original points "only the owner can burn" and "there is no close instruction, so the rent is locked permanently (about 0.0013 XNT)" are true, but they are design notes and have been merged into the integration recommendations. |
| **TEST-04** (reported Low) | No test reaches the `token_account.mint == mint` constraint | **Invalid** (both reviewers agree). Local M12b (real mint + ATA of a fake mint) returns 6002 `InvalidTokenAccount`, and B28b returns 6006; in both cases the constraint branch itself is executed. Remaining gap: the `recipient_token_account.mint` constraint of `process_mint_to` (`memo-mint/src/lib.rs:386-387`) is not reached by any test (M11b fails earlier at the mint constraint with 6003); adding a case is recommended. A legacy SPL Token account variant cannot be built: legacy InitializeAccount requires the mint to be owned by the legacy program. Token-2022's own MintMismatch also acts as a backstop. |
| **TEST-11** (reported Info) | Tests ran on three different Token-2022 binaries; testnet's is older, and the burn path was never tested on mainnet's Token-2022 | **Invalid** (the reviewers disagreed: one judged it invalid, the other an Info-level coverage note; final decision: invalid). The Token-2022 ELF extracted from the local validator genesis has sha256 `d32c6364ed5d2a0ad68a8f4b04baff9b24d2b7e60df1bcdd1894071d07d69de6`, byte-identical to the X1 mainnet dump, so every local mint and burn case (including small-amount burns and CPI Guard) ran on the mainnet Token-2022 build. Testnet's Token-2022 is older (541,936 bytes, `6840c7dd…`), but the MEMO mint has only metadata extensions, so none of the newer extension-related checks apply. The only combination not run end to end is "burn on the mainnet runtime + mainnet Token-2022", and each of those two parts is covered separately. A read-only burn simulation after the upgrade is recommended. |
| **OPS-06** (reported Info) | X1's Token-2022 is upgraded by "a single externally held key" | **Invalid as originally stated** (the reviewers disagreed). One reviewer inferred a single key only from "owned by System". The other checked further: `2sXr5THr5ZAwSfuJAc1KogPKZ2uKkhfNUAtEvNw5Q6Z7` is not on the ed25519 curve and can be derived by the Squads v4 program `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf` with seeds `["multisig", 89PksQjF…, "vault", 0]`; recent transactions are `VaultTransactionExecute`; the multisig account decodes to threshold 3, 6 members, time_lock 0; the same vault also controls the Squads program on X1. This report adopts the latter view. The testnet upgrade authority `FwJkGYPd…` is indeed a single key. **The underlying platform trust assumption still holds**: Token-2022 is upgradeable, and MEMO's cap, balances and authorities depend on it. This is recorded in 1.6 and section 6. |

---

## 5. Test results summary

### 5.1 Unit tests

| Program | Result | Notes |
|---|---|---|
| memo-mint | 61/61 pass (for both the testnet and the `--features mainnet` build) | 60 hand-written + Anchor `test_id`. Curve tests use the independent reference `expected_reward` (`tests.rs:23-38`); one-mint tests use the runtime serializer (`tests.rs:658-769`); `test_cpi_loop_mints_once` is tautological (TEST-01) |
| memo-burn | 87/87 pass (for both builds) | 86 hand-written + `test_id`. Covers `validate_memo_amount` (35), `validate_memo_length` (20) and `GlobalBurnStats::record_burn` (6). Several tests are empty or test only third-party libraries (TEST-08) |

Unit tests do **not** exercise: `check_memo_instruction`, `process_mint` / `process_mint_to` / `process_burn`, the Accounts constraints, or the two initialization instructions (TEST-03, TEST-08).

### 5.2 X1 testnet live tests (all 7 programs upgraded to this working tree)

| Item | Result |
|---|---|
| Smoke tests (mint, burn, profile, blog, forum) | Pass |
| memo-burn burns of 0.5, 0.000001 and 1.234567 tokens | Succeed |
| memo-burn burn of 0 | `BurnAmountTooSmall` (6003) |
| project burns of 420 / 420.5 / 419.999999 tokens | Succeed / succeed / rejected |
| chat group burns of 1 / 0.999999 tokens | Succeed / rejected |
| Read-only simulation `[memo, mint_to]` | Succeeds (mints 1,000,000) |
| Read-only simulation `[memo, mint_to, mint_to]`; `[memo, mint, mint]`; `[memo, mint, mint_to]` | `InstructionError [2, Custom 6008]` |
| Read-only simulation `[memo, CU, mint_to, memo, mint_to]` | `[4, Custom 6008]` |
| Read-only simulation `[CU, memo, mint_to]` | `[2, Custom 6000]` |
| Read-only simulation of app CPI: `[blog memo, mint_for_blog]` | One CPI mint succeeds |
| Read-only simulation `[blog memo, mint_for_blog, process_mint]`, `[blog memo, process_mint, mint_for_blog]`, `[blog memo, mint_for_blog, mint_for_blog]` | IX2 `Custom 6008` |
| Mainnet read-only simulation (HEAD bytecode, mainnet Token-2022): `memo + createATA + mint` | Succeeds |
| Mainnet read-only simulation: one more mint or mint_to | `[3, Custom 6008]` |

### 5.3 Local validator threshold matrix (chat group creation threshold = 42,069 × current per-mint reward)

| Initial supply (tokens) | Per-mint reward (tokens) | Group creation threshold (tokens) | Threshold − 1 unit | Threshold |
|---|---|---|---|---|
| 10,000,000 | 1.0 | 42,069 | Rejected | Pass |
| 10,000,000.000001 | 0.9 | 37,862.1 | Rejected | Pass |
| 20,000,000.000001 | 0.8 | 33,655.2 | Rejected | Pass |
| 100,000,000.000001 | 0.09 | 3,786.21 | Rejected | Pass |
| 9,999,999,999,999.999999 | 0.000001 | 0.042069 | Rejected | Pass |

Also:
- Observed mint rewards of 1.0 / 0.9 / 0.1 / 0.000001 match the curve.
- At supply exactly 10T, `process_mint` returns 6005.
- At supply 10T − 1 unit, the first mint mints 1 unit and the second is rejected.
- All 7 local smoke tests pass.

### 5.4 Local validator dynamic attack cases (131 cases, all pass)

Environment: see 2.4. "No change" means token balances, supply, `MintState`, and user and global statistics are all unchanged. All rejection cases are real on-chain transactions that paid fees.

**Environment checks**

| Case | Description | Expected | Actual | Pass |
|---|---|---|---|---|
| ENV-01 | Binary under test = working tree = testnet deployment | Byte-identical | `memo_mint.so` `caa266ae…dbd1bb` and `memo_burn.so` `eecad893…a018ee` match in all four places: target/deploy, independent rebuild, testnet dump, local dump | ✓ |
| ENV-02 | Seed data for blog / forum smoke tests | Pass | Both exit 0 | ✓ |

**memo-mint (M series, supply 5T)**

| Case | Description | Expected | Actual | Pass |
|---|---|---|---|---|
| M01 | Two `process_mint` in one transaction | 6008, no change | ix 2 6008, no change | ✓ |
| M02 | `process_mint` + `process_mint_to(self)` | 6008 | ix 2 6008 | ✓ |
| M03 | Two `process_mint_to`, recipients A and B | 6008 | ix 2 6008 | ✓ |
| M03b | A calls `process_mint_to(B)`, B calls `process_mint` (two signers) | 6008 | ix 2 6008 | ✓ |
| M03c | 30 `process_mint` in one transaction | 6008 | 6008 already at the 2nd | ✓ |
| M04a | Control: blog `mint_for_blog` alone | Succeeds, mints exactly along the curve | Mints 6 = spec value; `MintState` matches the fingerprint | ✓ |
| M04b | blog CPI mint + top-level `process_mint` | 6008 | ix 2 6008 | ✓ |
| M04c | Top-level `process_mint` + blog CPI mint | 6008 | ix 2 6008 | ✓ |
| M04d | Two blog `mint_for_blog` (two CPIs) | 6008 | ix 2 6008 (propagated through blog) | ✓ |
| M04e | Control: forum `mint_for_post` | Succeeds | Mints 6 = spec value | ✓ |
| M04f | forum CPI mint + top-level `process_mint` | 6008 | ix 2 6008 | ✓ |
| M04g | Top-level `process_mint_to(B)` + forum CPI mint | 6008 | ix 2 6008 | ✓ |
| M05 | ComputeBudget at index 0, memo at index 1 | 6000 | ix 2 6000 | ✓ |
| M06 | 68-byte memo | 6001 `MemoTooShort` | ix 1 6001 | ✓ |
| M07 | 69-byte memo | Succeeds | Mints 6; 41,639 CU total (memo 26,037 + mint 15,602) | ✓ |
| M08 | No memo | 6000 | ix 0 6000 | ✓ |
| M09 | System transfer at index 0 | 6000 | ix 2 6000 | ✓ |
| M09b | Legacy Memo v1 at index 0 | 6000 | ix 1 6000 | ✓ |
| M10 | `process_mint` at index 0 | 6000 | ix 0 6000 | ✓ |
| M10b | Empty memo | 6001 | ix 1 6001 | ✓ |
| M11 | New fake mint whose mint authority is the memo-mint PDA, via `process_mint` | 6003 `UnauthorizedMint` | ix 1 6003 | ✓ |
| M11b | Same, via `process_mint_to` | 6003 | ix 1 6003 | ✓ |
| M11c | Supply of the fake mint | Stays 0 | 0 | ✓ |
| M12 | A signs, mints into B's account | 6004 | ix 1 6004 | ✓ |
| M12b | Real mint + A's account of a fake mint | 6002 `InvalidTokenAccount` | ix 1 6002 | ✓ |
| M13 | `process_mint_to` with recipient = B, account belongs to A | 6004 | ix 1 6004 | ✓ |
| M13b | recipient = A, account belongs to B | 6004 | ix 1 6004 | ✓ |
| M14 | Random account posing as the instructions sysvar | 2012 `ConstraintAddress` | ix 1 2012 | ✓ |
| M14b | Clock sysvar posing as it | 2012 | ix 1 2012 | ✓ |
| M15 | Legacy SPL Token program | 3008 | ix 1 3008 | ✓ |
| M16 | Random non-existent account as `mint_state` | 3012 | ix 1 3012 | ✓ |
| M16b | Fake `MintState` owned by memo-mint, 113 zero bytes | Rejected | 3002 `AccountDiscriminatorMismatch` | ✓ |
| M16c | memo-burn's `GlobalBurnStats` as `mint_state` | 3007 | ix 1 3007 | ✓ |
| M16d | `mint_state` read-only | 2000 | ix 1 2000 | ✓ |
| M16e | user did not sign | 3010 | ix 1 3010 | ✓ |
| M16f | Another program's PDA with the same seeds as mint authority | 2006 | ix 1 2006 | ✓ |
| M17a | Non-admin calls `initialize_mint_state` (account already exists) | Rejected | System "already in use" (Custom 0), whole transaction rolled back | ✓ |
| M18a | Consecutive mint #1 | Along the curve | 6 = spec value | ✓ |
| M18b | Consecutive mint #2 (crosses 5T, tier drops) | Along the curve | 5 = spec value | ✓ |
| M18c | Consecutive mint #3 | Along the curve | 5 = spec value | ✓ |
| M19 | A calls `process_mint_to` for B | B increases, A unchanged | B +5, A unchanged | ✓ |
| M19b | ComputeBudget inserted between memo and mint, with 20 extra accounts | Succeeds | 5 = spec value, 47,480 CU | ✓ |
| M19c | `u64::MAX` appended to the instruction data | Ignored | 5 = spec value | ✓ |
| M19d | 780-byte memo | Succeeds | 5 = spec value; 291,299 CU total (SPL Memo 275,170) | ✓ |

**memo-burn (B series, supply 5T)**

| Case | Description | Expected | Actual | Pass |
|---|---|---|---|---|
| B01 | Burn 0 | 6003 | ix 1 6003 | ✓ |
| B02 | Burn 1 unit | Each value exactly −1 / +1 | Exact | ✓ |
| B02b | Burn 1.234567 tokens | Exact | Exactly −1,234,567 | ✓ |
| B03 | Exactly 1T | Succeeds | Exact, 54,787 CU | ✓ |
| B04 | 1T + 1 unit | 6004 | ix 1 6004 | ✓ |
| B04b | `u64::MAX` | 6004 | ix 1 6004 | ✓ |
| B05 | Balance + 1 unit | Token-2022 InsufficientFunds | Custom 1 | ✓ |
| B06 | Memo amount = amount + 1 | 6009 | ix 1 6009 | ✓ |
| B07 | Memo amount = amount − 1 | 6009 | ix 1 6009 | ✓ |
| B08 | version 0 | 6002 | ix 1 6002 | ✓ |
| B09 | version 2 | 6002 | ix 1 6002 | ✓ |
| B10 | Length prefix = actual length + 1 | 6001 | ix 1 6001 | ✓ |
| B11 | Length prefix = actual length − 1 | 6001 | ix 1 6001 | ✓ |
| B12 | Length prefix = `u32::MAX` | 6001, no memory allocated | ix 1 6001 | ✓ |
| B12b | Length prefix 0, but 60-byte payload | 6001 | ix 1 6001 | ✓ |
| B13 | `AAAA` appended after a valid memo | 6001 | ix 1 6001 | ✓ |
| B14 | `!!` appended | 6001 | ix 1 6001 | ✓ |
| B15 | Plain-text memo | 6001 | ix 1 6001 | ✓ |
| B16 | URL-safe alphabet (contains `-`) | 6001 | ix 1 6001 | ✓ |
| B16b | Control: same bytes in the standard alphabet (contains `+`) | Succeeds | Exact | ✓ |
| B17 | Newline in the middle | 6001 | ix 1 6001 | ✓ |
| B17b | Trailing newline | 6001 | ix 1 6001 | ✓ |
| B17c | Leading space | 6001 | ix 1 6001 | ✓ |
| B18 | Missing `=` padding | 6001 | ix 1 6001 | ✓ |
| B18b | Non-zero trailing bits (non-canonical encoding) | 6001 | ix 1 6001 | ✓ |
| B19 | Valid but only 68 bytes | 6010 | ix 1 6010 | ✓ |
| B19b | 72 bytes (shortest canonical length ≥ 69) | Succeeds | Exact, 44,446 CU | ✓ |
| B19c | Non-UTF-8 memo | Rejected by the Memo program | ix 0 InvalidInstructionData | ✓ |
| B20 | ComputeBudget at index 0 | 6000 | ix 2 6000 | ✓ |
| B21 | `process_burn` at index 0 | 6000 | ix 0 6000 | ✓ |
| B22 | No memo | 6000 | ix 0 6000 | ✓ |
| B22b | Legacy Memo v1 at index 0 (content is a valid burn memo) | 6000 | ix 1 6000 | ✓ |
| B23 | One memo backs two `process_burn(X)` (by design) | −2X, count +2 | Exactly −2,469,134, count +2 | ✓ |
| B23b | `burn(X)` + `burn(X+1)` sharing X's memo | Second returns 6009, whole transaction rolled back | ix 2 6009, no change | ✓ |
| B23c | A and B each burn X, sharing one memo | Each −X, global +2X | Exact (the first run in log 11 was marked FAIL because the harness wrongly expected a supply change of X; the on-chain result was the same, and the corrected re-run in log 12 passed) | ✓ |
| B24 | A signs, burns B's account | 6008 | ix 1 6008 | ✓ |
| B25 | B approves A as delegate; A burns via memo-burn | 6008 | ix 1 6008 | ✓ |
| B25b | Same, but passing B's statistics PDA | Rejected | ix 1 6008 | ✓ |
| B26 | A burns own tokens but passes B's statistics PDA | 2006 | ix 1 2006 | ✓ |
| B27 | User C without initialized statistics | 3012 | ix 1 3012 | ✓ |
| B28 | Fake mint G (pad statistics by burning worthless tokens) | 6007 | ix 1 6007 | ✓ |
| B28b | Real mint + token account of G | 6006 | ix 1 6006 | ✓ |
| B29 | Legacy SPL Token program | 3008 | ix 1 3008 | ✓ |
| B30 | Random sysvar | 2012 | ix 1 2012 | ✓ |
| B31 | B's `UserGlobalBurnStats` posing as global statistics | Rejected | 3002 | ✓ |
| B31b | memo-mint's `MintState` posing as global statistics | 3007 | ix 1 3007 | ✓ |
| B31c | Global statistics read-only | 2000 | ix 1 2000 | ✓ |
| B32 | user did not sign | 3010 | ix 1 3010 | ✓ |
| B32b | Token account read-only | 2000 | ix 1 2000 | ✓ |
| B33 | Re-initialize user statistics | already in use | Custom 0 | ✓ |
| B34 | Non-admin initializes global statistics (already exists) | Rejected | Custom 0 | ✓ |
| B35 | After prefunding D's statistics PDA (0.01 SOL), D initializes | Succeeds | Succeeds, 10,906 CU, owner is memo-burn | ✓ |
| B36 | One burn memo backs both `process_mint` and `process_burn(X)` | Balance change = reward − X | −1,234,560 (+7 − 1,234,567) | ✓ |
| B37 | One blog `burn_for_blog` memo backs both a blog CPI burn and a direct burn | Both succeed; blog records only +X | A −2X, `blog.burned_amount` +X, user statistics +2X / +2 | ✓ |

**Same-slot false positive, curve steps, cap, saturation, special cases**

| Case | Description | Expected | Actual | Pass |
|---|---|---|---|---|
| M20.1 | Two different transactions (fee payers A and B, user A in both), byte-identical instruction lists, landing in the same slot | At most one mint; the later one 6008 | slot 768: one mints 7, the other 6008; same fingerprint | ✓ |
| M20.2 | Repeat M20.1 | Same | slot 769: same | ✓ |
| M20c | Control: users A and C use the same memo text in the same slot | Both succeed | Both succeed, different fingerprints | ✓ |
| S-10M | Supply 10M, 3 consecutive mints | 1.0, 0.9, 0.9 | 1,000,000 / 900,000 / 900,000 | ✓ |
| S-20M | Supply 20M (step point), 3 consecutive | 0.9, 0.8, 0.8 | 900,000 / 800,000 / 800,000 | ✓ |
| S-100M | Supply 100M | 0.1, 0.09, 0.09 | 100,000 / 90,000 / 90,000 | ✓ |
| S-1B | Supply 1B | 0.01, 0.009, 0.009 | 10,000 / 9,000 / 9,000 | ✓ |
| S-9T | Supply 9T | 2, 1, 1 units | 2 / 1 / 1 | ✓ |
| C01 | `process_mint` at supply exactly 10T | 6005 | ix 1 6005 | ✓ |
| C01b | `process_mint_to` at 10T | 6005 | ix 1 6005 | ✓ |
| C02 | Burn 1 unit at the cap | Succeeds | Exact | ✓ |
| C03 | Mint at supply 10T − 1 | Exactly 1 unit, back to 10T | 1 unit; supply = 10,000,000,000,000,000,000 | ✓ |
| C04 | Mint again | 6005 | ix 1 6005 | ✓ |
| T01 | Supply 18,446,744,073,709 tokens (> 10T, defensive test) | 6005 | ix 1 6005 | ✓ |
| T02 | 3 transactions, each with 6 burns of 1T sharing one 1T memo | −6T per transaction, count +6 | User statistics 0 → 6e18 → 12e18 → 18e18 | ✓ |
| T04 | Burn 1 more unit when user statistics are already at 18T | Statistics stay at 18T, count +1, global +1 | Exact, with cap log | ✓ |
| X1a | `process_burn` on an account with CPI Guard enabled | Rejected by Token-2022 | Custom 43, no change | ✓ |
| X1b | Mint to an account with CPI Guard enabled | Succeeds | 6 = spec value | ✓ |
| X2a | v0 transaction + address lookup table, single mint | Succeeds | 5 = spec value | ✓ |
| X2b | v0 + lookup table, two mints | 6008 | ix 2 6008 | ✓ |
| X3a | `[AdvanceNonce, memo, process_mint]` | 6000 | ix 2 6000; nonce advanced, fee charged | ✓ |
| X3b | `[memo, AdvanceNonce, process_mint]` (simulation) | BlockhashNotFound | BlockhashNotFound | ✓ |
| X3c | Durable nonce burn transaction | 6000 | ix 2 6000 | ✓ |

**pen-test (`clients/mint/src/pen-test-memo-mint.rs`, simulation mode, 8 categories, 52/52 pass in total, 0 warnings)**

| Category | Cases | Result |
|---|---|---|
| memo-attacks | 12 | All pass: missing, empty, 68 bytes, Memo v1, non-memo instruction at index 0, index 0 too short while index 1 valid → rejected; 69 bytes, 780 bytes, control characters or Unicode, two memos → accepted and exactly along the curve; transaction too large, invalid UTF-8 → rejected |
| instruction-attacks | 11 | All pass: mint at 0, CB at 0 → `MemoRequired`; CB in the middle or at the end, unrelated instructions → accepted; 3 mints, 31 mints, mint + mint_to, two memos two mints, alternating, spaced-out mints → `OneMintPerTransaction` |
| mint-state-attacks | 4 | All pass: missing → 3005; random → 3012; mint account → 3007; read-only → 2000 |
| pda-attacks | 4 | All pass → `ConstraintSeeds` |
| mint-authority-attacks | 6 | All pass |
| account-attacks | 7 | All pass: legacy Token → `InvalidProgramId`; fake sysvar → `ConstraintAddress`; extra accounts or data ignored; unknown discriminator → `InstructionFallbackNotFound` |
| dos-attacks | 2 | All pass: insufficient CU → exceeded; 32 mints → 6008 |
| mint-to-attacks | 6 | All pass: recipient mismatch → 6004; missing argument → `InstructionDidNotDeserialize`; no memo → 6000; two recipients → 6008 |

**CU reference**: memo-mint about 15.6k–16k CU per call, memo-burn about 17.5k–18k CU. The SPL Memo program uses about 350 CU per byte (26,037 for 69 bytes, 36,890 for 100 bytes, 275,170 for 780 bytes).

### 5.5 CPI tampering harness (INT-12, LiteSVM, runtime crates byte-identical to the tachyon 4.0.3 source)

35 scenarios × 4 modes (M = X1 mainnet replica, T = X1 testnet replica, V = T + VASA, D = V + direct mapping) = 140 runs, 108 attack runs, **0 successes**.

| Scenario | Content | M (mainnet) | T / V / D |
|---|---|---|---|
| A0 (control) | No memo | 6000 | 6000 |
| A1, A1i, A2, E1u | Tamper with the sysvar copy, forge a Memo at index 0, modify the current index | `ReadonlyDataModified` | `ReadonlyDataModified` |
| A1h, A2h | Point the sysvar pointer at forged data on the heap | `ReadonlyDataModified` | "Invalid account info pointer" → `ProgramFailedToComplete` |
| S2 (control) | Malicious caller makes several `process_mint` CPIs without tampering | 6008 | 6008 |
| B1, B1h, B2, B3 | Reset or forge `MintState` between two CPIs | `ExternalAccountDataModified` | B1 same as M; heap variants `ProgramFailedToComplete` |
| B4 | Caller returns Ok with a stale buffer | `ExternalAccountDataModified` at exit | Fails earlier |
| C0 (control) | Supply 10T | 6005 | 6005 |
| C1, C2, C1h | Forge the supply copy | `ExternalAccountDataModified` | C1/C2 same as M; C1h `ProgramFailedToComplete` |
| D1c (control) | Use another user's token account | 6008 | 6008 |
| D1, D1h, D2, D3 | Tamper with the victim token account's owner, the user statistics, the global statistics | `ExternalAccountDataModified` | Same as M, or `ProgramFailedToComplete` |
| D4, E1–E7, E1t | Escalate the victim's signer privilege, make a sysvar or `MintState` writable/signer, forge a mint authority signature | `PrivilegeEscalation` | `PrivilegeEscalation` |

### 5.6 Offline harnesses and fuzzing

| Item | Scale | Result |
|---|---|---|
| Curve and cap (source extracted verbatim vs two independent references, overflow-checks on) | 41,809,355 points (all endpoints of the 57 intervals + 40 million random/boundary points, including 669,355 exact Python vectors) | 0 failures; the maximum of s + amount is exactly 1e19 |
| Curve fuzz (tests-coverage, decimal-digit reference) | 4,000,861 points | 0 mismatches |
| Closed-form simulation of issuance by minting alone | 2,857,543,690,884,166,656 mints to the cap | Stops exactly at MAX; never crosses it from any supply < MAX |
| Client mirror vs program | All intervals + exhaustive [0, 2.1M] + 2M round-trip conversions | Identical |
| Base64 strictness and unique reversibility | 3,000,000 strings (855,444 accepted) | Every accepted string re-encodes to the original |
| memo-burn header parser vs `BurnMemo::try_from_slice` | 2,000,000 cases (1,709,274 ×6001, 145,531 ×6002, 63,077 ×6009, 82,118 Ok) | 0 divergences |
| Same (tests-coverage) | 300,000 cases (31,694 accepted) | 0 divergences |
| `check_memo_instruction` matrix (real sysvar AccountInfo, byte-for-byte copies of both programs) | mint 11 cases, burn several cases | All as expected |
| fp-demo (real decompile and serialization pipeline) | A1/A2/A4/B1/C1/A5 | Different blockhash, different fee payer, legacy vs v0, crank with no signer: same fingerprint; different signer: different fingerprint |

### 5.7 Reproduction results

| Finding | Reproduced | Summary |
|---|---|---|
| MINT-01 | Yes | 4 pairs of same-slot transactions each gave one success and one failure (6008, 5000 lamports charged); in the slot right after the first was processed, preflight already returned 6008; a third-party crank with no signer also gave one success and one failure |
| MINT-03 | Yes (consequence simulated with a mutant) | Under the current layout the fingerprint equals the value rebuilt by the client; mutant U1 mints twice in one transaction while all checks pass |
| MINT-05 | Yes | At 10T the mint operations of all three apps return 6005; burn 1 unit → mint 1 unit → 6005 again |
| MINT-06 | Yes | After minting to the incinerator and System program ATAs, any key could burn directly; statistics unchanged |
| BURN-01 | Yes (read-only RPC) | Mainnet global statistics 1 token vs user statistics total 384,760 tokens; testnet 89,363 tokens vs 723,252 tokens |
| BURN-02 | Yes | 68 bytes → 6010; 69/70/71 bytes → 6001; 72 bytes succeeds |
| BURN-04 | Yes | One memo backs 3 burns, two signers, two blog CPIs |
| BURN-05 | Yes | Direct burn of 1,000 tokens, and the incinerator ATA burned by anyone; statistics unchanged in both cases |
| BURN-06 | Yes | CPI Guard → 43 (both for direct calls and via chat); minting not affected; 1-of-1 multisig → `MissingRequiredSignature` |
| BURN-07 | Yes | With a self-delegation of 1 token, burning 50 tokens → Custom 1; after burning 1 token the delegation is used up, and then 50 tokens can be burned |
| INT-01 | Yes | At 10,000,000.05, burn-then-mint gives 1.0 token, net +0.95; the HEAD memo-burn rejects the 0.05-token burn |
| INT-02 | Yes | After a front-running burn, the pre-signed group creation transaction returns 6020; the reverse combination creates a group at half price |
| INT-04 | Yes | X3a/X3b/X3c and ComputeBudget-at-0 behave as described in both the core programs and the apps |
| INT-05 | Yes | The group records only 2 tokens while 4 were actually burned; a "create group memo" burned 42,069 tokens without any group being created |
| INT-06 | Yes | One memo: 2 burns + 1 mint |
| INT-09 | Yes | In both windows the attacker's message lands first and the victim gets 6008 `MemoTooFrequent` |
| OPS-01 | Yes | HEAD memo-burn + working-tree apps: group creation at the 37,862.1-token threshold returns 6005; 420.5 → 6005; 0.5 → 6003 |
| OPS-08 | Yes (read-only) | Freeze authority not set, metadata extensions only; no corresponding checks in the tooling |
| OPS-10 | **No** (static check only) | Code order confirmed; deployment script not run (needs encrypted keys and acts on real clusters) |
| TEST-01 | Yes | Mutant U1: unit, pen-test, smoke and verify-deployment all pass, but a double mint happens |
| TEST-02 | Yes | Mutant B1: unit and smoke pass, and a delegate can burn and have it recorded in its own statistics; B2 is caught by smoke (2006) |
| TEST-03 | Yes | U4/U5/U6/B4/B5 all leave unit tests passing |
| TEST-05 | Yes | U7 wrongly mints 1.0 token, and smoke still reports PASSED |
| TEST-06 | Yes | B6/B7/B8/B12 all leave unit tests passing; B8 passes the on-chain smoke tests while global statistics stay unchanged |
| TEST-07 | Yes | B9/B10/B11 all leave unit tests passing |
| TEST-09 | Yes | Fresh ledger: non-admin → 6007/6012; before initialization → 3012; U8/B13 leave unit tests passing |

### 5.8 Mutation testing summary (built in a temporary copy; the repository was not changed)

| Mutant | Content | Unit tests | On chain (if run) |
|---|---|---|---|
| U1 | fingerprint ^= stack height | Pass 61/61 | pen-test 0 failures, smoke 7/7, verify-deployment functional checks pass; **double mint via CPI + top-level** |
| U4 | memo-mint takes the memo at current_index − 1 | Pass | Not run |
| U5 | Accepts Memo v1 | Pass | Not run |
| U6 | Index rule < 1 changed to < 2 | Pass | Not run |
| U7 | Pay 1.0 for everything below 20M | **5 failures** | smoke-mint/forum pass; pen-test 3 failures |
| U8 | memo-mint admin constraint removed | Pass | Not run |
| B1 | Owner constraint removed (`lib.rs:393`) | Pass 87/87 | smoke 7/7 pass; **a delegate can burn and have it counted in its own statistics** |
| B2 | User statistics seed changed | Pass | smoke-burn fails with 2006 (caught) |
| B3 | Memo amount rounded to whole tokens before comparison | Pass | Not run |
| B4 | memo-burn takes the memo at current_index − 1 | Pass | Not run |
| B5 | Lenient Base64 engine | Pass | Not run |
| B6 | Cap branch `>` changed to `>=` (equivalent mutant) | Pass | — |
| B7 | `burn_count` not updated | Pass | smoke should catch it (not run) |
| B8 | `GlobalBurnStats::record_burn` removed | Pass | burn/chat/project smoke pass; global statistics unchanged |
| B9 | `>` at `lib.rs:169` changed to `>=` | Pass | Not run |
| B10 | Whole-token check restored | Pass | Not run |
| B11 | Minimum restored to 1 token | Pass | Not run |
| B12 | `last_burn_time` not updated | Pass | smoke should catch it (not run) |
| B13 | memo-burn admin constraint removed | Pass | Not run |

Conclusion: **the tests committed to the repository can hardly catch regressions in the core security checks** (only U7 and B2 were caught). This audit's security conclusions rely on code proof and temporary harnesses; turning the latter into regression tests in the repository is recommended (section 7).

**Status: addressed — every non-equivalent mutant is now caught by the committed tests; see section 8.2.**

---

## 6. On-chain configuration check (read-only RPC, 2026-10-16)

### 6.1 Runtime

| Item | testnet | mainnet |
|---|---|---|
| Version | `solana-core 4.0.3`, feature-set 2137973216 | Same |
| Genesis hash | `C7ucgdDEhxLTpXHhWSZxavSVmaNTUJWwT5iTdeaviDho` | `4SvBP3omtvcCVWdxq1zBY5cDp4wndjsThb6nEMn6iMdN` |
| `account_data_direct_mapping` (`6xynfKa…`) | Not active | Not active |
| `virtual_address_space_adjustments` (`Dww8cW…`) | Not active | Not active |
| `syscall_parameter_address_restrictions` (SIMD-0459, `5Tx7ZWg…`) | Active (slot 185455980) | **Not active** |

### 6.2 Core programs

| Item | testnet | mainnet |
|---|---|---|
| memo-mint program ID | `A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy` | `8iq6zqaEVcfaym2u8t939PAN5jmfPVc6Z333RuxKTTZX` |
| memo-burn program ID | `FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP` | `2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh` |
| Loader | BPFLoaderUpgradeable (upgradeable) | BPFLoaderUpgradeable (upgradeable) |
| Upgrade authority (all 7 programs) | `Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn` (System-owned, single key) | `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ` (System-owned, single key, about 91.19 XNT) |
| memo-mint last deployed | slot 195702768 (2026-10-15T21:33:38Z) | slot 86131302 (2026-10-15T12:38:02Z) |
| memo-burn last deployed | slot 195702899 (2026-10-15T21:34:25Z) | slot 86109925 (2026-10-15T10:27:05Z) |
| memo-mint bytecode | Working tree: `caa266aeae1ed3518e357c045372ed9ac49e5dc949a69d3bf4a6cd2ea7dbd1bb` (ELF 270,624 B, ProgramData 289,376 B) | **HEAD** `ca4c8a0b`: `b62083db27480ab8…` (ELF 270,616 B, ProgramData 283,016 B) |
| memo-burn bytecode | Working tree: `eecad89398ed9c2b4a17c7a8f176c15813eaefbaa685c233c58282ab4fa018ee` (293,408 B, ProgramData 311,704 B) | **HEAD**: `a675b8646a45966736a2ae1a066e5c4b6c694dd9b5c335ee49f4520f7d15cc22` (292,256 B). Still requires whole tokens, at least 1 token |
| Expected mainnet hashes after the upgrade (working tree `--features mainnet`) | — | `memo_mint.so` `2423c17f850df4d41d32db57c7743b6fd15488851be92c52f3ef7c66079be5c7` (270,624 B); `memo_burn.so` `39d06125fca59a581b19fb9ab2925f79166b6d15dcfb4155de19b4e6faac3abf` (293,408 B, needs a 1,152 B extension) |
| IDL accounts (mint / burn) | `9yupG5vn2jMxoTE49Rn6fEAm8tpvEfQJtShVa8Y9sFd3` / `X8E5UM2sd4RbkPt6Xnx63ZzdKpxAZQ26XDAojp2HNFS` (match the working tree; authority = deploy admin) | `FXKbpNLR96JNavynQD3ALVJit1ZgMj3XDxgB1W5j7BFb` / `9ddAVtic1aaNQoWLAzeyGuPE8zD4ojJ6JBnW3ZZfnCXR` (match HEAD; the burn IDL still has the whole-token error messages) |
| Other | Copies of the mainnet IDs (`8iq6…`, `2sb3…`) also exist on testnet, with the mainnet key `FVvew…` as upgrade authority (OPS-02) | The testnet IDs do not exist on mainnet; the testnet deploy admin `Gkxz…` holds about 4 XNT on mainnet (OPS-09) |

### 6.3 Token mint

| Item | testnet `HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1` | mainnet `memoX1sJsBY6od7CfQ58XooRALwnocAZen4L7mW1ick` |
|---|---|---|
| Owner program | Token-2022 `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb` | Same |
| decimals | 6 | 6 |
| Mint authority | `HDz8d6UjzAsGUjVgKKXPeSYrdME9YCFSAcRLJEZTKA91` = memo-mint PDA `[b"mint_authority"]`, bump 255 | `C1r27YsPiY6wbVvRY2cVCsaRKK5HiXxdvWhxJd5wapSh` = memo-mint PDA, bump 254 |
| Freeze authority | None (COption tag 0, cannot be set again) | None |
| Extensions | Only MetadataPointer(18) and TokenMetadata(19) (no PermanentDelegate, TransferHook, MintCloseAuthority, Pausable, TransferFee, DefaultAccountState, ConfidentialMintBurn, ScaledUiAmount or NonTransferable, and none can be added later) | Same |
| Metadata / pointer authority | `Gkxz6…` (URI points to `refs/heads/dev`) | `FVvew…` (URI points to `refs/heads/main`, a mutable branch) |
| Account size / lamports | 455 B / 4,057,680 (= rent-exempt amount) | 456 B / 104,064,640 (0.1 XNT extra, permanently locked because there is no withdrawal path) |
| Supply | 8,241,228.855432 tokens | 8,730,757.000000 tokens (still in the 1-token-per-mint phase) |

### 6.4 State PDAs

| Account | testnet | mainnet | Common properties |
|---|---|---|---|
| `MintState` `[b"mint_state"]` | `7KF1A4PsYabWYf2DqueuN9Ybgbp4To1jPT1mBnG5nycq`, last_slot 195707916 | `EZheeAswQ9exUMpKFjHrQrczix5LSzjakroRFiMgJfWv`, last_slot 86131693 | 113 B, discriminator `51118f7817391675`, bump 254 (canonical), 1,677,360 lamports (rent-exempt), reserved all zero |
| `GlobalBurnStats` `[b"global_burn_stats"]` | `HY5VzmHtf3braVAz7ihJWsAfp3hyPHiycfcUbA3en26j`, bump 255, 89,363.234568 tokens / 39 burns | `73xgyJWmEocTF5BguwZ4bJAP8irpSyFZBsooL4yoDUFa`, bump 251, 1 token / 1 burn (last_burn_time 2026-10-15T10:56:54Z) | 105 B, discriminator `629cf4cb76303b33`, canonical bump, 1,621,680 lamports, reserved all zero |
| `UserGlobalBurnStats` | 34 accounts, total 723,252.234568 tokens / 139 burns | 156 accounts, total 384,760 tokens / 213 burns | 65 B; all pass PDA validation, 0 mismatches; no account exceeds 18T |

### 6.5 Platform programs

| Program | testnet | mainnet |
|---|---|---|
| SPL Memo v3 `MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr` | BPFLoader2, **not upgradeable** | Same |
| Token-2022 | Upgradeable; authority `FwJkGYPdpp3BjfRBtrU5duGHMBmiSHmEwGX6oBSMXEQx` (single key); 541,936 B (`6840c7dd…`), last deployed slot 53315980; contains no Pausable, ScaledUiAmount or ConfidentialMintBurn code | Upgradeable; ProgramData `DoU57AYuPFu2QU514RktNPG22QhApEjnKxnBcu4BHDTY`, authority `2sXr5THr5ZAwSfuJAc1KogPKZ2uKkhfNUAtEvNw5Q6Z7` = Squads v4 vault (multisig `89PksQjFwnpUNu3Rc3GLdnZnBCbUNLNruDHcjdU1zPfs`, 3/6, time_lock 0); 683,544 B (`d32c6364…`, byte-identical to the version built into the local validator) |
| Legacy SPL Token | — | Not upgradeable (Authority: none) |

---

## 7. Recommendations

### 7.1 Must be done before the mainnet upgrade

> **Owner note (2026-10-16)**: mainnet currently has very few users, so the upgrade is low-impact. The recommended order (memo-burn and memo-mint first, then the apps) still applies.

1. **Upgrade in the correct order** (OPS-01): upgrade memo-burn (and memo-mint) first, or do it in the same `deploy-mainnet.sh` run as the apps, in canonical order. Never deploy the apps alone. Add a cross-program version check to `deploy-common.sh`.
2. **Pin and verify the bytecode to deploy** (OPS-04):
   - Commit the working tree and build `--features mainnet` in a clean target directory.
   - Check that the hashes match the expected values in 6.2.
   - After the upgrade, use `verify-deployment` to check on-chain bytecode and IDL, and run one read-only small-amount burn simulation on mainnet.
3. **Separate the mainnet key from testnet staging** (OPS-02) — **accepted risk with a planned mitigation; not blocking the upgrade.** The recommendation stands: transfer the upgrade authority of the mainnet ID copies on testnet to a separate key; unlock the mainnet key only in a dedicated environment and lock it immediately after use. Owner response (2026-10-16): the owner accepts the risk for now; once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable). See 8.3.
4. **Fix the documentation before publishing the audit conclusions** (MINT-01, MINT-02, BURN-01, BURN-03, OPS-03) — **done (2026-10-16).** The statements in `MEMO-MINT-AUDIT.md` and `MEMO-BURN-AUDIT.md` about the last tier, batch minting, checked arithmetic, the Appendix C account list, the meaning of `GlobalBurnStats`, the deployment steps and the deployment status were wrong or outdated and would mislead third-party developers. Status: fixed in MEMO-MINT-AUDIT.md / MEMO-BURN-AUDIT.md (2026-10-16). See 8.1.

### 7.2 Should be fixed (by priority)

1. **Reduce centralization risk** (OPS-05, the largest remaining risk) — **accepted risk with a planned mitigation.** Recommendation: move the upgrade authority of memo-mint and memo-burn to a multisig with a timelock, or run `--final` once stable; use different keys for the metadata authority and the app upgrade authority; pin the metadata URI to a specific commit or immutable storage. Owner response (2026-10-16): accepted for now; once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable). See 8.3.
2. **Turn the temporary audit tests into regression tests in the repository** — **Status: addressed by committed regression tests — see section 8.2.**
   - CPI double-mint cases and a minimal CPI test program (TEST-01)
   - `pen-test-memo-burn` (TEST-02)
   - Unit test matrix for `check_memo_instruction` (TEST-03)
   - Error code pinning test (TEST-10)
   - Unit tests after extracting `check_burn_amount` and `UserGlobalBurnStats::record_burn` (TEST-06, TEST-07)
   - Smoke tests changed to compare strictly with an independent reference (TEST-05)
   - Fresh-ledger initialization script (TEST-09)
   - A case where the `process_mint_to` recipient uses the wrong mint (remainder of TEST-04)
   - CPI tampering harness (INT-12)
3. **Runtime monitoring** (MINT-03, INT-12): after every X1 validator upgrade or activation of a related feature, re-run `verify-deployment`, the app CPI double-mint simulation and the CPI tampering suite.
4. **Harden the deployment tooling**: detect the network by genesis hash (OPS-09); have `verify-deployment` check the freeze authority and an extension allow-list, and run it automatically after deployment (OPS-08); pin the toolchain and use verifiable builds (OPS-07); compile the admin initialization clients up front (OPS-10).
5. **Integration documentation** (for third-party developers) — **Status: documented in README integration notes (2026-10-16)** for BURN-04, INT-05, INT-06, INT-04, BURN-06 and BURN-02:
   - burn_amount binds a single instruction; statistics and attribution should use logs or increments (BURN-04, INT-05, INT-06)
   - `GlobalBurnStats` is only a lower bound (BURN-01, BURN-05)
   - Durable nonces are not supported; compute budget instructions must be at index ≥ 1 (INT-04)
   - CPI Guard and multisig accounts cannot burn (BURN-06)
   - The "69" minimum is a client-side input rule; on chain, a hand-built memo-burn memo must be at least 72 Base64 characters, and 69–71 characters return `InvalidMemoFormat` instead of `MemoTooShort` (BURN-02)
   - Quoting and retry strategy when thresholds jump a tier (INT-02)
   - Keep mint instruction lists unique to avoid same-slot false positives (MINT-01)
   - Fix the wrong compute budget comments in the 5 apps (INT-04)

### 7.3 Optional improvements

- Have `transaction_fingerprint` parse the sysvar header first and fail closed on an unexpected layout (MINT-03).
- Call `mint_state.exit(&crate::ID)?` explicitly after `record_mint`, or add a comment (MINT-04).
- Declare the `no-entrypoint` feature of `spl-memo` explicitly (INT-10); use `[lints.rust]` instead of crate-level allows (INT-11).
- Add an explicit Token-2022 owner constraint to the token accounts (INT-07).
- Make the client depend on the curve function in memo-mint (INT-03).
- Fix the error code message in `admin-init-mint-state` (OPS-11).
- At the next coordinated upgrade, reword the `MemoTooShort` error message, or change the memo-burn constant to 72 (BURN-02).
- Rate-limit memo-chat per sender (INT-09).
- If needed, review durable nonce support separately (INT-04).

### 7.4 Remaining risks

1. **Upgrade and admin key** — **accepted risk with a planned mitigation.** On mainnet all memo programs can be upgraded by one hot key; its holder could mint without limit and abuse users' signer privileges; this key has also been used for testnet staging. The keys are stored GPG-encrypted and unlocked to a RAM disk, but the key trees for both networks are on the same machine. Owner response (2026-10-16): accepted for now; once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable). See 8.3.
2. **Platform**: X1 Token-2022 can be upgraded by a 3/6 multisig with no timelock; the X1 runtime's sysvar layout and CPI account synchronization semantics (on mainnet the byte comparison is the only barrier).
3. **Deployment state**: mainnet currently runs the HEAD version. This report's conclusions about small-amount burns do not apply to mainnet before the upgrade; if the upgrade order is wrong, the apps' small-amount burn paths will fail (no funds lost). Mainnet currently has very few users, so the upgrade itself is low-impact.
4. **Bounded behavior by design**: same-slot false positives (MINT-01); burn-then-mint step arbitrage, cumulative < 1 token over the whole curve (INT-01); in-flight transactions failing when thresholds jump a tier (INT-02); paid congestion from global write locks (INT-08).
5. **Off-chain misreading**: indexers that count by memo text or by `GlobalBurnStats` will report wrong numbers.
6. **Insufficient regression protection**: the committed tests can hardly catch regressions in the core checks (5.8). Before any future code change is deployed, the tests in 7.2 item 2 should be in place first. **Status: addressed by committed regression tests — see section 8.2.**

---

## 8. Post-audit remediation

This section records what was done after the audit, based on the owner's decisions of 2026-10-16.

### 8.1 Documentation

| Item | Findings | Status |
|---|---|---|
| Integration notes for third-party developers in the README | BURN-02, BURN-04, BURN-06, INT-04, INT-05, INT-06 | Documented in README integration notes (2026-10-16) |
| Corrections to `MEMO-MINT-AUDIT.md` | MINT-01 (wording at line 110), MINT-02, OPS-03 | Fixed in MEMO-MINT-AUDIT.md (2026-10-16) |
| Corrections to `MEMO-BURN-AUDIT.md` | BURN-01, BURN-03, OPS-03 | Fixed in MEMO-BURN-AUDIT.md (2026-10-16) |

### 8.2 Regression tests

The audit harnesses (`clients/examples/audit_dynamic.rs`, `clients/examples/audit_repro.rs`) were turned into committed regression tests and then deleted (TEST-01 to TEST-10, 5.8, 7.2 item 2). No code in memo-mint or memo-burn `lib.rs` was changed; the program bytecode is identical to the audited build (`memo_mint.so` `caa266ae…`, `memo_burn.so` `eecad893…`).

**Unit tests** (`./scripts/run-unit-tests.sh`)

- memo-mint: 61 → 72. Adds a `check_memo_instruction` matrix built from real instructions-sysvar data (TEST-03) and a curve check against an independent decimal-digit reference plus hand-written tier values (TEST-05). The tautological CPI test was renamed `test_repeated_record_with_same_sysvar_mints_once` and documents what it does not prove (TEST-01, TEST-08).
- memo-burn: 87 → 99. Adds a `check_memo_instruction` matrix (TEST-03); error codes 6000–6012 pinned (TEST-10); strict Base64 (padding, canonical trailing bits, URL-safe alphabet, whitespace and trailing data rejected); exact amount binding including fractional amounts; the BURN-02 lengths 68 / 69–71 / 72; a deterministic 100,000-case differential test against `BurnMemo::try_from_slice` (TEST-07). Tests that only exercised the standard library or never asserted were removed or rewritten (TEST-08).

**On-chain regression suite** (`./scripts/run-regression-tests.sh`, local validator, about 3 minutes, 178 cases)

Every failing case asserts the exact `InstructionError [index, Custom(code)]` and that no watched state changed (balances, supply, `MintState`, user and global burn stats). Every successful mint is checked against an independent curve reference; every successful burn checks exact balance, supply, user-stats and global-stats deltas and that both `last_burn_time` fields equal the block time.

| Suite | Cases | Coverage |
|---|---|---|
| `regression-memo-curve` | 61 | Starts at 18.4T supply: mint above, at and 1 unit below 10T; walks the supply down with exact 1T burns and mints twice at 9T, 5T, 1T+1, 1B, 100M, 20M, 10M+1, 10M and 5M; user-stats 18T cap (TEST-05, TEST-06, TEST-07, MINT-05) |
| `regression-memo-mint` | 48 | One mint per transaction at top level (including 30 mints in one transaction); CPI double mint through memo-blog and memo-forum in both orders, combined with `process_mint` and `process_mint_to` (TEST-01); memo position and length rules; account substitution including `process_mint_to` with a wrong-mint recipient (TEST-04 residue); durable nonce (INT-04) |
| `regression-memo-burn` | 65 | Amount bounds; memo binding; strict Base64; BURN-02 lengths; delegate/owner checks; stats PDA; mint; exact stats deltas (TEST-02, TEST-06); CPI Guard → 43 (BURN-06); durable nonce (INT-04) |
| `regression-admin-init` | 4 | Fresh ledger without core state: mint and burn fail with 3012 before initialization; non-admin initialization fails with 6007 / 6012 and creates nothing (TEST-09) |

**Mutation re-test.** All mutants from 5.8 were rebuilt in a scratch copy and loaded into the local validator with `--so-dir`. The unmutated control build passed all 178 cases. Every non-equivalent mutant is now caught:

| Mutant | Unit tests | Caught on chain by |
|---|---|---|
| U1 (fingerprint ^= stack height) | not caught | M04b, M04c, M04f, M04g (app CPI + top-level mint) and the `MintState` fingerprint check in every successful mint (34 cases fail) |
| U4 (memo at current_index − 1) | 3 fail | M01–M03c, M05, M09, X3a and others (16 cases) |
| U5 (Memo v1 accepted) | 1 fails | M09b |
| U6 (index rule < 2) | 2 fail | every successful mint (67 cases) |
| U7 (1.0 per mint below 20M) | 8 fail | S-20M, S-10M+1, S-10M-2 (8 cases) |
| U8 (memo-mint admin check removed) | not caught | TEST-09.mint-non-admin |
| B1 (owner constraint removed) | not caught | B24, B25, B25b |
| B2 (user-stats seeds changed) | not caught | almost every burn case (2006) |
| B3 (memo compared after whole-token rounding) | not caught | B02, B02b, B02c, B16b, C02 and others |
| B4 (memo at current_index − 1) | 3 fail | B20, B22c, B23, X3c and others |
| B5 (lenient Base64) | 4 fail | B18, B18b, B19-70, B19-71 |
| B6 (`>` → `>=` in the cap branch) | — | equivalent mutant, not testable |
| B7 (`burn_count` not updated) | not caught | every exact-delta burn, T04, T05 |
| B8 (`GlobalBurnStats::record_burn` removed) | not caught | every exact-delta burn, B36 |
| B9 (upper bound `>` → `>=`) | not caught | D-10T-1 (exact 1T burn), B36, X1b |
| B10 (whole-token check restored) | not caught | B02, B02b, C02 and most burn cases |
| B11 (minimum back to 1 token) | not caught | B02, B02c, C02 |
| B12 (`last_burn_time` not updated) | not caught | B00b, every exact-delta burn |
| B13 (memo-burn admin check removed) | not caught | TEST-09.burn-non-admin |

**Still not covered:** successful admin initialization (needs the admin key); unit tests for the user-stats cap and handler amount bounds (the logic is inline in `process_burn`, so it is covered on chain only); the same-slot false positive (MINT-01), v0 transactions with address lookup tables, multisig owners, and two-level nested CPI (needs a dedicated CPI test program).

**App note found while migrating:** a transaction with both a memo-blog mint and a memo-forum mint fails because memo-forum runs out of memory while Borsh-decoding a memo written for memo-blog. The transaction still fails as it should (only one mint per transaction is allowed), but without a clean error code. This is an app robustness issue in memo-forum, not a core issue.

### 8.3 Accepted risks

| Finding | Severity | Owner decision (2026-10-16) |
|---|---|---|
| OPS-02 (mainnet key also used for testnet staging) | Low | Accepted for now. Once memo-mint and memo-burn have run stably for a while, their upgrade authority will be revoked (programs made immutable). The recommendations in OPS-02 remain valid but do not block the mainnet upgrade. |
| OPS-05 (one hot key holds all upgrade/admin/IDL/metadata authorities) | Info | Accepted for now, with the same planned mitigation: revoke the upgrade authority of memo-mint and memo-burn after a period of stable operation. |
| OPS-01 / OPS-04 (mainnet still on HEAD; upgrade order) | Info | Mainnet currently has very few users, so the upgrade is low-impact. The recommended order (memo-burn and memo-mint first, then the apps) still applies. |

---

## Appendix A: Evidence file locations (temporary audit directory, not part of the repository, may be cleaned up)

All paths are under `/private/tmp/claude-501/-Volumes-PortableSSD2T-github-memo-token/fff39611-676c-47c2-8a12-91cf6095c89f/scratchpad/audit/`:

| Content | Path |
|---|---|
| Dynamic case logs | `dynamic/` (`10-A-mint-suite.log`, `11-A-burn-suite.log`, `12-A-extra-suite.log`, `13-A-sameslot.log`, `20-A-pentest-*.log`, `31-step-*.log`, `41-cap-suite.log`, `51-saturation-suite.log`, `61-extra2-suite.log`, `all-cases.jsonl`) |
| Reproduction logs | `repro/` (`A-*.log`, `B-*.log`, `C-int02.log`, `D-cap5.log`, `E-ops01.log`, `F-*.log`, `G-test09fresh.log`) |
| Curve harness | `extract.py`, `refgen.py`, `main.rs`, `gen/`, `curve_check.out` |
| memo-burn harness | `burn-harness/` (`src/bin/fuzz.rs`, `out.txt`) |
| Test coverage harness | `tests-coverage/` (`tc-mint`, `tc-burn` and logs) |
| Fingerprint demo | `fp-demo/src/main.rs` |
| Read-only simulation scripts | `sim_one_mint_user2.py`, `runtime-deps/sim.py` |
| CPI tampering harness | `cpi-tamper/` (attacker, harness, `results.json`, `compare.json`) |
| Third-party crank demo program | `crank/src/lib.rs` |
| Program dumps and rebuilds | `dumps/`, `build_wt/`, `build_head/`, `out/`, `rebuild/` |

Related untracked files in the repository (shown as `??` by git status):
- `clients/examples/audit_dynamic.rs`, `clients/examples/audit_repro.rs`: temporary dynamic test harnesses created during the audit. **These two files have been replaced by the committed regression suite** (`clients/regression/`, `scripts/run-regression-tests.sh`; see section 8.2) and deleted.
- `scripts/local-validator.sh`, `clients/smoke-test/src/prepare-local-validator.rs`: tools created for local-validator testing before the audit started (not audit artifacts); this audit's dynamic tests depend on them. Whether to commit them is up to the project team.
