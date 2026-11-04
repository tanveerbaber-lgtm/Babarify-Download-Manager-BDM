// BDM "Download With BDM" panel. Injected into pages only after the user
// turns the panel on in the extension's options (which grants site access).
// Everything lives in a closed shadow root so pages can't restyle or read it.
(() => {
  // After the extension is updated or reloaded, a copy injected earlier can't
  // reach BDM any more. The extension injects a fresh copy into open tabs; it
  // takes over from a disconnected one but never runs twice next to a live one.
  if (window.__bdmVideoPanel?.alive()) return;
  window.__bdmVideoPanel?.stop();
  const alive = () => Boolean(chrome.runtime?.id);
  window.__bdmVideoPanel = { alive, stop };

  const MIN_WIDTH = 240;
  const STREAM_URL = /\.(m3u8|mpd)(\?|#|$)/i;
  const FILE_URL = /\.(mp4|webm|mkv|mov|m4v)(\?|#|$)/i;
  /** Pieces of adaptive streams aren't useful on their own. */
  const SEGMENT_URL = /\.(ts|m4s)(\?|#|$)|[?&](range|bytestart)=|\/seg-?\d+/i;
  /** Most player sources tried when BDM can't read the page itself. */
  const MAX_SOURCES = 5;

  // Keep enough network entries that a stream's master playlist isn't dropped.
  try {
    performance.setResourceTimingBufferSize(1000);
  } catch {
    // Not available in this frame.
  }
  const MIN_HEIGHT = 135;
  const BAR_HEIGHT = 26;

  const LOGO = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="132 120 760 800" width="15" height="16" aria-hidden="true"><defs><linearGradient id="bdmp-b" x1="0" y1="0.75" x2="1" y2="0.25"><stop offset="0" stop-color="#1D4ED8"/><stop offset="0.28" stop-color="#2563EB"/><stop offset="0.45" stop-color="#0EA5E9"/><stop offset="0.66" stop-color="#10B981"/><stop offset="1" stop-color="#34D399"/></linearGradient><linearGradient id="bdmp-fold" x1="0.1" y1="0.1" x2="0.9" y2="0.9"><stop offset="0" stop-color="#1E3A8A" stop-opacity="0.95"/><stop offset="0.55" stop-color="#1D4ED8" stop-opacity="0.7"/><stop offset="1" stop-color="#0EA5E9" stop-opacity="0.1"/></linearGradient><linearGradient id="bdmp-shine" x1="0" y1="0" x2="1" y2="0.6"><stop offset="0" stop-color="#BFE9FF" stop-opacity="0.75"/><stop offset="0.35" stop-color="#ffffff" stop-opacity="0.15"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient></defs><g transform="translate(512 515) skewX(-7) scale(1.12) translate(-530 -521)"><path d="M345 190H560C700 190 792 262 792 372C792 438 755 480 700 506C790 540 832 607 832 682C832 792 742 852 602 852H318C262 852 228 818 228 762L236 300C238 232 278 190 345 190Z" fill="url(#bdmp-b)"/><path d="M338 300C432 246 612 258 664 344C716 432 672 576 578 670C494 754 368 776 330 710C298 654 306 362 338 300Z" fill="url(#bdmp-fold)"/><path d="M345 190H560C642 190 702 212 746 254C692 238 612 234 522 238C420 242 332 264 288 322C264 352 260 424 258 522L252 760C240 748 228 752 228 762L236 300C238 232 278 190 345 190Z" fill="url(#bdmp-shine)"/></g><g transform="translate(512 520) scale(1.1) translate(-515 -512)"><path d="M462 318H568C584 318 596 330 596 346V528H652C676 528 688 557 671 574L539 706C526 719 504 719 491 706L359 574C342 557 354 528 378 528H434V346C434 330 446 318 462 318Z" fill="#ffffff"/></g></svg>`;

  // Compact, premium look: frosted navy glass with a fine brand-gradient edge.
  const STYLE = `
    :host { all: initial; }
    .wrap { position: fixed; z-index: 2147483647; font: 400 12px/1.3 Arial, "Helvetica Neue", Helvetica, sans-serif; color: #e2e8f0; display: none; -webkit-font-smoothing: antialiased; }
    .wrap.show { display: block; }
    .glass { background: rgba(15, 23, 42, .86); backdrop-filter: blur(14px) saturate(140%); -webkit-backdrop-filter: blur(14px) saturate(140%); }
    .bar { position: relative; display: flex; align-items: center; gap: 1px; height: ${BAR_HEIGHT}px; padding: 0 3px; border-radius: 999px; box-shadow: 0 4px 14px rgba(2, 6, 23, .35), inset 0 1px 0 rgba(255, 255, 255, .07); }
    /* 1px gradient edge drawn with a masked pseudo-element. */
    .bar::before { content: ""; position: absolute; inset: 0; border-radius: inherit; padding: 1px; background: linear-gradient(90deg, rgba(37, 99, 235, .9), rgba(16, 185, 129, .9)); -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0); -webkit-mask-composite: xor; mask-composite: exclude; pointer-events: none; }
    button { all: unset; box-sizing: border-box; cursor: pointer; }
    .main { display: flex; align-items: center; gap: 6px; height: 20px; padding: 0 9px 0 5px; border-radius: 999px; font-weight: 600; letter-spacing: .1px; color: #fff; white-space: nowrap; background-image: linear-gradient(90deg, #1d4ed8, #2563eb, #0369a1, #047857, #0369a1, #2563eb, #1d4ed8); background-size: 200% 100%; animation: bdm-flow 6s linear infinite; transition: filter .15s; }
    /* The logo shares these colours, so it sits on navy to stay visible. */
    .main svg { flex: none; box-sizing: content-box; margin-left: -3px; padding: 1px 2px; border-radius: 50%; background: #0f172a; }
    .main:hover, .main[aria-expanded="true"] { filter: brightness(1.15); }
    /* Moves a full image width, so the first and last colours meet without a jump. */
    @keyframes bdm-flow { to { background-position: 200% 0; } }
    .icon { display: grid; place-items: center; width: 18px; height: 18px; border-radius: 50%; color: #94a3b8; font-size: 11px; font-weight: 700; transition: background .15s, color .15s; }
    .icon:hover { background: rgba(255, 255, 255, .12); color: #fff; }
    .sep { width: 1px; height: 12px; margin: 0 2px; background: rgba(255, 255, 255, .14); }
    :focus-visible { outline: 2px solid #60a5fa; outline-offset: 1px; }
    .menu { position: absolute; right: 0; top: ${BAR_HEIGHT + 5}px; width: 248px; max-height: 344px; overflow: auto; padding: 4px; border-radius: 10px; border: 1px solid rgba(148, 163, 184, .18); box-shadow: 0 14px 34px rgba(2, 6, 23, .45), inset 0 1px 0 rgba(255, 255, 255, .06); display: none; }
    .menu.show { display: block; }
    .menu::-webkit-scrollbar { width: 6px; }
    .menu::-webkit-scrollbar-thumb { background: rgba(148, 163, 184, .35); border-radius: 3px; }
    .menu::-webkit-scrollbar-button { display: none; }
    .title { padding: 5px 7px 6px; font-weight: 600; font-size: 11.5px; color: #fff; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .group { padding: 5px 7px 2px; font-size: 9.5px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: #64748b; }
    .item { display: grid; grid-template-columns: 34px 1fr auto; align-items: center; gap: 6px; width: 100%; height: 25px; padding: 0 7px; border-radius: 6px; color: #e2e8f0; transition: background .12s; }
    .item:hover, .item:focus-visible { background: rgba(37, 99, 235, .32); }
    .tag { justify-self: start; padding: 0 4px; height: 14px; line-height: 14px; border-radius: 3px; font-size: 9px; font-weight: 700; letter-spacing: .04em; color: #93c5fd; background: rgba(37, 99, 235, .18); }
    .item.audio .tag { color: #6ee7b7; background: rgba(16, 185, 129, .16); }
    .name { display: flex; align-items: center; gap: 5px; overflow: hidden; white-space: nowrap; }
    .res { font-weight: 600; color: #fff; }
    .fps { font-size: 10px; color: #94a3b8; }
    .badge { padding: 0 3px; height: 13px; line-height: 13px; border-radius: 3px; font-size: 8.5px; font-weight: 800; color: #0f172a; background: linear-gradient(90deg, #60a5fa, #34d399); }
    .size { color: #94a3b8; font-size: 11px; font-variant-numeric: tabular-nums; }
    .note { padding: 7px 8px; color: #94a3b8; font-size: 11.5px; }
    .ok { padding: 8px; color: #6ee7b7; font-weight: 600; }
    .err { padding: 8px; color: #fca5a5; font-size: 11.5px; }
    .signin { display: block; margin: 0 8px 8px; padding: 5px 10px; border-radius: 999px; font-weight: 600; font-size: 11.5px; color: #fff; text-align: center; background: linear-gradient(90deg, #2563eb, #10b981); }
    .signin:hover { filter: brightness(1.1); }
    .license { margin: 3px 3px 1px; padding: 4px 4px 2px; border-top: 1px solid rgba(255, 255, 255, .08); color: #6ee7b7; font-size: 10px; }
    .spinner { display: inline-block; width: 10px; height: 10px; border: 2px solid rgba(148, 163, 184, .35); border-top-color: #60a5fa; border-radius: 50%; animation: spin .8s linear infinite; vertical-align: -2px; margin-right: 6px; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { * { transition: none !important; animation-duration: 2s !important; } .main { animation: none !important; } }
  `;

  // ---------------------------------------------------------------- UI shell
  const host = document.createElement("bdm-video-panel");
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `<style>${STYLE}</style>
    <div class="wrap" part="wrap">
      <div class="bar glass">
        <button class="main" aria-haspopup="menu" aria-expanded="false" title="Download With Babarify Download Manager">${LOGO}<span>Download With BDM</span></button>
        <span class="sep" aria-hidden="true"></span>
        <button class="icon help" title="About This Panel" aria-label="About This Panel">?</button>
        <button class="icon close" title="Hide For This Video" aria-label="Hide For This Video">×</button>
      </div>
      <div class="menu glass" role="menu"></div>
    </div>`;
  const wrap = root.querySelector(".wrap");
  const main = root.querySelector(".main");
  const menu = root.querySelector(".menu");

  let video = null;
  let dismissed = new WeakSet();
  let lastUrl = location.href;
  /** url → Promise of the BDM reply, so reopening the menu is instant. */
  const formats = new Map();

  function mount() {
    const parent = document.fullscreenElement ?? document.documentElement;
    if (host.parentNode !== parent) parent.appendChild(host);
  }

  // --------------------------------------------------------- video tracking
  function visibleArea(v) {
    const r = v.getBoundingClientRect();
    if (r.width < MIN_WIDTH || r.height < MIN_HEIGHT) return 0;
    const w = Math.min(r.right, innerWidth) - Math.max(r.left, 0);
    const h = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0);
    return w > 0 && h > 0 ? w * h : 0;
  }

  /** Videos inside open shadow roots (players built as web components). */
  function shadowVideos(root, out = []) {
    for (const el of root.querySelectorAll("*")) {
      if (!el.shadowRoot) continue;
      out.push(...el.shadowRoot.querySelectorAll("video"));
      shadowVideos(el.shadowRoot, out);
    }
    return out;
  }

  /** The largest visible video that has media loaded. */
  function pickVideo() {
    let best = null;
    let bestArea = 0;
    // The deeper search only runs on pages without a plain <video>.
    const plain = document.querySelectorAll("video");
    for (const v of plain.length ? plain : shadowVideos(document)) {
      if (dismissed.has(v) || v.readyState === 0) continue;
      const area = visibleArea(v);
      if (area > bestArea) {
        best = v;
        bestArea = area;
      }
    }
    return best;
  }

  function place() {
    if (!video) return;
    const r = video.getBoundingClientRect();
    const width = wrap.offsetWidth || 230;
    const left = Math.max(4, Math.min(r.right - width - 8, innerWidth - width - 4));
    // Sit just above the video like other download managers; inside it if
    // there's no room above.
    const top = r.top >= BAR_HEIGHT + 10 ? r.top - BAR_HEIGHT - 4 : Math.max(4, r.top + 8);
    wrap.style.left = `${left}px`;
    wrap.style.top = `${top}px`;
  }

  function refresh() {
    if (stopped) return;
    if (!alive()) {
      stop();
      return;
    }
    if (location.href !== lastUrl) {
      lastUrl = location.href;
      dismissed = new WeakSet();
      closeMenu();
    }
    const next = pickVideo();
    if (next !== video) {
      video = next;
      if (!video) closeMenu();
    }
    if (video) {
      mount();
      wrap.classList.add("show");
      place();
    } else {
      wrap.classList.remove("show");
    }
  }

  // ------------------------------------------------------------------ menu
  function closeMenu() {
    menu.classList.remove("show");
    main.setAttribute("aria-expanded", "false");
  }

  function formatSize(bytes) {
    if (!(bytes > 0)) return "";
    const units = ["B", "KB", "MB", "GB"];
    let v = bytes;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `≈ ${v.toFixed(i < 2 || v >= 100 ? 0 : 1)} ${units[i]}`;
  }

  function note(cls, text, spinner = false) {
    const div = document.createElement("div");
    div.className = cls;
    if (spinner) div.innerHTML = `<span class="spinner"></span>`;
    div.append(text);
    return div;
  }

  function send(message) {
    return new Promise((resolve) => {
      try {
        chrome.runtime.sendMessage(message, (reply) => {
          resolve(chrome.runtime.lastError ? { ok: false, error: "The BDM extension was updated. Reload this page." } : reply ?? { ok: false, error: "No reply from BDM." });
        });
      } catch {
        resolve({ ok: false, error: "The BDM extension was updated. Reload this page." });
      }
    });
  }

  /** An error, plus "Use My Browser Sign-In" when that would help. */
  function showError(reply, fallback) {
    const parts = [note("err", reply.error || fallback)];
    if (reply.signIn) {
      const b = document.createElement("button");
      b.className = "signin";
      b.textContent = "Use My Browser Sign-In";
      b.title = "Let BDM use your sign-in for this site";
      b.addEventListener("click", () => {
        send(reply.signInSite ? { type: "allowSignIn", site: reply.signInSite } : { type: "allowSignIn" });
        menu.replaceChildren(note("note", "Allow the site in the page that opened, then try again here."));
      });
      parts.push(b);
    }
    menu.replaceChildren(...parts);
  }

  const usable = (u) => u && /^https?:/i.test(u) && !SEGMENT_URL.test(u);

  /** The player's own source, when it's a real address (players that build the video in the page show blob:). */
  function playerAddresses(v) {
    return v ? [v.currentSrc, v.src, ...[...v.querySelectorAll("source")].map((s) => s.src)].filter(usable) : [];
  }

  /** Streams and video files the page loaded; load order puts a master playlist before its variants. */
  function loadedMedia() {
    const loaded = performance.getEntriesByType("resource").map((e) => e.name).filter(usable);
    return [...loaded.filter((u) => STREAM_URL.test(u)), ...loaded.filter((u) => FILE_URL.test(u)).reverse()];
  }

  /** What media detection saw on this tab, main video first, and the tab's title. */
  async function detectedOnTab() {
    const reply = await send({ type: "detectedMedia" });
    return { urls: Array.isArray(reply?.media) ? reply.media.map((m) => m.url) : [], title: String(reply?.title ?? "") };
  }

  /** The page's title; a player inside a frame uses the tab's, not its own. */
  function pageTitle(tabTitle) {
    const own = document.title.trim();
    return window.top === window ? own || tabTitle : tabTitle || own;
  }

  /**
   * A result much shorter than the video being watched is probably an ad
   * (or a preview), so other sources are tried before settling for it.
   */
  function looksLikeAd(media) {
    const watching = video?.duration;
    return Number.isFinite(watching) && watching > 60 && media?.duration > 0 && media.duration < watching * 0.6;
  }

  /** Asks BDM about each player source in turn; the first it can read wins. */
  async function fromPlayer(pageUrl) {
    if (video?.mediaKeys) {
      return { ok: false, error: "This video is protected by DRM. BDM doesn't download DRM-protected content." };
    }
    // The player's own address first, then media detection's list (which
    // carries the browser's exact request headers), then what the page loaded.
    const detected = await detectedOnTab();
    const sources = [...new Set([...playerAddresses(video), ...detected.urls, ...loadedMedia()])].slice(0, MAX_SOURCES);
    if (!sources.length) {
      return { ok: false, error: "This website isn't supported, and BDM couldn't find the video file its player is using." };
    }
    let last = null;
    let short = null;
    for (const src of sources) {
      const reply = await send({ type: "formats", url: src, referrer: pageUrl });
      if (reply.ok && !looksLikeAd(reply.media)) return { ...reply, source: src, title: pageTitle(detected.title) };
      if (reply.ok) {
        short ??= { ...reply, source: src, title: pageTitle(detected.title) };
        continue;
      }
      last = reply;
      // DRM or a sign-in applies to the whole video; other sources won't help.
      if (reply.signIn || /DRM/.test(reply.error ?? "")) return reply;
    }
    return short ?? { ok: false, error: last?.error || "BDM couldn't read the video this page is playing." };
  }

  /** Formats for the page, falling back to its player when yt-dlp can't read the page. */
  async function lookUp(url) {
    const reply = await send({ type: "formats", url });
    if (reply.ok || !reply.unsupported) return reply;
    return fromPlayer(url);
  }

  async function openMenu() {
    const url = location.href;
    menu.replaceChildren(note("note", "Getting Formats From BDM…", true));
    menu.classList.add("show");
    main.setAttribute("aria-expanded", "true");

    if (!formats.has(url)) formats.set(url, lookUp(url));
    const reply = await formats.get(url);
    if (url !== location.href || !menu.classList.contains("show")) return;
    if (!reply.ok) {
      formats.delete(url); // Let the user retry.
      showError(reply, "BDM couldn't read this video.");
      return;
    }

    const media = reply.media;
    // A stream from the player has no useful title of its own; use the page's.
    const title = reply.source ? reply.title || media.title : media.title;
    const items = [];
    const heading = document.createElement("div");
    heading.className = "title";
    heading.textContent = title;
    heading.title = title;
    items.push(heading);
    if (reply.source) items.push(note("group", "From This Page's Player"));
    let group = "";
    for (const choice of media.choices) {
      if (choice.kind !== group) {
        group = choice.kind;
        items.push(note("group", group === "audio" ? "Audio" : "Video"));
      }
      const b = document.createElement("button");
      b.className = `item ${choice.kind}`;
      b.setAttribute("role", "menuitem");
      b.title = choice.label;
      b.append(...describeChoice(choice));
      b.addEventListener("click", () => download(reply.source ?? url, title, choice, reply.source ? url : undefined));
      items.push(b);
    }
    if (media.license) items.push(note("license", `License: ${media.license}`));
    menu.replaceChildren(...items);
    menu.querySelector(".item")?.focus();
  }

  /** "MP4 · 2160p (4K) 60fps" → [MP4 tag] 2160p 60 [4K] … ≈ size */
  function describeChoice(choice) {
    const [format, rest = ""] = choice.label.split(" · ");
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = format;
    const name = document.createElement("span");
    name.className = "name";
    const res = document.createElement("span");
    res.className = "res";
    const height = Number(/(\d+)p/.exec(rest)?.[1] ?? 0);
    res.textContent = height ? `${height}p` : rest;
    name.append(res);
    const fps = /(\d+)fps/.exec(rest)?.[1];
    if (fps) {
      const f = document.createElement("span");
      f.className = "fps";
      f.textContent = fps;
      name.append(f);
    }
    const badgeText = height >= 2160 ? "4K" : height >= 1440 ? "2K" : height >= 720 ? "HD" : "";
    if (badgeText) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = badgeText;
      name.append(badge);
    }
    const size = document.createElement("span");
    size.className = "size";
    size.textContent = formatSize(choice.sizeBytes);
    return [tag, name, size];
  }

  async function download(url, title, choice, referrer) {
    menu.replaceChildren(note("note", `Sending ${choice.label} To BDM…`, true));
    const reply = await send({ type: "mediaDownload", url, title, options: choice.options, label: choice.label, size: choice.sizeBytes ?? undefined, referrer });
    if (reply.ok) {
      menu.replaceChildren(note("ok", reply.asked ? `✓ Opened In BDM: Confirm ${choice.label} There` : `✓ Downloading In BDM: ${choice.label}`));
      setTimeout(closeMenu, 2500);
    } else {
      showError(reply, "BDM couldn't start the download.");
    }
  }

  // Start asking BDM as soon as the pointer reaches the bar: analysis can take
  // several seconds on some sites, so this makes the list appear sooner.
  wrap.addEventListener("pointerenter", () => {
    const url = location.href;
    if (!formats.has(url)) formats.set(url, lookUp(url));
  });

  main.addEventListener("click", (e) => {
    e.stopPropagation();
    if (menu.classList.contains("show")) closeMenu();
    else openMenu();
  });
  root.querySelector(".close").addEventListener("click", (e) => {
    e.stopPropagation();
    if (video) dismissed.add(video);
    closeMenu();
    refresh();
  });
  root.querySelector(".help").addEventListener("click", (e) => {
    e.stopPropagation();
    menu.replaceChildren(
      note("note", "Pick a format and Babarify Download Manager downloads it. BDM doesn't download DRM-protected media. You can turn this panel off in the BDM extension's options."),
    );
    menu.classList.add("show");
  });
  // Clicks inside the panel shouldn't reach the page (e.g. pausing the player).
  for (const type of ["mousedown", "mouseup", "click", "dblclick", "pointerdown", "pointerup"]) {
    wrap.addEventListener(type, (e) => e.stopPropagation());
  }
  document.addEventListener("click", (e) => {
    if (!e.composedPath().includes(host)) closeMenu();
  }, true);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeMenu();
  }, true);

  // ------------------------------------------------------------ scheduling
  let scheduled = false;
  const schedule = () => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      refresh();
    });
  };
  addEventListener("scroll", schedule, { passive: true, capture: true });
  addEventListener("resize", schedule, { passive: true });
  document.addEventListener("fullscreenchange", schedule);
  document.addEventListener("loadedmetadata", schedule, true);
  document.addEventListener("play", schedule, true);
  // Single-page sites (e.g. YouTube) swap videos without reloading.
  const timer = setInterval(refresh, 1000);
  let stopped = false;

  /** Removes this copy: the extension was updated or reloaded, or a fresh copy took over. */
  function stop() {
    stopped = true;
    clearInterval(timer);
    host.remove();
  }

  refresh();
})();
