# memo-home Design (Draft)

**HOME Of Memories Engraved**: a base-layer program that gives every application the same NFT identity and NFT-bound custody, built on the MEMO economy.

Status: v1 implemented in `programs/memo-home` (deployed 2026-10-20: testnet `J1pxwy8V…`, mainnet `ArrrsGh5…`; upgradeable until locked). Core mechanics verified on testnet with a prototype (Appendix A); the implementation, revised after the second audit (MEMO-HOME-AUDIT.md), passes 41 unit tests and 160 local-validator regression cases against X1 mainnet's Metaplex Core, Token-2022, SPL Token and ATA builds (`clients/regression/src/regression-memo-home.rs`). Last updated 2026-10-20.

---

## 0. Position in the System

memo-home is a primitive, like memo-mint and memo-burn. It has no application logic, no fees and no admin key. Applications (chat, blog, forum, third-party programs) are built on top of it by CPI, the way they are built on memo-mint and memo-burn today.

```
Application layer (open, replaceable)
  memo-chat    memo-blog    memo-xxx    third-party apps …
  Each app defines: memo format, burn thresholds, statistics, rankings, permissions.
  Each app keeps its own data in its own PDAs keyed by the Asset, e.g. [b"room", asset].
                         │ CPI
─────────────────────────┼──────────────────────────────────────────────
Base layer (locked when stable)
  memo-mint              memo-home                         memo-burn
  issuance               identity: Core Collection/Asset   burning + burn stats
                         custody:  Vault bound to the Asset
                         routing:  mining reward split
```

What memo-home does:

1. **Identity**: creates Metaplex Core Collections and Assets. Assets display and trade in ordinary wallets and marketplaces.
2. **Custody**: every Asset has a Vault. Only the current Owner can move what the Vault holds.
3. **Routing**: an app can mine MEMO into an Asset's Vault and pay a bounded share of that fresh reward to someone (for example a message sender).

What memo-home does not do: parse memos, charge fees, set burn thresholds, store app data or statistics, rank, moderate. Those belong to applications.

---

## 1. Terminology

Names follow Metaplex Core where a matching concept exists. A part with no Metaplex equivalent is named by its function.

| Term | What it is | Stored in |
|---|---|---|
| **Collection** | A group of Assets owned by one application | Core Collection |
| **Collection Config** | The Collection's authority and share cap | memo-home PDA |
| **Authority** | The address that may create and update Assets in a Collection and call `mine`. Usually an app program's PDA; it can also be a wallet. | Collection Config |
| **Asset** | One NFT ("a home" in user-facing text) | Core Asset |
| **Owner** | Current holder of the Asset, read from Core at call time | Core Asset `owner` |
| **Vault** | A PDA bound to the Asset that holds tokens (and Core Assets) on its behalf | memo-home PDA + its token accounts |
| **Vault State** | The Vault's outflow counter (`nonce`), so markets and bidders can tell whether anything left the Vault | memo-home PDA |
| **Burn Guard** | A Core Oracle that rejects every burn of an Asset in a memo-home Collection | memo-home PDA |
| **Program Authority** | The memo-home PDA that is the Core update authority of every memo-home Collection | memo-home PDA |

Naming conventions shared by all apps built on memo-home:

- **MEMO ID** (`memo_id` in code): a globally unique, user-chosen ID, owned as an NFT through the `memo-id` app (§6.4). Never use a bare `id` for it; numeric identifiers stay `group_id`, `blog_id`, and so on.
- **Name** (`display_name` in code): a free, non-unique display text kept by each app (profile, chat room, blog, project). `display_name` avoids confusion with the Core `name` field, which is the NFT title shown in wallets.

---

## 2. Decisions

| Topic | Decision | Reason |
|---|---|---|
| NFT standard | **Metaplex Core** | One 195-byte account (rent 0.00375 XNT). Displays and transfers in the X1 wallet (iOS, web) and Backpack, including Assets in a Collection with an Oracle (Appendix B). |
| Transfer hooks | Not used | No tested X1 wallet can send hook tokens. |
| Owner check | Read the Core Asset `owner` at every call | Always matches on-chain ownership; no hook or sync needed. |
| Collections | Every Asset belongs to a memo-home Collection | The Collection carries the app's authority and the Burn Guard. |
| Assets per Collection | 0 to any number, or an optional `max_assets` fixed at creation | memo-home enforces `max_assets` against Core's `num_minted` (exact, because Assets cannot be burned). Core alone cannot cap a Collection: its MasterEdition `max_supply` is display-only. Other rules (one blog per user, allowlists, schedules) stay in apps. |
| Fees and burns | **None in memo-home** | Apps decide their own economics by calling memo-burn. |
| App data and statistics | **Not in memo-home** | Apps keep them in their own PDAs keyed by the Asset. |
| Burning Assets | **Not allowed in v1** | Assets are permanent, so a Vault is never orphaned. One fixed Burn Guard for all Collections. |
| Vault contents | Any SPL Token / Token-2022 token (including transfer-hook mints), Core Assets and native XNT can be withdrawn; empty token accounts other than the MEMO one can be closed for their rent | Owner-only outflows. Not everything can leave: non-transferable (soulbound) tokens, tokens whose hook needs an extra signer or whose destination requires memos, and Assets the Vault cannot transfer stay where they are (§5.9). Nested-ownership resolution is out of scope (§5.5). |
| Vault token accounts | Created on first use, not with the Asset | Most Assets (for example MEMO IDs) never mine; this saves ≈ 0.002 XNT per Asset. |
| MEMO issuance | Only memo-mint issues MEMO | memo-home only routes MEMO that memo-mint mints in the same instruction. |
| memo-mint / memo-burn | No changes | Both stay as audited and are locked as planned. |
| Admin key | None | Every instruction is permissionless or gated by Authority / Owner. |
| Collection and Asset addresses | **memo-home PDAs**: Collection `[b"collection", authority, seed]`, Asset `[b"asset", collection, seed]`, with a 32-byte seed chosen by the caller | Core treats a Collection's own address as the update authority of its Assets, so whoever held a Collection keypair could rename any Asset, take it out of the Collection (orphaning its Vault and lifting the Burn Guard) or move it into a collection they control (audit F01). An Asset keypair would be a lifelong key for the NFT's address (F64). PDAs have no private key. The seeds also give apps deterministic addresses, e.g. `sha256(name)`. |
| Vault State | One 17-byte account per Asset, created with it and never closed; `nonce` +1 on every owner outflow | The ERC-6551 `state()` idea: a market records the nonce when an Asset is listed and requires it unchanged at the sale, so a seller cannot empty the Vault first, whatever the listing style (§5.3). It can only be added before the program is locked. ≈ 0.001 XNT per Asset. |
| Names and URIs | At most 64 and 256 bytes | Unbounded strings could push Core past its heap limit and block every transfer in a Collection (audit F04). |
| Collection Authority | **Fixed at creation** and must sign `create_collection` (an app PDA signs by CPI); no transfer or renounce (simplicity and safety, like memo-mint / memo-burn) | An app that changes its logic upgrades its program; the PDA authority stays the same. A stolen wallet Authority can spam and rewrite metadata in its own Collection but can never take it over, take Vault holdings or raise `max_share_bps`. A new app or a new wallet creates a new Collection. Trade-off accepted: a wallet Authority cannot move to another wallet, and a leaked key cannot be rotated. |
| Royalties | Optional Core Royalties plugin on the Collection, fixed at creation, not enforced | The standard field marketplaces read for Core NFTs; enforcement rule sets could block ordinary wallet transfers. |
| Memos | Not required by memo-home | Keeps cheap writes cheap (a 700-byte SPL Memo costs ≈ 0.003 XNT). Indexing uses events (§4.5). `mine` and `burn` carry memos because memo-mint and memo-burn require them. |
| Share rounding | Rounded down (amounts in base units) | Keeps the share within `max_share_bps` even at the tail of the emission curve, where one mint is 1 unit: rounding up would pay 100% to the recipient. |

---

## 3. Accounts

| Account | Seeds | Contents | Rent |
|---|---|---|---|
| Program Authority | `[b"authority"]` | none (signer only) | — |
| Burn Guard | `[b"burn_guard"]` | Anchor discriminator + `OracleValidation::V1 { create: Pass, transfer: Pass, burn: Rejected, update: Pass }` | ≈ 0.001 XNT, once |
| Collection Config | `[b"collection_config", collection]` | `collection`, `authority`, `seed`, `max_share_bps`, `max_assets` (Option<u32>), `creator`, `created_at`, `bump` (152 bytes) | ≈ 0.0019 XNT per Collection |
| Vault | `[b"vault", asset]` | none (system address that owns token accounts) | — |
| Vault State | `[b"vault_state", asset]` | `nonce` (u64), `bump` (17 bytes) | ≈ 0.001 XNT per Asset, with the Asset |
| Vault token account | ATA(Vault, mint, token program) | standard token account | ≈ 0.002 XNT per token type, created on first use |

Core accounts (created by Core through CPI):

| Account | Update authority | Notes |
|---|---|---|
| Core Collection, at the PDA `[b"collection", authority, seed]` | Program Authority | Carries the Burn Guard Oracle (Burn check, `can_reject`). memo-home exposes no instruction to add other Collection plugins. |
| Core Asset, at the PDA `[b"asset", collection, seed]` | `Collection(collection)` | Owner-managed plugins (Freeze / Transfer / Burn Delegate) can be added by the Owner as usual, which is how non-custodial marketplaces list. |

Per Asset, memo-home creates the Core Asset (0.00225 XNT rent plus Core's 0.0015 XNT create fee, which anyone can later collect for Metaplex; neither is refundable) and its Vault State (0.001 XNT): ≈ 0.0048 XNT. A Vault token account is added when the Vault first receives that token: `mine` creates the MEMO one idempotently (and it can never be closed), and anyone can create one with the Associated Token Account program before a plain transfer.

Both Core addresses are PDAs, so no key exists for them. Anyone can send lamports to a PDA before it is created, which would make Core's `create_account` fail; `create_collection` and `create_asset` first move such lamports to the payer, signed by the PDA.

---

## 4. Instructions

### 4.1 Permissionless

| Instruction | Effect |
|---|---|
| `init_burn_guard` | Creates the Burn Guard account once. Its content is fixed, so no admin is needed. |
| `create_collection(seed, name, uri, max_share_bps, max_assets?, royalty?)`, signed by the Authority | CPI Core `CreateCollectionV2` at the PDA `[b"collection", authority, seed]`, with update authority = Program Authority and the Burn Guard Oracle; creates the Collection Config. The Authority signs to consent (an app PDA signs by CPI), so a Collection naming an app as Authority was always created by that app. `authority`, `max_share_bps ≤ 10_000` and `max_assets` are **immutable**. With `max_assets`, a Core MasterEdition plugin with the same `max_supply` (plugin authority None) is added so wallets and marketplaces show the limit. Optional `royalty` (creators with shares, bps) adds a Core Royalties plugin to the Collection with plugin authority None, so it cannot change later; no rule set, so marketplaces honour it voluntarily and wallet transfers are never blocked. |

### 4.2 Authority (signer = `config.authority`)

| Instruction | Effect |
|---|---|
| `update_collection(name?, uri?)` | CPI Core `UpdateCollectionV1`, signed by Program Authority. |
| `create_asset(seed, name, uri)`, `owner` account | Rejects when `num_minted ≥ max_assets`, or when the owner is the Core program (Core would give the Asset to the payer); then CPI Core `CreateV1` at the PDA `[b"asset", collection, seed]` into the Collection (Program Authority and the Asset PDA sign) and creates the Vault State. |
| `update_asset(name?, uri?)` | CPI Core `UpdateV1`, signed by Program Authority. |
| `mine(share_bps)` → `{ reward, share }` | ⓪ create the Vault's MEMO token account if missing (the caller pays); ① CPI memo-mint `process_mint_to(recipient = Vault)`; ② reward `R` = Vault balance change; ③ `share = R × share_bps / 10_000` (rounded down), `share_bps ≤ config.max_share_bps`, sent to the given recipient token account (not the Vault's own), signed by the Vault PDA; ④ the rest stays in the Vault. Returns the reward and the share to the calling app (Anchor return data). |

### 4.3 Owner (signer = current Core `owner`, Asset not listed)

Each of these is an outflow: it adds 1 to the Vault State `nonce` and reports the new value in its event. A destination equal to the source is rejected.

| Instruction | Effect |
|---|---|
| `withdraw(amount)` | Any token from the Vault to any token account, `transfer_checked` signed by the Vault PDA; `amount = u64::MAX` withdraws the whole balance (so withdraw-all and close fit in one transaction). SPL Token and Token-2022 are both supported; for a Token-2022 transfer hook the hook's extra accounts are passed as remaining accounts (forwarded without the Vault's signature). |
| `withdraw_lamports(amount)` | Native XNT sent to the Vault address, to any address. The Vault keeps 0 or at least the rent-exempt minimum. |
| `close_token_account` | Closes an empty Vault token account (amount 0, also for wrapped XNT) and sends its rent to any address. The Vault's MEMO token account cannot be closed: its rent was paid by the first `mine` caller, and closing it would let an owner collect that rent again from every caller (audit F02). |
| `withdraw_asset` | A Core Asset owned by the Vault, transferred to any address with Core `TransferV1` signed by the Vault PDA; a separate payer covers Core's fees, and accounts the inner Asset's plugins need are passed as remaining accounts. |
| `burn(amount)` | Burns MEMO from the Vault through memo-burn `process_burn`, the Vault PDA signing as the user. On first use a payer (any signer) tops the Vault up to the `UserGlobalBurnStats` rent and memo-home initializes it, so an owner that is a program account can burn too. |

That is the whole program: 11 instructions.

### 4.4 Read-only helper crate

The `memo-home` crate (feature `cpi`) also exports functions for apps, so every app validates Assets the same way:

| Function | Returns |
|---|---|
| `owner_of(asset)` | current Core owner (checks that the account is owned by Core) |
| `collection_of(asset)` | Collection address, from the `Collection(…)` update authority |
| `is_home_asset(asset, config: &Account<CollectionConfig>)` | Asset is a Core Asset in the Collection of this Collection Config. Membership is always `CollectionConfig` + `is_home_asset`, never "update authority = Program Authority": Core lets anyone create a Collection that names the memo-home PDA as update authority without its signature (such a Collection has no Config, so memo-home never signs for it) |
| `is_listed(asset)` | a Freeze / Transfer / Burn Delegate is held by anyone but the owner, a Freeze Delegate is frozen, a permanent delegate exists, or a plugin type or authority unknown to memo-home is present (fail closed). Sound only for memo-home Assets: it ignores Collection-level and external plugins, which memo-home Collections never get beyond the Burn Guard, MasterEdition and Royalties |
| `require_owner_not_listed(asset, config, signer)` | the gate of every owner outflow, for apps acting for an Asset's owner: a memo-home Asset in this Collection, owned by `signer`, not listed |
| `vault_of(asset)`, `vault_token_account(asset, mint, token_program)` | Vault PDA; its token account for a mint |
| `vault_state_of(asset)`, `vault_nonce_of(vault_state, asset)` | Vault State PDA; its outflow nonce, checked to belong to the Asset |
| `collection_address(authority, seed)`, `asset_address(collection, seed)`, `collection_config_of(collection)` | the PDAs |
| `share_of(reward, share_bps)` | the `mine` share, rounded down |
| `parse_asset`, `parse_collection`, `asset_data_is_listed` | raw parsers for trusted bytes only (the caller checks that the account is owned by Core) |

Integrating:
- Load the Config as `Account<'info, memo_home::CollectionConfig>`: Anchor then checks that memo-home owns it, and memo-home creates Configs only at their PDAs. Never deserialize a Config from an unchecked account.
- An app that serves only its own Collections also checks `config.authority` (its PDA) or the Collection address.
- Add `"memo-home/mainnet"` to the app's own `mainnet` feature. memo-home fails to compile when memo-mint or memo-burn is built for mainnet without it.
- Payments to an Asset go to `vault_of(asset)` (XNT) or `vault_token_account(asset, mint, token_program)`, never to the Asset address itself: Core lets anyone collect the lamports above rent of a Core account, and tokens sent to the Asset's own token accounts have no owner who can move them.

### 4.5 Events

memo-home emits Anchor events for indexers: `CollectionCreated { collection, authority, seed, … }`, `CollectionUpdated`, `AssetCreated { collection, asset, seed, owner }`, `AssetUpdated`, `Mined { collection, asset, recipient_token_account, reward, share }`, and for the outflows `Withdrawn`, `LamportsWithdrawn`, `TokenAccountClosed`, `AssetWithdrawn`, `VaultBurned`, each with the new Vault State `nonce`. Events are program logs: they are hints for indexers, which must check that a log comes from memo-home's own invocation, and they are not proof. Balances and the nonce are read from chain.

### 4.6 Memos

memo-home does not read or validate memos itself. `mine` and `burn` inherit memo-mint's and memo-burn's rules: an SPL Memo at index 0 (≥ 69 bytes; for burns, `BurnMemo` with `burn_amount` equal to the amount). Apps define and validate their own memo formats.

---

## 5. Security

### 5.1 Invariants

1. **Apps cannot touch what a Vault holds.** The only outflow an Authority can trigger is the `mine` share, which comes from MEMO minted in the same instruction and is at most `max_share_bps` of it. Through memo-home, everything else in a Vault moves only by the Owner's `withdraw`, `withdraw_lamports`, `close_token_account`, `withdraw_asset` or `burn`. Outside memo-home, the powers that a token or Asset carries itself still apply: a mint's permanent delegate, freeze, pause, fee or hook authority, and an inner Asset's permanent delegates (§5.9).
2. **Ownership is live.** Owner-gated instructions check that the Asset is owned by the Core program, is in the Collection of the given Collection Config, and that its `owner` equals the signer. After a transfer the new Owner controls the Vault immediately.
3. **Listings stop ordinary withdrawals; buyers check the Vault State.** Owner outflows fail while the Asset is escrowed (the seller is not the owner) or `is_listed`, checked when the outflow runs. A seller can still end any listing and recreate it in one transaction (through the market's own delist for escrow and frozen listings, by revoking the delegate for unfrozen ones), and offers and bids are filled by a transaction the seller builds. Every such drain adds to the Vault State `nonce`, so a purchase or bid that requires the nonce recorded at listing is protected in every listing style (§5.3).
4. **Vaults are never orphaned.** Burning is rejected for every Asset (Burn Guard), and no key can take an Asset out of its Collection: the Collection address is a memo-home PDA, and memo-home only ever renames.
5. **Collection membership is trustworthy.** Only the Program Authority can add Assets to a memo-home Collection, and memo-home does so only when the Collection's Authority signs and `max_assets` is not reached. memo-home never adds delegates or plugins to a Collection or changes an Asset's update authority, so there is no other path; after locking, a supply limit is enforced without trust.
6. **No privileged keys.** No admin, no global configuration, no fee switch, and no private key for any Collection or Asset address. After locking, behavior cannot change. This holds fully once memo-mint and memo-burn are locked too, since memo-home calls them (memo-burn with the Vault's signature); lock them first.

Not an invariant: MEMO in a Vault did not necessarily come from `mine`; memo-mint's `process_mint_to` is permissionless, so anyone can mint into any Vault's MEMO token account. Apps must not treat a Vault balance as mining history.

### 5.2 What an Authority can and cannot do

| Can | Cannot |
|---|---|
| create Assets in its Collection | take tokens or Assets out of any Vault |
| rename Assets and change their URI | change an Asset's Owner, freeze it or burn it |
| mine into Vaults and pay up to `max_share_bps` of the fresh reward to anyone | raise `max_share_bps` |
| | hand the Collection to another Authority, or add Core plugins to the Collection or its Assets |

A compromised Authority (for example a leaked wallet key) can create spam Assets and rewrite names and URIs in its own Collection. It cannot steal. Apps that need metadata to be owner-controlled check `owner_of` themselves before calling `update_asset`.

### 5.3 Listing protection

| Marketplace style | Owner while listed | Protection |
|---|---|---|
| Escrow | market PDA | automatic: the seller no longer passes the owner check |
| Non-custodial, frozen (market holds a frozen FreezeDelegate) | seller | protected: the owner cannot revoke or remove a frozen FreezeDelegate, so `is_listed` blocks Owner outflows until the market unfreezes (verified, H81). Core resets the delegates on the sale, so the buyer can use the Vault at once (H82). |
| Non-custodial, unfrozen (market holds only a Transfer / Burn Delegate) | seller | `is_listed` stops ordinary withdrawals. In one transaction the owner can still revoke the delegate, withdraw and approve it again (verified, H80); the Vault State nonce shows it (H80b). |
| Offers, bids, AMM pools, loan offers | seller | the seller builds the fill transaction, so only a check inside the fill protects the maker. Collection-wide and pool bids should value Vault contents at zero. |

The protection for buyers is the **Vault State nonce**: a market records `vault_nonce_of(vault_state(asset), asset)` when the Asset is listed (or a bidder when it bids on a single Asset) and requires the same value in the purchase or fill instruction. Any withdrawal, close, `withdraw_asset` or burn in between raises it, whatever the listing style. Inflows (`mine`, deposits) do not change it. Nested memo-home Assets have their own Vault States, so a check covers a nested Asset's Vault only when it checks that Asset's nonce too.

A buyer can also value only what the purchase transaction itself checks (balances of the Vault token accounts found off-chain, the Vault's XNT, the owners of inner Assets), and should value at zero holdings whose mint has a permanent delegate, a pause or freeze authority, and inner Assets with permanent delegates.

ERC-6551 token-bound accounts have the same limit without a commitment, and its standard leaves fraud prevention to marketplaces ("Preventing fraud is outside the scope of this proposal"); its `state()` counter is the model for the Vault State. memo-home cannot see listings and Core transfers do not pass through memo-home, so the check belongs to the market or bid program.

### 5.4 Asset Signer

Core's Asset Signer (`ExecuteV1`) is a separate address that the Owner can make sign anything. memo-home cannot protect it. UIs show the Vault as protected and the Asset Signer as not. memo-home instructions that call Core (`create_*`, `update_*`, `withdraw_asset`) cannot run under `ExecuteV1` at all, because Core cannot be re-entered.

Verified on testnet: a Core FreezeDelegate freeze blocks transfers but not `ExecuteV1`, so an Owner can empty the Asset Signer while the Asset is frozen for a non-custodial listing. Delegates cannot call `ExecuteV1`.

### 5.5 Nesting

- A Core Asset sent to a Vault is owned by the Vault PDA, and the outer Owner can take it out with `withdraw_asset`.
- If that Asset is itself a memo-home Asset, its own Vault is frozen while it is nested: its owner is a PDA that memo-home never signs for as an Owner. It works again once it is withdrawn.
- Cycles (A in B's Vault and B in A's Vault) can be created with plain Core transfers, outside memo-home, and lock both forever. UIs warn before sending a home into a Vault. Resolving ownership through nesting chains is later scope.

### 5.6 CPI depth

Solana allows an instruction stack height of 5. `app → memo-home → memo-mint → Token-2022` uses 4, so an app can itself be called by one more program, but not by two (verified with a test app, H133). `create_asset` reaches Core and then the System program: `app → memo-home → Core → System` is 4, `blog → memo-id → memo-home → Core → System` is 5, the limit. Apps that need deeper composition call memo-home from the transaction's top level.

### 5.7 One mint per transaction

memo-mint allows one mint per transaction, so a transaction contains at most one `mine` across all apps.

### 5.8 Signer scope and wallet previews

A wallet owner's signature on any instruction, at any CPI depth, lets that program call every owner instruction for every Asset the wallet owns: it can empty all their Vaults and transfer the Assets themselves, exactly as it can move the wallet's own tokens. Wallet balance previews may not show it, because Vault token accounts are not the wallet's. A check in memo-home that the owner signs at the top level would not help (a malicious program can still transfer the Asset itself) and would break program-owned Assets, so there is none.

Front ends (the first-party home UI now, and as a request to the X1 wallet team): resolve every key of a transaction, including lookup tables; for each signer-owned memo-home Asset derive `vault_of(asset)` and its token accounts; when any of them is writable, simulate and show the before/after balances, and warn "this transaction can empty the Vault of <Asset>".

### 5.9 Deposits and what cannot leave a Vault

- Send XNT to `vault_of(asset)` and tokens to `vault_token_account(asset, mint, token_program)`, never to the Asset address: Core lets anyone collect the lamports above rent of a Core account to Metaplex, and the Asset address (a PDA) has no owner who can move tokens.
- Do not derive an ATA from a token account address (a "nested" ATA of a Vault token account): memo-home has no `RecoverNested`, so tokens sent there are lost.
- These cannot leave a Vault: non-transferable (soulbound) Token-2022 tokens; tokens whose transfer hook needs an extra signer (forwarded accounts never get a signature); withdrawals to destinations that require incoming memos (Token-2022 MemoTransfer), and `mine` shares to such recipients; Core Assets whose own rules block the transfer. UIs warn before sending these to a Vault.
- Token-level powers stay with their holders: a permanent delegate can move or burn Vault tokens, a freeze or pause authority can block them, a fee authority takes transfer fees, a hook can reject transfers. Value such holdings accordingly.

### 5.10 General

Follow repo conventions: checked arithmetic, mainnet/testnet IDs by cargo feature, unit and local-validator regression tests, an audit document before mainnet, and immutability once stable (like memo-mint and memo-burn).

---

## 6. How Applications Build on memo-home

An app is an Anchor program with a PDA, e.g. `[b"app_authority"]`, that is the Authority of its Collection(s): it signs `create_collection`, `create_asset`, `update_*` and `mine` by CPI. Its instructions validate the memo and its own rules, update its own PDAs, and CPI into memo-home, memo-mint and memo-burn. `clients/regression/test-programs/home-app` is a minimal example (create a Collection and Assets, mine, and withdraw from an Asset the app owns), exercised by the regression suite.

### 6.1 Chat

```
Collection "Chat Rooms" (Authority = memo-chat PDA, max_share_bps = 10_000)
  create_room   app burns MEMO via memo-burn (anti-spam), then home.create_asset(owner = creator)
  send_message  app validates the memo, updates [b"room_stats", asset], then home.mine(share_bps = 7000)
                → sender gets 70% of the mint, the room's Vault keeps 30%
  burn_for_room app burns from the user via memo-burn, updates its leaderboard
  moderation    app PDA [b"mods", asset], editable only by owner_of(asset)
```

Selling the room transfers the Vault, its future income and moderation rights to the buyer. Messages and statistics stay with the Asset.

### 6.2 Blog

```
Collection "Blogs" (Authority = memo-blog PDA)
  create_blog   app PDA [b"blog_of", user] enforces one blog per user, then home.create_asset
  post          memo content, app stats in [b"blog", asset]
  mint_for_blog home.mine(share_bps = 0) → the whole reward goes to the blog's Vault
```

### 6.3 Single-piece or limited works

A Collection with one Asset, or an app counter that stops `create_asset` at a maximum supply.

### 6.4 MEMO ID (`memo-id`)

A MEMO ID is a globally unique, user-chosen ID such as `@xen-artist` or `memo.rip`. It is an NFT, so it can be transferred, listed and sold. It can point to a wallet or to any Asset (a profile, chat room, blog or project NFT). The non-unique display text is the app's `display_name`, as on X (`@xen-artist` vs. "Xen Artist").

#### Terms

| Term | Meaning |
|---|---|
| `memo_id` | The full ID string, e.g. `demo.memo.rip` |
| **MemoId** | A MEMO ID backed by its own Asset |
| **SubId** | A MEMO ID under a parent, e.g. `demo.memo.rip` under `memo.rip`; either a record or an NFT |
| **Target** | What a MEMO ID points to: `Address(pubkey)` or `Asset(asset)` |
| **Primary ID** | The reverse record: "the MEMO ID of this wallet / this Asset is …" |

#### Structure

- One memo-home Collection per suffix (e.g. `x1`, `rip`), Authority = the memo-id PDA, at `collection_address(memo-id PDA, sha256(suffix))`. Suffixes are created by the app.
- Each MemoId Asset is created with seed `sha256(memo_id)`, so its address follows from the ID and a second Asset for the same ID cannot exist in the Collection.
- Registry PDA `[b"memo_id", sha256(memo_id)]`, one per full `memo_id`, which makes it globally unique:
  `memo_id`, `kind` (Nft / Record), `asset` (for Nft), `parent`, `target`, `target_set_by`, `created_at`, `bump`.
- Primary ID PDA `[b"primary_id", target]`: `memo_id`, `set_by`, `bump`.
- Format: `a-z 0-9 -` (DNS hostname rules: no leading or trailing `-`), stored lowercase; at most 63 characters per label and 253 in total. No `_` and no Unicode, so every MEMO ID maps to a gateway subdomain (`xen-artist.x1.<gateway>`) and has no look-alike characters.
- A MEMO ID is permanent: no expiry and no renewal, because memo-home Assets cannot be burned or reclaimed. Registration burns MEMO through memo-burn, priced by length in mints (short IDs cost more).

#### Controller and verification

- **Controller** of a MEMO ID: `owner_of(asset)`. If the MemoId Asset is held by another Asset's Vault, the controller is `owner_of` that Asset (one level only). This lets a MEMO ID be sold together with a chat room or project by putting it in that Asset's Vault.
- **Forward** (`memo_id` → Target) is set by the controller and stamped with `target_set_by`. It is void when the controller changes.
- **Reverse** (Primary ID) is set by the Target's controller: the wallet itself for `Address`, or `owner_of(asset)` for `Asset`. It is void when that controller changes.
- **Verified** = forward and reverse match and neither is void. Apps display only verified MEMO IDs. Consequences:
  - pointing `scam.x1` at someone else's room shows nothing until the room's owner confirms;
  - after a room is sold, its MEMO ID is unverified until the new owner confirms;
  - a seller cannot redirect a MEMO ID just before a sale, because the forward record is void once the buyer is the controller.
- Default resolution when no forward record is valid: the MEMO ID resolves to its controller's wallet.
- One wallet or Asset can be the Target of several MEMO IDs; its Primary ID chooses which one is shown as `@handle`.

#### SubIds

```
① Record SubId (default)
   created by the parent's controller; not an NFT; ≈ 0.0015 XNT rent, refunded on delete
   controlled by the parent's controller; moves with the parent; editable and deletable
        │  issue (one way)
        ▼
② NFT SubId
   a MemoId Asset given to any owner; transferable and listable
   the parent can no longer revoke it
```

- Only NFT MEMO IDs (suffix level, second level, issued SubIds) can have SubIds, so deleting a Record SubId never cascades.
- Records and NFTs share the registry PDA, so they cannot collide.
- Optional public sale: the parent's controller sets a price, and anyone can buy an NFT SubId. The payment goes to the parent's Vault (a plain token transfer).
- Indexers show how many NFT SubIds a parent has issued, so buyers of the parent can see it.

#### Composition

- An app can register a MEMO ID in the same transaction that creates its Asset: `blog → memo-id → memo-home → Core → System`, 5 levels, the limit (§5.6).
- Records name the target kind explicitly (`Address` or `Asset`). An `Address` target is confirmed by that address's signature, an `Asset` target by `owner_of(asset)`. An Asset address is a memo-home PDA and can never sign, so nobody can act "as the Asset" in memo-id or any other program.
- memo-profile will be rebuilt on memo-home and memo-id: the profile keeps a non-unique `display_name`, and uniqueness comes from MEMO ID. The current `username` field (≤ 32 characters, not unique) is already a display name.

#### memo-home requirements

Covered by the base layer: `create_asset`, `owner_of`, `is_listed`, `withdraw_asset` (MemoIds held in Vaults), and Vault token accounts created on first use. No new memo-home instruction is needed.

---

### 6.5 MEMO Web (`memo-web`)

A website is an Asset. It can be bound to a MEMO ID, transferred and sold, and visitors can tip it and like it. One website is a **Site** in code.

| Feature | How | Building blocks |
|---|---|---|
| Site as an NFT | Collection "Sites", Authority = memo-web PDA, `home.create_asset` | memo-home |
| MEMO ID binding | MEMO ID Target = `Asset(site)`, Primary ID confirmed by the Site's owner | memo-id |
| Mining tip | a visitor sends an SPL Memo; memo-web calls `home.mine(share_bps)` with a small or zero share for the visitor; the reward goes to the Site's Vault | memo-home + memo-mint |
| Direct tip | plain MEMO transfer to the Site's Vault | memo-home |
| Like | the visitor burns MEMO through memo-burn; memo-web updates `[b"site_stats", asset]` (likes, burned total) and an optional leaderboard | memo-burn |
| Content | `[b"site", asset]` stores a manifest pointer: `memo://`, `ar://`, `ipfs://` or `https://`, plus an optional content hash | memo-file (§6.6) |

Tips go to the Vault, so accumulated income is sold together with the Site.

### 6.6 Storage (`memo-file`, `memo://`)

A shared module for any app that stores files for an Asset (sites, blogs, forum posts, avatars, project pages). Each file belongs to one Asset; only `owner_of(asset)` writes, and never while the Asset is listed (otherwise a seller could delete content and reclaim rent just before a sale).

All file content is stored in accounts, so it is readable from chain state for as long as the chain exists, with no archive RPC or indexer. Short text that apps already put in SPL Memos (chat messages, posts) stays there; memo-file is for files.

| Account | Seeds | Content | Lifetime |
|---|---|---|---|
| Block | `[b"block", sha256(data)]` | up to 9 KB of data (one account creation, under the ≈ 10 KB growth limit per instruction), written ≈ 1 KB per transaction; `finalize` checks the SHA-256 on-chain | immutable after `finalize`; never closed, because any file may reference it. Unfinished Blocks can be cleared (below) |
| Private Block | `[b"private_block", asset, sha256(data)]` | same as Block, but belongs to one Asset | the Asset's owner may `close` it and get the rent back; for data only that Asset uses (Machine overlays, drafts, old saves) |
| File | `[b"file", asset, sha256(path)]` | size, content type, current version, root = hash of the block list (small files up to 4 KB inline instead) | owner updates in place; `close` refunds rent unless sealed |
| File Version | `[b"file_version", file, version]` | block list, size, hash, created_at | permanent once written |

- **Content addressing**: identical blocks are stored once on the whole chain. A package used by many Machines, or an unchanged part of a new version, costs nothing extra. Anyone may pay to write a block; it is only usable by content hash, so it cannot be forged.
- **Versions**: a new version writes only changed blocks plus one File Version record.
- **Shared or private**: shared Blocks are for published content (Images, Packages, games, sites) and can never be closed. Private Blocks are for an Asset's own changing data and can be closed to refund rent; a File Version that points to a closed Private Block can no longer be read, and clients warn before closing. Sealed Files and published versions (§6.7, §6.8) may use shared Blocks only.
- **Cost**: ≈ 7.3 XNT per MB of new blocks (rent, 0.00713 XNT per KB; the fixed cost per account adds ≈ 1.3% at 9 KB). Clients compress files before splitting them into blocks.
- **Session keys**: one upload of 1 MB is about a thousand transactions. The owner signs `register_session(key, ttl)` once; the session key may then write and finalize Blocks and add File Versions for that Asset until it expires or `revoke_session` is called. It can never close, seal, transfer or withdraw. The session key's lamports for fees and rent come from a budget the owner sets; unused funds return to the owner.
- **Clearing unfinished uploads**: because Blocks are addressed by content, a squatter could start a Block for a hash and never finish it, blocking that content for everyone. Anyone may close a Block that is unfinished and idle for about 30 days; the rent goes back to whoever paid it, and the hash is free again. The same rule applies to unfinished Files.
- **Rent top-up**: anyone may add lamports to a Block, File or File Version, in case rent rules change.
- **Public read specification**: the account layouts and a short reference reader (fetch the File, its File Version, the Blocks, check every hash) are published with the program, so files can be read from any X1 RPC node without our gateway or library.

Address format:

| Form | Example | Use |
|---|---|---|
| Canonical (by Asset) | `memo://<asset address>/index.html` | stable references between programs |
| Readable (by MEMO ID) | `memo://xen-artist.x1/index.html` | for people; resolves through memo-id, so it follows the MEMO ID |
| Version | `memo://xen-artist.x1/post.md?v=3` | a specific File Version; latest if omitted |

External content (`ar://`, `ipfs://`, `https://`, and X1 VAULT files as `x1vault://<file address>`, checked by their BLAKE3 hash) is only referenced; a content hash in the manifest lets readers verify it.

**Sealing.** `seal(path)` makes a File permanent: no more versions, writes or closes. Blocks and File Versions are permanent anyway. Apps that sell or rank content (games in §6.7) reference File Versions or sealed Files with their hashes, so an owner cannot change what was bought or ranked.

Reference: X1 VAULT (x1vault.com, X1 mainnet program `34JxpA5L…`) already stores sealed files in 9 KB chunk accounts addressed by BLAKE3 hash, with session keys, cleanup of stale uploads and a public read format. memo-file differs in binding files to an Asset owner instead of a wallet, sharing Blocks between files and versions, and having no admin key, fees or reward pool.

#### Opening `memo://` in browsers and wallets

Browsers do not resolve unknown schemes, and web pages can only register `web+memo://`. Plan:

1. **HTTPS gateway** (works everywhere): `https://xen-artist.x1.<gateway domain>` serves `memo://xen-artist.x1/`.
2. **Open-source resolver library + browser extension**: reads and verifies content from RPC directly, so users do not have to trust the gateway.
3. **Wallet in-app browsers**: the X1 wallet can recognize `memo://` in its address bar using the same library.

Requirements:

- **One origin per Site**: the gateway, extension and wallets must serve each MEMO ID on its own origin (subdomain). Wallet connection approvals are per origin; a shared origin would let one approved Site act for all.
- **Verification labels**: "verified on-chain" for `memo://` content and matching hashes, "external, unverified" otherwise.
- **Moderation**: on-chain content cannot be deleted. The gateway needs a blocklist and a report process before launch.

#### memo-home requirements

None beyond the base layer: `create_asset`, `mine`, Vault token accounts created on first use, `owner_of`, `is_listed`.

### 6.7 Consoles (`memo-console`)

A console is an app program that defines a game runtime, like a fantasy console (PICO-8, TIC-80). `memo-console` ("MEMO Console") is the reference console. Anyone can deploy another console program for other game types; each console owns its own Collection(s) through memo-home.

#### Terms

| Term | Meaning |
|---|---|
| Console | a program plus a runtime spec (screen, palette, sound, memory, script language, cartridge limit) |
| Runtime | the emulator that plays games of one console, stored with memo-file so it is served from the chain |
| Game | an Asset in a console's Collection: code and resources stored with memo-file |
| Game version | a published, immutable snapshot of a Game's files |

#### Decisions

- **Games run off-chain.** One X1 transaction allows at most 1.4M CU, which is not enough for one frame. The chain stores games, the runtime, ownership and tips.
- **The Game NFT is the copyright and revenue right.** There is one NFT per game. Anyone can play for free; tips go to the Game's Vault and belong to its current owner. Selling the NFT sells future tips and the right to publish new versions.
- **No console NFT.** The console is a program, not an Asset. A console may add its own Asset types later (machine skins, profiles); they are not required to play.
- **Deterministic runtime.** The same inputs always give the same result: fixed-point arithmetic, no wall clock, randomness only from a seed. This keeps replays verifiable for a later leaderboard.
- **No on-chain leaderboard in v1.**
- **No cartridges in Vaults.** A Game in another Asset's Vault would freeze its own Vault (§5.5). A "library" is a client-side view of the Games a wallet owns or has played.

#### Reference runtime (MEMO Console v1)

Modelled on PICO-8: 128×128 pixels, 16-colour palette, 4-channel sound, Lua subset, fixed cartridge size limit (to be set, see §9). Exact numbers are part of the console spec, versioned with the runtime.

#### Structure

| Account | Seeds | Content |
|---|---|---|
| Console Config | `[b"console"]` | Collection, runtime URI and hash (`memo://<console asset>/runtime.js`), spec version, max cartridge size |
| Game | `[b"game", asset]` | asset, spec version, entry path, latest version, created_at |
| Game Version | `[b"game_version", asset, version]` | version, list of (path, File Version, hash), published_at |
| Game Stats | `[b"game_stats", asset]` | tips, likes (memo-burn), plays reported by clients are not stored on-chain |

Instructions:

| Instruction | Who | Effect |
|---|---|---|
| `create_game(name, uri)` | anyone | memo-home `create_asset` in the console Collection, owner = creator |
| `publish_version(files)` | Game owner, not listed | records memo-file File Versions with their hashes; rejects files larger than the spec limit |
| `tip(share_bps)` | anyone | memo-home `mine` into the Game's Vault, tipper share ≤ the Collection's `max_share_bps` |
| `like(amount)` | anyone | memo-burn, updates Game Stats |

#### Game Manifest standard

Every console writes the same manifest fields, so any gateway, wallet or launcher can open games from any console: `console` (program id), `spec` (name and version), `runtime` (URI and hash), `entry`, `version`, file hashes. A launcher loads the runtime, checks every hash, then starts the game in its own origin (§6.6).

#### Addresses

`memo://tetris.x1/` opens the latest version through the gateway; `memo://tetris.x1/?v=3` a specific one. A MEMO ID attaches to the Game Asset as usual (§6.4).

#### memo-home requirements

None beyond the base layer: `create_asset`, `mine`, `owner_of`, `is_listed`, Vault token accounts created on first use. memo-file needs `seal` (§6.6).

### 6.8 Linux in the browser (`memo-os`)

`memo-os` is a console (§6.7) whose runtime is a RISC-V emulator (TinyEMU class). It runs real Linux in the user's browser, boots it from the chain, and gives it AI through the browser. Linux cannot run on-chain: booting takes billions of instructions, one transaction allows 1.4M CU, programs cannot start themselves (every step needs a signed transaction), and they cannot use clocks, randomness or the network.

#### Terms

| Term | Meaning |
|---|---|
| Image | a bootable system (kernel, initrd, root filesystem) as an Asset with sealed files |
| Package | installable software as an Asset with sealed files, with dependencies on other Packages |
| Machine | a user's system as an Asset: a base Image plus the owner's overlay of changed disk blocks |
| Host bridge | a virtual device between the guest and the browser that hosts it |

For software we write, the Image or Package NFT is the copyright and tip revenue. For upstream software (Linux, BusyBox, Alpine packages) it only marks the publisher of that build; tips go to the publisher.

#### Images

- **Architecture: RISC-V (riscv64).** Current 64-bit distributions support it (Alpine, Debian), the emulator is small, and it matches later verifiable execution (Cartesi, RISC Zero). x86 is kept for a separate retro console (`memo-dos`: DOS and open-source DOS games), not for memo-os.
- **Default Image: Buildroot**, a few MB: BusyBox shell, editor, Lua or MicroPython, the `ai` command, a few small programs. Boots in seconds.
- **Extended Image: Alpine riscv64** with `apk` reading Packages from the chain.
- **Licences**: GPL binaries ship with their source (as memo-file files, or a source URI with hash in the manifest). Only redistributable software is published as Packages; proprietary tools (for example Claude Code) are installed by users themselves once networking exists. Upstream trademarks are not used as Image names or MEMO IDs (`memo-os-base`, "based on Alpine").

#### Boot from the chain

1. A small open-source loader (gateway page, extension or wallet browser, §6.6) reads the memo-os runtime from memo-file and checks its hash.
2. The runtime loads the kernel and initrd and checks their hashes.
3. The kernel starts. Root filesystem blocks load on demand, as they are read.
4. The Machine's overlay is applied over the Image.

Storage: everything is in memo-file Blocks (accounts), so booting needs only chain state, not an archive. The loader reads Blocks with `getMultipleAccounts` and checks each hash, so an RPC node cannot alter content, only refuse it; the loader supports several RPC endpoints. Images are kept small and compressed because every new MB costs ≈ 7.3 XNT; Packages and unchanged blocks are shared between Images and Machines.

#### AI through the host bridge

The guest's `ai` command (an open-source Package) sends requests over the host bridge; the browser calls the model API (Anthropic, xAI or others) and returns the answer to the terminal.

- **API keys live only in the browser's local storage.** They never enter the guest disk, an overlay, a snapshot or the chain, so selling a Machine never sells a key.
- The browser asks the user before a guest program first uses the bridge, and shows usage, so a malicious Package cannot spend the user's quota silently.
- The bridge is faster than running a heavy client inside the emulator and needs no network relay.
- `ai` can load an AI character's memory from memo-file (a later app), so a chain-hosted computer talks to a character whose memory is also on-chain.

#### Running, saving, resuming

- **Runs only while the page is open.** The chain stores the Machine; the user's computer runs it. Always-on Machines need a host (a hosted service, or later verifiable execution) and are out of scope.
- **Offline**: after the first load, the runtime and blocks are cached by hash (installable web app with persistent storage); a "download for offline" option fetches the whole Image. Running needs no network; saving, tipping and new Packages do.
- **Local autosave**: changed disk blocks and a full emulator snapshot (RAM, CPU, devices) are saved locally every few seconds, so a crash or power cut resumes where it stopped. Snapshots are tied to the runtime version.
- **Chain save**: `save` writes only changed disk blocks, as Private Blocks of the Machine, never RAM snapshots by default. Deleting an old save closes its Private Blocks and refunds their rent. Each save names the overlay version it is based on and fails if the chain has moved on (two devices); the user then overwrites, drops, or saves as a new Machine.
- **Everything saved is public.** Machine disks are readable by anyone, and selling a Machine sells its disk. The client warns before saving files that look like keys or private data, unless the overlay is encrypted client-side.

#### Network

No general network in v1 (the AI bridge covers the main need). Browsers only allow raw network access through a WebSocket relay, which someone must operate and which can be abused; a later version may allow allowlisted relays with rate limits.

#### Structure

| Account | Seeds | Content |
|---|---|---|
| Image | `[b"image", asset]` | arch, boot files (paths, hashes), root filesystem block list hash, source reference, version |
| Package | `[b"package", asset]` | arch, file list hash, dependencies (Package Assets), source reference, version |
| Machine | `[b"machine", asset]` | Image asset and version, installed Packages, overlay version and block map hash, saved_at |

Instructions: `create_image`, `create_package`, `publish_version` (sealed files, owner, not listed), `create_machine(image)`, `save(base_version, overlay blocks)` (Machine owner, not listed), `tip` (memo-home `mine`), `like` (memo-burn).

#### memo-home requirements

None beyond the base layer. memo-file needs: `seal`; files larger than one account (chunked); content-addressed blocks shared across Assets (one copy of a package for every Machine); block-level overlays.

## 7. Costs (X1, measured on testnet)

| Item | Cost |
|---|---|
| Asset: Core Asset rent 0.00225 XNT + Core create fee 0.0015 XNT (collectable by anyone for Metaplex; neither refundable) + Vault State rent 0.001 XNT | ≈ 0.0048 XNT |
| Collection: Collection Config rent, plus the Core Collection's rent and Core fee | 0.0019 XNT + Core's share |
| Vault MEMO token account (rent, first `mine` only; never closable) | ≈ 0.002 XNT, paid by that `mine` caller |
| First Vault `burn` (Vault burn stats rent, topped up by the payer if the Vault lacks it) | ≈ 0.0016 XNT plus the Vault's 0.00089 XNT rent minimum |
| MEMO ID (Asset + registry, estimate) | ≈ 0.0068 XNT plus the MEMO burned |
| `create_collection` (memo-home instruction, local validator with X1 mainnet program builds) | 34.7k CU |
| `create_asset` | 41–44k CU |
| `mine` (memo-home instruction, including memo-mint and the share transfer) | 51–71k CU depending on memo-mint's supply tier; 64–84k the first time (creates the Vault token account). The whole transaction also pays for the SPL Memo (≈ 370 CU per byte) |
| Vault `burn` | 55–63k CU; 77–80k the first time (Vault burn stats) |
| Prototype on testnet, for comparison | mining message (115-byte memo) 108k CU total, fee ≈ 0.003 XNT |

For comparison, one plain memo-mint mint costs ≈ 0.00057 XNT in fees.

**Fees follow the requested CU limit, not the CU used** (≈ 10 lamports per requested CU). Every client simulates each transaction first with a large limit (1.4M for large memos or file chunks; the CLI default of 200k is too small for a 1,000-byte SPL Memo), then sends it with the simulated units plus a small margin (x1-memo-miner uses +50). Bulk uploads simulate every chunk, or at least every chunk size. All costs in this document assume this.

Storage (X1 mainnet, measured 2026-10-20):

| Method | Per transaction | Cost per MB | Read 1 MB | Used for |
|---|---|---|---|---|
| memo-file Blocks (accounts) | ≈ 1 KB written per transaction | 7.3 XNT rent (0.00713 XNT per KB) plus fees | ≈ 115 Blocks, 2 `getMultipleAccounts` calls (100 per call) | all files |
| SPL Memo | ≈ 1,000 B UTF-8 text, ≈ 370 CU per byte, ≈ 0.0037 XNT | ≈ 4 XNT, readable only through the archive | 1–2 `getSignaturesForAddress` calls (1,000 per call, memo text included) | short app text (chat, posts) |

Instruction-data inscriptions (≈ 0.05 XNT per MB) were considered and dropped: their content lives only in transaction history, so they depend on the archive and an indexer, while account content lives in chain state.

---

## 8. Data Availability

- X1 mainnet RPC serves history from long-term storage (`getFirstAvailableBlock = 0`), from genesis (2025-10-06) to now.
- Exception: a gap around slots ~1,000 to ~1,100,000 (2025-10-06 to 2025-10-13).
- Long-term storage is run by the X1 team. Apps that store content in memos should run an indexer and may mirror important content to Arweave.

---

## 9. Open Questions

Items 1–4 (royalties, fixed Authority, memo requirement, share rounding) were decided on 2026-10-20; see §2. The second audit (MEMO-HOME-AUDIT.md) led to these decisions on the same day: PDA Collection and Asset addresses, the Authority signing `create_collection`, and the Vault State nonce.

5. **MEMO ID default suffix**: which suffix `@xen-artist` abbreviates (e.g. `xen-artist.x1`), whether `.x1` collides with an existing X1 naming service, and avoiding real ICANN TLDs (e.g. `.rip`) as suffixes.
6. **MEMO ID prices**: burn per length, and whether issuing an NFT SubId burns MEMO.
7. **Gateway domain** for MEMO Web, and the extension and wallet integration timeline.
8. **memo-file limits**: Block size (9 KB proposed), maximum file size and file count per Asset, and the inline limit for small files.
9. **MEMO Console spec**: cartridge size limit, Lua subset, and how the runtime itself is versioned and upgraded (old games must keep running on the spec version they were published for).
10. **memo-os**: maximum Image size, which RISC-V emulator to build on, whether and when to allow network relays, client-side encryption of Machine overlays, and which model APIs the AI bridge supports first.

---

## Appendix A. Prototype Results (X1 testnet, 2026-10-19)

Prototype program `home-proto` (testnet `D3kCMRKjxQJ4y3xe74SqiFcvnFGt1MMvcsMMQTzNvNzg`, throwaway). Collection with Oracle `ArfqLy1kEW4eyE6KgbkZtPeoKriiun3gnvDtoohK5JZt`, control Collection without Oracle `9KLHN5xmy897QmqUs3Xsy937qumC3eEvYtQSj2SgT634`. The prototype used per-Asset Burn Guards and its own stats PDA; the design above simplifies both.

| Check | Result |
|---|---|
| CPI `CreateCollectionV1` with the PDA `[b"authority"]` as update authority | ✓ |
| CPI `CreateV1` into that Collection, PDA signs as collection authority via `invoke_signed`; Vault ATA created in the same instruction | ✓ 68.8k CU (Core 12.5k, ATA 23k) |
| Asset update authority reads as `Collection(collection)` (tag 2), used to check membership | ✓ |
| SPL Memo at index 0, CPI `process_mint_to` into the Vault, balance diff = reward, `transfer_checked` of the sender share signed by the Vault PDA | ✓ reward 1,000,000, sender 300,000, Vault 700,000 |
| `withdraw` by a non-owner | rejected |
| Core `TransferV1` (the Collection account is required for Collection Assets), then old owner `withdraw` | rejected; new owner withdraws ✓ |
| `withdraw` while a Freeze / Transfer / Burn Delegate has an `Address` authority, or a Freeze Delegate is frozen | rejected (`Listed`) |
| Owner revokes the delegate, then `withdraw` | ✓ |
| Frozen Asset: owner Core transfer | rejected by Core |
| Re-listing an Asset whose delegate was revoked needs `ApprovePluginAuthorityV1` (the plugin stays, `AddPluginV1` fails with "Plugin already exists") | noted |
| Simulated sale: market (Transfer Delegate) transfers to a buyer; Core resets the delegate; buyer withdraws; market cannot transfer again | ✓ |
| Collection-level Oracle added through `AddCollectionExternalPluginAdapterV1`, signed by the PDA (Burn check, `can_reject`, results offset Anchor) | ✓ |
| Owner transfer of an Asset in that Collection, no extra accounts | ✓ unaffected |
| Burn without the Oracle account / with a missing Oracle account / while the Oracle says Rejected | all rejected |
| Burn after the Oracle says Pass | ✓ burned |
| Vault PDA (funded with lamports by the Owner) inits its `UserGlobalBurnStats` and calls `process_burn` via `invoke_signed` | ✓ stats recorded under the Vault PDA |
| Vault burn by a non-owner | rejected |

## Appendix B. Wallet Test Results (X1 testnet, 2026-10-18 to 2026-10-20)

| Asset type | X1 wallet (iOS) | X1 wallet (web) | Backpack |
|---|---|---|---|
| Metaplex Core | display ✓ transfer ✓ | ✓ ✓ | ✓ ✓ |
| Core Asset in a Collection | ✓ ✓ (wallet passes the Collection account) | not tested | not tested |
| Core Asset in a Collection with a Burn Oracle | ✓ ✓; the wallet has no burn option | not tested | not tested |
| **memo-home v1** (deployed program; PDA Collection and Asset, Burn Guard, MasterEdition/Royalties absent, metadata JSON with image) | ✓ ✓ (name, image, Collection shown); no burn option | ✓ ✓; no burn option | ✓ ✓; no burn option |
| SPL Token + Token Metadata | ✓ ✓ | ✓ ✓ | ✓ ✓ |
| Token-2022 + Token Metadata | ✓ ✓ | ✓ ✓ | not tested |
| Token-2022 metadata extension only | address only, transfer ✓ | address only, transfer ✓ | not tested |
| Token-2022 with transfer hook | transfer ✗ (legacy `Transfer`) | transfer ✗ (missing extra accounts) | transfer ✗ (missing extra accounts) |

Notes:

- The X1 wallet puts a token in the NFT tab when decimals = 0 and the holder's balance = 1.
- Metaplex Token Metadata rejects mints with the TransferHook extension.
- A newly received Asset may need a wallet restart to appear.
- memo-home v1 test (2026-10-20): Collection "HOME Wallet Test" `6yejSB4u…`, Assets HOME #1 (web) `4ivUQtZM…`, #2 (iOS) `22sijHuk…`, #3 (Backpack) `5MUkFmuC…`, each with 1 MEMO mined into its Vault. Each wallet displayed its Asset and transferred it; after the transfers the new owner could withdraw from each Vault (nonce 1) and the previous owner was rejected with `NotOwner`.
