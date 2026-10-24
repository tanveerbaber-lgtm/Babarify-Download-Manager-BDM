// BDM "Download With BDM" panel. Injected into pages only after the user
// turns the panel on in the extension's options (which grants site access).
// Everything lives in a closed shadow root so pages can't restyle or read it.
(() => {
  if (window.__bdmVideoPanel) return;
  window.__bdmVideoPanel = true;

  const MIN_WIDTH = 240;
  const MIN_HEIGHT = 135;
  const BAR_HEIGHT = 30;

  const LOGO = `<svg viewBox="0 0 64 64" width="18" height="18" aria-hidden="true"><defs><linearGradient id="g" x1=".05" y1=".9" x2=".95" y2=".15"><stop offset="0" stop-color="#2563EB"/><stop offset=".4" stop-color="#1F78E6"/><stop offset=".9" stop-color="#10B981"/></linearGradient></defs><path fill="url(#g)" d="M15 5h19.5C45 5 51 11.2 51 19.6c0 4.6-2.2 8.4-5.8 10.6C51.2 32.3 56 37.9 56 45.2 56 53.4 49.4 59 39.6 59H15c-4.4 0-7-2.6-7-7V12c0-4.4 2.6-7 7-7z"/><path fill="#fff" d="M18 15.5a3 3 0 0 1 3-3h4a3 3 0 0 1 3 3V35h5.6c1.9 0 2.8 2.3 1.5 3.6L24.6 49.2a2.3 2.3 0 0 1-3.2 0L10.9 38.6c-1.3-1.3-.4-3.6 1.5-3.6H18z"/></svg>`;

  const STYLE = `
    :host { all: initial; }
    .wrap { position: fixed; z-index: 2147483647; font: 13px/1.35 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; color: #e2e8f0; display: none; }
    .wrap.show { display: block; }
    .bar { display: flex; align-items: center; gap: 4px; height: ${BAR_HEIGHT}px; padding: 0 4px 0 2px; border-radius: 8px; background: #0f172a; box-shadow: 0 6px 18px rgba(15, 23, 42, .35); border: 1px solid rgba(255, 255, 255, .08); }
    button { all: unset; box-sizing: border-box; cursor: pointer; }
    .main { display: flex; align-items: center; gap: 7px; height: 26px; padding: 0 10px 0 6px; border-radius: 6px; font-weight: 600; color: #fff; white-space: nowrap; }
    .main:hover, .main[aria-expanded="true"] { background: rgba(37, 99, 235, .35); }
    .icon { display: grid; place-items: center; width: 22px; height: 22px; border-radius: 5px; color: #94a3b8; font-size: 13px; font-weight: 700; }
    .icon:hover { background: rgba(255, 255, 255, .1); color: #fff; }
    :focus-visible { outline: 2px solid #60a5fa; outline-offset: 1px; }
    .menu { position: absolute; right: 0; top: ${BAR_HEIGHT + 6}px; width: 330px; max-height: 360px; overflow: auto; padding: 6px; border-radius: 10px; background: #ffffff; color: #0f172a; box-shadow: 0 16px 40px rgba(15, 23, 42, .35); border: 1px solid #e2e8f0; display: none; }
    .menu.show { display: block; }
    .title { padding: 6px 8px 8px; font-weight: 700; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border-bottom: 1px solid #e2e8f0; margin-bottom: 4px; }
    .item { display: flex; width: 100%; justify-content: space-between; gap: 12px; padding: 8px 9px; border-radius: 7px; color: #0f172a; }
    .item:hover, .item:focus-visible { background: #eff6ff; }
    .item .size { color: #64748b; font-variant-numeric: tabular-nums; }
    .item.audio .label::before { content: "♪ "; color: #10b981; }
    .note { padding: 8px 9px; color: #64748b; font-size: 12px; }
    .ok { padding: 10px 9px; color: #059669; font-weight: 600; }
    .err { padding: 10px 9px; color: #dc2626; }
    .license { padding: 6px 9px 4px; color: #059669; font-size: 11px; border-top: 1px solid #e2e8f0; margin-top: 4px; }
    .spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid #cbd5e1; border-top-color: #2563eb; border-radius: 50%; animation: spin .8s linear infinite; vertical-align: -2px; margin-right: 6px; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-color-scheme: dark) {
      .menu { background: #131c31; color: #e2e8f0; border-color: #243049; }
      .title { border-color: #243049; }
      .item { color: #e2e8f0; }
      .item:hover, .item:focus-visible { background: rgba(59, 130, 246, .16); }
      .license { border-color: #243049; }
    }
  `;

  // ---------------------------------------------------------------- UI shell
  const host = document.createElement("bdm-video-panel");
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `<style>${STYLE}</style>
    <div class="wrap" part="wrap">
      <div class="bar">
        <button class="main" aria-haspopup="menu" aria-expanded="false" title="Download With Babarify Download Manager">${LOGO}<span>Download With BDM</span></button>
        <button class="icon help" title="About This Panel" aria-label="About This Panel">?</button>
        <button class="icon close" title="Hide For This Video" aria-label="Hide For This Video">×</button>
      </div>
      <div class="menu" role="menu"></div>
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
    for (const choice of media.choices) {
      const b = document.createElement("button");
      b.className = `item ${choice.kind}`;
      b.setAttribute("role", "menuitem");
      const label = document.createElement("span");
      label.className = "label";
      label.textContent = choice.label;
      const size = document.createElement("span");
      size.className = "size";
      size.textContent = formatSize(choice.sizeBytes);
      b.append(label, size);
      b.addEventListener("click", () => download(url, media.title, choice));
      items.push(b);
    }
    if (media.license) items.push(note("license", `License: ${media.license}`));
    menu.replaceChildren(...items);
    menu.querySelector(".item")?.focus();
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
