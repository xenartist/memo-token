// node --test test/home.test.mjs  (Node 20+). Expected PDAs come from `solana find-program-derived-address`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { b58decode, b58encode, findProgramAddress, isValidName, parseHomeUrl, resolveHref, WEB_PROGRAM } from "../lib/home.js";

const enc = new TextEncoder();
const HOME_TESTNET = "J1pxwy8V4xKvPgS5HMUn5jFo3u7At3MW4BvFWKg48Qcv";

test("base58 round trip", () => {
  for (const s of [WEB_PROGRAM, "11111111111111111111111111111111", HOME_TESTNET]) assert.equal(b58encode(b58decode(s)), s);
});

test("PDAs match the Solana CLI", async () => {
  for (const [seeds, program, expected] of EXPECTED) assert.equal(await findProgramAddress(seeds.map((s) => enc.encode(s)), program), expected);
});

test("names", () => {
  assert.ok(isValidName("xen-artist.memo") && isValidName("blog.xen-artist.memo"));
  for (const bad of ["a.x1", "-a.memo", "A.memo", "xn--a.memo", "a_b.memo", ".memo"]) assert.ok(!isValidName(bad), bad);
});

test("parse and resolve URLs", () => {
  assert.equal(parseHomeUrl("home://Xen-Artist.memo").url, "home://xen-artist.memo/index.html");
  assert.equal(parseHomeUrl("xen-artist.memo/blog/").url, "home://xen-artist.memo/blog/index.html");
  assert.equal(parseHomeUrl("web+home://xen-artist.memo/a/../b.html?v=3#x").url, "home://xen-artist.memo/b.html");
  assert.equal(parseHomeUrl(`home://${WEB_PROGRAM}/`).kind, "asset");
  assert.throws(() => parseHomeUrl("home://xen-artist.x1/"));
  const cur = "home://xen-artist.memo/blog/post.html";
  assert.equal(resolveHref("../about.html", cur), "home://xen-artist.memo/about.html");
  assert.equal(resolveHref("/", cur), "home://xen-artist.memo/index.html");
  assert.equal(resolveHref("home://other.memo/x.html", cur), "home://other.memo/x.html");
  assert.equal(resolveHref("https://example.com/", cur), null);
});

const EXPECTED = [
  [["burn_guard"], HOME_TESTNET, "2YdkLa2rjficaU99n5guu5Vx6y4k72khgegAHyCKqrip"],
  [["authority"], HOME_TESTNET, "4WB5wWajgPiCBT3sp2FDEN75KyC4akYwoWDVfMECo2aU"],
  [["name", "xen-artist.memo"], WEB_PROGRAM, "9yVsW5QAHmvpGup8sxPCKYozMWVyDhg28tifcw4ABD1R"],
  [["file"], WEB_PROGRAM, "7xKx9NW58natZ3fd759ktz2HAAmAbEQeY8X5e2khmfmL"],
];
