# MEMO Token Contract Security Review

- **Date**: 2026-09-25
- **Scope**: the 7 Anchor 0.32.1 programs under `programs/` (memo-mint, memo-burn, memo-chat, memo-profile, memo-project, memo-blog, memo-forum), plus `Anchor.toml`, the deploy scripts in `scripts/`, and `clients/src/config.rs`
- **Method**: each program was reviewed from 3 independent angles (access control / account validation, memo parsing and CPI, arithmetic / state / economics) over 3 rounds, plus 1 cross-program / deployment reviewer. After merging and deduplicating, each finding was checked independently by 3 adversarial verifiers (exploitability, code accuracy, existing mitigations / documentation); memo-chat / project / blog / forum findings were batch-verified by 2 verifiers per program. More than 150 verification verdicts in total. Apart from one live check with `solana program show` on X1 mainnet, no on-chain testing was done.
- **Baseline commit**: `8b6707f6` (branch `dev`)

---

## 1. Overall Conclusion

**No critical or high severity vulnerabilities were found.** None of the 7 programs has fund theft, unauthorized minting, cross-user state tampering, account substitution, or reentrancy issues. The core security mechanisms at the contract level — PDA seeds / bumps, signer binding (the pubkey in the memo must equal the signer), owner / mint constraints on token accounts, CPI targets typed as `Program<'info, X>`, the Instructions sysvar address check, checked / saturating arithmetic, and account space that is never too small — held up across multiple verification rounds.

**Only 3 medium issues need fixing**, and one of them is in the deployment process, not the contract code:

1. **One hot key (deploy_admin) is the upgrade authority of all 7 mainnet programs**, as well as the in-contract admin and the token metadata authority. This is the real root of trust of the whole system: the README / audit statement "the mint authority is a PDA, so nobody can mint" no longer holds if the upgrade authority is stolen.
2. **The memo-profile update path skips field validation** (`update_profile` never calls `ProfileUpdateData::validate()`). This is the only pure code bug.
3. **memo-chat's per-group rate limit is "one message per group per interval", and anyone can take the slot and be rewarded for it.** Once a group is captured there is no remedy.

The rest are low (defense in depth, docs not matching code, integration fragility) and info (intentional design trade-offs that should be written down explicitly). In short: **access control in the contracts is of high quality; the weak points are in the economics / anti-spam design, operational key management, and documentation accuracy.**

---

## 2. Confirmed Issues

### 🟠 Medium

#### M-1. A single hot wallet holds the upgrade authority of every mainnet program (deployment / operations)

- **Location**: `Anchor.toml:36`, `scripts/deploy-common.sh:409-412`, `scripts/setup-keypairs.sh:130-137` (self-described as "combined deploy + admin authority"), `clients/memo-token/src/admin-create-memo-token.sh:88-97`
- **Current state**: a verifier checked X1 mainnet: the Authority of all 7 mainnet programs is `FVvewrVHqg2TPWXkesc3CJ7xxWnPtAkzN9nCpvr6UCtQ`, which is also the `AUTHORIZED_ADMIN_PUBKEY` compiled into memo-chat/forum/project, the Token-2022 metadata update authority, and the deployment fee payer.
- **Scenario**: `anchor deploy` needs this key online and in plaintext on the deploy machine. If it leaks, an attacker can replace memo-mint with `solana program deploy --program-id <memo_mint mainnet ID> malicious.so` and sign with the same `[b"mint_authority"]` PDA to mint 10T in one go, or replace memo-burn so that burns become no-ops.
- **Documented**: no. `PROD-DEPLOYMENT.md §8.1` only says "consider".
- **Recommendation**: once mainnet is stable, at least run `solana program set-upgrade-authority --final` on memo-mint and memo-burn, or transfer the upgrade authority to a multisig with a timelock. Split the admin key used for one-time initialization from the upgrade authority.
- **Verification**: 7/7 verifiers rated it medium.

#### M-2. memo-profile: `update_profile` does not validate fields

- **Location**: `programs/memo-profile/src/lib.rs:589-672` (`parse_profile_update_borsh_memo`, which inline-checks only version/category/operation/user_pubkey); `ProfileUpdateData::validate()` at lines `213-285` is dead code on chain. By contrast, the create path calls `validate()` correctly at line `583`.
- **Scenario**: a user creates a profile normally (burning 420), then sends an update memo with `username: Some("")`, a username of several hundred bytes impersonating someone, an image over 256 bytes, or an about_me over 128 bytes. As long as the serialized data still fits in the 614-byte account it is written, and the `EmptyUsername` / `UsernameTooLong` errors never fire. It only affects the user's own profile, but it breaks invariants every downstream UI relies on.
- **Recommendation**: replace the inline checks with a single `profile_data.validate(expected_user)?`, add the `payload.len() > MAX_PAYLOAD_LENGTH` check the create path already has, and add boundary tests for `parse_profile_update_borsh_memo` (current tests cover only `validate()` itself).
- **Verification**: confirmed by 5/5 verifiers.

#### M-3. memo-chat: the single per-group rate-limit slot can be monopolized by anyone, and groups cannot be changed

- **Location**: `programs/memo-chat/src/lib.rs:558-566` (check), `584-585` (write-back), `1283-1284` (`last_memo_time` is a single per-group field); `SendMemoToGroup` (`1155-1195`) has no member / allowlist / creator constraint.
- **Scenario**: a bot submits `[memo, send_memo_to_group(G)]` with a priority fee at every `last_memo_time + min_memo_interval` boundary. It wins the slot and also gets the memo-mint reward through CPI (the same payoff as calling `process_mint` directly, so it costs nothing or even pays). Everyone else's messages fail with `MemoTooFrequent`. A group with a 24-hour interval can be silenced with one transaction per day. The `creator` field is written but never used as an authority, and there is no update/pause/close instruction (`MEMO-CHAT-AUDIT.md` records "immutable groups" as intentional, but not the consequence that one person can take every slot).
- **Recommendation**: change the rate limit to per (group, sender), e.g. a small `[b"member", group_id, sender]` PDA; or keep the per-group throttle but give the creator instructions to change the interval, pause, or set an allowlist.
- **Verification**: verifiers rated it low to medium; listed as medium because there is no on-chain remedy at all.

### 🟡 Low

#### L-1. memo-mint: one memo backs unlimited `process_mint` calls in the same transaction; app-level minting gates can all be bypassed

- `programs/memo-mint/src/lib.rs:169-196`: it only requires `current_index >= 1` and an SPL Memo at index 0. It does not check for multiple memo-mint instructions, does not bind the signer, and does not parse the content. A transaction `[memo (69 bytes), process_mint × ~40-50]` pays the base fee only once. memo-chat's interval, memo-blog's creator-only rule, and memo-forum's gate can all be bypassed by calling memo-mint directly.
- `MEMO-MINT-AUDIT.md` explicitly says "no rate limiting is a feature, not a bug", but the tier table footnote "*Assuming single mint per transaction" shows the batching multiplier was not modeled. The repository has a `test-memo-batch-mint` client, so it appears intentional.
- **Recommendation**: decide explicitly and document it. If it is not wanted: require `current_index == 1`, or scan the sysvar and reject a second memo-mint instruction.

#### L-2. memo-mint: `#[instruction(recipient: Pubkey)]` on `process_mint_to` does not match the handler signature

- `lib.rs:88` (handler has no parameter) vs `lib.rs:306` (Accounts declares `recipient`) and `327` (`owner == recipient` constraint). Anchor 0.32.1's `try_accounts` deserializes a 32-byte `recipient` from the instruction data, but the generated IDL has `args: []`, so every IDL / Anchor CPI client necessarily fails with `InstructionDidNotDeserialize`; only the hand-written `clients/mint/src/test-memo-mint-to.rs:711` works. `recipient` is chosen freely by the caller, so the constraint is a tautology: anyone can mint to any account.
- **Recommendation**: no on-chain caller in the repository uses it, so remove it; or add `recipient` as a handler parameter and drop the meaningless constraint. Confirmed by 6/6 verifiers.

#### L-3. memo-burn (and every program that parses a user payload): Borsh allocates by the length prefix before validating it

- `programs/memo-burn/src/lib.rs:205` (`try_from_slice`) runs before `226` (payload length check). borsh 0.10.4's `u8::vec_from_reader` does `vec![0u8; len.min(1024*1024)]`, so a forged 4-byte length prefix → 1 MiB allocation → SBF heap overflow → the program aborts instead of returning `InvalidMemoFormat`. memo-blog (`793/824/863/894/936/967/1004/1034`) and others follow the same pattern. Self-harm only (the transaction fails with an opaque error).
- **Recommendation**: before deserializing, read the u32 at `decoded_data[9..13]` and reject it if it exceeds the remaining length.

#### L-4. memo-burn: length constants do not match the Base64 pipeline, making field limits in three programs unreachable

- `lib.rs:26-39, 196-200, 225-230`: 800 bytes of Base64 decode to at most 600 bytes, so the payload is at most 587; the checks against `MAX_BORSH_DATA_SIZE = 800` / `MAX_PAYLOAD_LENGTH = 787` can never fire. `MEMO_MIN_LENGTH = 69` is not a valid Base64 length; the real minimum is 72.
- Knock-on effects: memo-forum's `Post` allocates 1,125 bytes (the audit doc's 1,097 is wrong), content can never reach 512, and about 520 bytes of rent are wasted forever with no close. memo-blog allocates 797 with a real maximum of 669, and `MessageTooLong(696)` is unreachable. Space is only ever too large, so no account can be bricked.
- **Recommendation**: derive the constants from the encoded size (payload ≤ 587) and add compile-time assertions; or move the length checks after decoding.

#### L-5. memo-burn / memo-project and others: the `ComputeBudget` comment is wrong, and a wallet that prepends a CU instruction makes every transaction fail

- `memo-burn/src/lib.rs:280-281` and `memo-project/src/lib.rs:1011-1012` claim "Compute budget instructions can be placed anywhere". In fact they take an instruction index: if Phantom / Jupiter / priority-fee middleware puts `SetComputeUnitLimit` at index 0, the memo moves to index 1 and every burn, and every profile/project/blog/forum call that depends on it, fails with `MemoRequired`.
- **Recommendation**: fix the comment and state in the integration docs that "the memo must literally be the first instruction"; or relax the check to skip leading ComputeBudget program instructions.

#### L-6. memo-burn: `UserGlobalBurnStats` can be inflated at almost no cost

- `lib.rs:112-114, 149-167`: during Tier 1, a transaction `[memo, process_mint, process_burn(1 token)]` has a net token cost of 0 while `burn_count` grows by 1 and `total_burned` by 1 token. Any reputation / ranking logic built on this account can be Sybil-inflated.
- **Recommendation**: downstream consumers should treat it as an activity counter, not proof of economic sacrifice. If resistance to inflation is needed, weight burns by the mint tier at burn time, or exclude burns that share a transaction with `process_mint`.

#### L-7. memo-forum: `mint_for_post` can inflate any post's `reply_count / last_reply_time` at zero cost (actually at a profit)

- `lib.rs:595-649`, `828-831` (the memo requires burn_amount == 0), `1061-1103` (`post` is bound only by `post_id`, not to `user`). Anyone can call it on any post, with no cooldown, and one memo can back N calls in one transaction. It writes the same fields as the paid `burn_for_post`, and the event carries no amount, so the two cannot be distinguished on chain. The "economic cost prevents spam" argument in `MEMO-FORUM-AUDIT.md` does not cover the mint branch.
- **Recommendation**: require a minimum burn or a cooldown, or move the mint branch's counts into separate fields and document what `reply_count` means.
- memo-blog's `mint_for_blog` (`716-766, 748-755`) has the same pattern, but the `creator == minter` restriction means users can only inflate their own blog, so it is rated info; `last_memo_time` is written but never read.

#### L-8. memo-chat / forum / project: sequential global IDs make creations collide with each other and allow paid front-running

- chat `457-466, 1102-1109`; forum `439-448, 184-187, 959-974`; project `496-503, 1209-1216`. The client must commit to the next ID in advance and sign it into the memo and the PDA seeds. Only one transaction can succeed per counter value; the rest fail with `*IdMismatch` (before the burn CPI, so only the fee is lost, but the memo must be rebuilt and re-signed). A front-runner must actually burn each time (chat/project 42,069; forum only 1 MEMO plus rent, the cheapest).
- **Recommendation**: at minimum, retry automatically in the client. The real fix is not to make the client pre-commit to an ID (e.g. derive PDA seeds from `creator + a user-side nonce`).

#### L-9. memo-project: misused version constant + duplicated `BurnMemo` definition

- `ProjectBurnData::validate` at `lib.rs:390-393` uses `PROJECT_CREATION_DATA_VERSION` instead of a dedicated constant (test `tests.rs:360` copies the same coupling). `107-118` redefines `BurnMemo` locally instead of `use memo_burn::BurnMemo` (memo-forum `94-105` and memo-blog `102-113` do the same). Both are 1 today, so there is no impact; but a future bump of the creation schema version would silently reject every burn client.

#### L-10. memo-project: every user instruction write-locks the singleton `burn_leaderboard`

- `1218-1223, 1276-1281, 1353-1358` (create also locks `global_counter`, `1202-1207`). All traffic to the program is serialized on one account; a single creator sending `burn_for_project` back to back at 420 MEMO each can raise latency for everyone. Bounded, paid griefing.

#### L-11. memo-chat: `min_memo_interval = 0` is allowed

- `210-215` (validates 0..=86400), `505`. `Clock` does not change within a transaction, so for groups with interval > 0 a second message in the same transaction fails; but in a group with interval 0, `[memo, send_memo_to_group × N]` collects N mint rewards at once and bumps `memo_count` N times. Test `test_group_creation_data_interval_zero` explicitly asserts that 0 is valid, so this is intentional, but the consequence should be known.

### ⚪ Info (design trade-offs; recommend documenting)

- **A memo is "one proof per transaction", not "one per instruction" (all programs)**: the memo is not bound to an instruction index, signer, or nonce, and one memo can be reused by any number of burn/mint/create instructions in the same transaction. Economically harmless (each one really burns / is really signed), but indexers must key on (tx signature, instruction index) and must not treat memo text as the source of truth. Likewise, memo-burn does not parse the payload, so a memo that looks like `ProjectBurnData` / `BlogBurnData` can go on chain with a direct `memo_burn::process_burn` without passing through memo-project/blog.
- **The supply cap and tiers are computed from the circulating `mint.supply`** (`memo-mint 134-135, 225-251`): burns reopen higher tiers, and cumulative issuance is unbounded. A hard cap would need a cumulative counter PDA.
- **memo-mint / memo-burn have no on-chain admin**: all trust rests on the upgrade authority (i.e. M-1). Also, the programs cannot verify that the Token-2022 mint's `mint_authority` really is the PDA and that `freeze_authority` is empty — recommend writing the mainnet verification results for these three items, with the commands, into `PROD-DEPLOYMENT.md`.
- **`MAX_BURN_PER_TX` is really a per-instruction limit**, which does not match `MEMO-BURN-AUDIT.md`'s description "prevents a single catastrophic supply reduction".
- **Admin keys are hard-coded with no rotation and used only for one-time init**: after init, an admin key leak has zero impact; losing the key before init requires a redeploy / upgrade. `PROD-DEPLOYMENT.md` lacks the init steps for memo-forum/blog; the `admin-clear-burn-leaderboard` tool mentioned in `MEMO-CHAT-AUDIT.md` / `MEMO-PROJECT-AUDIT.md` does not exist in the source (stale docs).
- **Accounts can never be closed**: `UserGlobalBurnStats`, `Blog`, `Project`, `ChatGroup`, and `Post` have no close / transfer path, rent is locked forever, and a lost private key cannot be recovered. Mostly already recorded in the individual audit docs.
- **Names are not unique** (usernames, group names, project names, blog names) and are only validated by byte length, with no character set rules or normalization; identity is the pubkey / ID. Frontends must always show the ID too.
- `delete_profile` needs neither a memo nor a burn (documented); `total_burned` silently caps at 18T (documented); accounts with Token-2022 CPI Guard enabled cannot burn through these programs; `send_memo_to_group` is hard-coupled to a successful mint (chat stops once the 10T cap is reached).
- The deploy scripts label the mint **address** keypair as "Mint Authority" and require every deploy machine to hold its private key (`scripts/setup-keypairs.sh:187-194`); the actual mint authority is a PDA, and the key that really needs protecting is deploy_admin.
- `clients/src/config.rs:113-135, 188-220` does not cross-check `program_env` against the RPC cluster, and silently falls back to testnet when `program_env` is missing or invalid.
- The prod-staging flow deploys the mainnet program IDs to testnet; verifiers noted that the docs require running it on the production machine, so the premise "keys leak to a test machine" does not hold, and the remaining risk is small.

---

## 3. Areas Verified as Sound

- **Instructions sysvar**: the account has an address constraint, and the memo check verifies the SPL Memo program ID and the 69–800 byte range; it cannot be forged.
- **PDA and signer binding**: all profile / blog / project / group PDAs are seeded by the signer or an ID with canonical bumps; `user_pubkey / creator / burner / minter` in the memo payload must equal the signer; nobody can operate on another user's accounts.
- **Token account constraints**: `mint == AUTHORIZED_MINT_PUBKEY`, `owner == signer`, and `token_program = Token2022` are consistent on every burn/mint path.
- **CPI wiring**: `Program<'info, MemoBurn/MemoMint>` is bound to the compile-time ID, so the target program cannot be swapped; the `mainnet` feature cascades correctly to all dependencies; both `declare_id!` sets in `lib.rs` match `Anchor.toml`; `burn_amount` is validated both in the caller and inside memo-burn.
- **Cross-program memo reuse is blocked by category/operation checks** (a chat memo cannot be used for project, etc.).
- **Arithmetic**: checked/saturating everywhere, `overflow-checks = true`, compile-time assertions on tier ordering; no user-triggerable overflow panic.
- **Account space** is never too small, so accounts cannot be bricked with oversized data; parsing failures happen before any CPI or state write, so a failed transaction only loses its fee.
- **Admin instructions** all `init` a fixed-seed PDA with checks in both the constraint and the handler, and can succeed only once.
- **No reentrancy / account aliasing issues**; memo replay for `create_chat_group` and similar is blocked by the counter binding.

---

## 4. Rejected Suspicions (for reference)

- The memo-chat `reply_to_sig` log line slicing by bytes could panic — unreachable: `validate()` runs first, and bs58 decoding guarantees pure ASCII.
- A memo-project create memo is not bound to the creator and could be copied — the copier gains nothing they could not write themselves (metadata is public and not unique); no extra capability.
- "It is a gap" that a memo-blog burn memo can be consumed by `process_burn` directly — this is required by the architecture (memo-burn re-reads the same memo during CPI); clients just must not add the instruction twice.
- A `config.rs` environment mismatch could make the mint authority transfer tool compute the wrong PDA — that tool does not read `program_env` at all, so the harm path does not exist (kept as info).

---

## 5. Prioritized Fix Recommendations

1. **Upgrade authority**: make at least memo-mint and memo-burn immutable, or move them to a multisig with a timelock; split the init admin key from the upgrade key; record the on-chain verification results. (M-1)
2. **memo-profile**: call `validate()` in `parse_profile_update_borsh_memo`, add the payload length check, and add tests. A one-line change. (M-2)
3. **memo-chat**: make the rate limit per sender, or add creator management instructions. (M-3)
4. **memo-mint**: remove or fix `process_mint_to`; decide explicitly whether "one memo, many mints" is by design and document it; if not, require `current_index == 1`. (L-1, L-2)
5. **memo-forum**: add a minimum burn / cooldown to `mint_for_post`, or split the counter fields. (L-7)
6. **memo-burn**: pre-check the length prefix; fix the 787/800/69 constants and add compile-time assertions; fix the ComputeBudget comment and the integration docs; shrink the account space in the three downstream programs accordingly. (L-3, L-4, L-5)
7. **Code hygiene**: a dedicated version constant in memo-project; `use memo_burn::BurnMemo` in forum/blog/project. (L-9)
8. **Documentation**: correct the wrong statements in the `MEMO-*-AUDIT.md` files (`MAX_BURN_PER_TX` semantics, ComputeBudget, Post size 1,097→1,125, the nonexistent clear-leaderboard tool), add the forum/blog init steps, and write each info-level design trade-off up as a "Design Confirmation".
