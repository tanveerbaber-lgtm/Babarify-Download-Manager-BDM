// BDM extension service worker: download capture, context menus and
// optional media detection. All listeners are registered at the top level so
// the worker wakes up for them.

import {
  classifyMedia,
  downloadMessage,
  explainCookieRefusal,
  headersToKeep,
  hostOf,
  isPanelMessage,
  normalizeSettings,
  rankDetected,
  sentCookies,
  shouldCapture,
  signInCookies,
  signInPermissions,
  signInSiteFor,
  baseSite,
} from "./rules.js";
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

// ------------------------------------------------------- sign-in downloads

/**
 * Adds the site's cookies to a message when the user allowed sign-in
 * downloads for that site. Never for private windows.
 */
async function withSignIn(msg, incognito = false) {
  if (incognito || !msg?.url) return msg;
  const { signInSites } = await getSettings();
  const site = signInSiteFor(msg.url, signInSites);
  if (!site || !chrome.cookies) return msg;
  try {
    if (!(await chrome.permissions.contains(signInPermissions(site)))) return msg;
    const cookies = signInCookies(await chrome.cookies.getAll({ url: msg.url }));
    return cookies.length ? { ...msg, cookies } : msg;
  } catch {
    return msg;
  }
}

/** Every video detected on any tab, most recent lists first. */
async function allDetected() {
  const all = await chrome.storage.session.get(null);
  return Object.entries(all).filter(([k]) => k.startsWith("media:")).flatMap(([, list]) => list);
}

/** The detected video a media message is about, if media detection saw it. */
async function detectedEntry(msg) {
  if (!["media", "formats", "mediaDownload"].includes(msg?.type)) return null;
  return (await allDetected()).find((m) => m.url === msg.url) ?? null;
}

/**
 * Sends to BDM with the headers the browser sent for a detected video (so
 * yt-dlp asks for it exactly as the page did) and sign-in cookies where
 * allowed. A refusal by a server the browser sent cookies to is explained.
 */
async function send(msg, incognito = false) {
  const entry = await detectedEntry(msg);
  const withHeaders = entry?.headers && Object.keys(entry.headers).length ? { ...msg, headers: entry.headers } : msg;
  const reply = await sendToBdm(await withSignIn(withHeaders, incognito));
  const { signInSites } = await getSettings();
  return explainCookieRefusal(reply, msg.url, entry, signInSites);
}

/** Opens the options page at the sign-in section, suggesting `host`'s site. */
function openSignInOptions(host) {
  const site = baseSite(host);
  const page = chrome.runtime.getURL("options/options.html");
  chrome.tabs.create({ url: site ? `${page}#signin=${encodeURIComponent(site)}` : `${page}#signin` });
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

  const reply = await send(downloadMessage(item));
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
    reply = await send({ type: "download", url: info.linkUrl, referrer: info.pageUrl }, tab?.incognito);
  } else if (info.menuItemId === MENU_SOURCE && info.srcUrl) {
    reply = await send({ type: "download", url: info.srcUrl, referrer: info.pageUrl }, tab?.incognito);
  } else if (info.menuItemId === MENU_MEDIA) {
    // A link to a video page, or the page itself; BDM asks which quality.
    const url = info.linkUrl || info.pageUrl;
    reply = await send({ type: "media", url, pageTitle: info.linkUrl ? undefined : tab?.title }, tab?.incognito);
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

/** Request headers by request id, until the response shows whether it's media. */
const sentHeaders = new Map();
const MAX_PENDING_HEADERS = 500;

function onSendHeaders(details) {
  if (details.tabId < 0) return;
  if (sentHeaders.size >= MAX_PENDING_HEADERS) sentHeaders.delete(sentHeaders.keys().next().value);
  sentHeaders.set(details.requestId, { headers: headersToKeep(details.requestHeaders), cookies: sentCookies(details.requestHeaders) });
}

function forgetRequest(details) {
  sentHeaders.delete(details.requestId);
}

function onHeaders(details) {
  const sent = sentHeaders.get(details.requestId);
  sentHeaders.delete(details.requestId);
  if (details.tabId < 0 || details.statusCode >= 400) return;
  const header = (name) => details.responseHeaders?.find((h) => h.name.toLowerCase() === name)?.value;
  const media = classifyMedia(details.url, header("content-type"));
  if (!media) return;
  getSettings().then((s) => {
    if (!s.detectMedia) return;
    addDetected(details.tabId, {
      url: details.url,
      ...media,
      size: Number(header("content-length")) || 0,
      at: Date.now(),
      headers: sent?.headers,
      sentCookies: sent?.cookies === true,
    });
  });
}

function enableMediaDetection() {
  if (!chrome.webRequest || chrome.webRequest.onHeadersReceived.hasListener(onHeaders)) return;
  const filter = { urls: ["http://*/*", "https://*/*"], types: ["media", "xmlhttprequest", "other"] };
  // "extraHeaders" makes Referer, Origin and the like visible, as they were sent.
  chrome.webRequest.onSendHeaders.addListener(onSendHeaders, filter, ["requestHeaders", "extraHeaders"]);
  chrome.webRequest.onErrorOccurred.addListener(forgetRequest, filter);
  chrome.webRequest.onHeadersReceived.addListener(onHeaders, filter, ["responseHeaders"]);
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
    // The panel's "Use My Browser Sign-In". The site must be the tab's own or
    // one of the video servers detected on it; a page can't pick any site.
    if (msg?.type === "allowSignIn" && Object.keys(msg).every((k) => k === "type" || k === "site")) {
      const tabSite = baseSite(hostOf(sender.tab.url ?? sender.url ?? ""));
      if (!msg.site || msg.site === tabSite) {
        openSignInOptions(tabSite);
        return false;
      }
      chrome.storage.session.get(`media:${sender.tab.id}`).then((stored) => {
        const seen = (stored[`media:${sender.tab.id}`] ?? []).some((m) => baseSite(hostOf(m.url)) === msg.site);
        openSignInOptions(seen ? msg.site : tabSite);
      });
      return false;
    }
    // Videos media detection saw on this tab, main video first (addresses
    // only), and the tab's title for players inside frames.
    if (msg?.type === "detectedMedia" && Object.keys(msg).length === 1) {
      chrome.storage.session.get(`media:${sender.tab.id}`).then((stored) => {
        const list = rankDetected(stored[`media:${sender.tab.id}`] ?? []);
        sendResponse({ media: list.map(({ url, kind, size }) => ({ url, kind, size })), title: sender.tab.title ?? "" });
      });
      return true;
    }
    if (!isPanelMessage(msg)) return false;
    send(msg, sender.tab.incognito).then(sendResponse);
    return true;
  }
  if (msg?.type === "send") {
    send(msg.payload).then(async (reply) => {
      if (reply.ok) await recordSuccess();
      else await recordError(reply.error || "BDM couldn't take that.");
      sendResponse(reply);
    });
    return true;
  }
  return false;
});
