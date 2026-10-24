// BDM extension service worker: download capture, context menus and
// optional media detection. All listeners are registered at the top level so
// the worker wakes up for them.

import { classifyMedia, downloadMessage, isPanelMessage, normalizeSettings, shouldCapture } from "./rules.js";
import { connectionState, sendToBdm } from "./native.js";

const MENU_LINK = "bdm-download-link";
const MENU_MEDIA = "bdm-download-media";
const MENU_SOURCE = "bdm-download-source";
const MAX_MEDIA_PER_TAB = 30;
/** How long a URL we handed back to the browser stays exempt from capture. */
const FALLBACK_GRACE_MS = 60_000;
/** How long a "BDM isn't connected" result is trusted before asking again. */
const CONNECTION_CACHE_MS = 60_000;

async function getSettings() {
  const { settings } = await chrome.storage.sync.get("settings");
  return normalizeSettings(settings);
}

// ------------------------------------------------------------ status badge

async function flashBadge(text, color, title) {
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setBadgeText({ text });
  if (title) await chrome.action.setTitle({ title });
  setTimeout(() => {
    chrome.action.setBadgeText({ text: "" });
    chrome.action.setTitle({ title: "Babarify Download Manager" });
  }, 4000);
}

async function recordError(message) {
  await chrome.storage.session.set({ lastError: { message, at: Date.now() } });
  await flashBadge("!", "#DC2626", `BDM: ${message}`);
}

async function recordSuccess() {
  await chrome.storage.session.remove("lastError");
  await flashBadge("✓", "#10B981");
}

// -------------------------------------------------------- download capture

/** URLs given back to the browser after BDM couldn't take them. */
async function isExempt(url) {
  const { exempt = {} } = await chrome.storage.session.get("exempt");
  return (exempt[url] ?? 0) > Date.now();
}

async function exempt(url) {
  const { exempt: map = {} } = await chrome.storage.session.get("exempt");
  const now = Date.now();
  for (const [k, until] of Object.entries(map)) if (until <= now) delete map[k];
  map[url] = now + FALLBACK_GRACE_MS;
  await chrome.storage.session.set({ exempt: map });
}

/** Lets the browser download it after all (BDM unavailable or refused). */
async function giveBack(item, reason) {
  const url = item.finalUrl || item.url;
  await exempt(url);
  try {
    await chrome.downloads.download({ url });
  } catch {
    // The user can still retry from the page.
  }
  await recordError(reason);
}

/** Whether BDM is reachable (running or startable), cached briefly. */
async function bdmReachable() {
  const { connection } = await chrome.storage.session.get("connection");
  if (connection && Date.now() - connection.at < CONNECTION_CACHE_MS) return connection.state !== "missing";
  const { state } = await connectionState();
  await chrome.storage.session.set({ connection: { state, at: Date.now() } });
  return state !== "missing";
}

async function capture(item) {
  const settings = await getSettings();
  if (await isExempt(item.finalUrl || item.url)) return;
  const decision = shouldCapture(item, settings, chrome.runtime.id);
  if (!decision.capture) return;
  // Leave the download alone if BDM can't take it anyway.
  if (!(await bdmReachable())) return;

  // Take the download away from the browser first, so it doesn't finish twice.
  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    return; // Already finished or gone; leave it to the browser.
  }

  const reply = await sendToBdm(downloadMessage(item));
  if (reply.ok) await recordSuccess();
  else await giveBack(item, reply.error || "BDM couldn't take the download.");
}

// onCreated fires for every download once the response headers have arrived
// (URL, MIME type and size are known). onDeterminingFilename is not used:
// Chrome skips it when a policy, DevTools or another extension controls
// file naming, which would silently disable capture.
chrome.downloads.onCreated.addListener((item) => {
  capture(item).catch((err) => recordError(String(err?.message ?? err)));
});

// ------------------------------------------------------------ context menus

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: MENU_LINK, title: "Download link with BDM", contexts: ["link"] });
    chrome.contextMenus.create({ id: MENU_SOURCE, title: "Download this file with BDM", contexts: ["video", "audio", "image"] });
    chrome.contextMenus.create({ id: MENU_MEDIA, title: "Download video or audio with BDM", contexts: ["page", "video", "audio", "link"] });
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  let reply;
  if (info.menuItemId === MENU_LINK && info.linkUrl) {
    reply = await sendToBdm({ type: "download", url: info.linkUrl, referrer: info.pageUrl });
  } else if (info.menuItemId === MENU_SOURCE && info.srcUrl) {
    reply = await sendToBdm({ type: "download", url: info.srcUrl, referrer: info.pageUrl });
  } else if (info.menuItemId === MENU_MEDIA) {
    // A link to a video page, or the page itself; BDM asks which quality.
    const url = info.linkUrl || info.pageUrl;
    reply = await sendToBdm({ type: "media", url, pageTitle: info.linkUrl ? undefined : tab?.title });
  } else {
    return;
  }
  if (reply.ok) await recordSuccess();
  else await recordError(reply.error || "BDM couldn't take that.");
});

// ---------------------------------------------------------- media detection

async function addDetected(tabId, entry) {
  const key = `media:${tabId}`;
  const { [key]: list = [] } = await chrome.storage.session.get(key);
  if (list.some((m) => m.url === entry.url)) return;
  const next = [entry, ...list].slice(0, MAX_MEDIA_PER_TAB);
  await chrome.storage.session.set({ [key]: next });
  // The tab may have closed meanwhile.
  await chrome.action.setBadgeBackgroundColor({ color: "#2563EB", tabId }).catch(() => {});
  await chrome.action.setBadgeText({ text: String(next.length), tabId }).catch(() => {});
}

function onHeaders(details) {
  if (details.tabId < 0 || details.statusCode >= 400) return;
  const header = (name) => details.responseHeaders?.find((h) => h.name.toLowerCase() === name)?.value;
  const media = classifyMedia(details.url, header("content-type"));
  if (!media) return;
  getSettings().then((s) => {
    if (!s.detectMedia) return;
    addDetected(details.tabId, { url: details.url, ...media, size: Number(header("content-length")) || 0, at: Date.now() });
  });
}

function enableMediaDetection() {
  if (chrome.webRequest && !chrome.webRequest.onHeadersReceived.hasListener(onHeaders)) {
    chrome.webRequest.onHeadersReceived.addListener(onHeaders, { urls: ["http://*/*", "https://*/*"], types: ["media", "xmlhttprequest", "other"] }, ["responseHeaders"]);
  }
}

// The webRequest API only exists once the user has granted it (optional permission).
enableMediaDetection();
chrome.permissions.onAdded.addListener(enableMediaDetection);

// A new page starts with an empty list. (Without the "tabs" permission the
// event carries no URL, so every page load clears the list.)
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === "loading") {
    chrome.storage.session.remove(`media:${tabId}`);
    chrome.action.setBadgeText({ text: "", tabId }).catch(() => {});
  }
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(`media:${tabId}`));

// ------------------------------------------------------ on-video panel

const PANEL_ID = "bdm-video-panel";
const PANEL_PERMISSIONS = { permissions: ["scripting"], origins: ["http://*/*", "https://*/*"] };

/** Registers the panel script when it's switched on and permitted; removes it otherwise. */
async function syncVideoPanel() {
  const { videoPanel } = await getSettings();
  const allowed = await chrome.permissions.contains(PANEL_PERMISSIONS);
  if (!chrome.scripting) return; // Not granted: the API doesn't exist.
  const existing = await chrome.scripting.getRegisteredContentScripts({ ids: [PANEL_ID] });
  if (videoPanel && allowed && existing.length === 0) {
    await chrome.scripting.registerContentScripts([{
      id: PANEL_ID,
      matches: ["http://*/*", "https://*/*"],
      js: ["src/video-panel.js"],
      runAt: "document_idle",
      allFrames: true,
      persistAcrossSessions: true,
    }]);
  } else if ((!videoPanel || !allowed) && existing.length > 0) {
    await chrome.scripting.unregisterContentScripts({ ids: [PANEL_ID] });
  }
}

const syncPanelSafely = () => syncVideoPanel().catch(() => {});
syncPanelSafely();
chrome.permissions.onAdded.addListener(syncPanelSafely);
chrome.permissions.onRemoved.addListener(syncPanelSafely);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "sync" && changes.settings) syncPanelSafely();
});

// Messages from the popup and from the on-video panel.
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  // Page scripts (sender.tab set) may only ask for formats or a media download.
  if (sender.tab) {
    if (!isPanelMessage(msg)) return false;
    sendToBdm(msg).then(sendResponse);
    return true;
  }
  if (msg?.type === "send") {
    sendToBdm(msg.payload).then(async (reply) => {
      if (reply.ok) await recordSuccess();
      else await recordError(reply.error || "BDM couldn't take that.");
      sendResponse(reply);
    });
    return true;
  }
  return false;
});
