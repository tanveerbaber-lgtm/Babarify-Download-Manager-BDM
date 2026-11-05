# Third-Party Software

Babarify Download Manager is built on open-source components. This page lists what ships in the app, what it downloads
on request, how each part is kept up to date, and what that means for licensing.

**Last reviewed:** 2026-10-25 (M6).

## Built into the app

| Source | Count | Licenses |
| --- | --- | --- |
| Rust crates (Windows x64, normal dependencies) | 315 | All permissive: MIT and/or Apache-2.0 (most), Unicode-3.0, BSD-2/3-Clause, ISC, Zlib, Unlicense, CC0, 0BSD |
| Rust crates under MPL-2.0 | 5 | `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`, pulled in by Tauri. MPL-2.0 is file-level copyleft: using them unmodified needs only the notice; changes to those files would have to be published. |
| npm packages in the UI bundle | 11 | MIT, ISC, Apache-2.0 OR MIT |

No GPL, AGPL or unknown licenses are in the app itself. The summary comes from `cargo metadata` (normal dependencies
reachable from `bdm`) and `npm ls --omit=dev`; the full list can be regenerated with `cargo about` or
`cargo license` before a release (M7).

## Downloaded on request (Settings → Media tools)

BDM never bundles these. It downloads them from their official GitHub releases when you ask, into
`%APPDATA%\com.babarify.bdm\tools`, and runs them as separate programs (no linking).

| Tool | Source | License | Checked with |
| --- | --- | --- | --- |
| yt-dlp | `yt-dlp/yt-dlp` releases, `yt-dlp_win.zip` (the folder build: `yt-dlp.exe` with its `_internal` support files) | Unlicense (the Windows build bundles Python and libraries under their own permissive licenses) | SHA-256 digest published by GitHub for the asset |
| FFmpeg | `BtbN/FFmpeg-Builds`, `win64-gpl` static build | GPL-3.0 | SHA-256 digest published by GitHub |
| Deno | `denoland/deno` releases, Windows x64 zip | MIT | SHA-256 digest published by GitHub |

A download whose SHA-256 doesn't match GitHub's digest is discarded. If a system copy is already on `PATH`, BDM can
use it instead.

**Licensing note for M7:** because FFmpeg is fetched by the user and runs as a separate process, BDM itself stays
under its own license. If the installer ever **bundles** FFmpeg, either ship the GPL build together with its license
and an offer of the source code, or switch to BtbN's `lgpl` build.

## How each part is updated

| Part | Update path |
| --- | --- |
| yt-dlp | Checked on every launch (setting "Keep yt-dlp up to date", on by default) and from Settings → Media tools. Sites change often, so this matters for video downloads. |
| FFmpeg, Deno | Settings → Media tools → Check for updates, then install. |
| Rust crates, npm packages | Updated with the app. `cargo audit` (RustSec) and `npm audit` are run before each milestone; results are in RESULTS.md. |
| The app itself | No self-update yet. Planned with the installer and signed releases (M7). |
| Browser extension | Chrome updates it through the Chrome Web Store. In Edge, load the new version until it's on Edge Add-ons. |
