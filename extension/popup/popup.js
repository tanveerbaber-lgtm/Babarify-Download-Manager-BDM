import { basename, formatSize, isHttpUrl, normalizeSettings } from "../src/rules.js";
import { connectionState } from "../src/native.js";

const $ = (id) => document.getElementById(id);

function showError(text) {
  $("error").textContent = text;
  $("error").hidden = !text;
}

async function send(payload) {
  const reply = await chrome.runtime.sendMessage({ type: "send", payload });
  if (reply?.ok) window.close();
  else showError(reply?.error || "BDM couldn't take that.");
}

async function renderStatus() {
  const { state, version, error } = await connectionState();
  const line = $("status");
  line.dataset.state = state;
  $("status-text").textContent =
    state === "running" ? `Connected to BDM${version ? ` ${version}` : ""}` : state === "stopped" ? "BDM will start when needed" : "BDM isn't connected";
  if (state === "missing") showError(error);
  return state;
}

async function renderCapture() {
  const { settings } = await chrome.storage.sync.get("settings");
  const s = normalizeSettings(settings);
  $("capture").checked = s.capture;
  $("capture").addEventListener("change", async (e) => {
    await chrome.storage.sync.set({ settings: { ...s, capture: e.target.checked } });
  });
}

async function renderMedia(tab) {
  const key = `media:${tab.id}`;
  const { [key]: list = [] } = await chrome.storage.session.get(key);
  if (!list.length) return;
  $("media-section").hidden = false;
  const ul = $("media-list");
  for (const m of list) {
    const li = document.createElement("li");
    const info = document.createElement("div");
    const name = document.createElement("div");
    name.className = "media-name";
    name.textContent = basename(new URL(m.url).pathname) || m.url;
    name.title = m.url;
    const meta = document.createElement("div");
    meta.className = "media-meta";
    meta.textContent = [m.label, formatSize(m.size)].filter(Boolean).join(" · ");
    info.append(name, meta);
    const button = document.createElement("button");
    button.className = "button";
    button.type = "button";
    // Streams need yt-dlp; plain files go straight to the download engine.
    button.textContent = m.kind === "stream" ? "Get video" : "Download";
    button.addEventListener("click", () =>
      send(m.kind === "stream" ? { type: "media", url: m.url, pageTitle: tab.title } : { type: "download", url: m.url, referrer: tab.url }),
    );
    li.append(info, button);
    ul.append(li);
  }
}

async function main() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const state = await renderStatus();
  await renderCapture();

  const page = $("send-page");
  if (!tab?.url || !isHttpUrl(tab.url) || state === "missing") {
    page.disabled = true;
  } else {
    page.addEventListener("click", () => send({ type: "media", url: tab.url, pageTitle: tab.title }));
  }
  if (tab?.id != null) await renderMedia(tab);

  const { lastError } = await chrome.storage.session.get("lastError");
  if (lastError && Date.now() - lastError.at < 5 * 60_000 && state !== "missing") showError(lastError.message);

  $("options").addEventListener("click", (e) => {
    e.preventDefault();
    chrome.runtime.openOptionsPage();
  });
}

main();
