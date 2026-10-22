// Runs in the sandboxed page (opaque origin, no extension APIs, no network: see the sandbox CSP
// in manifest.json). It receives one document from the viewer and replaces itself with it.
window.addEventListener("message", (e) => {
  if (e.source !== window.parent || !e.data || e.data.type !== "home-render") return;
  document.open();
  document.write(e.data.html);
  document.close();
}, { once: true });
window.parent.postMessage({ type: "home-ready" }, "*");
