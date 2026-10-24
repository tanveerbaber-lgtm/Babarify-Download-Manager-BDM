// Paint the cached theme before React mounts to avoid a light/dark flash.
// Loaded as a file (not inline) so the CSP can forbid inline scripts.
try {
  var t = localStorage.getItem("bdm.theme");
  var dark = t === "dark" || ((t === null || t === "system") && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
} catch (e) {}
