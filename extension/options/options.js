import { DEFAULT_SETTINGS, normalizeSettings, parseList, signInPermissions, siteOf } from "../src/rules.js";
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
  signInSites = s.signInSites;
  renderSignIn();
}

/** Sites allowed for sign-in downloads; changed only through Allow/Remove. */
let signInSites = [];

function renderSignIn() {
  const list = $("signin-list");
  list.replaceChildren(
    ...signInSites.map((site) => {
      const li = document.createElement("li");
      const name = document.createElement("span");
      name.textContent = site;
      const remove = document.createElement("button");
      remove.className = "button";
      remove.type = "button";
      remove.textContent = "Remove";
      remove.addEventListener("click", () => removeSignIn(site));
      li.append(name, remove);
      return li;
    }),
  );
  $("signin-empty").hidden = signInSites.length > 0;
}

async function allowSignIn() {
  const site = siteOf($("signin-site").value.replace(/^[a-z]+:\/\//i, "").split(/[/?#:]/)[0]);
  if (!/^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(site)) {
    $("signin-site").focus();
    return;
  }
  // The prompt must come straight from the click.
  if (!(await chrome.permissions.request(signInPermissions(site)))) return;
  const settings = read();
  settings.signInSites = [...new Set([...signInSites, site])];
  $("signin-site").value = "";
  $("signin-suggest").hidden = true;
  await save(settings);
}

async function removeSignIn(site) {
  const settings = read();
  settings.signInSites = signInSites.filter((s) => s !== site);
  await save(settings);
  const { origins } = signInPermissions(site);
  await chrome.permissions.remove({ origins }).catch(() => {});
  if (settings.signInSites.length === 0) await chrome.permissions.remove({ permissions: ["cookies"] }).catch(() => {});
}

function read() {
  return normalizeSettings({
    capture: $("capture").checked,
    extensions: parseList($("extensions").value),
    minSizeMb: Number($("min-size").value || 0),
    excludedHosts: parseList($("excluded").value),
    detectMedia: $("detect").checked,
    videoPanel: $("panel").checked,
    signInSites,
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
  $("signin-allow").addEventListener("click", allowSignIn);
  $("signin-site").addEventListener("keydown", (e) => {
    if (e.key === "Enter") allowSignIn();
  });

  // Opened from the video panel: #signin=youtube.com
  const hash = new URLSearchParams(location.hash.slice(1));
  if (hash.has("signin")) {
    const site = siteOf(hash.get("signin"));
    $("signin-section").scrollIntoView({ block: "center" });
    if (site && !signInSites.includes(site)) {
      $("signin-site").value = site;
      $("signin-suggest").textContent = `BDM needs your sign-in for ${site}. Click "Allow site", then try the download again.`;
      $("signin-suggest").hidden = false;
    }
    $("signin-allow").focus();
  }
}

main();
