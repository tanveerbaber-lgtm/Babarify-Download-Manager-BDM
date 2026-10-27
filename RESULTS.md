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

## "New Download" dialog and download window — 2026-10-25

Requested after M6 from reference screenshots; checked in the release build, with requests sent through the real native host.
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

### Real install confirmed (2026-10-26)

You installed `Babarify Download Manager_1.0.0_x64-setup.exe` from Explorer (outside Claude), restored
`E:\BDM Data Export\com.babarify.bdm` to `%APPDATA%`, started BDM and fully restarted Chrome: the extension connects
and all downloads are present. Edge on real Windows hasn't been checked yet.

## Fix: size and time left for video downloads (2026-10-26)

- Reported: during a YouTube download the download window showed no File size or Time left (and an empty progress
  bar) while the first of two streams downloaded.
- Cause: BDM reported a total only once the sizes of **all** streams were known; the audio stream's size is unknown
  until it starts.
- Fix: yt-dlp now also prints its exact-or-approximate size of everything it will download before starting
  (`%(filesize,filesize_approx)s`). BDM uses it until both streams' exact sizes arrive, and never shows less than
  what has already arrived. Checked against YouTube (Creative Commons Big Buck Bunny, 399+258): yt-dlp reports
  155,154,487 bytes up front.
- Tests: parser and progress-total tests added (142 Rust tests pass).

## Compact UI, Arial typography and compact popups (2026-10-26)

Requested with another download manager as a density reference (not copied), keeping the Babarify colours.
- **Fit:** the sidebar no longer grows past the window (Settings was cut off behind the taskbar); its middle scrolls
  while the brand and Settings stay put. Show, Queues and Categories fold, and the choice is remembered.
- **Density:** 13 px body text, 30 px controls, a slim summary strip instead of tall cards, tight table rows, shorter
  dates ("Oct 26, 2:36 AM"), wider Size/Progress/Status columns so nothing truncates.
- **Small windows:** under 1100 px the Added column hides; under 980 px the sidebar becomes an icon rail with tooltips
  and Time left hides; the Name column always keeps its width. Minimum window 820×520.
- **Typography:** Arial for body text, Arial Bold for headings and titles, in the app, splash, extension popup,
  options and video panel; the bundled Montserrat font was removed (smaller app, 323 notices).
- **Popups:** every dialog (Add Download, New Download, Batch, Queue, Category, Speed Limit, Confirm, video form),
  the right-click menu, toasts, the download window (now 540×480) and the extension pages were compacted.
- Checked in a sample-data preview at 1333×720 (150% scaling), 1050×640 and 900×540, light and dark, plus screenshots
  of every popup. TypeScript, 9 extension tests, 142 Rust tests and Clippy pass.

### Release v1.0.0 updated (2026-10-26)

- Tag `v1.0.0` moved from `0976427` to `c76d542` (compact UI, Arial, compact popups, video size fix) and force-pushed.
- Release assets replaced (sizes match the local builds):
  - `BabarifyDownloadManager_1.0.0_x64-setup.exe`, 3,286,012 bytes,
    SHA-256 `9cb93851e979a19ebd767fa43c61228dcecb4fb1b2be4ed170149516abadb14b`
  - `BabarifyDownloadManager_1.0.0_browser-extension.zip`, 60,911 bytes,
    SHA-256 `71a4ec5b105b7488ab4c693f36472bc60bbe946c14cc07288b8e9ebc5c325b8a`
  - `SHA256SUMS.txt`; release notes updated for the new look and the video fix.

## Similarity audit fixes, navy headers, Mark of the Web (2026-10-26)

From the BDM vs IDM audit (https://claude.ai/artifact/Q68KwaAAQN9XTfei7tEsz5):
- The browser confirm dialog is now **New Download** (dialog, Settings text, code comments); the download window's first
  tab is **Progress**. No "IDM", "Internet Download Manager" or "Download File Info" remains in the app, code or docs.
- **Mark of the Web:** finished downloads (files and media) get a `Zone.Identifier` stream (ZoneId=3, HostUrl and
  ReferrerUrl without query strings or credentials), as browsers write, so SmartScreen and Office warn before opening
  risky files. Unit test plus an engine test reading the stream of a real completed download (145 Rust tests pass).
- **Navy headers:** dialog headers and tab strips and the download window's tab strip use the sidebar navy with white
  text; on Windows 11 the main and download windows' title bars are painted navy (DWM caption, border and text colour).
  Checked in the sample-data preview; the title-bar colour shows only in the real app.

### Release v1.0.0 installer replaced again (2026-10-26)

- New installer built from `34fb97b` (audit fixes, Mark of the Web, navy headers) and uploaded over the old one:
  `BabarifyDownloadManager_1.0.0_x64-setup.exe`, 3,286,134 bytes,
  SHA-256 `a9fb64ef5d699083302da273ca255dcc9c8cc89f368dc3850ad62f1e86944463` (matches the release's digest).
- The extension zip is unchanged (no extension changes since `c76d542`). `SHA256SUMS.txt` replaced.
- Release notes now say **New Download** instead of "Download File Info" and list Mark of the Web and the navy headers.
- Tag `v1.0.0` moved from `c76d542` to `34fb97b` and force-pushed.

## Sign-in downloads (2026-10-26)

Fixes "This media requires a signed-in account" (for example YouTube's "Sign in to confirm you're not a bot").
- **Extension:** new optional `cookies` permission, requested per site (`*://site/*`, `*://*.site/*`) from the new
  "Sign-in downloads" section of the options. For allowed sites only, and never from private windows, requests to BDM
  (captured downloads, right-click menu, video panel) carry the cookies `chrome.cookies.getAll({url})` returns for that
  URL. When BDM answers that a sign-in is needed, the video panel shows **Use My Browser Sign-In**, which opens the
  options with the site filled in. Removing a site withdraws its permission (and `cookies` itself once none are left).
- **BDM:** the protocol accepts an optional `cookies` list (unknown fields still refused); each cookie must belong to the
  request's host and contain no control characters, at most 300 cookies / 48 KB. Cookies live in an in-memory jar
  (never the database) for 12 hours. yt-dlp (analysis and downloads) gets `--cookies` with a temporary Netscape file
  that is deleted when it exits; file downloads send a `Cookie` header following host, path and secure rules, which
  reqwest drops on redirects to another host. If a sign-in is still refused with cookies, BDM says to sign in to the
  site in the browser.
- **Checks:** 152 Rust tests (7 new: validation, jar matching and replacement, expiry, Netscape file and deletion,
  protocol, panel hint, and an engine download that sends the cookie on every request), 11 extension tests (2 new),
  Clippy and TypeScript clean. The real yt-dlp sent HttpOnly and session cookies from BDM's file format to their host
  and not another site's cookie. Not yet checked in your Chrome with a real signed-in site.
- PRIVACY.md, LIMITATIONS.md and the app's Browser page describe the feature.

## Stream capture fallback in the video panel (2026-10-26)

When yt-dlp has no extractor for a page and finds no video in it, the panel no longer stops at "This website isn't
supported".
- **BDM:** replies to `formats` carry `unsupported: true` for that case. `formats` and `mediaDownload` accept an optional
  `referrer` (validated); analysis passes it to yt-dlp as `--referer` (browser referrer policy applied) and media
  downloads store it, so yt-dlp sends it while downloading. The New Download dialog keeps it when you confirm.
- **Panel:** on `unsupported` it collects the playing video's sources: `currentSrc`/`src`/`<source>` when they're real
  addresses (not `blob:`), then `.m3u8`/`.mpd` streams in load order (master playlist first) and video files the page
  loaded, read from the page's resource timing (no new permission; buffer raised to 1,000 entries). It asks BDM about
  up to 4 in turn, with the page as referrer; the first that works is offered under "From This Page's Player", titled
  with the page's title. A player using encrypted media (`mediaKeys`) is refused as DRM before anything is sent.
- **Checks:** 154 Rust tests (2 new). The new media test uses the real yt-dlp and FFmpeg: an HLS stream served only
  with a `Referer` fails without one, is analysed with the page as referrer, and downloads to an MP4 with H.264 video.
  11 extension tests (panel referrer rules added). In Chrome with a stubbed BDM, a page playing a `blob:` source that
  had loaded `master.m3u8`, a `.ts` segment and an ad file: the panel asked for the page, then for `master.m3u8` with
  the page as referrer, and showed its choices. Clippy and TypeScript clean. Not yet tried in your Chrome.

## Browser request headers for detected videos, main video before ads (2026-10-26)

How IDM gets videos from sites it doesn't know: it repeats the browser's own request. BDM now does the same when
media detection is on.
- **Extension:** media detection also listens to `webRequest.onSendHeaders` (with `extraHeaders`, so Referer and
  Origin are visible) and keeps, per request, only User-Agent, Referer, Origin, Accept, Accept-Language and the page's
  `X-` headers (not `X-Forwarded-*`), never Cookie or Authorization. They're stored with the detected video in the
  extension's session storage and attached when that video is sent to BDM (panel, popup). The panel asks the
  background for the tab's detected videos (addresses only), ranked: ad hosts last, streams before files, master
  playlists before variants, then load order; files by size.
- **Panel:** tries the player's own address, then detected videos, then what the page loaded (up to 5). A result
  much shorter than the video being watched (under 60% of a video over a minute) is kept only as a last resort.
- **BDM:** `media`, `formats` and `mediaDownload` accept `headers`; names and values are validated (no control
  characters, at most 32) and filtered to the same allowlist, then kept in memory by video address for 12 hours.
  yt-dlp gets them as `--add-header` for analysis and downloads; a captured Referer replaces the guessed one.
- **Checks:** 160 Rust tests (5 new). Real yt-dlp and FFmpeg: an HLS stream that the server serves only with an
  `X-Stream-Token` header fails without it, is analysed with it, and BDM's engine downloads it (the header goes with
  every segment). 13 extension tests (header filtering, ranking). In Chrome with a stubbed BDM, a 15 s ad stream
  ranked first was skipped for the 90 s main stream. Clippy and TypeScript clean. Header capture inside a real
  Chrome with the permission granted hasn't been tried yet.

## More player setups in the video panel (2026-10-26)

Local test pages copying common player setups, each checked in Chrome with a stubbed BDM:
- **Players built as web components** (the `<video>` inside an open shadow root): the panel never appeared, because
  `querySelectorAll("video")` can't see into shadow roots. It now also searches open shadow roots, only on pages
  without a plain `<video>`. Test: a `<fancy-player>` element; the panel appeared, fell back to the stream and listed
  its choices.
- **Players embedded from another site** (a cross-origin frame): the panel already ran in the frame, but titled the
  download with the frame's title (often "player"). The background now returns the tab's title with the detected
  media, and the panel uses it inside frames. Test: player on `localhost` inside a `127.0.0.1` page; the menu showed
  the page's title.
- **Servers that want the browser's cookies:** media detection notes whether the browser sent cookies with a video's
  request (never their values). If BDM is then refused (HTTP 401/403) and that server's site isn't allowed for
  sign-in downloads, the panel says which site and offers "Use My Browser Sign-In" for it, using its base domain
  (`player.cdn.example.net` → `example.net`, `bbc.co.uk` kept whole). The background only opens the options for the
  tab's own site or a server media detection saw on that tab. Test: the panel showed the message and the button sent
  `allowSignIn` for `example.net`.
- 14 extension tests (cookie-refusal and base-site rules added). BDM itself is unchanged this round.

## Checked in your Chrome (2026-10-26)

With the installed test build and the reloaded extension:
- **Stream fallback:** on the hls.js demo page (https://hlsjs.video-dev.org/demo/, player with a `blob:` source,
  page not readable by yt-dlp) the panel showed "From This Page's Player" with the page's title and the stream's
  qualities: 1080p, 720p, 480p, 288p, 184p, MP3 and M4A.
- **YouTube:** the episode that earlier showed "This media requires a signed-in account" now lists 1080p to 144p and
  audio. Whether youtube.com is on the sign-in list couldn't be checked (extension pages can't be opened by the tool).
- Not done: an actual download (needs your go-ahead), the sign-in prompt itself, and header capture on a site that
  needs it.

### Full download in your Chrome, and a naming fix (2026-10-26)

- From the hls.js demo page: panel → MP4 · 184p → BDM's New Download dialog → Start Download. BDM downloaded
  21.4 MB of HLS segments at about 1.3 MB/s, merged them and saved a 19.1 MB MP4: H.264 320×184 and AAC, 10 min 34 s
  (checked with ffprobe). Its Zone.Identifier has ZoneId=3, HostUrl = the stream and ReferrerUrl = the demo page.
- **Bug found:** the file was saved as `x36xhzz [x36xhzz].mp4` (yt-dlp's own name) instead of "hls.js demo", and the
  title was replaced by yt-dlp's. Every media download ignored the name chosen in the New Download / Add dialogs.
  Fixed: the saved name is the chosen one with the extension yt-dlp produced; yt-dlp's title is only used when the
  download has none. Unit test plus the media download test now checks the chosen name is kept.
- **Correction:** commit `da8a079` said 160 tests passed, but the full suite hadn't been rerun after the last media
  test was added, and `pause_resume_and_cancel_media` failed under full-suite load: the 616 KB clip finished before
  the pause took effect, so there was nothing to resume. The test server now sends that clip more slowly; the full
  suite passed twice (161 tests). Clippy clean.

## M8 — Tray, Windows startup and copied-link offers (2026-10-26)

- **Tray** (`desktop.rs`, Tauri `tray-icon`): left click opens BDM; the menu has Open BDM, Add Download… (opens the Add
  dialog), Resume All, Pause All and Quit BDM. The tooltip updates every 2 s with the number of downloads moving and
  their combined speed (from the engine's progress events).
- **Close to tray** (on by default): closing the main window hides it; download windows close normally. The first
  time each session a notification says BDM is still running. The main window now starts hidden and is shown in
  setup, so "Start with Windows" launches (`--minimized`) stay in the tray; a second launch with that flag doesn't
  pop the window up either.
- **Start with Windows** (off by default): `HKCU\...\CurrentVersion\Run` value "Babarify Download Manager" =
  `"<bdm.exe>" --minimized`, written when switched on and rewritten each launch; the setting isn't saved if the
  registry can't be changed. The uninstaller deletes the value.
- **Copied links** (on by default): a thread polls `GetClipboardSequenceNumber` every 0.7 s and reads the text only
  when it changed. A single http(s) link (no spaces, no credentials, ≤ 2,048 characters) to one of the extension's
  default file types, not already in the list and not just offered, opens a 360×96 always-on-top window above the
  taskbar's corner for 12 s. Download sends it through the same path as a browser download (New Download dialog when
  confirmation is on). The window has its own capability: read, accept or dismiss the offer, and listen for a newer one.
- **Checks:** 164 Rust tests (3 new: which links are offered, tooltip and startup command, speed totals; settings
  defaults and patch cover the new switches), Clippy and TypeScript clean. The offer window and the new Settings →
  Windows section checked in the sample-data preview. Not yet checked in the installed app.

## M8 — Dynamic segmentation (2026-10-26)

- **Engine** (`engine/task.rs`): byte ranges are now slots with a fixed start and an end that can only move earlier.
  Up to N connections (as many as a fresh plan would use, also when resuming) each take an unclaimed unfinished
  range; when none is left, an idle connection takes the back half of the claimed range with the most bytes left,
  keeping 256 KiB (twice the write buffer) ahead of that range's connection and leaving both parts at least the
  minimum segment size (1 MiB; 64 KiB in tests). The connection working on the shrunk range re-reads its end for
  every chunk and stops there; any bytes it already wrote past the new end are the same data the other connection
  writes, and are counted once. Progress saves store the whole layout, and a saved layout is only reused if it
  covers the file exactly once. The download window's connection count is now the number of ranges in progress.
- **Tests:** 169 Rust tests, full suite passed twice. New: unit tests for taking over ranges, the minimum split, not
  counting bytes twice and checking saved layouts; an engine test with eight connections on a 16 MiB file where the
  server throttles the one starting at 4 MiB (alone it needs over 5 s) finishes in under 4 s with a split range and
  identical bytes; and a download paused after splits, restarted with a new engine and resumed, byte-exact. The
  test server now counts bytes actually sent, which resume tests use instead of the Range headers (a split
  connection stops before the end it asked for). Clippy clean. Not yet checked against real servers in the app.

## M8 — Refresh expired links (2026-10-26)

- **Engine** (`Engine::refresh_link`): for an unfinished file download that isn't running, BDM probes the new link
  (network guard, referrer and allowed sign-in cookies as usual) and accepts it only if it's the same file: the same
  size when the size is known, the same strong ETag when both have one, and a server that can resume when there's
  progress. Then the link, final address, size, ETag and Last-Modified are replaced, the error is cleared and a
  failed download becomes paused; segments and the partial file are kept. Video and audio downloads are refused
  with an explanation (yt-dlp fetches fresh addresses on every resume).
- **App:** right-click → **Refresh Download Link…** (file downloads that aren't finished, running or queued) opens a
  dialog to paste the new link, with "Resume right away". The **New Download** dialog shows "Continue It" when a
  browser download has the same name and size as a paused or failed download with progress, so a fresh link from
  the site continues the old download instead of starting a second copy.
- **Checks:** 171 Rust tests (2 new engine tests: a paused 4 MiB download continued from a new address is
  byte-exact, the old address isn't used again and only the missing part comes from the new one; a different size,
  a different ETag and a running download are refused and leave the link unchanged). Clippy and TypeScript clean.
  Both dialogs checked in the sample-data preview. Not yet tried in the installed app.

## M8 — Virus scan after download (2026-10-26)

- **Scanner** (`scanner.rs`): Microsoft Defender's `MpCmdRun.exe -Scan -ScanType 3 -File <file>`, from the active
  platform (`%ProgramData%\Microsoft\Windows Defender\Platform\CurrentVersion`), else the newest platform folder,
  else Program Files; no console window, 10-minute limit. Exit 0 = clean, 2 = threat; anything else (Defender off or
  replaced by another antivirus) = not scanned, logged only.
- **Engine:** when a download (file or video) completes and "Scan finished downloads for viruses" is on (default),
  its `scan_result` becomes `scanning` and the scan runs in the background, so it doesn't hold a download slot; then
  `clean`, `threat` or `unavailable` (migration 0006 adds the column; Download Again clears it). A threat always
  sends a notification ("Threat Found"); Defender quarantines or removes the file as it normally would.
- **App:** the status chip shows Scanning…, a shield on Completed when clean, or a red Threat Found; the download
  window's Download Complete view lists the result; Settings → Security has the switch.
- **Checks:** 175 Rust tests (4 new: Defender exit codes and platform choice; an engine test with a stand-in
  scanner script where one finished download is clean and one is a threat, only the threat is reported and the list
  showed "scanning" first; no scan when the setting is off). The real Defender on this PC scanned a downloaded MP4:
  exit code 0, "found no threats", 117 ms. Clippy and TypeScript clean; the list checked in the sample-data
  preview. A real threat wasn't tried (no test malware was written to this PC).

## M8 — Subtitles, chapters and cover art for video downloads (2026-10-26)

- **Settings → Video Downloads:** Add chapters (on), Add the thumbnail as cover art (on), Add subtitles (off) with
  languages (default "en") and "Use automatic captions too" (on). They apply to every video and audio download,
  including ones from the browser panel; subtitles only to video.
- **yt-dlp flags** (`media/args.rs`): `--embed-chapters`, `--embed-thumbnail`, and `--write-subs [--write-auto-subs]
  --sub-langs <codes> --embed-subs`. Languages are exact codes (checked: letters, digits, `-_.*`, at most 10); yt-dlp
  matches patterns ignoring case, so a pattern like `en.*` (or `en-[A-Z]+`) also requested 10 machine-translated
  tracks (en-bn, en-de, …) on a real YouTube video.
- **Subtitle refusals:** on that video YouTube answered HTTP 429 for one of those tracks and yt-dlp failed the whole
  download. If a run with subtitles fails with an ERROR line about subtitles, BDM runs it once more without them.
- **Real check** (YouTube, *Sprite Fright*, Blender, CC-BY, 144p): with exact codes yt-dlp requested only `en`;
  exit 0, 14.9 MB MP4 with H.264, AAC, an English `mov_text` subtitle track, the thumbnail as attached picture and
  7 chapters (ffprobe).
- **Checks:** 178 Rust tests (new: extras flags, language parsing, recognising subtitle failures, settings
  validation; the existing real-tool media tests now run with chapters and cover art on). Clippy and TypeScript
  clean; the settings card checked in the sample-data preview.

## M8 — Command line and bdm:// links (2026-10-26)

- **Parser** (`cli.rs`): `--add <url>`, bare http(s) links, `--name`, `--folder` and `--import` (relative to the
  current folder), `--queue <name>`, `--start`/`--later`, `--media <url>`, `--import <file>`; `--minimized` (Start
  with Windows) is ignored here. Problems are collected without dropping the rest; `--name` is ignored for several
  links. `bdm://add?url=&name=` and `bdm://media?url=` (also `bdm:add?…`); any other parameter (folder, start…) is
  ignored and other actions are refused.
- **Trust:** only command-line links with `--start`/`--later` are added without the dialog (a program already
  running on this PC). Everything else, and every `bdm://` link (any web page can open one), goes through the New
  Download dialog or the video dialog; problems show a notification.
- **Delivery:** a second launch hands its arguments to the running BDM through the single-instance plugin. At first
  launch the requests wait in a queue the main window collects when it has loaded (`take_startup_requests`), so
  nothing is lost while the window starts; later requests are sent as events. `--import` opens the Batch dialog
  with the file's links on the List of Links tab.
- **Registration:** installed (release) builds write `HKCU\Software\Classes\bdm` (URL protocol, icon,
  `"bdm.exe" "%1"`) on every launch; the uninstaller deletes it. `add_download` and `import_url_list` were split so
  the command line uses the same validation as the dialogs.
- **Checks:** 183 Rust tests (5 new: options and relative paths, bare links with media and import, problems kept
  apart, `bdm://` links can't choose a folder or start, the registered command). Clippy and TypeScript clean. In
  the sample-data preview, queued startup requests opened the New Download dialog and the Batch dialog filled with
  three links. Not run against the installed app (a test build can't run beside your BDM: one copy at a time).

## M8 — Disk-space guard (2026-10-26)

- **Free space** (`engine/disk.rs`): `GetDiskFreeSpaceExW` for the drive holding the folder (its nearest existing
  parent if the folder doesn't exist yet). BDM always leaves 256 MB free; unknown free space never blocks.
- **Before a file starts:** once the size is known, the bytes still to be written (size minus what's already on disk;
  a resumed segmented file already holds its full size) must fit with the reserve, else the download is paused with
  "Not enough space on E:: this download needs 1.2 GB more and 300 MB is free. Free up space, then resume it."
- **While running:** single-stream and video downloads (their size is unknown or an estimate) are checked every 2 s;
  below the reserve they're paused ("Paused: the drive E: is almost full (…)"), progress kept. Segmented files have
  reserved their full size at the start, so they aren't paused.
- **Disk full anyway:** Windows' disk-full errors (ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL) and yt-dlp/FFmpeg's "no
  space left"/"not enough space on the disk" pause the download with the reason instead of failing it and
  scheduling automatic retries.
- **New Download dialog:** checks the chosen folder's drive (again when the folder changes) and warns when the file
  won't fit.
- **Checks:** 188 Rust tests (5 new: shortfall message and reserve, a real free-space reading of this PC including a
  folder that doesn't exist yet, disk-full error codes; an engine test where a 4 MB file with 3 MB free pauses with
  the sizes before anything but the size probe is fetched; and a single-stream download of unknown size paused when
  free space drops, then completed byte-exact after resuming). Tests use a stand-in free-space reading so they don't
  depend on this PC's drive. Clippy and TypeScript clean; the dialog warning checked in the sample-data preview.

### M8 checked in the installed app (2026-10-26)

With the test installer installed from Explorer (BDM process started 7:33 PM), using computer use and your installed
`bdm.exe`:
- **Tray:** BDM's icon is in the notification area; its tooltip reads "Babarify Download Manager" (nothing was
  downloading); a left click brought the window forward. The title bar is navy.
- **Close to tray:** the window's ✕ hid BDM; its taskbar button went away, the tray icon stayed, and the same process
  (pid 6244) kept running. The one-time notice wasn't seen: Windows' Do Not Disturb was on.
- **Copied link:** with an Ubuntu ISO link on the clipboard, "Download with BDM?" (ubuntu-24.04.1-desktop-a…,
  releases.ubuntu.com) appeared above the taskbar's corner within 2 s and had closed by itself 20 s later. Nothing
  was downloaded.
- **Command line:** `bdm.exe --add <iso link>` reached the running BDM (no second copy) and opened New Download
  (Compressed, Main queue); `bdm.exe --import links.txt` (two links, on E:) opened the Batch dialog's List of Links
  with both links.
- **bdm://:** `bdm://add?url=…&name=Big%20Buck%20Bunny%201080p.mp4&folder=C%3A%5CWindows` opened through Windows
  showed New Download with that name, saving to Downloads\Videos: the folder in the link was ignored. Every dialog
  was cancelled.
- **Not checked:** the tray's right-click menu (computer use may not right-click the taskbar), starting with Windows
  after a sign-in, and `--start`/`--later` (they share the Add dialog's code; not run to keep your list unchanged).
- **Tray menu** (confirmed by you): right-clicking the tray icon shows Open BDM, Add Download…, Resume All, Pause All and
  Quit BDM.

## Final check (2026-10-26)

**Automated:** 189 Rust tests, 14 extension tests, TypeScript, the production frontend build and Clippy pass.
`npm audit`: 0 vulnerabilities. `cargo audit`: 0 vulnerabilities; 5 warnings (unmaintained `proc-macro-error`,
`zbus_names`, `zvariant`, `zvariant_derive`; unsound `glib` iterator), all in Tauri's Linux-only dependencies and not
built for Windows. The code has never been run through `rustfmt` (no config); left as is.

**In the installed app** (computer use):
- Add Download: typing a link filled in the file name; Download started it; the list showed size, speed, time left
  and filed `.bin` under a new "Other" category.
- Download window: progress, transfer rate, time left, the 8-range connections table and map; Pause kept 14.8 MB and
  Resume continued from there (16.2 MB next); the Speed Limit tab showed the current speed, switch and presets.
- **Found:** Hetzner's test server (`fsn1-speed.hetzner.com/100MB.bin`) allows only two downloads per visitor and
  answered the others with HTTP 429. Those connections retried, ran out of retries, and failed the whole download at
  21% although two connections were working. **Fixed:** a connection refused with 429/503 while others still work
  now hands its range back and stops; the others take it over (dynamic segmentation). The last connection never
  leaves this way. Test: a server limited to two connections, eight planned; the 4 MiB download completes
  byte-exact after the extra connections were refused.
- **Found:** a paused or failed download's window showed a made-up "single connection" row. **Fixed:** the window
  keeps the ranges it last saw, and the fallback row only says "single connection" while downloading.
- Not yet: browser capture, the right-click menu and the video panel (Claude in Chrome disconnected), and the Hetzner
  file again with the fix installed.

**Store listing guide:** STORE_LISTING.md (Chrome Web Store and Edge Add-ons: accounts, ZIP, images, privacy
answers, permission justifications, and the store extension IDs BDM must allow before publishing).

## LED speed meters on each download (2026-10-26)

- Each row in the list has 12 LED bars in front of its speed, lit from Babarify blue to green in proportion to the
  speed; a download that isn't running shows unlit bars and "—".
- The download window's Progress tab opens with a navy panel: the speed in green digits (KB/s below 1 MB/s), 24 bars,
  the peak bar marked in slate, and a scale that rounds up to 1, 2, 5, 10, 20… MB/s from the peak.
- The scale follows each download's own peak, so a slow download still fills its bars.
- Screen readers get each meter as a "meter" with its value; bars don't animate with reduced motion.
- Checked: TypeScript, the frontend preview at 1180 px and 900 px (two downloads running, 7.9 MB/s and 715 KB/s), and
  the download window at 500 px. In the installed app (2026-10-26): grey bars on stopped rows and "Not downloading" in
  the window, as designed.
- The download window opens 80 px taller (500 × 500) so the meter doesn't push the details and connections down.

### Found: YouTube video failed with "[Errno 22] Invalid argument" (2026-10-26)

- The title "Who Will Make PTI’s Decisions?" has a curly apostrophe. yt-dlp wrote it in the Windows code page
  (byte 0x92), which isn't UTF-8; BDM's line reader stopped at that line and closed the pipe, and yt-dlp's next
  write then failed on Windows with Errno 22 before any bytes were downloaded.
- Not reproducible with output to a file or `| cat`, which keep reading; yt-dlp's `--test` mode with BDM's options
  (formats, chapters, cover art, cookies file, browser headers) all worked.
- **Fixed:** yt-dlp is asked for UTF-8 output (`--encoding utf-8`), so titles and paths with ’, — or Urdu arrive
  intact, and BDM reads every line lossily instead of stopping at one it can't decode.
- Test: a line with a 0x92 byte followed by a progress line; both arrive. 190 Rust tests pass.

## Website screenshots (2026-10-26)

- 19 screenshots at 2x in `website-assets/` with captions (README.md there): the list (light and dark), download
  window and its tabs, Add Download, video downloader, browser confirm, batch, queue schedule, browser integration,
  settings (Windows, Security, Video Downloads, Speed and When Done), copied-link offer, the extension popup and
  options, and the on-video panel. Sample data only.
- **Found:** the extension options showed "No sites yet." under a listed sign-in site, because `small { display:
  block }` beat the `hidden` attribute. **Fixed** with a `[hidden]` rule in options.css. Extension tests: 14 pass.

## Compact "Download Complete" window (2026-10-27)

- The summary was centred in the 500 x 500 download window, leaving most of it empty. Now the icon and title share a
  line with the name below, the details follow (path in the body font, not large monospace) and the buttons sit
  at the right.
- The window shrinks around the summary through a new BDM command, `fit_download_window`: it only resizes the
  download window that calls it, keeps its width, and keeps the height between 200 and 500 px. The download
  window's minimum height went from 320 to 200 px so it can shrink that far.
- Preview with a long file name and a virus-scan result: about 200 px tall (was about 650 at the user's scale).
  TypeScript and Clippy clean. Not yet seen in the installed app.

## Open tabs keep working after an extension update (2026-10-27)

- **Problem:** after the extension was updated or reloaded, the "Download With BDM" panel already in open tabs
  (e.g. YouTube) was cut off from the extension and answered "The BDM extension was updated. Reload this page."
  Registered content scripts only reach pages loaded later.
- **Change:** after an update or reload (`onInstalled` reason "update"), and when the panel is switched on, the
  background injects the panel into every open http(s) tab. The page is **not** reloaded, so videos keep playing and
  nothing typed into other pages is lost. The old copy checks `chrome.runtime.id` every second and removes itself
  once disconnected; a new copy takes over from a disconnected one but never runs beside a live one.
- **Tests (real Chrome 155, throwaway profile):**
  - Reload with a video playing: same page (timeOrigin unchanged), video still playing, one panel, and it reached
    the reloaded extension. The same test on the code before this change: "The BDM extension was updated. Reload
    this page." (fail), so the test catches the problem.
  - Switching the panel on with a page already open: the panel appears there without a reload.
  - Extension unit tests: 14 pass.
- The extension already in your Chrome needs one last manual reload to get this; later updates refresh tabs by
  themselves. BDM app updates don't need it: the extension talks to BDM fresh for each request.

## Faster video downloads and format lists (2026-10-27)

Measured on this PC with the Sprite Fright video on YouTube (yt-dlp 2026.10.21 single-file and 2026.10.26 folder build):

| Step | Before | Now |
| --- | --- | --- |
| yt-dlp startup | 1.7 s (single-file exe unpacks itself every run) | 0.7 s (folder build) |
| Format list (panel or Video Downloader) | about 8 s | about 5 s |
| Click a quality → first video bytes | about 8–9 s (the site was asked again) | about 1 s (starts from the format check) |

- **Download starts from the format check.** The JSON from the format list is kept in memory for 20 minutes
  (16 videos at most) and used once: the download runs `yt-dlp --load-info-json` instead of asking the site again.
  If that run fails (e.g. the stream addresses expired), BDM retries at once the normal way. The JSON can hold
  sign-in cookies, so it only goes to disk as a temporary file deleted when yt-dlp exits. Playlists, live streams and
  DRM pages aren't kept.
- **yt-dlp folder build.** BDM now installs `yt-dlp_win.zip`, one folder per version
  (`tools/yt-dlp/<release>-<sha>/`), so an update never has to replace a running copy; older versions and the
  earlier single-file `yt-dlp.exe` are removed when not in use. Unpacking uses each entry's enclosed path and caps
  the size, so an archive can't write outside its folder. Existing installs get it with the next yt-dlp update
  (automatic within a day, or Settings → Media tools → Update).
- Checked for real: yt-dlp took the saved check and picked the same formats; a `--test` download from it worked
  (no 403), first bytes at 2.15 s with the single-file exe. The folder build's SHA-256 matched yt-dlp's
  SHA2-256SUMS.
- Tests: InfoCache (used once, oldest dropped, temp file deleted), folder install and update, the move from the
  single-file build, and a zip with `../` entries refused. 194 Rust tests pass; Clippy clean.
- Remaining time on YouTube is the site itself: page, player script and its JS challenge (about 4 s), which every
  downloader has to do once per video.
