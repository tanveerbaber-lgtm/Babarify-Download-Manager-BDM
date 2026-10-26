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
  /** Show the "Download With BDM" panel on videos (needs optional permissions). */
  videoPanel: false,
  /** Sites whose sign-in cookies BDM may use (each granted separately). */
  signInSites: [],
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
    videoPanel: s.videoPanel === true,
    signInSites: parseList(s.signInSites).map(siteOf).filter((h) => /^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(h)),
  };
}

/** The site a host belongs to for sign-in purposes: "www.youtube.com" → "youtube.com". */
export function siteOf(host) {
  return String(host ?? "").trim().toLowerCase().replace(/^\*\./, "").replace(/^(www|m)\./, "");
}

const SECOND_LEVEL = ["co", "com", "net", "org", "gov", "edu", "ac", "gob", "nic"];

/**
 * The site a host's cookies usually belong to, for sites BDM suggests:
 * "player.cdn.example.net" → "example.net", "www.bbc.co.uk" → "bbc.co.uk".
 * Sites the user types are kept as entered (see siteOf).
 */
export function baseSite(host) {
  const labels = siteOf(host).split(".").filter(Boolean);
  if (labels.length <= 2 || /^\d+$/.test(labels.at(-1))) return labels.join(".");
  const keep = labels.at(-1).length === 2 && SECOND_LEVEL.includes(labels.at(-2)) ? 3 : 2;
  return labels.slice(-keep).join(".");
}

/** The allowed sign-in site covering `url`, or "". */
export function signInSiteFor(url, sites) {
  if (!isHttpUrl(url)) return "";
  const host = hostOf(url);
  return sites.find((s) => host === s || host.endsWith(`.${s}`)) ?? "";
}

/** Permissions needed to read one site's cookies. */
export function signInPermissions(site) {
  return { permissions: ["cookies"], origins: [`*://${site}/*`, `*://*.${site}/*`] };
}

const MAX_COOKIES = 300;
const MAX_COOKIE_BYTES = 40 * 1024;
const CONTROL = /[\u0000-\u001f\u007f]/;

/**
 * chrome.cookies results → the fields BDM accepts. Cookies BDM would refuse
 * (odd characters, too large) are left out rather than failing the request.
 */
export function signInCookies(list) {
  const out = [];
  let bytes = 0;
  for (const c of Array.isArray(list) ? list : []) {
    const name = String(c?.name ?? "");
    const value = String(c?.value ?? "");
    if (!name || CONTROL.test(name) || CONTROL.test(value) || /[=; ]/.test(name) || value.includes(";")) continue;
    if (name.length > 256 || value.length > 8192) continue;
    bytes += name.length + value.length;
    if (out.length >= MAX_COOKIES || bytes > MAX_COOKIE_BYTES) break;
    const cookie = {
      name,
      value,
      domain: String(c.domain ?? ""),
      path: String(c.path || "/"),
      secure: c.secure === true,
      httpOnly: c.httpOnly === true,
      hostOnly: c.hostOnly === true,
    };
    if (!c.session && Number.isFinite(c.expirationDate)) cookie.expires = Math.floor(c.expirationDate);
    out.push(cookie);
  }
  return out;
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

const KEPT_HEADERS = ["user-agent", "referer", "origin", "accept", "accept-language"];
const MAX_KEPT_HEADERS = 32;

/**
 * The request headers worth repeating when BDM fetches a detected video:
 * what describes the request, plus the page's own X- headers. Cookies and
 * Authorization are never taken (sign-in goes through sign-in downloads).
 */
export function headersToKeep(requestHeaders) {
  const out = {};
  for (const h of Array.isArray(requestHeaders) ? requestHeaders : []) {
    const name = String(h?.name ?? "");
    const value = String(h?.value ?? "");
    const lower = name.toLowerCase();
    if (!/^[A-Za-z0-9-]{1,64}$/.test(name) || value.length > 4096 || /[\u0000-\u001f\u007f]/.test(value)) continue;
    if (!KEPT_HEADERS.includes(lower) && !(lower.startsWith("x-") && !lower.startsWith("x-forwarded"))) continue;
    if (Object.keys(out).length >= MAX_KEPT_HEADERS) break;
    out[name] = value;
  }
  return out;
}

/** Whether the browser sent cookies with a request (their values are never kept). */
export function sentCookies(requestHeaders) {
  return (Array.isArray(requestHeaders) ? requestHeaders : []).some((h) => String(h?.name ?? "").toLowerCase() === "cookie");
}

/**
 * When a video's server refused BDM and the browser had sent it cookies,
 * says which site's sign-in is missing so the panel can offer to allow it.
 * `entry` is the detected video; `sites` are the allowed sign-in sites.
 */
export function explainCookieRefusal(reply, url, entry, sites) {
  if (!reply || reply.ok || reply.signIn || !entry?.sentCookies || signInSiteFor(url, sites)) return reply;
  if (!/\b40[13]\b|refused access|forbidden|unauthori[sz]ed/i.test(reply.error ?? "")) return reply;
  const site = baseSite(hostOf(url));
  return {
    ...reply,
    signIn: true,
    signInSite: site,
    error: `This video's server (${site}) wants the cookies your browser sends it. Allow sign-in downloads for ${site}, then try again.`,
  };
}

/** Addresses that usually belong to advertising rather than the page's video. */
const AD_PATTERN = /(^|[./_-])(ads?|adserver|advert\w*|doubleclick|googlesyndication|imasdk|vast|vpaid|preroll|midroll)([./_?-]|$)/i;
const MASTER_PATTERN = /(master|index|playlist|manifest|main)[^/]*\.(m3u8|mpd)(\?|#|$)/i;

/**
 * Orders media detected on a page so the page's main video comes first:
 * ads last, streams before plain files, master playlists before variants
 * (then in load order), and larger files before smaller ones.
 */
export function rankDetected(entries) {
  const list = (Array.isArray(entries) ? entries : []).filter((m) => m && isHttpUrl(m.url) && !SEGMENT_PATTERN.test(m.url));
  const score = (m) => [
    AD_PATTERN.test(m.url) ? 1 : 0,
    m.kind === "stream" ? 0 : 1,
    m.kind === "stream" && MASTER_PATTERN.test(m.url) ? 0 : 1,
  ];
  return [...list].sort((a, b) => {
    const [sa, sb] = [score(a), score(b)];
    for (let i = 0; i < sa.length; i++) if (sa[i] !== sb[i]) return sa[i] - sb[i];
    if (a.kind === "stream") return (a.at ?? 0) - (b.at ?? 0);
    return (b.size ?? 0) - (a.size ?? 0);
  });
}

/** Messages page scripts may send; anything else is ignored. */
export function isPanelMessage(msg) {
  if (!msg || typeof msg !== "object" || !isHttpUrl(msg.url)) return false;
  // A stream taken from the page's player names the page it plays on.
  const referrerOk = msg.referrer === undefined || isHttpUrl(msg.referrer);
  if (msg.type === "formats") return referrerOk && Object.keys(msg).every((k) => ["type", "url", "referrer"].includes(k));
  if (msg.type === "mediaDownload") {
    const okKeys = Object.keys(msg).every((k) => ["type", "url", "title", "options", "label", "size", "referrer"].includes(k));
    return (
      okKeys &&
      referrerOk &&
      typeof msg.options === "object" &&
      msg.options !== null &&
      (msg.title === undefined || typeof msg.title === "string") &&
      (msg.label === undefined || typeof msg.label === "string") &&
      (msg.size === undefined || msg.size === null || (Number.isInteger(msg.size) && msg.size >= 0))
    );
  }
  return false;
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
