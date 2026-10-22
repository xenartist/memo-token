# memo-id Design (Specification)

**MEMO ID**: a globally unique, user-chosen ID such as `xen-artist.memo`, owned as a memo-home NFT. It can point to a wallet or to any Asset, and it can be transferred, listed and sold.

Status: implemented in `programs/memo-id` (2026-10-21) and tested on the local validator (unit tests and `regression-memo-id`); not deployed. The program ID in the code is a local development key until the testnet and mainnet keypairs are set up. Builds on `MEMO-HOME-DESIGN.md` §6.4, which this document refines into an implementable specification.

Decided (2026-10-21): the URL scheme is `home://` (MEMO-HOME-DESIGN.md §6.6); label rules stay strict (DNS hostname labels, ASCII only, §2), following the industry standard rather than X1NS's looser rule.
Also decided: suffix `memo`; no royalties on the MEMO ID Collection; no reserved names. Metadata: the registrant passes the URI (at most 256 bytes, must start with `https://`, `ar://` or `ipfs://`); there is no project server or gateway (decided 2026-10-21), so project clients default to one shared metadata JSON with a generic MEMO ID image, stored permanently on Arweave (confirmed 2026-10-21; uploaded once before launch, its `ar://` URI fixed in the clients); wallets show the on-chain name as the title. Prices: decided, see §4.

---

## 1. Scope of v1

In v1:
- one suffix, `memo` (§2);
- second-level MEMO IDs (`<label>.memo`) as NFTs;
- Record SubIds and NFT SubIds under any NFT MEMO ID;
- forward records (MEMO ID → Target) and Primary IDs (Target → MEMO ID), with verification;
- metadata URI updates by the controller.

Later:
- more suffixes (never `x1`, which belongs to X1NS);
- links to X1NS names (e.g. showing a verified `.x1` name next to a MEMO ID), once X1NS documents its accounts. X1NS's published label rule is looser ("no spaces, dots or slashes; no leading or trailing hyphen"), so a `.x1` name does not always have a `.memo` equivalent; links are by reference, never by copying the label;
- public SubId sales;
- renaming;
- an on-chain resolver for other programs beyond the helper functions.

---

## 2. Names

| Rule | Value |
|---|---|
| Default suffix | **`memo`**: it is the project name, pairs with the `home://` scheme (`home://xen-artist.memo/`: the scheme names where content lives, the suffix names the namespace), and is not an IANA top-level domain. As of the IANA root zone list of 2026-10-20, `id`, `rip`, `me`, `mo` and `meme` are TLDs; `memo`, `x1`, `xnt` and `home` are not, and `home` is permanently reserved by ICANN for name-collision reasons. A future gTLD round could still delegate `.memo`. On X1, the X1 team runs X1NS (`id.x1.xyz`) with the suffix `.x1`, so memo-id never issues `.x1` names; `.memo` does not collide with it. |
| `@handle` | `@xen-artist` is short for `xen-artist.memo`. |
| Label characters | `a-z`, `0-9`, `-`; stored lowercase; no leading or trailing `-`; no `xn--` prefix (reserved by DNS for internationalized names, so no look-alike Unicode can appear in any reader) |
| Label length | 1–63 bytes |
| Full ID length | at most 253 bytes, the DNS hostname limit (decided 2026-10-21). The Asset is named after the full `memo_id`, so memo-home's Asset name limit was raised from 64 to 253 bytes for this. A second-level ID is at most 68 bytes (a 63-byte label plus `.memo`); longer IDs come from SubId levels. |
| Levels | `label.memo` (registered), then any number of SubId levels below an NFT ID, e.g. `blog.xen-artist.memo` |
| Uniqueness | per full `memo_id`, through the Registry PDA `[b"memo_id", sha256(memo_id)]` and the Asset seed `sha256(memo_id)` |
| Permanence | no expiry, no renewal (memo-home Assets cannot be burned or reclaimed) |
| Reserved names | none on-chain: first come, first served. Short IDs are expensive enough (§4) to limit squatting. |

These rules are stricter than X1NS's (which only forbids spaces, dots, slashes and leading or trailing hyphens) on purpose: every MEMO ID is a valid DNS hostname, so it works anywhere a hostname is expected, and ASCII only rules out look-alike names.

The clients normalize input (trim, lowercase) before sending. The program rejects anything that is not already in normal form, so one ID has exactly one byte representation.

---

## 3. Accounts

| Account | Seeds | Contents | Rent (approx.) |
|---|---|---|---|
| memo-id Authority | `[b"authority"]` (memo-id program) | none; Authority of the MEMO ID Collection, signs memo-home CPIs | — |
| MEMO ID Collection | memo-home `collection_address(memo-id Authority, sha256("memo"))` | Core Collection "MEMO ID", URI `COLLECTION_URI` (fixed at `init`), `max_share_bps = 0`, no `max_assets`, no royalties | once |
| Registry | `[b"memo_id", sha256(memo_id)]` | see below; 160 bytes plus the ID's length (166–413) | ≈ 0.0020–0.0038 XNT |
| SubId Index | `[b"subid", parent Registry, i as u32 LE]` | `registry` (the issued SubId's Registry), `bump`; 41 bytes | ≈ 0.0012 XNT per NFT SubId |
| Primary ID | `[b"primary_id", target_key]` | `target` (Target), `memo_id_registry` (Pubkey), `set_by` (Pubkey), `bump`; 106 bytes | ≈ 0.0016 XNT |
| MemoId Asset | memo-home `asset_address(MEMO ID Collection, sha256(memo_id))` | Core Asset, name = the full `memo_id`, URI from the registrant; with its Vault and Vault State | ≈ 0.0048 XNT |

Registry fields, fixed-size ones first so readers can filter with memcmp at fixed offsets:

| Offset | Field | Meaning |
|---|---|---|
| 8 | `kind` (u8) | 0 Nft, 1 Record |
| 9 | `asset` (Pubkey) | the Asset (Nft only; all zeros otherwise) |
| 41 | `parent` (Pubkey) | the parent's Registry (SubIds only; all zeros for `<label>.memo`) |
| 73 | `created_at` (i64) | |
| 81 | `nft_subids_issued` (u32) | NFT SubIds ever issued under this ID; only grows, because they can never be revoked |
| 85 | `record_subids` (u32) | Record SubIds that exist now |
| 89 | `bump` (u8) | |
| 90 | `target_set_by` (Pubkey) | the controller that set the target |
| 122 | `target` (Option<Target>) | 1 or 34 bytes |
| then | `memo_id` (String) | the full ID |

**Issued NFT SubIds without an indexer** (decided 2026-10-21). A buyer of a parent ID must see which sub-names are already permanently out of the parent's control. `nft_subids_issued` gives the count, and entries `0 … n-1` of the SubId Index give each one: the reader derives the n addresses and reads them with `getAccountInfo`, which every RPC node serves. Entries are never closed, so the list is always complete. Record SubIds move with the parent and can be deleted, so they only have a count; listing them needs `getProgramAccounts` with a memcmp on `parent` (offset 41), which some public RPC nodes restrict.

`Target` is `Address(Pubkey)` (a wallet or any account) or `Asset(Pubkey)` (a Core Asset). The kind is part of the record, so the two are never confused.

Cost of a registration, measured on the local validator: ≈ 0.0064 XNT for the Asset, its Vault State, the Registry, Core's fees and the transaction fee, plus the MEMO burned (§4). Issuing an NFT SubId costs ≈ 0.0055 XNT for its Asset, Vault State and SubId Index entry, on top of the Record's rent paid at creation.

---

## 4. Prices

Prices are counted in **mints**: one mint is what memo-mint pays for one mint at the current supply (`memo_mint::mint_amount_at_supply(supply)`, 1 MEMO today). Because they follow the emission curve, prices stay meaningful as MEMO gets scarcer. A mint costs a miner ≈ 0.00057 XNT, so 1,000 mints is the output of about 1,000 mining transactions.

| Label length | Burn (decided 2026-10-21) | Centi-mints (on-chain constant) |
|---|---|---|
| 1 | 420,690 mints | 42,069,000 |
| 2 | 42,069 mints | 4,206,900 |
| 3 | 4,206.9 mints | 420,690 |
| 4 | 420.69 mints | 42,069 |
| 5 and longer | 69 mints | 6,900 |
| Issuing an NFT SubId | 69 mints | 6,900 |
| Record SubId | no burn, rent only (refunded on delete) | — |
| Updating target, Primary ID or URI | no burn | — |

On-chain, a price is `ceil(reward × centi_mints / 100)` in token units, where `reward = memo_mint::mint_amount_at_supply(supply)`. Rounding up only matters at the last curve steps, where the reward is a few units.

At today's mainnet supply (8.93M, reward 1 MEMO) and ≈ 0.00057 XNT per mint, mining the burn costs ≈ 239.8 / 23.98 / 2.398 / 0.240 / 0.039 XNT for lengths 1 / 2 / 3 / 4 / 5+, plus ≈ 0.0068 XNT of rent and Core fees per registration.

The numbers follow the existing app prices (memo-chat group 42,069 MEMO, memo-profile 420 MEMO). The burn goes through memo-burn and counts toward the payer's burn statistics.

---

## 5. Control and verification

- **Controller** of an NFT MEMO ID: `owner_of(asset)`. If that owner is the Vault of another memo-home Asset H, the controller is `owner_of(H)`. This is one level only: the client passes H, and memo-id checks `vault_of(H) == owner`. A MEMO ID can therefore be sold together with a room or a project by putting it in that Asset's Vault.
- **Controller** of a Record SubId: the controller of its parent.
- **Forward** (`memo_id` → Target): set by the controller. memo-id stores `target_set_by = controller`. The record is valid only while `target_set_by` equals the current controller, so it lapses automatically on a sale.
- **Reverse** (Primary ID): set for a Target by that Target's controller. For an `Address`, that is the address itself as a signer. For an `Asset`, it is `owner_of(asset)`, with the same one-level Vault rule. It is valid only while `set_by` equals that controller.
- **Verified** means the forward record and the reverse record point at each other and both are valid. Apps display only verified MEMO IDs. When no valid forward record exists, a MEMO ID resolves to its controller's wallet.
- Listing does not block controller actions: whatever a seller sets lapses once the buyer is the controller.

Helper functions exported by the crate (feature `cpi`):
- `controller_of(registry, asset?, holder?)`
- `forward_of(registry, ...)`, which returns a Target only if it is valid
- `primary_of(target)`
- `is_verified(...)`

---

## 6. Instructions

Every instruction is permissionless or gated by a controller; there is no admin key.

| Instruction | Signer | Effect |
|---|---|---|
| `init` | anyone, once | creates the MEMO ID Collection through memo-home `create_collection` (the memo-id Authority signs by CPI); a second call fails in memo-home |
| `register(memo_id_hash, label, uri)` | payer | checks the label and that `memo_id_hash = sha256(memo_id)` (it seeds the Registry address); burns the price through memo-burn; creates the Registry (kind Nft) and the Asset through memo-home `create_asset`, owner = payer, name = `memo_id` |
| `create_subid(memo_id_hash, label, target?)` | parent's controller (+ payer) | creates a Record SubId `label.<parent>`; the parent must be an NFT ID; parent `record_subids` + 1 |
| `delete_subid` | parent's controller | closes a Record SubId, rent to the controller; parent `record_subids` − 1 |
| `issue_subid(index, uri)` + account `new_owner` | parent's controller (+ payer) | turns a Record SubId into an NFT owned by `new_owner` (one way); `index` must equal the parent's `nft_subids_issued`; burns 69 mints; creates the Asset and SubId Index entry `index`; parent `nft_subids_issued` + 1, `record_subids` − 1 |
| `set_target(target)` / `clear_target` | controller | forward record; for a Record SubId the parent Registry and its Asset are passed |
| `set_primary(target)` / `clear_primary` | the Target's controller (+ payer) | Primary ID, created or replaced; cleared rent goes to the signer |
| `update_uri(uri)` | controller (+ payer) | memo-home `update_asset` (URI only; the name is always the `memo_id`) |

Every controller check takes an optional `holder` account: the memo-home Asset whose Vault holds the MEMO ID's Asset (§5).

Memos: `register` and `issue_subid` carry the BurnMemo that memo-burn requires. Its payload is Base64 Borsh, following the repo convention: `{ version: 1, category: "memo_id", operation: "register" | "issue_subid", user_pubkey (the payer, base58), memo_id }`. memo-id checks every field, and that `burn_amount` equals the price. The `user_pubkey` field also keeps the shortest memo (`a.memo`) above memo-burn's 69-byte minimum. The other instructions need no memo.

Compute (local validator): `register` ≈ 103–107k CU, `issue_subid` ≈ 110–118k CU (they vary with the PDA bumps).

**Transaction size and the lookup table** (measured 2026-10-21). `register` and `issue_subid` pass about 25 accounts, and `issue_subid` carries the full ID in its memo. As legacy transactions, `register` with a 68-byte ID fits a URI of at most 101 bytes, and `issue_subid` with a 64-byte SubId does not fit at all (1,309 bytes with a 48-byte URI; the limit is 1,232). Clients therefore send these two as v0 transactions with an Address Lookup Table holding the 12 accounts that are the same for everyone: memo-id Authority, Collection Config, MEMO ID Collection, memo-home program authority, Core, memo-home, MEMO mint, GlobalBurnStats, Token-2022, memo-burn, the instructions sysvar and the System program. The table is created once per network and frozen, so it has no authority and nobody can change it; it is plain chain state, like the programs, and anyone can create an identical one.

With the table, `register` always fits: a 68-byte ID with a 256-byte URI is 1,051 bytes. For `issue_subid`, the longest URI that fits:

| Full SubId length | Payer, controller and holder all separate | Owner pays and signs alone |
|---|---|---|
| up to 100 bytes | 256 (the limit) | 256 |
| 150 bytes | 190 | 256 |
| 200 bytes | 122 | 250 |
| 253 bytes | 55 | 182 |

The default `ar://` URI is 48 bytes, so every case fits with it. A client warns before sending when a long custom URI would not fit. The regression suite issues a 253-byte SubId (four levels) and transfers it.

CPI depth:
- `user → memo-id → memo-home → Core → System` is 4.
- `memo-id → memo-burn → Token-2022` is 3.
- An app that registers a MEMO ID for its own Asset in one transaction (`app → memo-id → memo-home → Core → System`) reaches 5, the limit.

---

## 7. Security notes

- An Asset address is a memo-home PDA and never signs, so nobody can act "as an Asset" in memo-id. Asset targets are always confirmed through `owner_of`.
- The Registry and Asset seeds both derive from `sha256(memo_id)`. A second registration of the same ID fails at both, and pre-funded addresses are handled by memo-home and by Anchor `init`.
- Prices are computed on-chain from the live supply. The memo's `burn_amount` must equal the price exactly, so the client reads the supply just before sending (simulate first, §7 of the home design).
- Record SubIds cannot have children, so deleting one never orphans anything.
- `set_primary` uses `init_if_needed`: every field is rewritten on each call, so an existing Primary ID is simply replaced.
- The Collection URI is a program constant, because no admin can change the Collection after `init`. A mainnet build fails until `COLLECTION_URI` is set to the shared metadata JSON on Arweave.
- No admin, no fees beyond the burn, no upgrade-sensitive state. Locked when stable, after memo-home.

---

## 8. Decisions

1. **Suffix**: `memo`. Whether and how MEMO IDs should link to `.x1` names (X1NS) is left for later; X1NS has published no program ID or account layout yet.
2. **Prices**: see §4.
3. **Royalties**: none.
4. **Reserved names**: none.
5. **Metadata**: see the top of this document.
6. **Full ID length**: at most 253 bytes; memo-home's Asset name limit raised to 253 (§2). This needs a memo-home upgrade on testnet and mainnet before memo-id is deployed.
7. **Issued NFT SubIds**: a count in the Registry plus the SubId Index (§3).

Before deployment: create the testnet and mainnet program keypairs (`scripts/setup-keypairs.sh`) and update `declare_id!` and `Anchor.toml`; set `COLLECTION_URI`; after `init`, create and freeze the lookup table and fix its address in the clients.
