# BDM Master Checklist

Updated after every completed task. Verification evidence is in [RESULTS.md](RESULTS.md).

**Legend:** `[x]` done and verified · `[~]` implemented, verification pending · `[ ]` not started

**Last updated:** 2026-10-26 — v1.0.0 release installer replaced with the audit-fix build (New Download dialog, Mark of the Web, navy headers); release notes refreshed. Tag still at c76d542. Open: Edge on real Windows, clean-PC test, code signing.

## Progress

| Milestone | Done | In progress | Total | Status |
| --- | --- | --- | --- | --- |
| M1 — Desktop Foundation | 9 | 0 | 9 | ✅ Complete |
| M2 — Core Download Engine | 10 | 0 | 10 | ✅ Complete |
| M3 — Video and Audio Engine | 11 | 0 | 11 | ✅ Complete |
| M4 — Chrome and Edge Integration | 10 | 2 | 12 | In progress (2 manual checks left) |
| M5 — Advanced Download Management | 9 | 1 | 10 | In progress (1 check left) |
| M6 — Security, Reliability and Performance | 10 | 1 | 11 | In progress |
| M7 — Windows Installer and Release | 7 | 3 | 10 | In progress |
| **Total** | **66** | **7** | **73** | |

## M1 — Desktop Foundation
Build the application shell and establish the project architecture.
- [x] Initialize Tauri 2, React, TypeScript, Vite and Rust.
- [x] Create Babarify branding, application icon and professional UI.
  - [x] Compact layout, Arial / Arial Bold typography and compact popups (2026-10-26)
  - [x] Refreshed from the Brandboard (2026-10-25): glossy "B" app icon (all sizes, ICO), logo, splash screen, pill buttons,
        status chips with icons, gradient progress bars, stat cards with icon tiles, extension icons and panel logo
- [x] Implement dashboard, sidebar, navigation and download table.
- [x] Create SQLite database and versioned migrations.
- [x] Implement settings persistence and light/dark themes.
- [x] Create Add Download dialog and pending download records.
- [x] Implement typed Rust commands and least-privilege permissions.
- [x] Add README, ARCHITECTURE, MILESTONES and RESULTS documentation.
- [x] Verify Windows development build, TypeScript checks and Rust compilation.
  - [x] TypeScript type check (`tsc --noEmit`) passes
  - [x] Frontend production build (`vite build`) passes
  - [x] Rust compiles and unit tests pass (18/18)
  - [x] Clippy passes with `-D warnings`
  - [x] `npm run tauri dev` launches and core flows work (scripted checks through the real app)

## M2 — Core Download Engine
Enable reliable downloads of ordinary files from supported HTTP/HTTPS URLs.
- [x] Implement direct HTTP/HTTPS downloads.
- [x] Display real-time progress, speed, file size and estimated time remaining.
- [x] Implement pause, resume and cancellation.
- [x] Implement segmented downloads using HTTP Range requests when supported.
- [x] Support retries, timeouts and configurable concurrent downloads.
- [x] Persist progress and recover interrupted downloads after restart.
- [x] Validate ETag, Last-Modified and remote file identity before resuming.
- [x] Handle redirects, expired links, authentication errors and server limitations.
- [x] Implement safe temporary files, filename validation and atomic finalization.
- [x] Test large files, unreliable connections, checksum failures and range fallbacks.

## M3 — Video and Audio Engine
Download supported online videos and process media into usable files.
- [x] Integrate and manage yt-dlp.
- [x] Analyze supported URLs and list available formats.
- [x] Offer available resolutions and formats, including 360p, 720p, 1080p and 4K where available.
- [x] Support separate video and audio streams.
- [x] Integrate FFmpeg for compatible stream merging and media processing.
- [x] Support audio-only downloads in available formats.
- [x] Support playlists and batch video downloads where permitted.
- [x] Display extraction progress, processing progress and meaningful errors.
- [x] Handle extractor updates and supported website changes.
- [x] Test media compatibility, interrupted downloads and output integrity.
- [x] Respect website permissions, copyright and DRM restrictions.

## M4 — Chrome and Edge Integration
Connect the browser to the Windows application.
- [x] Build a Chrome Manifest V3 extension.
- [x] Verify Edge compatibility and package the extension appropriately.
- [x] Implement the Native Messaging host.
- [x] Register the native host securely during installation.
- [x] Validate the permitted extension ID and incoming messages.
- [x] Offer user-controlled interception of supported file downloads.
- [~] Add supported media detection and Send to BDM actions.
  - [x] Send to BDM: page, link and media-file actions verified in Chrome and Edge
  - [ ] Media detection on live pages (needs a person to accept the optional permission prompt)
- [x] Pass URLs, filenames and permitted request metadata safely.
- [x] Handle application-not-running and browser-disconnected states.
- [x] On-video "Download With BDM" panel: opt-in site access, format list from BDM, one-click download *(added 2026-10-24)*.
  - [x] Panel, format list and one-click download verified in Chrome 155 (local page and YouTube) and Edge 156
  - [x] Off by default; no script on any page until switched on
  - [x] Turned on in your own Chrome: site-access prompt accepted, bar and full format list shown on YouTube
- [x] "New Download" confirm dialog and per-download window *(added 2026-10-25)*.
  - [x] Browser downloads and panel choices open a confirm dialog: name, size, format, category, folder, queue
  - [x] Category picks the folder (created if new); "Always save <category> here"; duplicate warning
  - [x] Start Download / Download Later / Cancel; a setting lets browser downloads start without asking
  - [x] Download window: status, speed, time left, resume support, live map and table of every connection
  - [x] Speed Limit tab (applies live) and When Done tab (summary, open file/folder, all-done PC action)
  - [x] Verified in the release build: file dialog → window → completion summary; video dialog (MP4 · 1080p 60fps)
- [~] Test browser updates, permissions, installation and uninstallation.
  - [x] Extension install (fixed ID) and uninstall in Chrome 155 and Edge 156
  - [x] Native host register / unregister (HKCU) and the disconnected fallback
  - [x] Required permissions limited to downloads, contextMenus, nativeMessaging, storage, activeTab
  - [ ] Optional media-detection permission prompt (manual)
  - [ ] Behaviour across a browser version update (observe on the next Chrome/Edge update)

## M5 — Advanced Download Management
Add the convenience features expected from a mature download manager.
- [x] Implement download queues and priorities.
  - [x] Queues table, Main queue, priority (low/normal/high) and position per download
  - [x] Engine starts queued downloads by priority, then position, from running queues only
  - [x] Queue list in the sidebar (start/stop, counts), queue view in start order, new/edit/delete queue
  - [x] Right-click menu: priority, move to queue, move to front/back; priority arrows in the list
- [x] Implement scheduling and automatic queue processing.
  - [x] Per-queue schedule: start time, optional stop time, days of week, overnight windows
  - [x] Ticker starts/stops queues; stopping requeues running downloads with progress kept
  - [x] Launching inside a window starts the queue; a scheduled queue stops once empty
  - [x] Schedule editor; new scheduled queues wait for their window (verified in the app)
- [x] Add bandwidth limits and per-download speed controls.
  - [x] Token-bucket limiter: global limit plus each download's own limit, applied live
  - [x] Media downloads pass the tighter limit to yt-dlp (`-r`)
  - [x] Global limit in Settings, per-download limit in the Add dialog and right-click menu
  - [x] In the app: 512 KB/s cap held at ~505 KB/s; lifting it mid-download jumped to ~8.9 MB/s
- [x] Implement batch URL import and bulk actions.
  - [x] Numbered patterns (`file[01-20].zip`, `[a-z]`), pasted lists and .txt import (Batch dialog, Ctrl+B)
  - [x] Bulk priority, queue, speed limit, download again, open, show in folder, copy links, clear finished
- [x] Add categories and automatic destination folder rules.
  - [x] Default categories (Videos, Music, Documents, Compressed, Programs) by extension
  - [x] New downloads go to `<download folder>\<Category>` unless a folder is chosen
  - [x] Category filters in the sidebar, category editor in Settings, older downloads sorted on upgrade
- [x] Resolve filename conflicts safely.
  - [x] Finished files never overwrite: a new copy is saved as "name (1).ext" (engine test)
- [x] Add duplicate detection and download history.
  - [x] Add dialog warns when a link is already in the list; browser links can be added again or skipped
  - [x] History stays in the list (Completed view) and is searchable by name, title or link
- [ ] Add desktop notifications and completion actions. *(in progress)*
  - [x] Windows notifications for completed and failed downloads (settings to turn each off)
  - [x] When all downloads are done: sleep, shut down (60 s, cancellable) or close BDM; runs once
  - [x] "All done" fires exactly once, even when downloads finish together (race found and fixed)
  - [ ] Confirm the notification looks right on your desktop; sleep/shutdown not triggered in testing
- [x] Add retry policies and configurable concurrency limits.
  - [x] Retries per attempt and automatic retries of failed downloads (count and delay) in Settings
  - [x] Broken links (404, 403, sign-in) and checksum failures are never retried automatically
- [x] Test queue recovery, scheduling and simultaneous downloads.
  - [x] Engine tests: priority order, stopped queues, schedule start/stop, launch inside a window,
        auto-retry, speed limits, "all done", restart recovery, concurrency limit (130 Rust tests)
  - [x] In the app: scheduled queue waited, ran on start, sorted files into folders, stopped when empty

## M6 — Security, Reliability and Performance
Harden the application and validate production-quality behavior.
- [x] Review Tauri capabilities and filesystem permissions.
  - [x] Webview gets only event listening, the file/folder picker and BDM's own commands (`core:default` removed)
  - [x] No filesystem, shell, HTTP, opener or notification APIs in the webview; files are opened by Rust from saved records only
  - [x] CSP: scripts and connections limited to the app itself; prototypes frozen; DevTools off in release builds
  - [x] Release build checked: window and opener calls refused by the ACL, live events and all features still work
- [x] Validate URLs, redirects, local paths and subprocess arguments.
  - [x] URLs: http/https only, a host, no embedded user name or password, length limit
  - [x] Redirects: at most 10, http/https only, never from HTTPS down to unencrypted HTTP
  - [x] Folders must be full paths to existing folders; picked .txt imports are size-limited
  - [x] yt-dlp: arguments as a list (no shell), URL after `--`, format ids checked, user config and plugins ignored
- [x] Protect against unsafe filenames, path traversal and unintended file overwrites.
  - [x] Reserved characters, device names (CON, NUL…), `..` and trailing dots cleaned; length capped
  - [x] Invisible right-to-left and zero-width characters removed so extensions can't be disguised
  - [x] Finished files never overwrite ("name (1).ext"); yt-dlp output only accepted from its own temp folder
  - [x] Cleanup only ever deletes BDM's own `.bdmpart` files and `.bdm-media-*` folders
- [x] Prevent unsafe access to local network resources and sensitive endpoints.
  - [x] Every connection checked after DNS (covers redirects and DNS rebinding); literal IPs checked before requests and redirects
  - [x] This PC and the local network blocked unless "Allow local network downloads" is on (Settings → Security)
  - [x] Cloud metadata (169.254.169.254, fd00:ec2::254), link-local, multicast and reserved addresses always blocked
  - [x] Links are checked before yt-dlp runs; configured proxies still work
- [x] Keep credentials and signed URL secrets out of logs.
  - [x] Network errors never include the URL; yt-dlp messages have links stripped (tests)
  - [x] Links with a user name or password are refused, so they never reach the database
  - [x] Cross-site requests send only the page's origin as Referer, never its full address (browser default policy)
  - [x] No log files; console messages carry no URLs or tokens
- [x] Verify downloaded file integrity where checksums are available.
  - [x] Optional SHA-256 per download: a mismatching file is never saved (engine test)
  - [x] yt-dlp, FFmpeg and Deno installs are checked against GitHub's SHA-256 digests
  - [x] Right-click → Copy SHA-256 Checksum for any finished file
  - [x] Sizes are checked against Content-Length / Content-Range before a file is finished
- [x] Test crashes, application restarts and partial-file recovery.
  - [x] Engine tests: restart recovery, pause/resume from saved ranges, queued downloads restart after launch, changed-file restarts
  - [x] Real app: killed mid-download at 5.0 MB of 20 MB → reopened as paused ("Interrupted…"), resumed and finished; SHA-256 matched the server's file
  - [x] Partial files keep their exact size on disk, so a crash never leaves a truncated file under the real name
- [x] Test high concurrency, memory usage and CPU utilization.
  - [x] 16 downloads × 100 MB × 8 connections at once: all 16 complete with exact sizes in 6.4 s (local server)
  - [x] BDM memory: 38 MB idle, 141 MB peak under load, back to ~45 MB after; ~22% CPU of 8 cores at full speed
  - [x] Threads and handles return to idle levels afterwards (19 threads, ~450 handles): no leaks
  - [x] 1,000 downloads in the list: rows on screen at once, search in ~40 ms, add 1,000 links in ~2 s
  - [x] Fixed: removing 1,000 downloads took 64 s → 0.7 s (one bulk command, engine events applied in batches)
- [x] Review third-party dependency licenses and update mechanisms.
  - [x] 315 Rust crates and 12 npm packages in the app: all permissive (MIT/Apache/BSD/ISC/Zlib…); 5 MPL-2.0 crates from Tauri; Montserrat under OFL
  - [x] yt-dlp (Unlicense), FFmpeg (GPL build) and Deno (MIT) are downloaded on request, run as separate programs, never bundled
  - [x] Every tool download checked against GitHub's SHA-256; yt-dlp updates itself on launch; see THIRD_PARTY.md
  - [x] Bundling FFmpeg in the M7 installer would need the GPL source offer or the LGPL build (noted for M7)
- [ ] Run regression, security and Windows compatibility tests. *(in progress)*
  - [x] Regression: 141 Rust tests, 9 extension tests, Clippy (-D warnings) and TypeScript checks pass
  - [x] Security: `cargo audit` finds no vulnerabilities in 548 crates (5 warnings, all in Linux-only or build-time crates); `npm audit` 0
  - [x] Release build exercised on Windows 11 (build 26300): permissions, blocking, crash recovery, load
  - [ ] Windows 10 not tested yet (no Windows 10 machine here)
- [x] Document known limitations and unsupported websites or protocols.
  - [x] LIMITATIONS.md: platforms, protocols (HTTP/HTTPS only), sign-in links, DRM and live streams, browsers, schedules, limits

## M7 — Windows Installer and Release
Package BDM for installation, maintenance and possible public distribution.
- [x] Create Windows x64 installer.
  - [x] NSIS installer `Babarify Download Manager_1.0.0_x64-setup.exe` (3.3 MB), per-user, no admin prompt
  - [x] Ships `bdm.exe`, the browser helper `bdm-native-host.exe` (as a Tauri sidecar) and `legal\`; installs WebView2 if missing
- [x] Configure application metadata, icons and version information.
  - [x] Version 1.0.0 everywhere (app, Cargo, package.json, extension); publisher Babarify; © 2026 Babarify
  - [x] Windows file properties, Add/Remove Programs entry and Start menu shortcut verified; new Brandboard icons
- [x] Implement clean installation and uninstallation.
  - [x] Real install from Explorer on your PC (2026-10-26): BDM starts and registers its browser helper
  - [x] Uninstall cleanup verified by the installer test (program, shortcuts, browser registration removed; data kept)
  - [x] First launch registers the browser helper for Chrome and Edge; re-checked every launch (moves and upgrades)
  - [x] Uninstall removes program, shortcuts, Add/Remove entry and the Chrome/Edge registration; downloads untouched
- [x] Verify download data survives application upgrades and remains user-controlled.
  - [x] Your 7 downloads, settings and media tools carried into the real install from the exported data folder
  - [x] Development build → installed 1.0.0, uninstall → reinstall, and install-over-install: download list, settings and queues kept
  - [x] Data lives in `%APPDATA%\com.babarify.bdm`; the uninstaller only deletes it if you tick the box
- [x] Package or manage required media dependencies in a license-compliant manner.
  - [x] yt-dlp, FFmpeg (GPL) and Deno are not bundled; downloaded on request from official releases, SHA-256 checked (THIRD_PARTY.md)
- [x] Implement a secure signed-update strategy if automatic updates are enabled.
  - [x] 1.0.0 has no automatic updates (install over to upgrade); the signed-update plan (updater key + Authenticode) is in RELEASE.md
- [x] Prepare privacy policy, license notices and release documentation.
  - [x] LICENSE.txt (proprietary), PRIVACY.md, THIRD_PARTY_NOTICES.txt (324 components), RELEASE.md; opened from Settings → About
- [ ] Test on a clean Windows environment. *(in progress)*
  - [x] Installed and uninstalled on this PC (Windows 11 Home); fresh browser profiles used for the browser checks
  - [ ] A PC or VM that never had BDM or its tools (Windows Sandbox needs Windows Pro; none available here)
- [~] Verify Chrome and Edge integration after installation. *(Edge on real Windows left)*
  - [x] Chrome 155 and Edge 156 reach the installed BDM 1.0.0 through the installed helper ("running", version 1.0.0)
  - [x] Your own Chrome connects to the real install (2026-10-26)
  - [ ] Your own Edge (load the extension in Edge and open its popup)
- [ ] Complete final acceptance testing and release checklist. *(in progress)*
  - [x] Release checklist written (RELEASE.md); automated checks pass; install, upgrade, uninstall and browser checks done
  - [x] Tagged `v1.0.0` and published the GitHub release (private repo) with the installer, extension zip and SHA256SUMS
  - [ ] Code signing (needs a certificate) and the clean-PC test
