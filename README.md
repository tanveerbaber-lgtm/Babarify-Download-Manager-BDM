# Babarify Download Manager (BDM)

A fast, reliable download manager for Windows, built with **Tauri 2**, **React**, **TypeScript** and **Rust**.

> **Status:** Milestones 1–3 complete; M4 (Chrome and Edge integration) verified in real browsers with two manual checks left. Next: advanced download management (M5).
>
> *Faster Downloads. More Possibilities.* See [MILESTONES.md](MILESTONES.md).

## Features

- Multi-connection downloads using HTTP Range requests, with automatic single-stream fallback
- Live progress, speed and time left; pause, resume and cancel
- Safe resume: checks ETag, Last-Modified and size; survives crashes and restarts
- Automatic retries for dropped connections, timeouts and server errors; clear messages for expired links and access errors
- Optional SHA-256 verification; atomic finalization that never overwrites existing files
- Configurable simultaneous downloads and connections per download
- Video and audio from sites supported by yt-dlp: 360p–4K, MP4 (most compatible) or MKV (best quality), audio-only MP3/M4A/Opus, exact formats, and playlists
- yt-dlp, FFmpeg and Deno installed and updated by BDM from their official releases, checksum-verified (or use copies on PATH)
- DRM-protected media is detected and not downloaded
- Chrome and Edge extension: optional capture of browser downloads (by file type, with excluded sites and a fallback to the browser), "Download with BDM" menus, "Download video or audio from this page", an optional "Download With BDM" panel on videos (pick a format, the download starts), optional media detection; BDM starts automatically when needed
- Dashboard with download summary, sidebar navigation and download table
- Add Download dialog with URL validation, filename suggestion and folder picker
- SQLite storage with versioned migrations
- Persistent settings: theme (System / Light / Dark), default folder, simultaneous downloads, remove confirmation
- Least-privilege Tauri capabilities: the webview can only call BDM's own commands and a folder picker

## Prerequisites (Windows)

| Tool | Version used | Notes |
| --- | --- | --- |
| Node.js | 24.x | npm 11 |
| Rust | stable (1.99) | `x86_64-pc-windows-msvc` toolchain via rustup |
| Visual Studio C++ build tools | VS 2026 "Desktop development with C++" | Provides MSVC linker and Windows SDK |
| WebView2 | Built into Windows 11 | |

> **Git Bash users:** run Cargo from PowerShell or a Developer prompt. Git Bash's `/usr/bin/link` shadows the MSVC `link.exe` and breaks linking.

## Getting started

```powershell
npm install
npm run tauri dev
```

## Useful commands

| Command | What it does |
| --- | --- |
| `npm run tauri dev` | Run the desktop app with hot reload |
| `npm run typecheck` | TypeScript type check |
| `npm run build` | Type-check and build the frontend into `dist/` |
| `npm run test:rust` | Run Rust unit tests |
| `npm run check` | Type check + Rust tests + Clippy |
| `npm run tauri build` | Produce release installers (NSIS and MSI) |
| `cargo run --example test_server` (in `src-tauri`) | Local server on `127.0.0.1:8787` with slow, flaky and no-Range test files |
| `npm run test:extension` | Extension logic tests (Node's built-in test runner) |
| `npm run ext:pack` | Zip the extension for the Chrome Web Store / Edge Add-ons (`dist-extension/`) |

## Where data lives

- Database: `%APPDATA%\com.babarify.bdm\bdm.sqlite3`
- Media tools (when installed by BDM): `%APPDATA%\com.babarify.bdm\tools\`
- The data folder is shown in **Settings → About**.

## Project layout

```
src/                 React frontend
  components/        UI components
  hooks/             Data hooks (downloads, settings/theme)
  lib/               Typed API wrappers, shared types, formatting
  styles/            Design tokens and app styles
src-tauri/           Rust core
  src/commands.rs    Typed Tauri commands (the only IPC surface)
  src/engine/        Download engine: scheduler, transfers, retries, events
  src/media/         yt-dlp/FFmpeg integration: tools, analysis, arguments, output parsing
  src/db/            SQLite connection, migrations, repositories
  src/validation.rs  URL / filename / path validation
  capabilities/      Tauri permission grants
  src/browser/       Native-messaging host, pipe IPC, Chrome/Edge registration
  src/bin/           bdm-native-host.exe entry point
  testsupport/       Local HTTP test server (engine tests, example)
extension/           Chrome/Edge MV3 extension (popup, options, service worker)
assets/brand/        Brand mark and app icon SVGs (icons are generated from them)
assets/*.png         Brand guide
```

Regenerate icons after changing the logo:

```powershell
cd src-tauri; npx tauri icon ../assets/brand/bdm-app-icon.svg -o icons
```

## Browser extension (development)

1. Run BDM, open **Browser integration** and choose **Connect Chrome and Edge**.
2. In `chrome://extensions` or `edge://extensions`, turn on Developer mode, choose **Load unpacked** and select the `extension` folder.
3. The extension's ID is fixed (`emaohblojdddihemfkbcchbfgaaanjkb`), so the connection works straight away.
4. If the popup still says "BDM isn't connected" right after connecting, restart the browser once.

When BDM isn't running, the browser starts whichever BDM build you ran last. A development build (`npm run tauri dev`) needs its dev server, so for everyday use run the release build (`npx tauri build --no-bundle` → `src-tauri	arget
eleasedm.exe`) once and choose **Repair connection** on the Browser integration page.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — design, data model, security model
- [MILESTONES.md](MILESTONES.md) — master checklist and progress
- [RESULTS.md](RESULTS.md) — verification results per milestone
