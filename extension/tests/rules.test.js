// Run with: npm run test:extension
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  basename,
  classifyMedia,
  downloadMessage,
  explainCookieRefusal,
  extensionOf,
  baseSite,
  headersToKeep,
  hostExcluded,
  isPanelMessage,
  normalizeSettings,
  parseList,
  rankDetected,
  sentCookies,
  shouldCapture,
  signInCookies,
  signInPermissions,
  signInSiteFor,
  siteOf,
} from "../src/rules.js";

const SELF = "emaohblojdddihemfkbcchbfgaaanjkb";
const settings = normalizeSettings({});
const item = (over = {}) => ({
  id: 1,
  url: "https://cdn.example.com/files/setup.exe",
  finalUrl: "https://cdn.example.com/files/setup.exe",
  filename: "C:\\Users\\me\\Downloads\\setup.exe",
  mime: "application/x-msdownload",
  referrer: "https://example.com/download",
  totalBytes: 50 * 1024 * 1024,
  incognito: false,
  state: "in_progress",
  ...over,
});

test("captures selected file types", () => {
  assert.equal(shouldCapture(item(), settings, SELF).capture, true);
  assert.equal(shouldCapture(item({ filename: "", url: "https://e.com/a.zip?x=1", finalUrl: "" }), settings, SELF).capture, true);
  // Type known only from the MIME type.
  assert.equal(shouldCapture(item({ filename: "", url: "https://e.com/get", finalUrl: "", mime: "application/pdf" }), settings, SELF).capture, true);
});

test("leaves other downloads to the browser", () => {
  const no = (over, s = settings) => shouldCapture(item(over), s, SELF);
  assert.equal(no({ filename: "notes.txt", url: "https://e.com/notes.txt", finalUrl: "" }).capture, false);
  assert.equal(no({ incognito: true }).capture, false, "never in private windows");
  assert.equal(no({ byExtensionId: SELF }).capture, false, "not our own fallback downloads");
  assert.equal(no({ url: "blob:https://e.com/1", finalUrl: "" }).capture, false);
  assert.equal(no({ url: "data:application/zip;base64,AA", finalUrl: "" }).capture, false);
  assert.equal(no({ state: "complete" }).capture, false);
  assert.equal(no({}, normalizeSettings({ capture: false })).capture, false);
  assert.equal(no({}, normalizeSettings({ excludedHosts: ["example.com"] })).capture, false, "subdomain of excluded site");
  assert.equal(no({ totalBytes: 1000 }, normalizeSettings({ minSizeMb: 1 })).capture, false);
  // Unknown size isn't treated as "too small".
  assert.equal(no({ totalBytes: -1 }, normalizeSettings({ minSizeMb: 1 })).capture, true);
});

test("builds a minimal download message", () => {
  assert.deepEqual(downloadMessage(item()), {
    type: "download",
    url: "https://cdn.example.com/files/setup.exe",
    fileName: "setup.exe",
    referrer: "https://example.com/download",
    mime: "application/x-msdownload",
    size: 50 * 1024 * 1024,
  });
  const m = downloadMessage(item({ referrer: "chrome://newtab", totalBytes: 0, mime: "" }));
  assert.equal(m.referrer, undefined, "non-http referrers are dropped");
  assert.equal(m.size, undefined);
  assert.equal(Object.keys(m).includes("cookies"), false);
});

test("normalizes settings safely", () => {
  const s = normalizeSettings({ extensions: ".ZIP, exe ,, ../bad, mp4", excludedHosts: "*.Example.com\nhttps://bad.example/path", minSizeMb: -5 });
  assert.deepEqual(s.extensions, ["zip", "exe", "mp4"]);
  assert.deepEqual(s.excludedHosts, ["example.com"]);
  assert.equal(s.minSizeMb, 0);
  assert.equal(normalizeSettings(null).capture, true);
  assert.deepEqual(parseList("a b,a;c"), ["a", "b", "c"]);
});

test("page scripts may only ask for formats or a media download", () => {
  assert.equal(isPanelMessage({ type: "formats", url: "https://www.youtube.com/watch?v=x" }), true);
  assert.equal(isPanelMessage({ type: "mediaDownload", url: "https://e.com/v", title: "t", options: { mode: "audio", format: "mp3" } }), true);
  assert.equal(isPanelMessage({ type: "download", url: "https://e.com/a.zip" }), false, "no plain downloads from pages");
  assert.equal(isPanelMessage({ type: "media", url: "https://e.com" }), false);
  assert.equal(isPanelMessage({ type: "formats", url: "file:///C:/x" }), false);
  assert.equal(isPanelMessage({ type: "formats", url: "https://e.com", extra: 1 }), false);
  assert.equal(isPanelMessage({ type: "mediaDownload", url: "https://e.com" }), false, "options required");
  assert.equal(isPanelMessage({ type: "mediaDownload", url: "https://e.com/v", options: {}, label: "MP4 · 1080p", size: 1234 }), true);
  assert.equal(isPanelMessage({ type: "mediaDownload", url: "https://e.com/v", options: {}, size: "big" }), false);
  // A stream from the page's player carries the page as referrer.
  assert.equal(isPanelMessage({ type: "formats", url: "https://cdn.e.net/master.m3u8", referrer: "https://e.com/watch/1" }), true);
  assert.equal(isPanelMessage({ type: "formats", url: "https://cdn.e.net/master.m3u8", referrer: "javascript:alert(1)" }), false);
  assert.equal(isPanelMessage({ type: "mediaDownload", url: "https://cdn.e.net/v.mp4", options: {}, referrer: "https://e.com/watch/1" }), true);
  assert.equal(normalizeSettings({}).videoPanel, false, "panel is off until the user turns it on");
});

test("sign-in downloads are per site and off by default", () => {
  assert.deepEqual(normalizeSettings({}).signInSites, []);
  const s = normalizeSettings({ signInSites: "www.YouTube.com\n*.example.org\nbad site\nlocalhost" });
  assert.deepEqual(s.signInSites, ["youtube.com", "example.org"]);
  assert.equal(siteOf("m.facebook.com"), "facebook.com");
  assert.equal(signInSiteFor("https://music.youtube.com/watch?v=1", s.signInSites), "youtube.com");
  assert.equal(signInSiteFor("https://notyoutube.com/", s.signInSites), "");
  assert.equal(signInSiteFor("file:///C:/x", s.signInSites), "");
  assert.deepEqual(signInPermissions("youtube.com"), { permissions: ["cookies"], origins: ["*://youtube.com/*", "*://*.youtube.com/*"] });
});

test("sign-in cookies keep only what BDM accepts", () => {
  const out = signInCookies([
    { name: "SID", value: "abc", domain: ".youtube.com", path: "/", secure: true, httpOnly: true, hostOnly: false, session: false, expirationDate: 1900000000.5, storeId: "0" },
    { name: "tmp", value: "x", domain: "www.youtube.com", path: "/", hostOnly: true, session: true },
    { name: "bad", value: "a\nb", domain: ".youtube.com", path: "/" },
    { name: "a b", value: "x", domain: ".youtube.com", path: "/" },
  ]);
  assert.deepEqual(out, [
    { name: "SID", value: "abc", domain: ".youtube.com", path: "/", secure: true, httpOnly: true, hostOnly: false, expires: 1900000000 },
    { name: "tmp", value: "x", domain: "www.youtube.com", path: "/", secure: false, httpOnly: false, hostOnly: true },
  ]);
  const many = Array.from({ length: 500 }, (_, i) => ({ name: `c${i}`, value: "v", domain: ".e.com", path: "/" }));
  assert.equal(signInCookies(many).length, 300);
  assert.deepEqual(signInCookies(null), []);
});

test("keeps request-describing headers only", () => {
  const kept = headersToKeep([
    { name: "User-Agent", value: "Mozilla/5.0" },
    { name: "Referer", value: "https://site.example/watch/1" },
    { name: "Origin", value: "https://site.example" },
    { name: "X-Playback-Session-Id", value: "abc" },
    { name: "Cookie", value: "s=1" },
    { name: "Authorization", value: "Bearer t" },
    { name: "Range", value: "bytes=0-" },
    { name: "X-Forwarded-For", value: "1.2.3.4" },
    { name: "X-Bad", value: "a\nb" },
  ]);
  assert.deepEqual(kept, {
    "User-Agent": "Mozilla/5.0",
    Referer: "https://site.example/watch/1",
    Origin: "https://site.example",
    "X-Playback-Session-Id": "abc",
  });
  assert.deepEqual(headersToKeep(undefined), {});
});

test("ranks the page's main video before ads", () => {
  const ranked = rankDetected([
    { url: "https://ads.example.net/promo/clip.mp4", kind: "file", size: 9e6, at: 1 },
    { url: "https://cdn.example.com/hls/720p/chunklist.m3u8", kind: "stream", size: 0, at: 3 },
    { url: "https://cdn.example.com/hls/master.m3u8", kind: "stream", size: 0, at: 2 },
    { url: "https://cdn.example.com/files/small.mp4", kind: "file", size: 2e6, at: 4 },
    { url: "https://cdn.example.com/files/movie.mp4", kind: "file", size: 900e6, at: 5 },
    { url: "https://cdn.example.com/hls/seg-12.ts", kind: "file", size: 1e6, at: 6 },
    { url: "https://imasdk.googleapis.com/js/preroll.mp4", kind: "file", size: 50e6, at: 0 },
  ]).map((m) => m.url.split("/").slice(-2).join("/"));
  assert.deepEqual(ranked, ["hls/master.m3u8", "720p/chunklist.m3u8", "files/movie.mp4", "files/small.mp4", "js/preroll.mp4", "promo/clip.mp4"]);
});

test("explains a refusal by a server that wanted the browser's cookies", () => {
  const url = "https://player.example.net/hls/master.m3u8";
  const refused = { ok: false, error: "The site refused access (HTTP 403). Updating yt-dlp in Settings → Media tools often fixes this." };
  const entry = { url, sentCookies: true };
  const r = explainCookieRefusal(refused, url, entry, []);
  assert.equal(r.signIn, true);
  assert.equal(r.signInSite, "example.net");
  assert.match(r.error, /sign-in downloads for example\.net/);
  assert.equal(explainCookieRefusal(refused, url, { url, sentCookies: false }, []), refused, "no cookies were involved");
  assert.equal(explainCookieRefusal(refused, url, entry, ["example.net"]), refused, "already allowed");
  assert.equal(explainCookieRefusal({ ok: false, error: "Network problem" }, url, entry, []).signIn, undefined);
  assert.equal(sentCookies([{ name: "Cookie", value: "a=1" }]), true);
  assert.equal(baseSite("player.cdn.example.net"), "example.net");
  assert.equal(baseSite("www.bbc.co.uk"), "bbc.co.uk");
  assert.equal(baseSite("videos.site.com.pk"), "site.com.pk");
  assert.equal(baseSite("music.youtube.com"), "youtube.com");
  assert.equal(baseSite("127.0.0.1"), "127.0.0.1");
  assert.equal(sentCookies([{ name: "Accept", value: "*/*" }]), false);
});

test("helpers", () => {
  assert.equal(basename("C:\\dl\\a b.zip"), "a b.zip");
  assert.equal(extensionOf("/x/movie.MKV?t=1"), "mkv");
  assert.equal(extensionOf(".hidden"), "");
  assert.equal(hostExcluded("a.b.example.com", ["example.com"]), true);
  assert.equal(hostExcluded("notexample.com", ["example.com"]), false);
});

test("classifies media responses", () => {
  assert.deepEqual(classifyMedia("https://v.example.com/master.m3u8", "application/vnd.apple.mpegurl"), { kind: "stream", label: "M3U8 stream" });
  assert.deepEqual(classifyMedia("https://v.example.com/clip.mp4", "video/mp4"), { kind: "file", label: "MP4" });
  assert.equal(classifyMedia("https://v.example.com/seg-12.ts", "video/mp2t"), null, "stream segments ignored");
  assert.equal(classifyMedia("https://v.example.com/chunk.m4s", "video/iso.segment"), null);
  assert.equal(classifyMedia("https://e.com/page", "text/html"), null);
  assert.equal(classifyMedia("blob:https://e.com/x", "video/mp4"), null);
});

test("manifest key yields the extension ID the native host allows", () => {
  const manifest = JSON.parse(readFileSync(new URL("../manifest.json", import.meta.url), "utf8"));
  const der = Buffer.from(manifest.key, "base64");
  const id = [...createHash("sha256").update(der).digest("hex").slice(0, 32)].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
  assert.equal(id, SELF);
  const rust = readFileSync(new URL("../../src-tauri/src/browser/mod.rs", import.meta.url), "utf8");
  assert.ok(rust.includes(`"${id}"`), "EXTENSION_IDS in browser/mod.rs lists this ID");
});

test("manifest asks only for what it needs", () => {
  const m = JSON.parse(readFileSync(new URL("../manifest.json", import.meta.url), "utf8"));
  assert.equal(m.manifest_version, 3);
  assert.deepEqual([...m.permissions].sort(), ["activeTab", "contextMenus", "downloads", "nativeMessaging", "storage"]);
  assert.equal(m.host_permissions, undefined, "no host access unless media detection is turned on");
  assert.deepEqual(m.optional_permissions, ["webRequest", "scripting", "cookies"], "panel, media detection and sign-in ask only when turned on");
});
