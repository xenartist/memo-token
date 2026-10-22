# home:// browser extension (prototype)

A Chrome extension (Manifest V3) that opens `home://<name>.memo/<path>` and
`home://<asset address>/<path>`. It reads a page and its files from X1 chain state over RPC,
checks every file's SHA-256, and renders the page in a sandbox. There is no gateway or server.

**Prototype.** memo-id and memo-file do not exist yet. Names and files come from the throwaway
program `home_web_test` (`clients/regression/test-programs/home-web`), which runs only on the
local validator. Sites are real memo-home Assets. The address format and the viewer are what
the real programs will use (MEMO-HOME-DESIGN.md §6.6, MEMO-ID-DESIGN.md).

## Try it

```bash
# 1. Local validator with memo-home, Core and home_web_test
./scripts/run-regression-tests.sh home        # builds the test programs (or: cargo build-sbf, see below)
./scripts/local-validator.sh start --no-build

# 2. Upload the demo site (demo-site/) as xen-artist.memo
cd clients
MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=../target/local-validator/wallet.json \
    cargo run --bin home-web-seed -- [--name xen-artist.memo] [--site home-extension/demo-site]
```

3. Load the extension: open `chrome://extensions`, turn on Developer mode, click
   **Load unpacked** and choose `clients/home-extension`.
4. Open the site in any of these ways:

| Type in the address bar | What happens |
|---|---|
| `home xen-artist.memo/about.html` | the `home` keyword (type `home`, then a space) |
| `xen-artist.memo/` (with the slash) | Chrome tries `http://xen-artist.memo/`; the extension redirects it |
| `home://xen-artist.memo/` or `xen-artist.memo` | Chrome sends it to the search engine; searches for a `home://` URL or a bare `.memo` name on Google, Bing or DuckDuckGo are redirected |
| click the extension's toolbar icon | opens `home://xen-artist.memo/` |

The viewer has its own address field, which accepts `home://…` directly. It also has a network
selector (local validator, X1 testnet, X1 mainnet) and a badge with the on-chain details.

Building the program on its own:
`cargo build-sbf --manifest-path clients/regression/test-programs/home-web/Cargo.toml --sbf-out-dir target/test-programs`

## How it works

1. **Resolve** (`lib/home.js`):
   - the name record `[b"name", sha256(name)]` points to an Asset;
   - the Core Asset is read for its name and owner;
   - the file is read from `[b"file", asset, sha256(path)]`.

   PDAs are derived in the browser, so no indexer is needed.
2. **Verify**: the SHA-256 of each file's content must equal the hash stored with it.
3. **Assemble** (`viewer.js`): relative stylesheets, scripts, images and CSS `url()`s are
   fetched from the same Asset and inlined. The result is one self-contained document.
4. **Render** (`sandbox.html`): a sandboxed extension page.
   - It has an opaque origin and no extension APIs.
   - Its CSP blocks all network access, so only on-chain content can appear.
   - Link clicks are passed to the viewer:
     - relative and `home://` links navigate inside the viewer, and the browser's Back button works;
     - other links are shown as a notice and are not opened.

## Checked (headless Chrome 156, local validator)

- All four routes redirect and render with matching hashes:
  - the `.memo` host;
  - a Google search for `home://…`;
  - a Google search for a bare `.memo` name;
  - a DuckDuckGo search for `home://…`.
- The name form and the Asset-address form open the same files.
- Relative paths work, including `../`.
- Inline scripts run. Link navigation and history work.
- Missing files and invalid names (such as `.x1`) show an error.
- Inside the sandbox:
  - the origin is `null` and `chrome.runtime` is unavailable;
  - `fetch`, external images, `top.location` and `localStorage` are all blocked.
- `node --test test/home.test.mjs` passes. It checks base58, PDAs against `solana find-program-derived-address`, the name rules and URL resolution.

Not checked automatically: the `home` keyword, and whether Chrome's real address bar sends a typed
`home://…` to the search engine. That depends on the browser, the OS and any app registered for
`home:`.

## Limits of the prototype

- Files are at most 9 KB each (one account created by CPI); no blocks, versions or sessions.
- No MEMO ID verification (forward + reverse records); any registered name is shown.
- The viewer's address bar shows `chrome-extension://…/viewer.html#home://…`, and all sites share
  the extension's origin for the viewer itself (content runs in its own opaque sandbox origin).
  Connecting a site to a wallet needs a bridge in the reader that names the `home://` site in the
  approval (MEMO-HOME-DESIGN.md §6.6); the project runs no gateway.
- The sandboxed page can still navigate its own frame to an external URL by script. The real viewer
  should block or flag that.
