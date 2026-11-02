# Memo-Burn Smart Contract Security Audit Report

## Executive Summary

**Contract**: memo-burn  
**Audit Date**: November 8, 2025 (original), revised October 13 and 16, 2026, re-audited November 2, 2026  
**Auditor**: Pre-Production Security Review  
**Version**: Revision 4, program version 0.3.0 (Revision 4: compute-unit optimization after the November 2026 re-audit; Revision 3 plus the October 16, 2026 change to burns of any amount from 1 unit was 0.2.0; Revision 3: global burn statistics; Revision 2: length-prefix check, header-only memo parsing, no maximum memo length, compute-unit optimization)  
**Language**: Rust (Anchor Framework)  
**Network**: X1 (SVM-based)

> **Note (October 16, 2026)**: the final audit of memo-mint and memo-burn, `MEMO-CORE-AUDIT-2026-10-16.md`, re-verified this contract against the source code and corrected several statements in this document (finding IDs such as BURN-01 and OPS-03 refer to that report). Where the two documents differ, the core audit report takes precedence.

> **Note (November 2, 2026)**: the re-audit below (findings BURN-R01 to BURN-R22) supersedes both documents where they differ. Statements it found stale have been corrected in place.

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
- **Minimum burn**: 1 unit (0.000001 token, `MIN_BURN_AMOUNT = 1`); fractional token amounts are allowed. The apps built on memo-burn (chat, profile, project, blog, forum) enforce their own, supply-scaled minimums before the CPI; memo-id burns computed prices, and memo-home burns any amount its Vault holds
- **Maximum burn per `process_burn` instruction**: 1,000,000,000,000 tokens (a transaction may contain several burn instructions)
- **Token decimals**: 6 (DECIMAL_FACTOR = 1,000,000)
- **Memo length**: at least 69 bytes checked on chain (in practice at least 72 characters, because padded Base64 lengths are multiples of 4), no contract maximum
- **Memo payload**: no contract maximum; a memo decodes to 3/4 of its length, and the payload is that minus the 13-byte Borsh header. With the 9 account keys a burn transaction needs since Revision 3 (the 7 `process_burn` accounts plus the Memo and memo-burn program IDs), a single-burn legacy transaction on X1 today holds a memo of about 812 bytes (about 772 with a SetComputeUnitLimit instruction), that is a payload of about 596 bytes; a v0 transaction with an address lookup table holds about 960 bytes

### Deployment Status (updated November 2, 2026)

| Network | Deployed memo-burn | Burn amount rule |
|---|---|---|
| X1 testnet | **Revision 4 (0.3.0), upgraded November 2, 2026**: sha256 `46d526ef…` (290,544 bytes), the local testnet build; before it 0.2.0, `eecad893…` | Any amount from 1 unit (this document) |
| X1 mainnet | **Upgraded October 17, 2026** to commit `c47f1eb9` (slot 86507344, together with memo-mint and all apps); bytecode sha256 `39d06125…` = the audited `--features mainnet` build | Any amount from 1 unit (this document). Before the upgrade mainnet ran `ca4c8a0b` (Revision 3, whole tokens only) |

Testnet runs Revision 4 (0.3.0) since November 2, 2026; mainnet runs 0.2.0 (`39d06125…`). See `deployments/*.sha256`. For future upgrades, memo-burn must still be upgraded **before or together with** the apps: the apps set supply-scaled minimums that become fractional above 10M supply, and those burns would fail with 6005 against a whole-token memo-burn (the transaction rolls back, no funds are lost).

---

## Re-audit (November 2, 2026)

**Scope**: `programs/memo-burn` at commit `583d6d5c` (0.2.0, the build deployed on both networks), its unit tests, the regression and smoke-test clients, this report, the memo-burn parts of `MEMO-CORE-AUDIT-2026-10-16.md`, and how the seven dependents call it via CPI (memo-chat, memo-profile, memo-project, memo-blog, memo-forum, memo-home, memo-id). It was done after the memo-mint re-audit and Revision 5 deployment (`MEMO-MINT-AUDIT.md`), to the same depth.

**Method**: seven independent review lenses (access control and accounts, memo discovery and parsing, state and arithmetic, integration with the dependents, compute units and DoS, tests and documentation, build and deployment). Every finding was then checked by two independent reviewers who tried to refute it, one on correctness and one on impact. A final pass looked for areas no lens covered. Measurements used LiteSVM 0.13.1 with X1 mainnet's own Token-2022 and Memo program dumps, and bit-identical rebuilds of the deployed program.

**Result**: no critical, high or medium findings. 4 low and 18 informational findings survived verification (BURN-R01 to BURN-R22, below). Two candidate findings were refuted.

**Verified properties** (selection):
- Both deployed binaries reproduce bit for bit from the source at `583d6d5c`: testnet `eecad893…` and mainnet `39d06125…` (`deployments/*.sha256`).
- Every account in `process_burn` is bound: `user` signs; the mint is the fixed `AUTHORIZED_MINT_PUBKEY`; the token account's mint and owner are checked; both stats accounts are checked against their PDA; `token_program` is `Program<Token2022>`; the instructions sysvar is checked by address. A substituted account fails before any state changes.
- The Token-2022 burn runs before the statistics are written, in one instruction, so the stats never count a failed burn.
- Memo discovery reads the top-level instruction 0 through the instructions sysvar, also when `process_burn` is reached via CPI. The Base64 decoder is strict (padding required, no whitespace, canonical trailing bits); the length prefix is checked before any allocation of the declared size; version and amount are checked exactly.
- No caller program can change another user's stats or burn another user's tokens through `process_burn`: the token owner must sign as `user`.
- No reachable path aborts the program: arithmetic saturates, the heap holds about 7 KB at the 1.4M CU limit (32 KB available), and the transaction size binds the memo first on X1.

### Findings

| ID | Severity | Status | Title |
|---|---|---|---|
| BURN-R01 | Low | Fixed | `initialize_global_burn_stats` success path was never tested |
| BURN-R02 | Low | Fixed | Manual upgrade steps lacked the MINT-22 guard; no test pinned the program and mint IDs |
| BURN-R03 | Low | Fixed | README told indexers to count burns from log text that any program can print |
| BURN-R04 | Low | Fixed | This report named only five dependents; memo-home and memo-id also call memo-burn |
| BURN-R05 | Info | Fixed in Revision 4 | About 4,300 avoidable CU per burn |
| BURN-R06 | Info | Documented | `UnauthorizedUser` (6011) can never be returned |
| BURN-R07 | Info | Corrected | This report said `InterfaceAccount` enforces Token-2022 |
| BURN-R08 | Info | Owner decision: frozen | `UserGlobalBurnStats` has no reserved space and no close instruction |
| BURN-R09 | Info | Fixed | The `UserGlobalBurnStats::SPACE` test checked a hand-written sum |
| BURN-R10 | Info | Fixed | Source comments contradicted the code |
| BURN-R11 | Info | Documented | Redundant branches in memo discovery |
| BURN-R12 | Info | Fixed | No test pinned the success log line |
| BURN-R13 | Info | Corrected | Memo size is bounded by the transaction size and CU, not by the heap |
| BURN-R14 | Info | Fixed | Burn smoke test: fixed 300k CU, float balance check, flaky time check |
| BURN-R15 | Info | Open | Regression gaps: app CPI burns, cross-app memo reuse, memo-home memo errors, maximum memo, CU bound |
| BURN-R16 | Info | Corrected | Core audit statuses overstated what was fixed |
| BURN-R17 | Info | Documented | A burn's `user` can be a program PDA (memo-home Vault) |
| BURN-R18 | Info | Fixed | `MEMO-HOME-DESIGN.md` gave the wrong rent for the Vault's burn stats |
| BURN-R19 | Info | Open (apps) | Five apps re-derive memo-burn's PDAs before the CPI |
| BURN-R20 | Info | Open | `spl-memo` is a dependency without `no-entrypoint` |
| BURN-R21 | Info | Open (ops) | `PROD-DEPLOYMENT.md` requires every partial run to include memo-burn |
| BURN-R22 | Info | Corrected | Stale statements in this report |

#### 🟡 BURN-R01 (Low, fixed): `initialize_global_burn_stats` success path was never tested

The committed tests covered only its rejections: a non-admin on a fresh deployment (6012) and a second call (already in use). The local validator plants `GlobalBurnStats` through `prepare-local-validator`, so the instruction handler never wrote the account that every burn test then read. A future change that stored a wrong bump or broke the layout would have passed every test. On the next fresh deployment, every burn (direct and through all seven dependents) would then fail `ConstraintSeeds` or fail to deserialize, and since the account cannot be closed or created again, only a program upgrade could fix it. The account already exists on both networks, so the deployed programs are not affected. Core audit TEST-09 asked for this test and was marked addressed, which was true for memo-mint only.

**Fixed (November 2, 2026)**: `regression-admin-init` runs memo-burn's admin success path in `simulateTransaction` with signature verification off, as it already did for memo-mint: the account it creates (owner, 105 bytes, discriminator, zero counters, canonical bump, zero `reserved`), `[memo, initialize_global_burn_stats, process_burn]` in one transaction, a second initialization, initialization at an address that is not the PDA, the admin not signing, and that nothing persists. 6 new cases (17 in the suite).

#### 🟡 BURN-R02 (Low, fixed): manual upgrade steps lacked the MINT-22 guard; no test pinned the program and mint IDs

Appendix B step 1 was a bare `anchor build -p memo-burn -- --features mainnet`. In a checkout without `target/deploy`, anchor-cli 0.32.1 rewrites the first `declare_id!` of every program, which is the mainnet one (`MEMO-MINT-AUDIT.md`, MINT-22). The memo-burn unit tests pinned only the admin key, so such a build passed all of them. Upgrading it into `2sb3gz5C…` would make every memo-burn instruction fail with `DeclaredProgramIdMismatch`, and with it every burn through the apps, until a corrective upgrade.

**Fixed**: Appendix B now uses the guarded build and checks the hash before the upgrade. `test_program_and_mint_ids_per_network` pins `crate::ID` and `AUTHORIZED_MINT_PUBKEY` for each network; run the unit tests under both features.

#### 🟡 BURN-R03 (Low, fixed): README told indexers to count burns from log text

`README.md` listed the `Burned N units from ...` log as a source for counting burns. Any program in the same transaction can print the identical `Program log:` line, so an indexer that matched log text could be made to record burns that never happened. Only the surrounding `Program <id> invoke` frame tells the lines apart, and logs are truncated at 10 KB.

**Fixed**: the README now names the Token-2022 `Burn` inner instructions under memo-burn's program id and balance or stats deltas as the sources; a log line counts only inside memo-burn's own invoke frame.

#### 🟡 BURN-R04 (Low, fixed): only five dependents named

The minimums rationale, the Revision 2 compatibility note, the deploy order, the checklist, the key note and Appendix B named chat, profile, project, blog and forum. memo-home calls `initialize_user_global_burn_stats` and `process_burn` via CPI (with a Vault PDA as `user` and payer), and memo-id calls `process_burn`. Both embed memo-burn's program ID and mint. **Fixed**: all seven dependents are listed.

#### ℹ️ BURN-R05 (Info, fixed in Revision 4): about 4,300 avoidable CU per burn

memo-burn cost 16,345 CU for a minimum (72-byte) memo, of which the Token-2022 burn is 1,117. Options measured (LiteSVM, testnet build, deltas against 0.2.0):

| Option | Change | CU |
|---|---|---|
| B1 | Check `GlobalBurnStats` against a per-network address constant instead of seeds and stored bump (memo-mint's O1b; same guarantee, see Revision 4 Changes) | −1,597 |
| B2 | Drop the seeds check on the user stats account, keep owner, discriminator and `user` field | −1,621 (not adopted: removes defense in depth) |
| B3a | Drop the base58 user from the success log | −2,264 |
| B3b | Log only `Burned {amount} units` | −2,589 |
| B4 | Drop `std::str::from_utf8` before the Base64 decode; the Memo program at index 0 has already rejected invalid UTF-8, and decoding rejects every non-Base64 byte with the same error | −143 at 72 bytes, −586 at 764 |
| B5 | `no-log-ix-name` feature | −105 (not adopted, as for memo-mint) |

**Owner decision (November 2, 2026)**: B1 + B3b + B4. B3b was chosen over B3a because clients read the stats accounts, not the log, and a fixed log line keeps the CU independent of the totals' digit count (4–13 CU per digit). See Revision 4 Changes.

#### ℹ️ BURN-R06 (Info, documented): `UnauthorizedUser` (6011) can never be returned

`user_global_burn_stats` is checked against the seeds `[b"user_global_burn_stats", user]` with its stored bump. The only writer of its `user` field is `initialize_user_global_burn_stats`, which sets it to the signer whose key is in those seeds. Any account that passes the seeds check therefore already has `user == user.key()`, and a substituted stats account fails earlier with `ConstraintSeeds` (2006; regression B26). The constraint is kept as defense in depth, and 6011 keeps its number. Design Confirmation #4 and section 5 are corrected.

#### ℹ️ BURN-R07 (Info, corrected): `InterfaceAccount` does not enforce Token-2022

This report said that `InterfaceAccount` enforces the token standard. `InterfaceAccount<Mint>` and `InterfaceAccount<TokenAccount>` accept accounts of either SPL Token or Token-2022. Token-2022 is enforced by the fixed mint address, `token_account.mint == mint`, `Program<Token2022>` and Token-2022's own owner checks (core audit INT-07). Corrected in Design Confirmation #5, Best Practice #4 and section 4.

#### ℹ️ BURN-R08 (Info, owner decision: frozen): `UserGlobalBurnStats` layout and rent

`GlobalBurnStats` has 64 reserved bytes; `UserGlobalBurnStats` has none, no version byte and no close instruction. 158 accounts exist on mainnet and 39 on testnet, all 65 bytes; memo-chat, memo-blog, memo-forum, memo-profile and memo-project compile the layout in. memo-home creates one per Vault that burns. Adding a field later would need a migration of every account and new accounts in the dependents' CPIs; once memo-burn is immutable (README, Security), nothing can be added.

**Owner decision (November 2, 2026)**: the layout is frozen: discriminator, `user` at offset 8, `total_burned` at 40, `burn_count` at 48, `last_burn_time` at 56, `bump` at 64. The rent (1,343,280 lamports on X1) cannot be recovered. A future per-user field would go in a separate account. Stated in the README.

#### ℹ️ BURN-R09 (Info, fixed): `SPACE` test checked a hand-written sum

`test_user_global_burn_stats_space_constant` compared `SPACE` with a sum of literals, so a field change would pass it and break initialization on chain. **Fixed**: `test_space_matches_serialized_layout` compares both accounts' `SPACE` with the serialized struct.

#### ℹ️ BURN-R10 (Info, fixed): source comments contradicted the code

- `MAX_USER_GLOBAL_BURN_AMOUNT` was described as preventing overflow and tracking more. `saturating_add` prevents overflow; the 18T cap sits just below `u64::MAX` (about 18.45T tokens) and only stops the total slightly earlier.
- `GlobalBurnStats` was said to count every burn; it counts burns since its initialization (core audit BURN-01, which corrected only this report).
- `check_memo_instruction` was said to return `None` for an invalid memo; a short memo at index 0 returns `Err(MemoTooShort)`.
- The length-prefix comment said Borsh would allocate the declared length. memo-burn does not call Borsh; Borsh 0.10.4's `Vec` reader would first allocate up to the declared length, beyond the 32 KiB heap.

**Fixed** in the Revision 4 source (comments only).

#### ℹ️ BURN-R11 (Info, documented): redundant branches in memo discovery

`validate_memo_length`'s empty-memo branch is covered by the length check, and it always returns `Ok(true)`. `check_memo_instruction`'s error arm for `load_instruction_at_checked(0)` cannot be reached once the sysvar address and `current_index >= 1` are checked. They cost a few CU and change no outcome. Left unchanged; the separate UTF-8 check was removed as B4.

#### ℹ️ BURN-R12 (Info, fixed): no test pinned the success log line

The regression harness checked balances, supply and both stats accounts, but not the log line. **Fixed**: `burn_ok` / `burn_deltas_ok` require one `Burned N units` line per burn and the exact line for a single burn (Revision 4 format); B23c and B36, which check their own deltas, assert the line too.

#### ℹ️ BURN-R13 (Info, corrected): what bounds the memo

The Memo program costs a flat 350–376 CU per Base64 character, the same for every character of the alphabet. On X1's 1,232-byte transactions the size limit binds first (a memo of at most about 772 bytes with `SetComputeUnitLimit`; a 764-byte memo used 293,900 CU in total). The 1.4M CU limit would cap a direct burn memo at about 3.8 KB, where the heap holds about 7 KB of 32 KB. The "Memory note" scenario of a heap failure past about 16 KB can never happen. Corrected under Revision 2 and in the README.

#### ℹ️ BURN-R14 (Info, fixed): burn smoke test

`smoke-test-memo-burn` sent a fixed 300,000 CU limit (about 7x the need; X1's fee follows the requested limit), compared balances as floats and only warned on a mismatch, never read `GlobalBurnStats`, and required `last_burn_time` to increase strictly, which fails falsely when the same wallet burned earlier in the same second. **Fixed**: it simulates first and sends the simulated CU + 50, compares integer units and fails on a mismatch, checks that the global stats counted the burn (at least, since others may burn at the same time), and requires `last_burn_time` to equal the burn transaction's block time. It reads at `confirmed` commitment: with the client's default (`finalized`) the balance read before the burn could miss the wallet's latest mints, which made the first testnet run report a false mismatch. `test-memo-burn` keeps its 300,000 fallback for simulations that fail (it sends burns meant to fail) or return no CU figure.

#### ℹ️ BURN-R15 (Info, open): regression gaps

The regression suites do not yet cover:
- a successful burn through each of memo-chat, memo-profile, memo-project, memo-blog and memo-forum (memo-home and memo-id are covered by their suites);
- the core audit's B37: one app memo backing the app's burn and a direct `process_burn` (INT-05);
- memo-home `burn` with no memo, with the memo after it, with a short memo and with a non-Base64 memo (memo-home does not check the memo itself, so these reach memo-burn's check through a CPI);
- a maximum-size memo, and a CU bound comparable to memo-mint's M24n–p;
- a third-party caller program (core audit INT-12).

Recommended; not required for Revision 4, which changes no account list and no memo or amount rule. Its one changed check, the `GlobalBurnStats` address, is covered by regression B31d.

#### ℹ️ BURN-R16 (Info, corrected): core audit statuses

In `MEMO-CORE-AUDIT-2026-10-16.md`, TEST-09 was marked addressed though memo-burn's admin success path was untested (now BURN-R01); TEST-05/06 were marked addressed though the smoke test still used float balances and never read the global stats (now BURN-R14); section 8.2 said the library-only tests were removed, but some remain in `tests.rs`; invariants B-6/B-7 cite local cases B03/B37 that were not committed. Notes were added there.

#### ℹ️ BURN-R17 (Info, documented): a burn's `user` can be a program PDA

memo-home burns with the Vault PDA as `user`, so the Vault's `UserGlobalBurnStats` records the burn, and a Vault's history moves with its Asset when it is sold. memo-home also accepts any BurnMemo with the right amount, without reading the payload. memo-id records burns under the payer and needs the payer's stats to exist first. No on-chain program reads the per-user totals. Incentive programs should attribute Vault burns to the Asset owner at burn time (memo-home's `VaultBurned` event). Documented in the README and Design Confirmation #4.

#### ℹ️ BURN-R18 (Info, fixed): Vault burn stats rent

`MEMO-HOME-DESIGN.md` gave 0.0016 XNT, the rent of the 105-byte `GlobalBurnStats`. The Vault's 65-byte `UserGlobalBurnStats` needs 1,343,280 lamports (0.00134 XNT). Fixed.

#### ℹ️ BURN-R19 (Info, open, app scope): apps re-derive memo-burn's PDAs

memo-chat, memo-blog, memo-forum, memo-profile and memo-project check both stats accounts themselves before the CPI (24 `seeds::program = memo_burn_program` constraints): the user stats with `find_program_address` (1,500 CU per bump tried, so 3,000 CU at bump 255 and more for some users) and the global stats with `create_program_address` (1,500 CU). memo-burn checks both again. memo-home and memo-id already pass them unchecked. When the apps are next upgraded, pass both accounts through as `UncheckedAccount` (memo-burn is the authority) or at least use the stored bump. Not part of this upgrade.

#### ℹ️ BURN-R20 (Info, open): `spl-memo` without `no-entrypoint`

memo-burn uses `spl-memo` only for the Memo program ID, but depends on it without `no-entrypoint`. The deployed ELF has a single entrypoint and none of spl-memo's code, so this is harmless today; every program in the workspace does the same. A fix changes the ELF and is only worth folding into a later upgrade.

#### ℹ️ BURN-R21 (Info, open, ops): partial deploy runs must include memo-burn

`PROD-DEPLOYMENT.md` requires every app deployment run to name memo-burn and memo-mint as well, so every app release re-upgrades them with the hot key, and once they are immutable `deploy-common.sh` would abort the run (it treats an authority other than the deploy admin as a mismatch). Recommended: include them only when their ELF hash differs from `deployments/<env>.sha256`, and treat an immutable core program whose hash matches as OK. To be resolved before the upgrade authority is revoked.

#### ℹ️ BURN-R22 (Info, corrected): stale statements in this report

Corrected in place: the testing section (unit test count, committed regression suites); Appendix B (the 1,152-byte size note from the October 17 upgrade; `verify-deployment` compares the bytecode with `deployments/<env>.sha256` and does not check the IDL); the minimum-length note (memo-mint counts characters since Revision 5); the "Seeds check kept" row (memo-id and memo-home rely on memo-burn's check, memo-mint now checks constant addresses, and a constant-address check has the same strength); and the log line descriptions.

**Refuted candidates**: that the deployed builds cannot be reproduced (they reproduce bit for bit from `583d6d5c`; recording the toolchain is still advisable), and that the crate-level `#![allow(...)]` attributes hide a defect.

---

## Revision 4 Changes (November 2, 2026)

Program version 0.3.0. Accounts, instruction arguments, discriminators, error codes and the memo format are unchanged, so clients and the seven dependents need no change or rebuild.

| Change | Details |
|---|---|
| `GlobalBurnStats` by address (B1) | `process_burn` checks the account against `GLOBAL_BURN_STATS_PDA` (testnet `HY5VzmHtf3braVAz7ihJWsAfp3hyPHiycfcUbA3en26j`, mainnet `73xgyJWmEocTF5BguwZ4bJAP8irpSyFZBsooL4yoDUFa`) instead of seeds plus stored bump. A substituted account fails the owner or discriminator check first (3007 / 3002, regression B31, B31b), because memo-burn creates a `GlobalBurnStats` only at the PDA; an account with the right owner and discriminator at another address fails the address check with `ConstraintSeeds` (2006), as the seeds check did (regression B31d, against a planted shadow account). `initialize_global_burn_stats` keeps its seeds. `test_pda_constants_per_network` checks the constant against `find_program_address` under both features. |
| Success log (B3b) | `Burned {amount} units`. The user and the totals are no longer logged: the transaction names the user, and the stats accounts hold the totals. The 18T cap notice is unchanged. |
| No separate UTF-8 check (B4) | The memo bytes go straight to the Base64 decoder. Accepted and rejected memos are unchanged; a non-UTF-8 memo cannot reach memo-burn, and a non-Base64 byte still fails with `InvalidMemoFormat` (6001). |
| Comments (BURN-R10) | Corrected; no code effect. |
| Tests | 102 unit tests (testnet and `--features mainnet` builds): new `test_program_and_mint_ids_per_network`, `test_pda_constants_per_network` and `test_space_matches_serialized_layout`. |

**Compute units** (LiteSVM with X1 mainnet's Token-2022 and Memo, testnet build; the real mint's metadata extensions may add a few hundred CU to the absolute numbers, not to the deltas):

| Memo | memo-burn 0.2.0 | 0.3.0 | Whole direct burn transaction |
|---|---|---|---|
| 72 bytes (minimum) | 16,345 | 12,014 (−4,331, −26.5%) | 43,538 → 39,207 (−9.9%) |
| 764 bytes | 24,185 | 19,411 (−4,774) | 293,900 → 289,126 |
| 3,752 bytes | 57,641 | 51,015 (−6,626) | 1,374,022 → 1,367,396 |

Every burn through an app saves the same amount.

**Verification**: the regression suites passed on the Revision 4 build (local validator, testnet IDs): curve 61/61, mint 107/107, burn 66/66 (now also asserting the `Burned N units` lines on the successful burns, and B31d for the address check), home 162/162, id 181/181 at 5M and at 2.5T supply, admin-init 17/17 (including the 6 new BURN-R01 cases). The build left the sources unchanged. The revised burn smoke test passes on the local validator (simulated 47,285 CU, sent with a 47,335 limit).

**Deployment**: memo-burn is upgraded on its own (no dependent changes); the deploy scripts record the hash in `deployments/<env>.sha256`.
- Testnet (November 2, 2026): `./scripts/deploy-testnet.sh memo_burn`, upgrade signature `oc8YC1GfpGwrTk5C32D7FphCzQusBCoHzJTaFUDmzk4Eevq7aNz7hfEESrgab4x4o9peTsj1kRFAJXtAoQBa3tH`, on-chain ELF sha256 `46d526ef40fee688c3298b7ca740345e2359eb3c6f5bee5cf3e632efc78f772f` (290,544 bytes) = the local testnet build; IDL upgraded. `verify-deployment` 30/30 (including a 1 MEMO burn and the rejection of an old client without `global_burn_stats`). Smoke tests with the test wallet: memo-burn (simulated 47,067 CU), memo-home (Vault burn via CPI, first use creating the Vault's stats), memo-id (two burns via CPI) and memo-forum (burns via CPI) pass. The memo-chat smoke test reached memo-burn through `create_chat_group` and failed in Token-2022 with insufficient funds, because the test wallet does not hold the 42,069 MEMO the group needs; not a memo-burn issue.
- Mainnet: pending.

---

## Revision 2 Changes (October 13, 2026)

The instruction interface is unchanged: same accounts, same `amount` argument and same `BurnMemo` format. Error codes changed: `MemoTooLong` and `PayloadTooLong` were removed, so `UnauthorizedUser` moved from 6013 to 6011. memo-chat, memo-profile, memo-project, memo-blog and memo-forum call `process_burn` via CPI without any change (memo-home and memo-id, which also call it, came later).

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

**Behavior change integrators must know**: a successful burn now logs one line, `Burned {amount} units from {user}, user total burned: {total} units, burn count: {count}` (integer units), plus a notice when the 18T per-user cap is reached. **Superseded in Revision 4**: the line is now `Burned {amount} units`. It replaces the previous seven log lines, including `Successfully burned X tokens ...` and the payload preview. Off-chain parsers of the old lines must be updated; indexers should prefer token balance changes or inner instructions, because a transaction's logs are truncated at 10 KB.

**Memory note** (corrected, BURN-R13): the program heap is 32 KB and is not freed, and a burn holds a copy of the memo plus its decoded bytes (about 1.75x the memo length; the payload is not copied again). The heap is never the limit: the transaction size binds first on X1 (about 772 bytes of memo), and the Memo program's cost (350–376 CU per Base64 character) caps a direct burn memo at about 3.8 KB under the 1.4M CU limit, where the heap holds about 7 KB.

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
| Seeds check kept | The global account is verified by seeds and stored bump in `process_burn` and in the five apps. Without it, `Account<GlobalBurnStats>` would still check the owner and discriminator, and only one such account can exist today, so the check is defense in depth. It cost about 1,630 CU per burn. **Superseded in Revision 4** (BURN-R05, B1): `process_burn` checks the exact address against a per-network constant, which keeps the same guarantee without deriving the PDA, as memo-mint does since its Revision 5. memo-home and memo-id pass the account unchecked and rely on memo-burn's check. |
| No `init_if_needed` | The account is created once by the admin, not lazily inside `process_burn`, so burns never pay rent or carry the extra system-program account. |
| Concurrency | Every burn already write-locks the mint account, so the global account adds no new write-lock bottleneck. |

**Deploy order (required)**: upgrade memo-burn → run `admin-init-global-burn-stats` → upgrade the apps that build `process_burn` (at the time the five apps; memo-home and memo-id came later and pass the account from the start). Until the account exists, every burn (direct or through an app) fails with `AccountNotInitialized`. `scripts/deploy-common.sh` now runs the init client right after deploying memo-burn (the client does nothing if the account already exists), deploys selected programs in canonical order (memo-burn before the apps), and verifies memo-burn's `AUTHORIZED_ADMIN_PUBKEY` against the deploy admin keypair.

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

**Binding is per instruction, not per transaction** (core audit BURN-04, INT-06): `burn_amount` is compared with each `process_burn` instruction's own `amount`, and nothing records that a memo was already used. A memo with `burn_amount = X` can therefore back any number of burns of exactly X in the same transaction (top-level or via CPI, by different signers), and the same memo also satisfies memo-mint for the transaction's one mint. Burns of different amounts cannot share a memo. On-chain accounting is exact (each instruction burns and records its own amount), but indexers must not count burns by counting memos: use the Token-2022 burn inner instructions under memo-burn's program id, or balance and stats deltas (a `Burned N units` log line counts only inside memo-burn's own invoke frame, BURN-R03).

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
    // First, decode the Base64-encoded memo data. No separate UTF-8 check (Revision 4): the memo
    // program at index 0 has already rejected invalid UTF-8, and decoding rejects any byte outside
    // the Base64 alphabet with the same error
    let decoded_data = general_purpose::STANDARD.decode(memo_data)
        .map_err(|_| {
            msg!("Invalid Base64 encoding in memo");
            ErrorCode::InvalidMemoFormat
        })?;

    // check decoded borsh data holds at least the fixed header
    if decoded_data.len() < BORSH_FIXED_OVERHEAD {
        return Err(ErrorCode::InvalidMemoFormat.into());
    }

    // Check the payload length prefix against the bytes actually present (Revision 2); the
    // payload is never deserialized with Borsh, whose Vec reader would first allocate up to the
    // declared length
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
3. **Payload Safety** – The length prefix is checked against the bytes present, and the payload is never deserialized with Borsh, so a forged prefix cannot exhaust the heap (Revision 2). There is no maximum payload size; the transaction size limit bounds it.
4. **Encoding Hygiene** – Rejects anything that is not strict, padded Base64 before reading the header (invalid UTF-8 never reaches memo-burn: the Memo program at index 0 rejects it).
5. **Observability** – Every validation failure logs the reason and the values involved.

**Note on the minimum length** (core audit BURN-02, informational, no code change): in the project's design, 69 is the minimum the official client enforces on the user's input **before** Borsh/Base64 encoding (users must type at least 69 characters). On chain, memo-burn checks the raw memo instruction bytes against `MEMO_MIN_LENGTH = 69`; those bytes are the Base64 string, where bytes and characters are the same. (memo-mint counts characters since its Revision 5.) `general_purpose::STANDARD` requires padding, so valid Base64 lengths are multiples of 4 and the shortest memo memo-burn can decode is 72 characters (52–54 decoded bytes, a payload of at least 39 bytes). Memos of 69–71 characters pass the length check and then fail with `InvalidMemoFormat` (6001) rather than `MemoTooShort` (6010); memos under 69 characters fail with `MemoTooShort`. With the official client's 69-character input rule the encoded memo is always far longer than 72 characters, so this only matters to third-party integrators who build memos by hand. The effective rule is stricter than the stated one, so there is no security impact.

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
5. **Enforcement** – The seeds check binds the account to `user`: a substituted stats account fails with `ConstraintSeeds` (2006). The additional `user` field constraint can never fail, so `UnauthorizedUser` (6011) is never returned (BURN-R06); it is kept as defense in depth.
6. **Stored Bump** – Since Revision 2 the PDA is checked with the bump saved at initialization (the canonical bump), which avoids re-deriving it on every burn.
7. **Initialization Required** – Users must call `initialize_user_global_burn_stats` once before their first burn. Creating the account automatically on first burn would change the instruction accounts and break the apps that call `process_burn` via CPI, so it is kept as a separate step.

8. **Who `user` is** – the signing burn authority, which can be a program PDA: memo-home burns with the Vault PDA as `user`, so a Vault's history moves with its Asset (BURN-R17).
9. **Frozen layout** – 65 bytes, no reserved space, no close instruction; owner decision (BURN-R08): the layout stays as it is and the rent cannot be recovered.

**Verdict**: Per-user tracking is deliberate and gives a tamper-resistant record of burns per signing authority; incentive programs should attribute Vault burns to the Asset owner at burn time.

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
4. **Interface Accounts** – `InterfaceAccount` accepts accounts of either token program; Token-2022 is enforced by the fixed mint address, `token_account.mint == mint`, `Program<Token2022>` and Token-2022's own owner checks (core audit INT-07, BURN-R07).
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
- Rejects anything that is not strict, padded Base64, and any header whose length prefix does not match the bytes present, before reading it (invalid UTF-8 is rejected earlier by the Memo program).
- Requires the decoded data to hold at least the 13-byte Borsh header and checks the Borsh length prefix against the bytes present before reading, preventing memory abuse (there is no maximum size since Revision 2).

### ✅ Best Practice #2: Explicit Error Codes
- Descriptive `ErrorCode` enum covers every failure path (`BurnAmountMismatch`, `MemoTooShort`, etc.); `InvalidBurnAmount` (6005) and `UnauthorizedUser` (6011) are kept for numbering but never returned.
- Errors are surfaced with actionable guidance for integrators.

### ✅ Best Practice #3: Compile-Time Constants
- All thresholds (`MEMO_MIN_LENGTH`, `MIN_BURN_AMOUNT`, `MAX_BURN_PER_INSTRUCTION`, etc.) are constants in module scope.
- No maximum memo or payload constants: the transaction size limit is the single source of truth (Revision 2).

### ✅ Best Practice #4: Token2022 Interface Usage
- `InterfaceAccount` deserializes the mint and token account of either token program without manual parsing; the fixed mint address and `Program<Token2022>` pin Token-2022 (BURN-R07).
- The MEMO mint has only the MetadataPointer and TokenMetadata extensions. Mint extensions cannot be added to an initialized mint, so extensions such as transfer fees will not appear on it.

### ✅ Best Practice #5: Saturation over Panic
- Uses `saturating_add` to maintain liveness even when counters approach `u64::MAX`.
- Clamps totals to documented maxima instead of failing.

### ✅ Best Practice #6: Focused Logging
- A successful burn logs one integer line, `Burned {amount} units` (Revision 4; before, it also named the user and the cumulative total and count).
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
- Interface accounts accept either token program; the fixed mint address pins Token-2022 (BURN-R07).

### 5. PDA Integrity ✅ SECURE
- User stats PDA derived via fixed seed and user key.
- Bump recorded at initialization and verified through Anchor using the stored value.
- The seeds check binds the user stats to `user`; `GlobalBurnStats` is checked against its per-network address constant (Revision 4).

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
- Logs record each burn's amount; the stats accounts hold the user's and the global totals.
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
   - [ ] Run each dependent's burn path (profile, chat, project, blog, forum, home, id) against the new program.

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

`programs/memo-burn/src/tests.rs`: 102 tests in Revision 4, all passing for the testnet and `--features mainnet` builds (run both: several tests pin per-network constants). They cover:
- `validate_memo_amount`: valid memos, wrong version, amount mismatch, invalid UTF-8 / Base64, strict Base64 (padding, whitespace, URL-safe alphabet, non-canonical trailing bits), a differential test against full Borsh deserialization, payloads above the former limits
- Length prefix (Revision 2): forged `0xFFFFFFFF` prefix, prefix smaller than the bytes present, decoded data shorter than the 13-byte header
- `validate_memo_length`: empty, below/at/above the 69-byte minimum, long memos
- `check_memo_instruction`, run against real instructions-sysvar data: memo at index 0, a non-Memo instruction at index 0, process_burn at index 0, wrong sysvar account, short memo
- `GlobalBurnStats::record_burn`, including saturation
- Error code numbering
- Per-network constants: program ID, mint, admin, `GLOBAL_BURN_STATS_PDA` (Revision 4)
- `SPACE` of both accounts against the serialized struct (Revision 4)

Not unit-testable: the handler's amount bounds and user-stats cap and the account constraints; the regression suites cover them on chain. Some tests only exercise Borsh or Base64 library behaviour (core audit TEST-08, BURN-R16).

### Regression Suites (committed, local validator)

`./scripts/run-regression-tests.sh` loads X1 mainnet's Token-2022 and Memo builds (pinned hashes) and runs:
- `regression-memo-burn` (66 cases): every `process_burn` rejection path, memo position and format, account substitution (including a shadow `GlobalBurnStats` for the Revision 4 address check), memo reuse across burns, stats deltas, timestamps, and the success log lines of its successful burns
- `regression-memo-curve` (61 cases): including the exact 1T burn and the 18T per-user cap
- `regression-admin-init` (17 cases): the admin-only initialization of `MintState` and `GlobalBurnStats`, including their success paths by simulation (BURN-R01)
- `regression-memo-home` and `regression-memo-id`: the memo-home Vault burn and the memo-id burns through CPI

Gaps are listed under BURN-R15.

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
- **Version**: Revision 4 (0.3.0), re-audited November 2, 2026; deployed to testnet November 2, 2026, mainnet pending (see Deployment Status)  
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

**Remaining operational risk**: the program upgrade authority is a single hot key (mainnet `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ`, not a multisig) shared with memo-mint, the apps (chat, profile, project, blog, forum, home, id), the IDL and the token metadata, and also used for testnet staging deployments (core audit OPS-02, OPS-05). Whoever holds it can replace the program. Consider a multisig with a timelock or making the program immutable (`--final`) once the current revision is stable on mainnet.

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

memo-burn is already deployed on X1 mainnet (`2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh`) and `GlobalBurnStats` is initialized, so new versions are **upgrades**, not fresh deployments. Use `./scripts/deploy-mainnet.sh memo_burn` (name the programs explicitly) as described in `PROD-DEPLOYMENT.md`; it builds with the `mainnet` feature through the guarded `anchor_build` (`scripts/anchor-build.sh`, MINT-22), upgrades memo-burn before the apps, runs the admin init clients, upgrades the IDL and records the hash in `deployments/mainnet.sha256`. A memo-burn change that alters accounts or the burn rule must be upgraded before or together with the apps, never the apps alone (see Deployment Status); Revision 4 changes neither and can be upgraded alone.

> **Corrected (core audit OPS-03)**: earlier versions of this appendix used `anchor build ... --features mainnet` (rejected by Anchor; the feature must follow `--`), `anchor deploy ... --provider.cluster mainnet` (`mainnet` means Solana mainnet-beta in Anchor, not X1, and `anchor deploy` makes a fresh deployment at a new address instead of upgrading), read the program ID from `target/deploy/memo_burn-keypair.json` (not the real program keypair; the real keypairs are kept encrypted, see `scripts/keys.sh`) and omitted the `GlobalBurnStats` initialization. Those steps could not bypass any safety check, but they would not upgrade the real program.

1. **Build** (BURN-R02)
   ```bash
   mkdir -p target/deploy                          # without it, anchor build rewrites the mainnet declare_id! (MINT-22)
   anchor build -p memo-burn -- --features mainnet
   git diff --quiet -- 'programs/*/src/lib.rs' Anchor.toml   # must succeed on the committed release tree; otherwise stop
   cargo test -p memo-burn --lib --features mainnet          # pins the mainnet program, mint and PDA constants
   sha256sum target/deploy/memo_burn.so            # record and publish; compare again right before step 3
   ```
   Do not run the regression suites or `local-validator.sh` between steps 1 and 3: they rebuild `target/deploy` for testnet.

2. **Unlock the Mainnet Keys** (dedicated mainnet machine only)  
   `./scripts/keys.sh unlock mainnet`

3. **Upgrade** (X1 mainnet RPC, never `--provider.cluster mainnet`; the CLI extends ProgramData if a new binary is larger)  
   `anchor upgrade target/deploy/memo_burn.so --program-id 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh --provider.cluster https://rpc.mainnet.x1.xyz --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json`

4. **Initialize Global Burn Stats (only if it does not exist yet)**  
   `MEMO_PROGRAM_ENV=mainnet cargo run --manifest-path clients/Cargo.toml --bin admin-init-global-burn-stats -- 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh https://rpc.mainnet.x1.xyz`  
   (does nothing if the account already exists; it exists on testnet and mainnet)

5. **Update the On-Chain IDL**  
   `anchor idl upgrade 2sb3gz5Cmr2g1ia5si2rmCZqPACxgaZXEmiS5k6Htcvh --filepath target/idl/memo_burn.json --provider.cluster https://rpc.mainnet.x1.xyz --provider.wallet ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.json`

6. **Upgrade the Dependents if needed** (chat, profile, project, blog, forum, home; memo-id on networks where it is deployed) in the same run, only when the memo-burn change requires it, then record the hash in `deployments/mainnet.sha256` and lock the keys: `./scripts/keys.sh lock mainnet`

7. **Test Burn**  
   `./scripts/run-smoke-tests.sh memo-burn` (or `test-memo-burn 1 valid-memo`), then `verify-deployment`, which compares the on-chain bytecode with `deployments/mainnet.sha256` (it does not check the IDL)

8. **Monitor**  
   - Confirm the `Burned N units` log line appears inside memo-burn's invoke frame.
   - Inspect user and global stats.

---

## Appendix C: Client Integration Highlights

- **Required Accounts** (7, in this order): `user` (signer, writable), `mint` (writable), `token_account` (writable, owned by `user`), `user_global_burn_stats` (PDA `[b"user_global_burn_stats", user]`, writable), `token_program` (Token-2022), `instructions` (sysvar), `global_burn_stats` (PDA `[b"global_burn_stats"]`, writable, last; testnet `HY5VzmHt…`, mainnet `73xgyJWm…`). Omitting `global_burn_stats` fails with `AccountNotEnoughKeys` (3005).
- **Memo Construction**:
  1. Serialize `BurnMemo` via Borsh.
  2. Base64-encode the serialized bytes (standard alphabet with `=` padding, not URL-safe, no whitespace or newlines).
  3. Ensure the final string is at least 72 characters (69 is checked on chain, but padded Base64 lengths are multiples of 4, so 69–71 fail with `InvalidMemoFormat`) and that the whole transaction fits in 1,232 bytes.
  4. Include in instruction 0 using `spl_memo`.
- **Transaction Order**: Memo → ProcessBurn (one or more, each with `amount` equal to the memo's `burn_amount`) → optional compute budget instruction, never at index 0. Durable nonce transactions are not supported.
- **Token Accounts**: accounts with CPI Guard enabled (error 43) or owned by a multisig cannot burn through memo-burn; see Informational #4.
- **Compute Budget**: X1 charges by the requested compute-unit limit; simulate first and set the exact limit instead of relying on the 200k default.
- **Error Handling**: Surface contract error messages to end users for actionable feedback.
- **Indexing**: attribute burns per instruction (Token-2022 burn inner instructions under memo-burn, balance or stats deltas), not by counting memos or matching log text (BURN-R03); `GlobalBurnStats` covers only burns through memo-burn since its initialization. See also the integration notes in `README.md`.

---

## Appendix D: Analytics & Observability Ideas

- **Leaderboard**: Rank users by cumulative burn (using PDA data).
- **Payload Explorer**: Off-chain service to decode and display payloads by schema.
- **Alerts**: Notify when payload validation failures spike (possible client regression).
- **Supply Dashboard**: Combine mint and burn data for net supply visibility.

---

**Audit Report End**

**Audit Date**: November 8, 2025 (original), revised October 13 and 16, 2026, re-audited November 2, 2026  
**Contract Version**: Revision 4 (0.3.0)  
**Final Status**: ✅ APPROVED FOR MAINNET  
**Deployment Status (November 2, 2026)**: testnet runs Revision 4 (0.3.0, `46d526ef…`); mainnet runs 0.2.0 (upgraded October 17, 2026 to commit `c47f1eb9`; `GlobalBurnStats` initialized October 15, 2026); the mainnet upgrade is pending.  

*This report is informational only and does not constitute financial or legal advice. The auditor confirmed security properties as of the audit date.*


