// BDM "Download With BDM" panel. Injected into pages only after the user
// turns the panel on in the extension's options (which grants site access).
// Everything lives in a closed shadow root so pages can't restyle or read it.
(() => {
  if (window.__bdmVideoPanel) return;
  window.__bdmVideoPanel = true;

  const MIN_WIDTH = 240;
  const MIN_HEIGHT = 135;
  const BAR_HEIGHT = 26;

  const LOGO = `<svg viewBox="0 0 64 64" width="15" height="15" aria-hidden="true"><defs><linearGradient id="g" x1=".05" y1=".9" x2=".95" y2=".15"><stop offset="0" stop-color="#2563EB"/><stop offset=".4" stop-color="#1F78E6"/><stop offset=".9" stop-color="#10B981"/></linearGradient></defs><path fill="url(#g)" d="M15 5h19.5C45 5 51 11.2 51 19.6c0 4.6-2.2 8.4-5.8 10.6C51.2 32.3 56 37.9 56 45.2 56 53.4 49.4 59 39.6 59H15c-4.4 0-7-2.6-7-7V12c0-4.4 2.6-7 7-7z"/><path fill="#fff" d="M18 15.5a3 3 0 0 1 3-3h4a3 3 0 0 1 3 3V35h5.6c1.9 0 2.8 2.3 1.5 3.6L24.6 49.2a2.3 2.3 0 0 1-3.2 0L10.9 38.6c-1.3-1.3-.4-3.6 1.5-3.6H18z"/></svg>`;

  // Compact, premium look: frosted navy glass with a fine brand-gradient edge.
  const STYLE = `
    :host { all: initial; }
    .wrap { position: fixed; z-index: 2147483647; font: 500 12px/1.3 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; color: #e2e8f0; display: none; -webkit-font-smoothing: antialiased; }
    .wrap.show { display: block; }
    .glass { background: rgba(15, 23, 42, .86); backdrop-filter: blur(14px) saturate(140%); -webkit-backdrop-filter: blur(14px) saturate(140%); }
    .bar { position: relative; display: flex; align-items: center; gap: 1px; height: ${BAR_HEIGHT}px; padding: 0 3px; border-radius: 999px; box-shadow: 0 4px 14px rgba(2, 6, 23, .35), inset 0 1px 0 rgba(255, 255, 255, .07); }
    /* 1px gradient edge drawn with a masked pseudo-element. */
    .bar::before { content: ""; position: absolute; inset: 0; border-radius: inherit; padding: 1px; background: linear-gradient(90deg, rgba(37, 99, 235, .9), rgba(16, 185, 129, .9)); -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0); -webkit-mask-composite: xor; mask-composite: exclude; pointer-events: none; }
    button { all: unset; box-sizing: border-box; cursor: pointer; }
    .main { display: flex; align-items: center; gap: 6px; height: 20px; padding: 0 9px 0 5px; border-radius: 999px; font-weight: 600; letter-spacing: .1px; color: #fff; white-space: nowrap; transition: background .15s; }
    .main:hover, .main[aria-expanded="true"] { background: rgba(37, 99, 235, .45); }
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
    .license { margin: 3px 3px 1px; padding: 4px 4px 2px; border-top: 1px solid rgba(255, 255, 255, .08); color: #6ee7b7; font-size: 10px; }
    .spinner { display: inline-block; width: 10px; height: 10px; border: 2px solid rgba(148, 163, 184, .35); border-top-color: #60a5fa; border-radius: 50%; animation: spin .8s linear infinite; vertical-align: -2px; margin-right: 6px; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { * { transition: none !important; animation-duration: 2s !important; } }
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

  /** The largest visible video that has media loaded. */
  function pickVideo() {
    let best = null;
    let bestArea = 0;
    for (const v of document.querySelectorAll("video")) {
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

  async function openMenu() {
    const url = location.href;
    menu.replaceChildren(note("note", "Getting Formats From BDM…", true));
    menu.classList.add("show");
    main.setAttribute("aria-expanded", "true");

    if (!formats.has(url)) formats.set(url, send({ type: "formats", url }));
    const reply = await formats.get(url);
    if (url !== location.href || !menu.classList.contains("show")) return;
    if (!reply.ok) {
      formats.delete(url); // Let the user retry.
      menu.replaceChildren(note("err", reply.error || "BDM couldn't read this video."));
      return;
    }

    const media = reply.media;
    const items = [];
    const title = document.createElement("div");
    title.className = "title";
    title.textContent = media.title;
    title.title = media.title;
    items.push(title);
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
      b.addEventListener("click", () => download(url, media.title, choice));
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

  async function download(url, title, choice) {
    menu.replaceChildren(note("note", `Sending ${choice.label} To BDM…`, true));
    const reply = await send({ type: "mediaDownload", url, title, options: choice.options });
    if (reply.ok) {
      menu.replaceChildren(note("ok", `✓ Downloading In BDM: ${choice.label}`));
      setTimeout(closeMenu, 2500);
    } else {
      menu.replaceChildren(note("err", reply.error || "BDM couldn't start the download."));
    }
  }

  // Start asking BDM as soon as the pointer reaches the bar: analysis can take
  // several seconds on some sites, so this makes the list appear sooner.
  wrap.addEventListener("pointerenter", () => {
    const url = location.href;
    if (!formats.has(url)) formats.set(url, send({ type: "formats", url }));
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
  setInterval(refresh, 1000);
  refresh();
})();
