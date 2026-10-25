# Releasing Babarify Download Manager

How to build, check and ship a Windows release. The acceptance checklist for each release is at the end.

## Build the installer

Requirements: Windows 10/11 x64, Rust (MSVC toolchain), Visual Studio C++ build tools, Node.js. Run from PowerShell
(Git Bash's `link.exe` shadows the MSVC linker).

```powershell
npm ci
python scripts/third-party-notices.py   # refresh THIRD_PARTY_NOTICES.txt
npm run check                           # TypeScript, extension tests, Rust tests, Clippy
npx tauri build                         # builds the app, the browser helper and the installer
```

`tauri build` runs `scripts/prepare-sidecar.mjs`, which builds `bdm-native-host.exe` and places it where the bundler
ships it next to `bdm.exe`. The first run downloads Tauri's NSIS tooling from GitHub.

Output: `src-tauri/target/release/bundle/nsis/Babarify Download Manager_<version>_x64-setup.exe`.

The browser extension is packed separately with `npm run ext:pack` (zip for the Chrome Web Store / Edge Add-ons).

## What the installer does

- **Per-user install, no admin prompt**, to `%LOCALAPPDATA%\Programs\Babarify Download Manager` (or the folder you
  choose), with Start menu and optional desktop shortcuts and an Add/Remove Programs entry.
- Installs WebView2 if it's missing (Microsoft's bootstrapper, silent).
- Ships `bdm.exe`, `bdm-native-host.exe`, and `legal\` (LICENSE, PRIVACY, THIRD_PARTY_NOTICES).
- On first launch BDM registers its browser helper for Chrome and Edge (HKCU). Every launch re-checks this, so
  upgrades and moved installs keep working.
- **Upgrades:** run the new installer over the old one. Settings, the download list, queues and media tools live in
  `%APPDATA%\com.babarify.bdm` and are kept; database migrations run on first launch.
- **Uninstall** (Settings → Apps, or the Start menu): removes the program, shortcuts and the browser registration.
  The uninstaller offers to delete `%APPDATA%\com.babarify.bdm` too. **Downloaded files are never removed.**

## Version numbers

Keep these in step (semantic versioning): `src-tauri/tauri.conf.json` → `version`, `src-tauri/Cargo.toml`,
`package.json`, `extension/manifest.json`. The installer and Windows file properties take theirs from
`tauri.conf.json`.

## Code signing (recommended before public release)

1.0.0 is **unsigned**. Windows SmartScreen shows "Windows protected your PC" for unsigned installers until they build
reputation, and some antivirus products are stricter with them.

To sign:

1. Get an Authenticode code-signing certificate (an OV or EV certificate from a CA, or Azure Trusted Signing).
2. Set `bundle.windows.certificateThumbprint` (certificate in the Windows store), `digestAlgorithm: "sha256"` and
   `timestampUrl` (e.g. your CA's RFC 3161 server) in `tauri.conf.json`, or a `signCommand` for a cloud signer.
3. `npx tauri build` then signs `bdm.exe`, `bdm-native-host.exe` and the installer.
4. Check with `Get-AuthenticodeSignature` that all three show `Valid`.

## Updates

1.0.0 has **no automatic updates**: you install a new version over the old one, and your data is kept.

If automatic updates are added later, they must be signed end to end:

- Use Tauri's updater plugin (`tauri-plugin-updater`). Generate an updater key pair with `npx tauri signer generate`;
  keep the private key offline and out of the repository; put only the public key in `tauri.conf.json`.
- Sign each release's installer with that key (`TAURI_SIGNING_PRIVATE_KEY` at build time) and publish the
  installer, its `.sig` and a `latest.json` over HTTPS (e.g. GitHub Releases on a public repository).
- The app accepts an update only if its signature verifies against the built-in public key. HTTPS alone isn't enough.
- Combine with Authenticode signing so the downloaded installer is also trusted by Windows.
- Ask before installing, and never update while downloads are running.

yt-dlp, FFmpeg and Deno already update through GitHub releases, checked against GitHub's SHA-256 digests (see
THIRD_PARTY.md).

## Release checklist

Tick each line for every release (results go in RESULTS.md):

- [ ] Version bumped in all four places; release notes written.
- [ ] `python scripts/third-party-notices.py` re-run; `cargo audit` and `npm audit` clean.
- [ ] `npm run check` passes (TypeScript, extension tests, Rust tests, Clippy).
- [ ] `npx tauri build` produces the installer; installer, `bdm.exe` and helper signed (once a certificate exists).
- [ ] Fresh install: app starts, splash and icons correct, Start menu entry, Add/Remove Programs entry and version.
- [ ] Chrome and Edge: extension connects ("BDM will start when needed"), a captured download opens the Download File
      Info dialog, the on-video panel lists formats.
- [ ] Downloads: a file, a segmented file with pause/resume, a video (Creative Commons), a queue with a schedule.
- [ ] Upgrade over the previous version: download list, settings and queues kept.
- [ ] Uninstall: program, shortcuts and browser registration removed; downloads untouched; app data removed only if
      ticked.
- [ ] LICENSE, PRIVACY and THIRD_PARTY_NOTICES open from Settings → About.
- [ ] Tag the release in git and publish the installer.
