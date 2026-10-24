// Packs the extension for the Chrome Web Store and Edge Add-ons:
//   npm run ext:pack  →  dist-extension/bdm-extension-<version>.zip
// Both stores accept the same Manifest V3 package. Tests are left out.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import process from "node:process";

const root = resolve(import.meta.dirname, "..");
const src = join(root, "extension");
const out = join(root, "dist-extension");
const stage = join(out, "stage");
const { version } = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8"));

rmSync(out, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
for (const entry of ["manifest.json", "icons", "src", "popup", "options"]) {
  cpSync(join(src, entry), join(stage, entry), { recursive: true });
}

const zip = join(out, `bdm-extension-${version}.zip`);
// Windows ships bsdtar, which writes zip archives with -a. Call it by full
// path: Git Bash's GNU tar would read "E:" as a remote host.
const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
execFileSync(tar, ["-a", "-c", "-f", zip, "-C", stage, "."], { stdio: "inherit" });
rmSync(stage, { recursive: true, force: true });
console.log(`Packed ${zip}`);
