import { DEFAULT_SETTINGS, normalizeSettings, parseList } from "../src/rules.js";
import { connectionState } from "../src/native.js";

const $ = (id) => document.getElementById(id);
const SITE_ORIGINS = ["http://*/*", "https://*/*"];
const MEDIA_PERMISSIONS = { permissions: ["webRequest"], origins: SITE_ORIGINS };
const PANEL_PERMISSIONS = { permissions: ["scripting"], origins: SITE_ORIGINS };

function fill(s) {
  $("capture").checked = s.capture;
  $("extensions").value = s.extensions.join(", ");
  $("min-size").value = String(s.minSizeMb);
  $("excluded").value = s.excludedHosts.join("\n");
  $("detect").checked = s.detectMedia;
  $("panel").checked = s.videoPanel;
}

function read() {
  return normalizeSettings({
    capture: $("capture").checked,
    extensions: parseList($("extensions").value),
    minSizeMb: Number($("min-size").value || 0),
    excludedHosts: parseList($("excluded").value),
    detectMedia: $("detect").checked,
    videoPanel: $("panel").checked,
  });
}

async function save(settings) {
  await chrome.storage.sync.set({ settings });
  fill(settings);
  $("saved").hidden = false;
  setTimeout(() => ($("saved").hidden = true), 2000);
}

async function testConnection() {
  const el = $("connection");
  el.textContent = "Checking…";
  const { state, version, error } = await connectionState();
  el.dataset.state = state;
  el.textContent =
    state === "running"
      ? `Connected to BDM${version ? ` ${version}` : ""}.`
      : state === "stopped"
        ? "BDM is set up and will start when the browser sends it something."
        : error;
}

async function main() {
  const { settings } = await chrome.storage.sync.get("settings");
  fill(normalizeSettings(settings));
  testConnection();

  // Permission prompts must come straight from the click. Site access is
  // shared by both features, so it's only given up when neither needs it.
  const toggle = (id, perms, otherId) => async (e) => {
    if (e.target.checked) {
      e.target.checked = await chrome.permissions.request(perms);
    } else {
      await chrome.permissions.remove({ permissions: perms.permissions });
      if (!$(otherId).checked) await chrome.permissions.remove({ origins: SITE_ORIGINS });
    }
    // Takes effect right away, without pressing Save.
    await save(read());
  };
  $("detect").addEventListener("change", toggle("detect", MEDIA_PERMISSIONS, "panel"));
  $("panel").addEventListener("change", toggle("panel", PANEL_PERMISSIONS, "detect"));
  $("save").addEventListener("click", () => save(read()));
  $("defaults").addEventListener("click", () => save(normalizeSettings(DEFAULT_SETTINGS)));
  $("test").addEventListener("click", testConnection);
}

main();
