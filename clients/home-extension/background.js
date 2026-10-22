// Address bar entry points. Chrome has no API for a new URL scheme, so three routes lead to
// viewer.html#home://...:
//   1. "home" keyword:   type `home xen-artist.memo/about.html`
//   2. .memo hosts:      type `xen-artist.memo/` (Chrome tries http://xen-artist.memo/, redirected)
//   3. home:// typed:    Chrome sends `home://xen-artist.memo` (and a bare `xen-artist.memo`) to the
//                        search engine; searches that start with home:// or are exactly a .memo
//                        name (Google, Bing, DuckDuckGo) are redirected

const VIEWER = chrome.runtime.getURL("viewer.html");
const Q = "(?:[^#]*&)?q=(?:web%2B|web\\+)?home(?:%3A|:)(?:%2F|/)(?:%2F|/)([^&#]*)";
const N = "(?:[^#]*&)?q=((?:[a-z0-9-]+\\.)+memo(?:(?:%2F|/)[^&#]*)?)(?:&.*)?$";

async function installRules() {
  const rules = [
    { id: 1, regex: "^https?://((?:[a-z0-9-]+\\.)+memo)(/[^?#]*)?(?:[?#].*)?$", to: `${VIEWER}#home://\\1\\2` },
    { id: 2, regex: `^https://www\\.google\\.com/search\\?${Q}.*$`, to: `${VIEWER}#q=\\1` },
    { id: 3, regex: `^https://www\\.bing\\.com/search\\?${Q}.*$`, to: `${VIEWER}#q=\\1` },
    { id: 4, regex: `^https://duckduckgo\\.com/\\?${Q}.*$`, to: `${VIEWER}#q=\\1` },
    { id: 5, regex: `^https://www\\.google\\.com/search\\?${N}`, to: `${VIEWER}#q=\\1` },
    { id: 6, regex: `^https://www\\.bing\\.com/search\\?${N}`, to: `${VIEWER}#q=\\1` },
    { id: 7, regex: `^https://duckduckgo\\.com/\\?${N}`, to: `${VIEWER}#q=\\1` },
  ];
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: rules.map((r) => r.id),
    addRules: rules.map((r) => ({
      id: r.id,
      priority: 1,
      action: { type: "redirect", redirect: { regexSubstitution: r.to } },
      condition: { regexFilter: r.regex, resourceTypes: ["main_frame"] },
    })),
  });
}

chrome.runtime.onInstalled.addListener(installRules);
chrome.runtime.onStartup.addListener(installRules);

function open(url, disposition = "currentTab") {
  if (disposition === "currentTab") chrome.tabs.update({ url });
  else chrome.tabs.create({ url, active: disposition === "newForegroundTab" });
}

chrome.omnibox.setDefaultSuggestion({ description: "Open home://%s" });
chrome.omnibox.onInputEntered.addListener((text, disposition) => {
  open(`${VIEWER}#home://${text.trim().replace(/^(web\+)?home:\/\//i, "")}`, disposition);
});

chrome.action.onClicked.addListener(() => open(`${VIEWER}#home://xen-artist.memo/`, "newForegroundTab"));
