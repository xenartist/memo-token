// The viewer: resolves the home:// URL in its fragment, reads the files from chain state, and
// hands one self-contained document to the sandboxed frame. Untrusted content never runs here:
// it runs in sandbox.html (opaque origin, no extension APIs, no network).

import { parseHomeUrl, resolve, resolveHref } from "./lib/home.js";

const $ = (id) => document.getElementById(id);
const DEFAULT_RPC = "http://127.0.0.1:8899";
let rpc = DEFAULT_RPC;
let current = null;
let frame = null;

// ---------------------------------------------------------------- settings

async function loadSettings() {
  try {
    const s = await chrome.storage.local.get("rpc");
    if (s.rpc) rpc = s.rpc;
  } catch {}
  $("rpc").value = rpc;
}

$("rpc").addEventListener("change", async () => {
  rpc = $("rpc").value;
  try {
    await chrome.storage.local.set({ rpc });
  } catch {}
  load();
});

// ---------------------------------------------------------------- navigation

function target() {
  const h = decodeURIComponent(location.hash.slice(1));
  if (h.startsWith("q=")) return decodeURIComponent(h.slice(2).replace(/\+/g, " "));
  return h || "home://xen-artist.memo/";
}

function go(url) {
  location.hash = url;
}

$("bar").addEventListener("submit", (e) => {
  e.preventDefault();
  try {
    go(parseHomeUrl($("address").value).url);
  } catch (err) {
    showError("Invalid address", err.message);
  }
});

$("badge").addEventListener("click", () => {
  const d = $("details");
  d.hidden = !d.hidden;
  $("badge").setAttribute("aria-expanded", String(!d.hidden));
});

window.addEventListener("hashchange", load);

window.addEventListener("message", (e) => {
  if (!frame || e.source !== frame.contentWindow || !e.data) return;
  const m = e.data;
  if (m.type === "home-navigate" && typeof m.href === "string") {
    const next = resolveHref(m.href, current);
    if (next) go(next);
    else external(m.href);
  } else if (m.type === "home-title" && typeof m.title === "string") {
    document.title = m.title ? `${m.title.slice(0, 120)} · ${current}` : current;
  }
});

function external(href) {
  const n = $("notice");
  n.replaceChildren("External link, not opened inside home://: ");
  if (/^https?:\/\//i.test(href)) {
    const a = document.createElement("a");
    a.href = href;
    a.textContent = href;
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    n.append(a);
  } else {
    n.append(href);
  }
  n.hidden = false;
}

// ---------------------------------------------------------------- loading

async function load() {
  $("notice").hidden = true;
  let url;
  try {
    url = parseHomeUrl(target()).url;
  } catch (err) {
    $("address").value = target();
    document.title = "home://";
    return showError("Invalid address", err.message);
  }
  if (location.hash.slice(1) !== url && decodeURIComponent(location.hash.slice(1)) !== url) {
    history.replaceState(null, "", `#${url}`);
  }
  current = url;
  $("address").value = url;
  document.title = url;
  setBadge("", "Loading…");
  let page;
  try {
    page = await resolve(rpc, url);
  } catch (err) {
    showDetails(err.steps || [], null);
    return showError(err.notFound ? "Not found" : "Cannot open this address", err.message);
  }
  const files = [page];
  let html;
  try {
    html = await render(page, files);
  } catch (err) {
    return showError("Cannot render this file", err.message);
  }
  const allVerified = files.every((f) => f.verified);
  setBadge(allVerified ? "ok" : "bad", allVerified ? "✓ On-chain, hashes match" : "✗ Hash mismatch");
  showDetails(page.steps, files);
  mount(html);
}

function setBadge(cls, text) {
  const b = $("badge");
  b.className = cls;
  b.textContent = text;
}

function showDetails(steps, files) {
  const dl = document.createElement("dl");
  const row = (k, v) => {
    const dt = document.createElement("dt");
    dt.textContent = k;
    const dd = document.createElement("dd");
    dd.textContent = v;
    dl.append(dt, dd);
  };
  row("RPC", rpc);
  for (const s of steps) row(s.label, s.account && s.account !== s.value ? `${s.value}  (${s.account})` : s.value);
  for (const f of (files || []).slice(1)) row("Included", `${f.url.path} (${f.content.length} bytes, ${f.verified ? "hash ok" : "HASH MISMATCH"})  (${f.file})`);
  const note = document.createElement("p");
  note.textContent = "Prototype: names and files come from the throwaway home_web_test program, not memo-id or memo-file. "
    + "The page runs in a sandbox with no network access; only on-chain content is shown.";
  $("details").replaceChildren(dl, note);
}

function showError(title, message) {
  setBadge("bad", "✗ " + title);
  const box = document.createElement("div");
  box.className = "error";
  const h = document.createElement("h1");
  h.textContent = title;
  const p = document.createElement("p");
  p.textContent = message;
  box.append(h, p);
  frame = null;
  $("stage").replaceChildren(box);
}

function mount(html) {
  const f = document.createElement("iframe");
  f.src = "sandbox.html";
  f.title = current;
  const ready = (e) => {
    if (e.source !== f.contentWindow || !e.data || e.data.type !== "home-ready") return;
    window.removeEventListener("message", ready);
    f.contentWindow.postMessage({ type: "home-render", html }, "*");
  };
  window.addEventListener("message", ready);
  frame = f;
  $("stage").replaceChildren(f);
}

// ---------------------------------------------------------------- rendering

const text = (bytes) => new TextDecoder().decode(bytes);

function dataUri(contentType, bytes) {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return `data:${contentType.split(";")[0]};base64,${btoa(bin)}`;
}

/** Loaded once per page, so the frame needs no network: links and form posts go through the viewer */
const INTERCEPT = `<script>(() => {
  document.addEventListener("click", (e) => {
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a) return;
    const href = a.getAttribute("href");
    if (href.startsWith("#")) return;
    e.preventDefault();
    parent.postMessage({ type: "home-navigate", href }, "*");
  }, true);
  const title = () => parent.postMessage({ type: "home-title", title: document.title }, "*");
  document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", title) : title();
})();</script>`;

/** Turn the file into one self-contained document; included files are appended to `files` */
async function render(page, files) {
  const type = page.contentType.split(";")[0].trim();
  if (type.startsWith("image/")) {
    return `<!doctype html><meta charset="utf-8"><title>${escape(page.url.path)}</title>${INTERCEPT}`
      + `<body style="margin:0;display:grid;place-items:center;min-height:100vh;background:#111">`
      + `<img src="${dataUri(page.contentType, page.content)}" style="max-width:100%;max-height:100vh"></body>`;
  }
  if (type !== "text/html") {
    return `<!doctype html><meta charset="utf-8"><title>${escape(page.url.path)}</title>${INTERCEPT}`
      + `<pre style="white-space:pre-wrap;font:13px ui-monospace,monospace;padding:16px">${escape(text(page.content))}</pre>`;
  }

  const doc = new DOMParser().parseFromString(text(page.content), "text/html");
  const fetchFile = async (href) => {
    const url = resolveHref(href, page.url.url);
    if (!url) return null;
    try {
      const f = await resolve(rpc, url);
      files.push(f);
      return f;
    } catch {
      return null;
    }
  };
  const inlineCssUrls = async (css) => {
    const refs = [...css.matchAll(/url\(\s*(['"]?)([^'")]+)\1\s*\)/g)];
    for (const m of refs) {
      if (/^(data:|#)/.test(m[2])) continue;
      const f = await fetchFile(m[2]);
      if (f) css = css.replace(m[0], `url("${dataUri(f.contentType, f.content)}")`);
    }
    return css;
  };

  for (const el of [...doc.querySelectorAll("link[rel~=stylesheet][href]")]) {
    const f = await fetchFile(el.getAttribute("href"));
    if (!f) continue;
    const style = doc.createElement("style");
    style.textContent = await inlineCssUrls(text(f.content));
    el.replaceWith(style);
  }
  for (const el of [...doc.querySelectorAll("style")]) el.textContent = await inlineCssUrls(el.textContent);
  for (const el of [...doc.querySelectorAll("script[src]")]) {
    const f = await fetchFile(el.getAttribute("src"));
    if (!f) continue;
    el.removeAttribute("src");
    el.textContent = text(f.content);
  }
  for (const el of [...doc.querySelectorAll("img[src], source[src], video[src], audio[src], link[rel~=icon][href]")]) {
    const attr = el.hasAttribute("src") ? "src" : "href";
    const f = await fetchFile(el.getAttribute(attr));
    if (f) el.setAttribute(attr, dataUri(f.contentType, f.content));
  }
  return "<!doctype html>" + doc.documentElement.outerHTML.replace(/<head[^>]*>/i, (h) => h + INTERCEPT);
}

function escape(s) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

await loadSettings();
load();
