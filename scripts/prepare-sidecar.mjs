// Builds the browser helper (bdm-native-host.exe) in release mode and puts it
// where Tauri's bundler expects a sidecar, so the installer ships it next to
// bdm.exe. Runs as part of `tauri build` (see beforeBuildCommand).
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = join(root, "src-tauri", "Cargo.toml");
const triple = "x86_64-pc-windows-msvc";

// No shell: the project path contains spaces.
execFileSync("cargo", ["build", "--release", "--bin", "bdm-native-host", "--manifest-path", manifest], { stdio: "inherit" });

const built = join(root, "src-tauri", "target", "release", "bdm-native-host.exe");
const sidecar = join(root, "src-tauri", "binaries", `bdm-native-host-${triple}.exe`);
mkdirSync(dirname(sidecar), { recursive: true });
copyFileSync(built, sidecar);
console.log(`sidecar ready: ${sidecar}`);
