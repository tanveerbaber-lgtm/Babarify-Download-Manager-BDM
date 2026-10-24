# BDM Master Checklist

Updated after every completed task. Verification evidence is in [RESULTS.md](RESULTS.md).

**Legend:** `[x]` done and verified · `[~]` implemented, verification pending · `[ ]` not started

**Last updated:** 2026-10-24 — M5 built and verified in the app: queues, schedules, speed limits, batch import, categories, duplicate warnings, retries, notifications and completion actions. One check left: notifications on your desktop.

## Progress

| Milestone | Done | In progress | Total | Status |
| --- | --- | --- | --- | --- |
| M1 — Desktop Foundation | 9 | 0 | 9 | ✅ Complete |
| M2 — Core Download Engine | 10 | 0 | 10 | ✅ Complete |
| M3 — Video and Audio Engine | 11 | 0 | 11 | ✅ Complete |
| M4 — Chrome and Edge Integration | 9 | 2 | 11 | In progress (2 manual checks left) |
| M5 — Advanced Download Management | 9 | 1 | 10 | In progress (1 check left) |
| M6 — Security, Reliability and Performance | 0 | 0 | 11 | Not started |
| M7 — Windows Installer and Release | 0 | 0 | 10 | Not started |
| **Total** | **48** | **3** | **72** | |

## M1 — Desktop Foundation
Build the application shell and establish the project architecture.
- [x] Initialize Tauri 2, React, TypeScript, Vite and Rust.
- [x] Create Babarify branding, application icon and professional UI.
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
- [~] Test browser updates, permissions, installation and uninstallation.
  - [x] Extension install (fixed ID) and uninstall in Chrome 155 and Edge 156
  - [x] Native host register / unregister (HKCU) and the disconnected fallback
  - [x] Required permissions limited to downloads, contextMenus, nativeMessaging, storage, activeTab
  - [ ] Optional media-detection permission prompt (manual)
  - [ ] Behaviour across a browser version update (observe on the next Chrome/Edge update)

## M5 — Advanced Download Management
Add the convenience features expected from a mature IDM alternative.
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
- [ ] Review Tauri capabilities and filesystem permissions.
- [ ] Validate URLs, redirects, local paths and subprocess arguments.
- [ ] Protect against unsafe filenames, path traversal and unintended file overwrites.
- [ ] Prevent unsafe access to local network resources and sensitive endpoints.
- [ ] Keep credentials and signed URL secrets out of logs.
- [ ] Verify downloaded file integrity where checksums are available.
- [ ] Test crashes, application restarts and partial-file recovery.
- [ ] Test high concurrency, memory usage and CPU utilization.
- [ ] Review third-party dependency licenses and update mechanisms.
- [ ] Run regression, security and Windows compatibility tests.
- [ ] Document known limitations and unsupported websites or protocols.

## M7 — Windows Installer and Release
Package BDM for installation, maintenance and possible public distribution.
- [ ] Create Windows x64 installer.
- [ ] Configure application metadata, icons and version information.
- [ ] Implement clean installation and uninstallation.
- [ ] Verify download data survives application upgrades and remains user-controlled.
- [ ] Package or manage required media dependencies in a license-compliant manner.
- [ ] Implement a secure signed-update strategy if automatic updates are enabled.
- [ ] Prepare privacy policy, license notices and release documentation.
- [ ] Test on a clean Windows environment.
- [ ] Verify Chrome and Edge integration after installation.
- [ ] Complete final acceptance testing and release checklist.
