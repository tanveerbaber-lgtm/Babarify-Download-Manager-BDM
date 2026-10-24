import { DEFAULT_SETTINGS, normalizeSettings, parseList } from "../src/rules.js";
import { connectionState } from "../src/native.js";

const $ = (id) => document.getElementById(id);
const MEDIA_PERMISSIONS = { permissions: ["webRequest"], origins: ["http://*/*", "https://*/*"] };

function fill(s) {
  $("capture").checked = s.capture;
  $("extensions").value = s.extensions.join(", ");
  $("min-size").value = String(s.minSizeMb);
  $("excluded").value = s.excludedHosts.join("\n");
  $("detect").checked = s.detectMedia;
}

function read() {
  return normalizeSettings({
    capture: $("capture").checked,
    extensions: parseList($("extensions").value),
    minSizeMb: Number($("min-size").value || 0),
    excludedHosts: parseList($("excluded").value),
    detectMedia: $("detect").checked,
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

  // Permission prompts must come straight from the click.
  $("detect").addEventListener("change", async (e) => {
    if (e.target.checked) {
      e.target.checked = await chrome.permissions.request(MEDIA_PERMISSIONS);
    } else {
      await chrome.permissions.remove(MEDIA_PERMISSIONS);
    }
  });
  $("save").addEventListener("click", () => save(read()));
  $("defaults").addEventListener("click", () => save(normalizeSettings(DEFAULT_SETTINGS)));
  $("test").addEventListener("click", testConnection);
}

main();
