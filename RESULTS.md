# Verification Results

Evidence for each checklist item in [MILESTONES.md](MILESTONES.md). Updated as each check runs.

## M1 — Desktop Foundation

Environment: Windows 11 Home (10.0.26300), Node 24.15.0, npm 11.12.1, Rust 1.99.1 (stable MSVC), Visual Studio Community 2026.

| Check | Command | Result | Date |
| --- | --- | --- | --- |
| TypeScript type check | `npm run typecheck` | ✅ Pass, no errors | 2026-10-24 |
| Frontend production build | `npx vite build` | ✅ Pass: JS 248.9 kB (77.9 kB gzip), CSS 12.3 kB | 2026-10-24 |
| Rust compile + unit tests | `npm run test:rust` | ✅ Pass: 18/18 (migrations, settings, downloads repo, validation) | 2026-10-24 |
| Clippy | `npm run lint:rust` | ✅ Pass: no warnings with `-D warnings` | 2026-10-24 |
| Dev build launches | `npm run tauri dev` | ✅ Pass: window opens, migration v1 applied, DB at `%APPDATA%\com.babarify.bdm\bdm.sqlite3` | 2026-10-24 |

### Interactive checks — 2026-10-24

Method: the dev build was started with WebView2 remote debugging (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`) and driven through the Chrome DevTools Protocol. Every action went through the real React UI and Rust backend; results were cross-checked in the SQLite database.

| Check | Result |
| --- | --- |
| Window opens with Babarify branding and icon | ✅ |
| Sidebar navigation: all 6 views switch, active item highlighted, counts update | ✅ |
| Dashboard stats and empty state | ✅ `Total/Active/Pending/Completed` update as rows are added and removed |
| Add dialog opens with Ctrl+N and the toolbar button; URL field focused; default folder filled in | ✅ |
| Invalid URLs rejected inline: `ftp://`, `file://`, `not a url`, `user:pw@` URL | ✅ Each shows a specific message; dialog stays open |
| Valid URL → filename auto-suggested (`My%20Big%20File.iso` → `My Big File.iso`), Pending row created | ✅ |
| Filename with `\` or `/` rejected; `CON.txt` saved as `_CON.txt`; `%5C` in URL becomes `_` | ✅ |
| Relative or missing destination folder rejected | ✅ |
| Rows reload from the database after a page reload | ✅ |
| Shift-click multi-select; Delete key and toolbar open the confirmation; Cancel keeps rows; Remove deletes them | ✅ |
| With "Confirm before removing" off, removal happens without a prompt | ✅ |
| Theme System/Light/Dark applies immediately (background and `data-theme` change) | ✅ |
| Simultaneous downloads: 99 rejected with an error, 5 saved | ✅ (bug found and fixed, see below) |
| `update_settings` rejects a missing folder and unknown fields | ✅ |
| After restarting the app: dark theme on first paint, downloads, folder, concurrency and confirm setting all kept | ✅ |
| Folder picker (Browse / Change…) | ⚠️ Not exercised: it's a native Windows dialog outside the webview. The backend path it feeds (`update_settings` / `add_download` with a folder) is verified. |
| Files on disk untouched by Remove | ✅ by design: `remove_download` only deletes the database row (no filesystem access in M1) |

Test data was removed and settings were reset to defaults afterwards.

### Issues found

1. **Rejected "Simultaneous downloads" value stayed in the field.** Entering 99 showed the error but left 99 in the input. Fixed in `src/components/SettingsView.tsx`: the field now goes back to the saved value when the backend rejects it. Re-tested ✅.

## M2 — Core Download Engine

### Automated tests — 2026-10-24

`npm run test:rust`: **56 passed, 0 failed** (55 at first, plus 1 regression test added below). The suite was run 5 times with no flaky failures. Clippy is clean with `-D warnings`.

The engine tests (`src-tauri/src/engine/tests.rs`) run real downloads against a local HTTP server (`src-tauri/testsupport/server.rs`). The server can switch Range support on and off, set ETags, throttle, drop connections, stall, redirect and return errors.

| Checklist area | Test(s) | Result |
| --- | --- | --- |
| Direct HTTP downloads | `downloads_without_range_support_as_single_stream`, `unknown_length_downloads_to_end_of_stream`, `zero_byte_file_completes` | ✅ |
| Segmented Range downloads | `segmented_download_reassembles_exactly`: 8 ranged requests, each with `If-Range`, byte-identical output | ✅ |
| Large files | `large_file_download_is_byte_identical`: 64 MiB, SHA-256 verified | ✅ |
| Pause / resume | `pause_then_resume_continues_from_saved_ranges`: resume requests only the missing bytes | ✅ |
| Cancel | `cancel_deletes_partial_file_and_progress`: partial file deleted, progress reset, can restart | ✅ |
| Remove while running | `removing_an_active_download_cleans_up` | ✅ |
| Recovery after restart | `recovers_after_app_restart` (running → paused with a message, then resumes from saved ranges), `queued_downloads_restart_after_launch` | ✅ |
| Unreliable connections | `retries_dropped_connections_with_ranges` (6 drops), `survives_many_drops_on_a_throttled_server` (20 drops), `retries_dropped_connection_without_ranges_from_zero`, `stalled_connection_times_out_and_retries` (read timeout) | ✅ |
| Remote identity | `changed_etag_on_resume_restarts_with_new_content`: never mixes old and new bytes | ✅ |
| Range fallback | `falls_back_to_single_stream_when_ranges_disappear` | ✅ |
| Redirects | `follows_redirects_and_records_final_url`, `redirect_loops_fail` | ✅ |
| Expired links / auth / server limits | `http_errors_fail_with_clear_messages` (401, 403, 404, 410; no URL in messages), `server_errors_are_retried_then_fail` (503) | ✅ |
| Checksums | `checksum_match_and_mismatch`: a mismatched file is not saved and no partial file is left | ✅ |
| Safe finalization | `existing_files_are_never_overwritten` → `report (1).pdf`; `finalize_never_overwrites_existing_files` | ✅ |
| Concurrency limit | `respects_concurrency_limit`: 5 downloads with a limit of 2, never more than 2 running | ✅ |
| Progress, speed, ETA | `reports_progress_speed_and_eta`: monotonic progress, speed and ETA present, final event shows 100% | ✅ |

### Issues found while testing

- Progress was only counted when a 256 KB write buffer filled, so slow connections showed no movement for long stretches. The buffer is now 128 KB, it also flushes at least every 200 ms, and a final progress event is sent when a transfer stops.
- **Mid-download connection drops could fail instead of retrying.** reqwest sometimes reports a dropped body as a "decode" error, which wasn't classed as retryable. Any error while streaming a body is now retried. Found in the app with the flaky test server, reproduced by `survives_many_drops_on_a_throttled_server`, then fixed.
- **The UI could show a running download as "Queued".** The engine reported "downloading" before the Add command returned, and the command's older snapshot then overwrote that state, so progress was ignored. The Add result now never replaces a row that an engine event has already updated.

### In-app checks — 2026-10-24

Method: the dev build ran with WebView2 remote debugging and was driven through the real UI over the DevTools Protocol, against `cargo run --example test_server` on `127.0.0.1:8787`. Downloaded files were compared by SHA-256 with full copies fetched by `curl`.

| Check | Result |
| --- | --- |
| Existing v1 database upgrades to schema v2 on launch | ✅ |
| 100 MB segmented download (`big.bin`) | ✅ byte-identical, no `.bdmpart` left |
| Live row: `6.3 MB / 20.0 MB · 31% · 2.5 MB/s · 6s`; status bar `1 active · 2.5 MB/s` | ✅ |
| Toolbar enables only the valid actions for the selection | ✅ |
| Pause holds progress (41%, unchanged after 1.2 s); Resume continues from there | ✅ |
| Cancel → Cancelled, partial file deleted | ✅ |
| Server without Range support (15 MB) | ✅ completed via a single stream |
| Flaky server (20 dropped connections) | ✅ after the fix above; byte-identical |
| Expired link (HTTP 410) | ✅ Failed: "The link has expired (HTTP 410). Get a fresh link and try again." shown under the name |
| SHA-256 mismatch | ✅ Failed with expected/actual hashes; no file saved |
| Malformed SHA-256 in the Add dialog | ✅ rejected inline |
| Simultaneous downloads = 1 (set in Settings) | ✅ second download stays Queued until the first finishes |
| **Hard kill mid-download** (`Stop-Process -Force` at 38%) | ✅ on relaunch: Paused, "Interrupted when the app closed. Resume to continue.", 5.0 MB kept; Resume finishes; byte-identical |

Test downloads and the scratch folder were deleted afterwards, and settings were restored.

### Known limitations (to address later)

- Filenames come from the URL; the server's `Content-Disposition` filename isn't used yet.
- If a server sends no `Content-Length` and closes early, the truncation can't be detected (there's no size to compare against).
- Downloads that need cookies or a sign-in aren't supported; these arrive with browser integration (M4).
- Bandwidth limits and scheduling are M5.

## M3 — Video and Audio Engine

### Automated tests — 2026-10-24

`npm run test:rust`: **84 passed, 0 failed** (28 new for M3). Clippy is clean with `-D warnings`.

| Area | Tests | What they prove |
| --- | --- | --- |
| Tool manager | `installs_verified_tools_from_releases`, `rejects_downloads_that_fail_verification`, `picks_newest_static_gpl_ffmpeg`, `requires_a_valid_digest`, `extracts_only_wanted_files_by_name` | Installs yt-dlp/FFmpeg/Deno from a (fake, local) GitHub API; keeps only the needed executables; detects updates; refuses tampered or unverifiable downloads and leaves the existing copy untouched; zip entries can't escape the target folder |
| Analysis | `lists_qualities_from_360p_to_4k`, `excludes_storyboards_and_drm_formats`, `detects_drm_only_media`, `parses_flat_playlists`, `single_url_results_get_one_format` | Qualities 360p–2160p with fps and size estimates that match yt-dlp's choice; DRM formats hidden and DRM-only media blocked; playlist entries parsed |
| Command lines | `video_selectors`, `audio_selectors`, `rejects_unsafe_format_ids`, `url_comes_after_double_dash` | Correct `-f`/`-S` selectors; format ids can't inject flags; the URL always comes after `--` |
| Output & errors | `parses_progress_lines`, `parses_other_lines`, `maps_common_errors`, `unknown_errors_drop_prefix_and_urls`, `stage_labels`, `combines_progress_of_two_streams` | Progress across two streams; DRM / sign-in / unavailable / unsupported / 403 messages; URLs (and tokens) stripped |
| **Real yt-dlp + FFmpeg** (local clips) | `downloads_video_with_audio_and_never_overwrites`, `extracts_audio_to_mp3`, `pause_resume_and_cancel_media`, `unsupported_pages_fail_clearly`, `analyzes_a_direct_media_url` | H.264+AAC output verified with ffprobe; second copy saved as "(1)"; MP3 conversion with a "Converting audio" stage; pause keeps the temp folder and resume continues with a Range request; cancel removes it; no temp folders left after failures |

### Real-site checks in the app — 2026-10-24

All on Creative Commons films from the official Blender channel, driven through the real UI (WebView2 remote debugging). Files were checked with ffprobe and then deleted.

| Check | Result |
| --- | --- |
| Settings → Media tools shows the system copies (winget) with versions | ✅ |
| **Install / update BDM's copies** downloads yt-dlp 2026.10.21, FFmpeg n9.0 (BtbN, GPL static) and Deno 2.9.8 | ✅ checksums in the manifest match GitHub's published digests; yt-dlp re-hashed independently |
| Check for updates | ✅ "BDM's tools are up to date." |
| Analyze *Big Buck Bunny* | ✅ title, uploader, 10:35, site and "License: Creative Commons Attribution license (reuse allowed)"; 144p–2160p (4K) 60fps with sizes; 57 exact formats |
| 360p MP4 (most compatible) | ✅ H.264 640×360 + stereo AAC, full length; size 27.2 MB matched the ≈27.2 MB estimate |
| Separate streams merged by FFmpeg | ✅ "Downloading stream 1 of 2 → 2 of 2 → Merging video and audio → Finishing" |
| 720p MKV, **paused at 70% and resumed** | ✅ resumed from 68.2 MB (no restart); AV1 1280×720 + 6-channel AAC; the interrupted audio stream decodes with zero errors |
| Audio only → MP3 | ✅ stereo MP3, full length; stages "Converting audio → Finishing" |
| Playlist "Official Blender Open Movies" | ✅ 17 entries with durations; 2 selected (*Tears of Steel*, *Sintel*) → "Download 2 videos" → both completed as original Opus (no re-encode) |
| Unsupported site (`https://example.com/`) | ✅ "This website isn't supported for video downloads. Try adding the link as a regular file download." |

### Issues found while testing (all fixed)

1. **"MP4 (most compatible)" picked AV1 + 6-channel audio.** Selecting by `.mp4` extension wasn't enough. Now yt-dlp's format sorting ranks resolution first (so 4K stays 4K), then H.264, stereo, AAC. 360p and 1080p now give H.264 + stereo AAC; 4K gives VP9 + AAC because no H.264 exists there.
2. **Size estimates were up to 2× off.** The estimates now mirror the same sort order, with separate figures for MP4 and MKV.
3. **Post-processing stages never showed.** yt-dlp prints them on stderr; both pipes are now read in order.
4. **A failed download left an empty hidden temp folder.** It's now removed (folders with partial data are kept so a retry can continue).
5. **Deadlock in that cleanup**: a database lock held across an `if let` was taken again. Caught by the test suite hanging; fixed by releasing the lock first.
6. **Tool status took 2.1 s.** The three version checks now run in parallel, and the UI shows "Checking tools…" meanwhile.
7. **Disk full during a rebuild** (E: had 0.1 GB left; the build output was 13.6 GB). Dev builds now use line-table debug info only for BDM and none for dependencies, which brought the build folder to about 1.8 GB.

### Known limitations

- No live DRM site was tested; DRM handling is covered by unit tests on yt-dlp's `has_drm` flags and by its error messages.
- Daily yt-dlp auto-update reuses the tested install/update path; the once-a-day timing itself was not exercised.
- Sites needing sign-in or cookies, and live streams, are not supported yet.
- Thumbnails aren't shown (remote images are blocked by the CSP).
- yt-dlp's own download is a single connection for direct files; HLS/DASH fragments use up to "Connections per download" in parallel (`-N`).

## M4 — Chrome and Edge Integration (and branding pack)

### Automated tests — 2026-10-24

- `npm run test:rust`: **99 passed** (15 new): framing and size limits, strict message parsing (unknown types and fields such as `cookies` are rejected), URL and referrer validation, constant-time token check, the pipe server (authorized requests, wrong tokens, refusing a second server on the same pipe name), the native host (refuses unknown extension origins; a ping never starts BDM; starts BDM and forwards when it isn't running), and a registry manifest that lists only BDM's extension ID.
- `npm run test:extension`: **8 passed**: capture decisions (file types, MIME fallback, private windows, own downloads, blob/data URLs, excluded sites, minimum size), message contents (no cookies; non-http referrers dropped), settings normalization, media classification (segments ignored), **the manifest key produces the ID the native host allows**, and a minimal permission set.
- Clippy clean; TypeScript and frontend build clean.

### Real browsers — 2026-10-24

Each run used a throwaway profile, with the unpacked extension loaded over the DevTools pipe (branded Chrome no longer supports `--load-extension`) and downloads clicked as real mouse events. Downloads came from the local test server.

| Check | Chrome 155 | Edge 156 |
| --- | --- | --- |
| Extension loads with the fixed ID `emaohblojdddihemfkbcchbfgaaanjkb` | ✅ | ✅ |
| Popup: "Connected to BDM 0.1.0" | ✅ | ✅ |
| Clicked `archive.zip` (8 MB) is captured: nothing in the browser's folder; completed in BDM with the page as referrer | ✅ | ✅ |
| Clicked `small.txt` (type not selected) stays in the browser | ✅ | ✅ |
| "Download link with BDM" path | ✅ | ✅ |
| `file://` / `javascript:` URLs rejected | ✅ | ✅ |
| "Download video or audio from this page" opens BDM's Video / audio tab, prefilled and analyzed | ✅ | — |
| Uninstall removes the extension | — | ✅ |
| **BDM not running**: popup "BDM will start when needed"; a click starts BDM via the host and the download completes; BDM keeps running after the browser closes | ✅ | — |
| **Disconnected** (registration removed): popup explains how to reconnect; downloads stay in the browser | ✅ | — |

Registry: `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.babarify.bdm` and `...\Microsoft\Edge\...` point at `%APPDATA%\com.babarify.bdm\native-host\com.babarify.bdm.json`, whose `allowed_origins` lists only `chrome-extension://emaohblojdddihemfkbcchbfgaaanjkb/`.

### Issues found while testing (all fixed)

1. **Capture silently stopped working when something else controls file naming.** `onDeterminingFilename` isn't fired when DevTools, a policy or another download extension manages file names. Capture now uses `downloads.onCreated`, which always fires.
2. **When BDM wasn't connected, downloads were cancelled and restarted in the browser**, wasting the bytes already received. The extension now checks a cached connection state (60 s) and leaves downloads alone while BDM is unreachable.
3. **Badge update on a closed tab** threw an unhandled rejection in the service worker. Fixed.
4. Test-server fix: responses now send `Content-Type: application/octet-stream`, so browsers download rather than display them.

### Your own Chrome — 2026-10-24

| Check | Result |
| --- | --- |
| Popup in your everyday Chrome profile after **Connect** | ❌ at first ("Specified native messaging host not found"); ✅ after restarting Chrome: "BDM will start when needed" |
| "Download video or audio from this page" with BDM closed | ❌ with the development build (blank page: it needs the dev server); ✅ with the release build: BDM started and opened the Video / audio dialog for the page |

Fixes from this: the popup now shows the browser's exact error and says to restart the browser after connecting; the README and Browser integration page explain using the release build when the browser starts BDM.

### Not covered automatically

- Media detection on live pages: the optional `webRequest` and host permission prompt has to be accepted by a person.
- Behaviour across a Chrome or Edge version update.
- Downloads that need the browser's sign-in cookies are not supported by design (cookies are never sent to BDM); excluded sites in the extension options keep those in the browser.
- In this test harness, navigating straight to a throttled binary URL didn't start a browser download even without the extension; clicking links (the real user path) works.

### Branding pack

Applied from `assets/Babarify Download Manager Brand Guide.png`:
- The "B" mark redrawn as SVG (`assets/brand/bdm-mark.svg`) and the navy-tile app icon (`bdm-app-icon.svg`); Windows/app icons and extension icons regenerated from it.
- Palette: Primary Blue #2563EB, Primary Green #10B981, Dark Navy #0F172A, Gray #64748B, Light Gray #E2E8F0.
- Montserrat bundled locally (OFL-1.1, `@fontsource-variable/montserrat`), so the app works offline and the CSP stays strict.
- Navy sidebar with Downloads / Video downloader / Browser integration / Settings, status dots, and the tagline "Faster Downloads. More Possibilities." on the empty state and in the extension.

### On-video "Download With BDM" panel — 2026-10-24

**Automated:** `npm run test:rust` 103 passed (new: choice list from an analysis, including DRM and playlist refusals; strict parsing of the two new messages, with media options validated so a format id can't inject flags). `npm run test:extension` 9 passed (new: pages may send only `formats` and `mediaDownload` with an http(s) URL; the panel is off by default; manifest asks for `scripting` only as an optional permission).

**Real browsers** (throwaway profiles; test copy of the extension with site access pre-granted because a script can't click Chrome's permission prompt; same code and ID):

| Check | Result |
| --- | --- |
| Setting off: no script registered, no panel on a video page | ✅ Chrome |
| Local page with an HTML5 video: panel just above the video's top-right corner | ✅ Chrome, Edge |
| Format list: local page in 4–6 s (MP4 best, MP3, M4A) | ✅ Chrome, Edge |
| YouTube (*Big Buck Bunny*, CC-BY): list in 13 s with 4K/1440p/1080p/720p/480p/360p/240p/144p, sizes, MP3/M4A, title and license | ✅ Chrome |
| One click → "✓ Downloading in BDM" and the file completes in BDM (MP4 local 243,751 B; MP3 local; YouTube 360p 28,523,658 B) | ✅ |
| Clicking the panel doesn't pause or play the video; × hides it for that video | ✅ |

Improvement made while testing: formats are requested as soon as the pointer reaches the bar, so the list appears sooner.

**Your own Chrome:** panel switched on, site-access prompt accepted, bar shown above a YouTube video and the full format list loaded (1080p → 144p with sizes, MP3/M4A). ✅

**Restyle (your feedback: smaller, more premium):** bar 30 → 26 px as a frosted navy pill with a blue → green edge; menu 330 → 248 px, dark glass, 25 px rows with MP4/MP3/M4A tags, 4K/2K/HD badges, frame rate, right-aligned sizes, Video/Audio groups and a thin scrollbar. Checked on YouTube in Chrome: the full list and license fit without scrolling.

## M5 — Advanced Download Management

### Automated tests — 2026-10-24
- 130 Rust tests pass (`cargo test`), Clippy clean with `-D warnings`, 9 extension tests, `tsc --noEmit` clean.
- New engine tests: start order by priority then position; a stopped queue starts nothing; stopping a queue
  requeues running downloads with progress kept; a schedule window starts and stops a queue; launching inside
  a window starts it and it stops once empty; failed downloads retry automatically, then give up (404s never
  retry); a 400 KB/s per-download limit is respected and lifting it applies at once; "all done" fires once.
- Unit tests: schedule windows (overnight, day filter, missed windows), token-bucket limiter, batch patterns and
  URL extraction, queue/category repositories, category backfill.

### In the app (release build, driven over WebView2 DevTools) — 2026-10-24
- Upgrade: database moved to schema v5; Main queue and five categories present; your existing downloads were
  sorted into Videos/Music.
- Scheduled queue "Night Test" (03:00–04:00) was created stopped; three batch-added links stayed queued, were
  saved under `Downloads\Compressed`, `Downloads\Documents` and `Downloads` (no category), completed after
  Start Queue, and the queue stopped itself when empty.
- Speed limit: 512 KB/s measured at ~505 KB/s; removing it mid-download gave ~8.9 MB/s.
- Right-click menu, Priority and Move to Queue submenus, queue header (schedule pill, Start Queue, Schedule…),
  schedule editor, Batch dialog (120-link pattern preview), Add dialog duplicate warning and new Settings cards
  all checked by screenshot. Test downloads, files and queues were removed afterwards.

### Issues found while testing (all fixed)
- A new scheduled queue started running immediately → it now waits for its window.
- Downloads from before M5 had no category → they're sorted on startup.
- Long queue names pushed the queue buttons off-screen → the header wraps.
- Two downloads finishing together could report "all done" twice (and so sleep/shut down twice) → fixed.

### Not covered
- How the Windows notification looks on your desktop, and the sleep / shutdown actions (not triggered on
  this PC during testing).

## M6 — Security, Reliability and Performance

### Automated — 2026-10-25
- 141 Rust tests, 9 extension tests, Clippy `-D warnings`, `tsc --noEmit`: all pass.
- `cargo audit`: 0 vulnerabilities in 548 crates; 5 warnings (unmaintained `proc-macro-error`, `zbus_names`,
  `zvariant*`; unsound `glib`), none of which is in the Windows build (`cargo tree -i … --target x86_64-pc-windows-msvc`).
- `npm audit`: 0 vulnerabilities.
- New tests: address classification (private, metadata, NAT64, IPv4-mapped), local-network blocking for literal IPs and
  `localhost` (nothing reaches the server, no retry), HTTPS→HTTP redirect refused, metadata redirects refused, Referer
  policy, yt-dlp errors without links, invisible characters in file names, temp-file deletion guard.

### Release build — 2026-10-25
- Permissions: `plugin:window|set_title` and `plugin:opener|open_url` refused by the ACL; live events still arrive.
- `http://127.0.0.1:8787/…` refused with "points to this PC or your local network" and no retry; after allowing local
  downloads the file arrived and Copy SHA-256 matched a reference copy.
- Crash: BDM killed with `Stop-Process -Force` at 5.0 MB of 20 MB; on restart the download was paused ("Interrupted…"),
  resumed from 5.0 MB, finished in 6 s, and its SHA-256 matched the server's file.
- Load: 16 × 100 MB at 8 connections each finished in 6.4 s with exact sizes; memory 38 MB idle → 141 MB peak → ~45 MB
  after; ~22% CPU of 8 cores; threads/handles back to 19 / ~450 afterwards.
- 1,000 downloads: added in ~2 s, shown at once, search ~40 ms. Removing them took **64 s** → fixed with a bulk command
  and batched UI updates → **0.7 s**.

### Issues found and fixed
- The webview had `core:default` (window, webview, menu, tray, path APIs) → only event listening is granted now.
- A web page could make BDM fetch from this PC, the LAN or cloud metadata → blocked (setting to allow the LAN).
- HTTPS downloads could be redirected to plain HTTP → refused.
- Full page addresses (possibly with tokens) were sent as Referer to other sites → origin only.
- Right-to-left override characters could disguise an extension → removed.
- Cleanup trusted the stored temp path → only BDM's own temp files can be deleted.
- yt-dlp could load plugins from the user's folders → `--no-plugin-dirs`.

### Not covered
- Windows 10 (no machine available).

## "Download File Info" dialog and download window — 2026-10-25

Requested after M6 from IDM screenshots; checked in the release build, with requests sent through the real native host.
- File request (`download`, 20 MB, browser path in the name, referrer): dialog showed "Sample Video Archive.mp4",
  20.0 MB, category Videos (from the extension), folder `Downloads\Videos`, queue Main. Start Download → the download
  window opened → completed in ~4 s → completion summary (Open / Open Folder / Close); file saved in the new
  `Downloads\Videos` folder.
- Panel choice (`mediaDownload` for Big Buck Bunny, "MP4 · 1080p 60fps", ≈ 340 MB): dialog showed the title, format
  chip, size estimate, editable name with `.mp4` kept, category Videos. Cancelled (nothing downloaded).
- Download window on a 100 MB download limited to 3,000–4,000 KB/s: status, size, %, speed (~3–4 MB/s), time left,
  resume support; connection map and table for all 8 connections; Speed Limit tab showed the live limit.
- Found and fixed: the window's sections shrank (connection map a thin line, panel text overflowing) → fixed layout.
- Test downloads, the test file and the local-network setting were restored afterwards.

## Brandboard refresh — 2026-10-25

- New app icon drawn as SVG from the Brandboard (`assets/brand/bdm-app-icon.svg`, `bdm-mark.svg`), compared side by side
  with the Brandboard art, then generated to every size with `tauri icon` (PNG, ICO, Store logos) and to the extension's
  16/32/48/128 icons.
- UI: pill buttons (gradient primary, soft secondary), status chips with icon circles, gradient progress bars, stat cards
  with colored icon tiles, solid blue current page in the sidebar, navy splash screen; checked in the installed app.

## M7 — Windows Installer and Release — 2026-10-25

- `npx tauri build` → `Babarify Download Manager_1.0.0_x64-setup.exe` (3.3 MB). `bdm.exe` file properties: product and
  description "Babarify Download Manager", version 1.0.0, company Babarify, © 2026 Babarify. All rights reserved.
- Silent install (`/S`, 8 s): `%LOCALAPPDATA%\Babarify Download Manager\` with `bdm.exe`, `bdm-native-host.exe`
  (721 KB, the real helper), `uninstall.exe`, `legal\` (LICENSE, PRIVACY, THIRD_PARTY_NOTICES); Add/Remove Programs
  entry (1.0.0, Babarify); Start menu shortcut.
- First launch: Chrome and Edge registration and the helper manifest switched to the installed helper; existing data
  (7 downloads, schema v5) opened by the installed app.
- Silent uninstall: program folder, Add/Remove entry, Start menu folder, Chrome/Edge keys and helper manifest removed;
  app data and downloaded files kept.
- Reinstall and install-over-install: data and browser registration intact.
- Chrome 155 and Edge 156 (fresh profiles with the extension) → installed helper → `{"ok":true,"app":"running","version":"1.0.0"}`.
- Build issues fixed: sidecar build failed on the space in the project path (no shell now); file locks from a running
  helper/BDM during rebuilds.
- Not covered: a clean PC/VM, code signing (no certificate), the interactive installer pages (silent mode was used),
  and the uninstaller's "delete app data" option (not ticked, to keep your data).

### Release v1.0.0 — 2026-10-26

- Annotated tag `v1.0.0` on commit `0976427`, pushed to GitHub.
- GitHub release "Babarify Download Manager 1.0.0" (private repo, not a draft or pre-release):
  https://github.com/tanveerbaber-lgtm/Babarify-Download-Manager-BDM/releases/tag/v1.0.0
- Assets (uploaded sizes match the local files):
  - `BabarifyDownloadManager_1.0.0_x64-setup.exe`, 3,459,347 bytes,
    SHA-256 `40eee55c567b74fd311f39937afd1ea9a2df068e5b6c2891556ea8aa454b80db`
  - `BabarifyDownloadManager_1.0.0_browser-extension.zip`, 60,615 bytes (extension files only, no keys),
    SHA-256 `6636948e7c43d76b35aaee0bbe8bd5e1e7598f3d9f96e0867784038d8d881a6d`
  - `SHA256SUMS.txt`

### Correction — install tests ran inside the Claude app's container (2026-10-26)

The Claude desktop app is a packaged (MSIX) app. Every program started from Claude, including the installer, BDM and
the Chrome relaunched with logging, runs inside its container. Windows redirects their writes to `%APPDATA%`,
`%LOCALAPPDATA%` and `HKCU\Software` into `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\...`, where the
user's own programs can't see them.

- The installer's behaviour (files, shortcut, uninstall entry, helper registration, uninstall cleanup, data kept) was
  real, but the results existed only inside that container. Your real Windows had no BDM install, data or browser
  registration.
- This explains the recurring "Specified native messaging host not found" in your own Chrome, and why a Chrome
  restarted from Claude (inside the container) connected while your own restarts didn't. The "Not authorized" error
  came from mixing the two environments.
- Fix: your data (7 downloads, settings, media tools) was exported to `E:\BDM Data Export\com.babarify.bdm`
  (integrity check ok), the container copy was uninstalled, and the real install is done by you from Explorer.
  The M7 install and browser items are back to "confirm on real Windows".
