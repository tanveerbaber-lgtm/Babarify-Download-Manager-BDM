// Run with: npm run test:extension
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  basename,
  classifyMedia,
  downloadMessage,
  extensionOf,
  hostExcluded,
  isPanelMessage,
  normalizeSettings,
  parseList,
  shouldCapture,
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
  assert.equal(normalizeSettings({}).videoPanel, false, "panel is off until the user turns it on");
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
  assert.deepEqual(m.optional_permissions, ["webRequest", "scripting"], "panel and media detection ask only when turned on");
});
