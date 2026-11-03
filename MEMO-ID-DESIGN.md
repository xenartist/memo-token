# memo-id Design (Specification)

**MEMO ID**: a globally unique, user-chosen ID such as `xen-artist.memo`, owned as a memo-home NFT. It can point to a wallet or to any Asset, and it can be transferred, listed and sold.

Status: implemented in `programs/memo-id` (2026-10-21) and tested on the local validator (unit tests and `regression-memo-id`); version 0.1.0 deployed to testnet (2026-10-27, redeployed 2026-10-30), not to mainnet. Version 0.2.0 (2026-11-03) applies the owner's decisions on the third review (§8, item 8): it changes the Registry and Primary ID layouts, the arguments of `set_primary`, and the account lists of the eight instructions that take a holder (a `holder_config` account follows `holder`), so 0.1.0 accounts and clients do not work with it. It was therefore deployed to testnet as a new program, not as an upgrade (2026-11-03, `CDbmi4gSZ47P5EMRH7GU7ztrPDrk9gn1uMgeZR7AXeLA`, with a new Collection and a new frozen lookup table recorded in `Anchor.toml`); the 0.1.0 program `3U7nmWAr…` was closed the same day. Builds on `MEMO-HOME-DESIGN.md` §6.4, which this document refines into an implementable specification.

Decided (2026-10-21): the URL scheme is `home://` (MEMO-HOME-DESIGN.md §6.6); label rules stay strict (DNS hostname labels, ASCII only, §2), following the industry standard rather than X1NS's looser rule.
Also decided: suffix `memo`; no royalties on the MEMO ID Collection; no reserved names. Metadata: the registrant passes the URI (at most 256 bytes, must start with `https://`, `ar://` or `ipfs://`); there is no project server or gateway (decided 2026-10-21), so project clients default to one shared metadata JSON with a generic MEMO ID image, stored permanently on Arweave (confirmed 2026-10-21; uploaded through Turbo; decided 2026-10-30: the default URI and `COLLECTION_URI` are both `https://ardrive.net/Ff6UIKWrVHgbQkj68w-VHc8GLoi4gKR690F8jJAXkBA`, copy in `metadata/memo_id-metadata.json`; its `image` is `https://ardrive.net/YtT8Mte_qBwHmc2_UDRwziXdQi-5QYP_Y2m2Y2__AN8`, and its `"collection": { "name": "MEMO ID", "family": "MEMO" }` field makes the X1 wallet title the group `MEMO ID`). `ardrive.net` serves Turbo uploads at once; `arweave.net` still answered 404 for them a day after they were final. If the gateway goes away, the data stays on Arweave: controllers move an ID with `update_uri`, while the Collection loses only its image and description. Wallets need `https://` for both the URI and the JSON's `image`: X1 wallet test 2026-10-29 showed nothing for an `ar://` URI or an `ar://` image, and the image with both as https (MEMO-ID-AUDIT.md, testnet wallet tests); wallets show the on-chain name as the title. Prices: decided, see §4.

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
| Label characters | `a-z`, `0-9`, `-`; stored lowercase; no leading or trailing `-`; no `xn--` prefix (reserved by DNS for internationalized names, so no look-alike Unicode can appear in any reader). The label `memo` itself is not allowed at any level, so no ID contains `.memo.`: a truncated `xen-artist.memo.evil.memo` would otherwise read as `xen-artist.memo` (MEMO-ID-AUDIT.md F14). |
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
| Registry | `[b"memo_id", sha256(memo_id)]` | see below; 192 bytes plus the ID's length (198–445) | ≈ 0.0023–0.0040 XNT |
| SubId Index | `[b"subid", parent Registry, i as u32 LE]` | `registry` (the issued SubId's Registry), `bump`; 41 bytes | ≈ 0.0012 XNT per NFT SubId |
| Primary ID | `[b"primary_id", kind (0 Address, 1 Asset), target_key]` | `target` (Target), `memo_id_registry` (Pubkey), `set_by` (Pubkey), `set_holder` (Pubkey: the holder used for an Asset target, all zeros otherwise), `bump`; 138 bytes | ≈ 0.0019 XNT |
| MemoId Asset | memo-home `asset_address(MEMO ID Collection, sha256(memo_id))` | Core Asset, name = the full `memo_id`, URI from the registrant; with its Vault and Vault State | ≈ 0.0048 XNT |

Registry fields. Every field before `memo_id` has a fixed size, so readers can filter with memcmp at fixed offsets, and the account is exactly its Borsh encoding: strict decoders read every Registry (MEMO-ID-AUDIT.md F31). Without a forward record, bytes 90–187 are all zero.

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
| 122 | `target_holder` (Pubkey) | the holder whose Vault held the controlling Asset when the target was set; all zeros if none (§5) |
| 154 | `has_target` (bool) | whether there is a forward record |
| 155 | `target` (Target, 33 bytes) | kind and key; all zeros without a forward record |
| 188 | `memo_id` (String) | the full ID (length at 188, bytes from 192) |

**Issued NFT SubIds without an indexer** (decided 2026-10-21). A buyer of a parent ID must see which sub-names are already permanently out of the parent's control. `nft_subids_issued` gives the count, and entries `0 … n-1` of the SubId Index give each one: the reader derives the n addresses and reads them with `getAccountInfo`, which every RPC node serves. Entries are never closed, so the list is always complete. Record SubIds move with the parent and can be deleted, so they only have a count; listing them needs `getProgramAccounts` with a memcmp on `parent` (offset 41), which some public RPC nodes restrict.

`Target` is `Address(Pubkey)` (a wallet or any account) or `Asset(Pubkey)` (a Core Asset). The kind is part of the record, so the two are never confused.

Cost of a registration, measured on the local validator with 0.2.0: ≈ 0.0066 XNT for the Asset, its Vault State, the Registry, Core's fees and the transaction fee, plus the MEMO burned (§4). Issuing an NFT SubId costs ≈ 0.0055 XNT for its Asset, Vault State and SubId Index entry, on top of the Record's rent paid at creation.

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

At today's mainnet supply (8.93M, reward 1 MEMO) and ≈ 0.00057 XNT per mint, mining the burn costs ≈ 239.8 / 23.98 / 2.398 / 0.240 / 0.039 XNT for lengths 1 / 2 / 3 / 4 / 5+, plus ≈ 0.0066 XNT of rent, Core fees and the transaction fee per registration (§3).

The numbers follow the existing app prices (memo-chat group 42,069 MEMO, memo-profile 420 MEMO). The burn goes through memo-burn and counts toward the payer's burn statistics.

---

## 5. Control and verification

- **Controller** of an NFT MEMO ID: `owner_of(asset)`. If that owner is the Vault of another memo-home Asset H, the controller is `owner_of(H)`. This is one level only: the client passes H with H's Collection Config, and memo-id checks that H is in that Collection (`memo_home::is_home_asset`) and `vault_of(H) == owner`. A MEMO ID can therefore be sold together with a room or a project by putting it in that Asset's Vault. H must be a memo-home Asset (decided 2026-11-03, MEMO-ID-AUDIT.md F20): a third-party Asset can carry permanent transfer or burn delegates that would move or destroy the ID's control, and memo-home could not move the ID out of its Vault. An ID moved into the Vault of any other Asset can no longer be controlled, so clients refuse such transfers.
- **Controller** of a Record SubId: the controller of its parent.
- **Forward** (`memo_id` → Target): set by the controller. memo-id stores `target_set_by = controller` and `target_holder` = the holder passed, if any. The record is valid only while `target_set_by` equals the current controller, so it lapses automatically on a sale.
- **Reverse** (Primary ID): set for a Target by that Target's controller. For an `Address`, that is the address itself as a signer. For an `Asset`, it is `owner_of(asset)`, with the same one-level Vault rule; `set_holder` records the holder passed. It is valid only while `set_by` equals that controller.
- **Reading by address only** (decided 2026-11-03, F32). The recorded holder lets a reader that uses only `getAccountInfo` re-derive the controller that set a record: read the Asset's owner O; if the record names a holder H and `O == vault_of(H)`, the controller is `owner_of(H)`, otherwise it is O. A record is valid when that controller equals `target_set_by` (or `set_by`). This never verifies a record that is not valid. It can miss a valid record whenever the controlling Asset (or Target Asset) now sits in a Vault other than the recorded holder's, including when no holder was recorded: for example an ID whose target was set while its owner held it directly, then deposited into a room the same wallet owns, or moved from one holder's Vault to another's. Such a reader shows the record as unverified. Clients therefore set the target (and the Primary ID) again after a deposit; a reader that finds the Asset owned by a Vault PDA not matching the record can find the holder with a Core `getProgramAccounts` search by owner.
- **Verified** means the forward record and the reverse record point at each other and both are valid. Apps display only verified MEMO IDs. When no valid forward record exists, a MEMO ID resolves to its controller only if that controller is a wallet (a System-owned address, not a program-owned account or PDA). Otherwise it does not resolve: an ID listed in escrow, held by an Asset signer or left in a non-memo-home Vault has a PDA as its controller, and funds sent there are usually lost (MEMO-ID-AUDIT.md F15). With a verified `holder`, the holder's owner is the controller and the same rule applies.
- **Display**: apps and markets show the on-chain name (the Asset name, equal to `Registry.memo_id`), never the `name` field of the URI's JSON, which the controller can set to anything (F16). Long IDs are shortened in the middle so the `.memo` end and the first label stay visible; wallets that cut the end can hide the real suffix.
- **Listings and locks.** Issued NFT SubIds are permanent, so `issue_subid` is refused (`Listed`) while the parent's Asset, or the holder whose Vault holds it, carries a freeze, transfer or burn delegate held by someone else, or any permanent delegate (`memo_home::is_listed`, the rule memo-home applies to Vault outflows; decided 2026-11-03, F19). A renter, or a borrower whose ID is frozen as collateral, therefore cannot issue SubIds that would outlive the loan. The other controller actions stay open: the next controller can undo them (forward records lapse once the buyer is the controller; Record SubIds move with the ID and can be deleted). For offers that do not lock the Asset on chain (a bid, an escrowless listing), a market records `nft_subids_issued` (Registry offset 81) at listing and requires it unchanged at the sale (MEMO-ID-AUDIT.md F01).

Helper functions exported by the crate (feature `cpi`):
- `controller_of(asset, holder?)`: the controller of an Asset, `holder` being `(H, H's Collection Config)`. It does not bind the Asset to any Registry: the caller checks that the Asset is the Registry's `asset` (or, for a Record SubId, its parent's).
- `Registry::forward(controller)`: the forward record if it is valid for that controller.
- `PrimaryId::is_valid(target_controller)`.
- `is_verified(registry_key, registry, controller, primary, target_controller)`: trusts the two controllers it is given, which the caller derives with `controller_of` from checked accounts.
- Address helpers: `registry_address`, `asset_address`, `subid_index_address`, `primary_id_address`, `memo_id_collection`.

---

## 6. Instructions

Every instruction is permissionless or gated by a controller; there is no admin key.

| Instruction | Signer | Effect |
|---|---|---|
| `init` | anyone, once | creates the MEMO ID Collection through memo-home `create_collection` (the memo-id Authority signs by CPI); a second call fails in memo-home |
| `register(memo_id_hash, label, uri)` | payer | checks the label and that `memo_id_hash = sha256(memo_id)` (it seeds the Registry address); burns the price through memo-burn; creates the Registry (kind Nft) and the Asset through memo-home `create_asset`, owner = payer, name = `memo_id` |
| `create_subid(memo_id_hash, label, target?)` | parent's controller (+ payer) | creates a Record SubId `label.<parent>`; the parent must be an NFT ID; parent `record_subids` + 1 |
| `delete_subid` | parent's controller | closes a Record SubId, rent to the controller; parent `record_subids` − 1 |
| `issue_subid(index, uri)` + account `new_owner` | parent's controller (+ payer) | turns a Record SubId into an NFT owned by `new_owner` (one way); refused while the parent's Asset or the holder is listed (§5); `index` must equal the parent's `nft_subids_issued`; burns 69 mints; creates the Asset and SubId Index entry `index`; parent `nft_subids_issued` + 1, `record_subids` − 1 |
| `set_target(target)` / `clear_target` | controller | forward record; for a Record SubId the parent Registry and its Asset are passed |
| `set_primary(target_kind, target_key)` / `clear_primary` | the Target's controller (+ payer) | Primary ID, created or replaced; cleared rent goes to the signer. The Target is passed as its kind (0 Address, 1 Asset) and key, which are the Primary ID's seeds, so the IDL derives the address correctly (F25) |
| `update_uri(uri)` | controller (+ payer) | memo-home `update_asset` (URI only; the name is always the `memo_id`) |

Every controller check takes two optional accounts, `holder` (the memo-home Asset whose Vault holds the MEMO ID's Asset, §5) and `holder_config` (its Collection Config), passed together or not at all.

Memos: `register` and `issue_subid` carry the BurnMemo that memo-burn requires. Its payload is Base64 Borsh, following the repo convention: `{ version: 1, category: "memo_id", operation: "register" | "issue_subid", user_pubkey (the payer, base58), memo_id }`. memo-id checks every field, and that `burn_amount` equals the price. The `user_pubkey` field also keeps the shortest memo (`a.memo`) above memo-burn's 69-byte minimum. The other instructions need no memo.

Compute (local validator): `register` ≈ 103–107k CU, `issue_subid` ≈ 110–118k CU (they vary with the PDA bumps).

**Transaction size and the lookup table** (measured 2026-10-21 with 0.1.0; 0.2.0 adds one byte, the index of the `holder_config` account). `register` and `issue_subid` pass about 25 accounts, and `issue_subid` carries the full ID in its memo. As legacy transactions, `register` with a 68-byte ID fits a URI of at most 101 bytes, and `issue_subid` with a 64-byte SubId does not fit at all (1,342 bytes with a 48-byte URI in the 0.2.0 regression suite; the limit is 1,232). Clients therefore send these two as v0 transactions with an Address Lookup Table holding the 12 accounts that are the same for everyone: memo-id Authority, Collection Config, MEMO ID Collection, memo-home program authority, Core, memo-home, MEMO mint, GlobalBurnStats, Token-2022, memo-burn, the instructions sysvar and the System program. The table is created once per network and program ID, and frozen, so it has no authority and nobody can change it; it is plain chain state, like the programs, and anyone can create an identical one.

With the table, `register` always fits: a 68-byte ID with a 256-byte URI is 1,051 bytes. For `issue_subid`, the longest URI that fits:

| Full SubId length | Payer, controller and holder all separate | Owner pays and signs alone |
|---|---|---|
| up to 100 bytes | 256 (the limit) | 256 |
| 150 bytes | 190 | 256 |
| 200 bytes | 122 | 250 |
| 253 bytes | 55 | 182 |

The table was measured with 0.1.0: with 0.2.0, subtract 1 from every figure below 256. It assumes the new owner is one of the signers. A holder in the MEMO ID Collection passes the MEMO ID Collection Config, which is already in the message; a holder from another memo-home Collection adds its Config, 32 bytes. A new owner who is none of payer, controller and holder adds 32 bytes to the message: subtract 32 from the first column. At 253 bytes that leaves 22 bytes, so even the default 63-byte gateway URI does not fit (MEMO-ID-AUDIT.md F17). Clients check the serialized size before asking for a signature; when it is over 1,232 bytes they issue the SubId to the controller (or payer) and transfer it in a second transaction, or ask for a shorter URI. The regression suite issues a 253-byte SubId (four levels) and transfers it.

CPI depth:
- `user → memo-id → memo-home → Core → System` is 4.
- `memo-id → memo-burn → Token-2022` is 3.
- An app that registers a MEMO ID for its own Asset in one transaction (`app → memo-id → memo-home → Core → System`) reaches 5, the limit.

---

## 7. Security notes

- An Asset address is a memo-home PDA and never signs, so nobody can act "as an Asset" in memo-id. Asset targets are always confirmed through `owner_of`. This holds for holders because a holder must be a memo-home Asset (§5); an `Asset` target can still be any Core Asset.
- The Registry and Asset seeds both derive from `sha256(memo_id)`. A second registration of the same ID fails at both, and pre-funded addresses are handled by memo-home and by Anchor `init`.
- Prices are computed on-chain from the live supply. The memo's `burn_amount` must equal the price exactly, so the client reads the supply just before sending (simulate first, §7 of the home design).
- Record SubIds cannot have children, so deleting one never orphans anything.
- `set_primary` uses `init_if_needed`: every field is rewritten on each call, so an existing Primary ID is simply replaced.
- The kind is part of the Primary ID seed. A third-party Core Asset created at a keypair address X can be owned by one party while another still holds key X; `Address(X)` and `Asset(X)` therefore have separate Primary IDs, and neither can replace or close the other's (MEMO-ID-AUDIT.md F13).
- The Collection URI is a program constant, because no admin can change the Collection after `init`. A mainnet build fails until `COLLECTION_URI` is set to the shared metadata JSON on Arweave.
- No admin, no fees beyond the burn, no upgrade-sensitive state. Locked when stable, after memo-home.

---

## 8. Decisions

1. **Suffix**: `memo`. Whether and how MEMO IDs should link to `.x1` names (X1NS) is left for later; X1NS has published no program ID or account layout yet.
2. **Prices**: see §4.
3. **Royalties**: none.
4. **Reserved names**: none.
5. **Metadata**: see the top of this document. Project clients offer two choices, at `register` and later through `update_uri` (decided 2026-10-29): the shared default JSON, or a URI the user pastes (content the user hosts on Arweave, IPFS or HTTPS). Clients do not upload images or JSON for the user, so they depend on no upload service. A custom image changes nothing about identity: apps show the on-chain name (§5, F16).
6. **Full ID length**: at most 253 bytes; memo-home's Asset name limit raised to 253 (§2). This needs a memo-home upgrade on testnet and mainnet before memo-id is deployed.
7. **Issued NFT SubIds**: a count in the Registry plus the SubId Index (§3).
8. **Third review** (decided 2026-11-03, version 0.2.0; MEMO-ID-AUDIT.md, Third review): `issue_subid` is refused while the parent's Asset or the holder is listed (F19); a holder must be a memo-home Asset, passed with its Collection Config (F20); `set_primary` takes the Target's kind and key (F25); the Registry has a fixed-size encoding (F31) and records the holder (`target_holder`), as the Primary ID does (`set_holder`) (F32).

Before mainnet: create the mainnet program keypair (`scripts/keys.sh new mainnet programs memo_id`) and update `declare_id!` and `Anchor.toml`; choose the Collection URI's gateway (F26); after `init`, create and freeze the lookup table and record its address in `Anchor.toml`.
