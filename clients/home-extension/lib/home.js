// home:// resolver: parses home:// URLs, derives PDAs and reads MEMO IDs, Assets and files from
// any X1 RPC node. No dependencies; runs in the extension and in Node (tests).
//
// Layouts are those of the throwaway prototype program home_web_test
// (clients/regression/test-programs/home-web), which stands in for memo-id and memo-file.

export const CORE_PROGRAM = "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d";
export const WEB_PROGRAM = "6uknhjicEvg7B7oAvZ3y7TWojo8YfvKicn7HFVDPtiip";
export const SUFFIX = ".memo";
const FILE_HEADER = 156;

// ---------------------------------------------------------------- base58

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function b58encode(bytes) {
  let n = 0n;
  for (const b of bytes) n = (n << 8n) | BigInt(b);
  let out = "";
  while (n > 0n) {
    out = B58[Number(n % 58n)] + out;
    n /= 58n;
  }
  for (const b of bytes) {
    if (b !== 0) break;
    out = "1" + out;
  }
  return out;
}

export function b58decode(s) {
  let n = 0n;
  for (const c of s) {
    const i = B58.indexOf(c);
    if (i < 0) throw new Error(`invalid base58 character '${c}'`);
    n = n * 58n + BigInt(i);
  }
  const bytes = [];
  while (n > 0n) {
    bytes.unshift(Number(n & 0xffn));
    n >>= 8n;
  }
  for (const c of s) {
    if (c !== "1") break;
    bytes.unshift(0);
  }
  return Uint8Array.from(bytes);
}

/** A base58 string that decodes to exactly 32 bytes */
export function isPubkey(s) {
  try {
    return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(s) && b58decode(s).length === 32;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------- hashing and PDAs

const enc = new TextEncoder();

export async function sha256(bytes) {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
}

function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

// ed25519: a PDA must not be a valid point. Same test as curve25519-dalek's decompress:
// y = bytes (sign bit cleared) mod p, x² = (y² - 1) / (d·y² + 1) must be a square.
const P = 2n ** 255n - 19n;
const mod = (a) => ((a % P) + P) % P;
function pow(b, e) {
  let r = 1n;
  b = mod(b);
  while (e > 0n) {
    if (e & 1n) r = (r * b) % P;
    b = (b * b) % P;
    e >>= 1n;
  }
  return r;
}
const D = mod(-121665n * pow(121666n, P - 2n));

export function isOnCurve(bytes) {
  let y = 0n;
  for (let i = 31; i >= 0; i--) y = (y << 8n) | BigInt(i === 31 ? bytes[i] & 0x7f : bytes[i]);
  y = mod(y);
  const y2 = (y * y) % P;
  const x2 = mod((y2 - 1n) * pow(D * y2 + 1n, P - 2n));
  return x2 === 0n || pow(x2, (P - 1n) / 2n) === 1n;
}

export async function findProgramAddress(seeds, programId) {
  const program = b58decode(programId);
  const marker = enc.encode("ProgramDerivedAddress");
  for (let bump = 255; bump >= 0; bump--) {
    const h = await sha256(concat([...seeds, Uint8Array.of(bump), program, marker]));
    if (!isOnCurve(h)) return b58encode(h);
  }
  throw new Error("no viable bump");
}

// ---------------------------------------------------------------- names and URLs

/** `label(.label)*.memo` with DNS hostname labels; same rule as the program */
export function isValidName(name) {
  if (name.length > 253 || !name.endsWith(SUFFIX) || name.length === SUFFIX.length) return false;
  return name
    .slice(0, -SUFFIX.length)
    .split(".")
    .every((l) => l.length >= 1 && l.length <= 63 && /^[a-z0-9-]+$/.test(l) && !l.startsWith("-") && !l.endsWith("-") && !l.startsWith("xn--"));
}

/**
 * Parse `home://<name.memo | asset address>/<path>`. The host may be given without the scheme.
 * Returns { host, kind: "name" | "asset", path, url } or throws with a readable message.
 */
export function parseHomeUrl(input) {
  let s = input.trim();
  s = s.replace(/^(web\+)?home:\/\//i, "");
  const cut = s.search(/[/?#]/);
  let host = cut < 0 ? s : s.slice(0, cut);
  let rest = cut < 0 ? "" : s.slice(cut);
  rest = rest.replace(/[?#].*$/, "");
  let kind;
  if (isPubkey(host)) {
    kind = "asset";
  } else {
    host = host.toLowerCase();
    if (!isValidName(host)) throw new Error(`"${host}" is neither a MEMO ID (name.memo) nor an Asset address`);
    kind = "name";
  }
  let path = decodeURIComponent(rest).replace(/^\/+/, "");
  if (path === "" || path.endsWith("/")) path += "index.html";
  const parts = [];
  for (const seg of path.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") parts.pop();
    else parts.push(seg);
  }
  path = parts.join("/") || "index.html";
  return { host, kind, path, url: `home://${host}/${path}` };
}

/** Resolve `href` (absolute home:// or relative) against the current home:// URL */
export function resolveHref(href, current) {
  if (/^(web\+)?home:\/\//i.test(href)) return parseHomeUrl(href).url;
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return null; // another scheme
  const { host, path } = parseHomeUrl(current);
  const base = new URL(`http://h/${path}`);
  const u = new URL(href, base);
  return parseHomeUrl(`home://${host}${u.pathname}`).url;
}

// ---------------------------------------------------------------- RPC and accounts

export async function getAccount(rpc, address) {
  const res = await fetch(rpc, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "getAccountInfo", params: [address, { encoding: "base64", commitment: "confirmed" }] }),
  });
  if (!res.ok) throw new Error(`RPC ${rpc}: HTTP ${res.status}`);
  const json = await res.json();
  if (json.error) throw new Error(`RPC ${rpc}: ${json.error.message}`);
  const v = json.result.value;
  if (!v) return null;
  const bin = atob(v.data[0]);
  const data = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) data[i] = bin.charCodeAt(i);
  return { owner: v.owner, lamports: v.lamports, data };
}

async function discriminator(name) {
  return (await sha256(enc.encode(`account:${name}`))).slice(0, 8);
}

const eq = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
const dec = new TextDecoder();

function readString(data, o) {
  const len = new DataView(data.buffer, data.byteOffset + o, 4).getUint32(0, true);
  return [dec.decode(data.slice(o + 4, o + 4 + len)), o + 4 + len];
}

/** Core AssetV1: key 1, owner, update authority (tag + key), name, uri */
export function parseCoreAsset(data) {
  if (data.length < 34 || data[0] !== 1) throw new Error("not a Core AssetV1");
  const owner = b58encode(data.slice(1, 33));
  let o = 33;
  const tag = data[o++];
  let updateAuthority = null;
  if (tag === 1 || tag === 2) {
    updateAuthority = { kind: tag === 2 ? "collection" : "address", key: b58encode(data.slice(o, o + 32)) };
    o += 32;
  }
  const [name, o2] = readString(data, o);
  const [uri] = readString(data, o2);
  return { owner, updateAuthority, name, uri };
}

/**
 * Resolve a home:// URL to its file: the MEMO ID record (if a name), the Asset and the file.
 * Every step is read from chain state and checked; `verified` means the SHA-256 of the
 * content equals the hash stored with the file.
 */
export async function resolve(rpc, input) {
  const u = parseHomeUrl(input);
  const steps = [];
  let asset = u.host;
  if (u.kind === "name") {
    const record = await findProgramAddress([enc.encode("name"), await sha256(enc.encode(u.host))], WEB_PROGRAM);
    const acc = await getAccount(rpc, record);
    if (!acc) throw Object.assign(new Error(`${u.host} is not registered`), { steps, url: u });
    if (acc.owner !== WEB_PROGRAM || !eq(acc.data.slice(0, 8), await discriminator("NameRecord")))
      throw new Error(`name record ${record} has an unexpected owner or layout`);
    asset = b58encode(acc.data.slice(8, 40));
    const [stored] = readString(acc.data, 40);
    if (stored !== u.host) throw new Error(`name record ${record} holds "${stored}", not "${u.host}"`);
    steps.push({ label: "MEMO ID", value: u.host, account: record });
  }
  const assetAcc = await getAccount(rpc, asset);
  if (!assetAcc || assetAcc.owner !== CORE_PROGRAM) throw Object.assign(new Error(`Asset ${asset} not found`), { steps, url: u });
  const core = parseCoreAsset(assetAcc.data);
  steps.push({ label: "Asset", value: core.name, account: asset });
  steps.push({ label: "Owner", value: core.owner, account: core.owner });

  const file = await findProgramAddress([enc.encode("file"), b58decode(asset), await sha256(enc.encode(u.path))], WEB_PROGRAM);
  const fileAcc = await getAccount(rpc, file);
  if (!fileAcc) throw Object.assign(new Error(`${u.path} not found in this Asset`), { steps, url: u, notFound: true });
  const d = fileAcc.data;
  if (fileAcc.owner !== WEB_PROGRAM || d.length < FILE_HEADER || !eq(d.slice(0, 8), await discriminator("WebFile")))
    throw new Error(`file ${file} has an unexpected owner or layout`);
  if (b58encode(d.slice(8, 40)) !== asset) throw new Error(`file ${file} belongs to another Asset`);
  const size = new DataView(d.buffer, d.byteOffset + 104, 4).getUint32(0, true);
  const contentType = dec.decode(d.slice(108, 156)).replace(/\0+$/, "");
  const content = d.slice(FILE_HEADER, FILE_HEADER + size);
  const verified = content.length === size && eq(await sha256(content), d.slice(72, 104));
  steps.push({ label: "File", value: `${u.path} (${size} bytes)`, account: file });
  return { url: u, asset, core, file, contentType, content, verified, steps };
}
