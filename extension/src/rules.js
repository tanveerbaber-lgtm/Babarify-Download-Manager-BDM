// Pure decision logic for the BDM extension (no chrome.* calls), so it can be
// unit-tested with `node --test`.

/** Default file types BDM takes over when capture is on. */
export const DEFAULT_EXTENSIONS = [
  "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img",
  "exe", "msi", "msix", "appx", "dmg", "pkg", "deb", "rpm", "apk",
  "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv",
  "mp3", "m4a", "flac", "wav", "ogg", "opus", "aac",
  "pdf", "epub",
  "bin", "torrent",
];

export const DEFAULT_SETTINGS = Object.freeze({
  /** Take over matching browser downloads. */
  capture: true,
  /** File extensions (lowercase, no dot) that are captured. */
  extensions: DEFAULT_EXTENSIONS,
  /** Only capture files at least this big (MB) when the size is known; 0 = any size. */
  minSizeMb: 0,
  /** Sites never captured (matches the host and its subdomains). */
  excludedHosts: [],
  /** Watch pages for video/audio streams (needs optional permissions). */
  detectMedia: false,
});

/** Settings from storage, with defaults for anything missing or malformed. */
export function normalizeSettings(raw) {
  const s = { ...DEFAULT_SETTINGS, ...(raw && typeof raw === "object" ? raw : {}) };
  return {
    capture: s.capture !== false,
    extensions: parseList(s.extensions).map((e) => e.replace(/^\./, "")).filter((e) => /^[a-z0-9]{1,10}$/.test(e)),
    minSizeMb: Number.isFinite(Number(s.minSizeMb)) && Number(s.minSizeMb) >= 0 ? Number(s.minSizeMb) : 0,
    excludedHosts: parseList(s.excludedHosts).map((h) => h.replace(/^\*\./, "")).filter((h) => /^[a-z0-9.-]+$/.test(h)),
    detectMedia: s.detectMedia === true,
  };
}

/** Accepts an array or a comma/space/newline separated string. */
export function parseList(value) {
  const items = Array.isArray(value) ? value : String(value ?? "").split(/[\s,;]+/);
  return [...new Set(items.map((v) => String(v).trim().toLowerCase()).filter(Boolean))];
}

export function isHttpUrl(url) {
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

export function hostOf(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return "";
  }
}

/** "example.com" matches example.com and any subdomain of it. */
export function hostExcluded(host, excluded) {
  return excluded.some((e) => host === e || host.endsWith(`.${e}`));
}

/** Last path component of a Windows or URL path. */
export function basename(path) {
  return String(path ?? "").split(/[\\/]/).filter(Boolean).pop() ?? "";
}

export function extensionOf(name) {
  const base = basename(name).split("?")[0].split("#")[0];
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

const MIME_EXTENSIONS = {
  "application/zip": "zip",
  "application/x-zip-compressed": "zip",
  "application/x-7z-compressed": "7z",
  "application/vnd.rar": "rar",
  "application/x-rar-compressed": "rar",
  "application/x-msdownload": "exe",
  "application/x-msi": "msi",
  "application/pdf": "pdf",
  "application/x-iso9660-image": "iso",
  "video/mp4": "mp4",
  "video/webm": "webm",
  "video/x-matroska": "mkv",
  "audio/mpeg": "mp3",
  "audio/mp4": "m4a",
  "audio/flac": "flac",
};

/**
 * Decides whether a browser download should go to BDM.
 * `item` has the fields of chrome.downloads.DownloadItem we use.
 * Returns { capture: boolean, reason: string }.
 */
export function shouldCapture(item, settings, ownExtensionId) {
  const url = item.finalUrl || item.url;
  if (!settings.capture) return { capture: false, reason: "capture is off" };
  if (item.incognito) return { capture: false, reason: "private window" };
  if (item.byExtensionId && item.byExtensionId === ownExtensionId) return { capture: false, reason: "started by BDM" };
  if (item.state && item.state !== "in_progress") return { capture: false, reason: "already finished" };
  if (!isHttpUrl(url)) return { capture: false, reason: "not an http(s) link" };
  if (hostExcluded(hostOf(url), settings.excludedHosts)) return { capture: false, reason: "site excluded" };
  if (item.referrer && hostExcluded(hostOf(item.referrer), settings.excludedHosts)) return { capture: false, reason: "site excluded" };

  const ext = extensionOf(item.filename) || extensionOf(new URL(url).pathname) || MIME_EXTENSIONS[(item.mime || "").split(";")[0].trim().toLowerCase()] || "";
  if (!ext || !settings.extensions.includes(ext)) return { capture: false, reason: `file type "${ext || "unknown"}" not selected` };

  const size = Number(item.totalBytes ?? item.fileSize ?? -1);
  if (settings.minSizeMb > 0 && size > 0 && size < settings.minSizeMb * 1024 * 1024) {
    return { capture: false, reason: "smaller than the minimum size" };
  }
  return { capture: true, reason: `.${ext}` };
}

/** The message sent to BDM for a captured download. */
export function downloadMessage(item) {
  const msg = { type: "download", url: item.finalUrl || item.url };
  const name = basename(item.filename);
  if (name) msg.fileName = name;
  if (item.referrer && isHttpUrl(item.referrer)) msg.referrer = item.referrer;
  if (item.mime) msg.mime = item.mime;
  if (Number(item.totalBytes) > 0) msg.size = Number(item.totalBytes);
  return msg;
}

const MEDIA_TYPES = /^(video\/|audio\/|application\/(vnd\.apple\.mpegurl|x-mpegurl|dash\+xml))/i;
const MEDIA_EXTENSIONS = /\.(mp4|webm|mkv|mov|m4v|m3u8|mpd|mp3|m4a|aac|ogg|opus|flac|wav)(\?|#|$)/i;
/** Fragments of adaptive streams aren't useful on their own. */
const SEGMENT_PATTERN = /\.(ts|m4s)(\?|#|$)|[?&](range|bytestart)=|\/seg-?\d+/i;

/**
 * Classifies a network response seen by media detection.
 * Returns { kind: "stream" | "file", label } or null if it isn't media.
 */
export function classifyMedia(url, contentType) {
  if (!isHttpUrl(url) || SEGMENT_PATTERN.test(url)) return null;
  const type = String(contentType ?? "").split(";")[0].trim().toLowerCase();
  const byType = MEDIA_TYPES.test(type);
  const byExt = MEDIA_EXTENSIONS.test(new URL(url).pathname);
  if (!byType && !byExt) return null;
  const stream = /mpegurl|dash\+xml/.test(type) || /\.(m3u8|mpd)(\?|#|$)/i.test(url);
  const ext = extensionOf(new URL(url).pathname) || type.split("/")[1] || "media";
  return { kind: stream ? "stream" : "file", label: stream ? `${ext.toUpperCase()} stream` : ext.toUpperCase() };
}

/** Readable size for popups. */
export function formatSize(bytes) {
  if (!(bytes > 0)) return "";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 || v >= 100 ? 0 : 1)} ${units[i]}`;
}
