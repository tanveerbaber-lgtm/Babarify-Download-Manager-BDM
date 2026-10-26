# Architecture

## Overview

BDM is a Tauri 2 desktop application. The **Rust core** owns all state, storage and networking. The **React webview** is a presentation layer that talks to the core only through a small set of typed commands.

```
┌──────────────────────── Webview (React + TS) ────────────────────────┐
│  App ─ Sidebar / Toolbar / DashboardStats / DownloadTable / Dialogs  │
│  hooks: useDownloads, useSettings          lib/api.ts (typed invoke) │
└───────────────────────────────┬──────────────────────────────────────┘
                                │ Tauri IPC (capability-gated)
┌───────────────────────────────▼──────────────────────────────────────┐
│  commands.rs  ── validates every input ──▶  validation.rs            │
│        │                                                             │
│        ├──▶ engine/  scheduler · task · http · files · events ──▶ UI │
│        ▼                (reqwest, tokio)          emits progress     │
│  db/ (rusqlite)  downloads.rs · settings.rs · migrations/*.sql       │
│        │                                                             │
│        ▼                                                             │
│  %APPDATA%\com.babarify.bdm\bdm.sqlite3                              │
└──────────────────────────────────────────────────────────────────────┘
```

## Key decisions

| Decision | Why |
| --- | --- |
| Database owned by Rust (`rusqlite`, bundled SQLite) instead of `tauri-plugin-sql` | The SQL plugin lets the webview run arbitrary SQL. Keeping SQL in Rust means the webview can only perform the operations commands expose. Bundled SQLite avoids depending on a system DLL. |
| Migrations tracked with `PRAGMA user_version` | No extra table; each migration and its version bump commit in one transaction. A database newer than the build is refused instead of silently misread. |
| Settings stored as JSON values per key | Adding a setting needs no migration; unknown or corrupt values fall back to defaults instead of blocking startup. |
| Commands declared in `build.rs` app manifest | Tauri then generates `allow-<command>` permissions and denies any command not granted in `capabilities/main.json`. |
| Typed errors (`AppError`, serialized as `{kind, message}`) | The UI can distinguish validation errors (show inline) from internal ones. |
| Plain CSS with design tokens | No CSS framework dependency; light/dark themes switch by a single `data-theme` attribute. |
| Single `Mutex<Connection>` behind `Arc<Database>` | Shared by commands and the engine. The engine writes progress at most every 2 s per download, so contention is negligible. |
| reqwest with native TLS (SChannel), no content decoding | Uses the Windows certificate store and needs no extra build tools. `Accept-Encoding: identity` keeps byte offsets referring to the real file, which Range resume depends on. |
| Engine behind an `EventSink` trait | The Tauri app forwards events to the webview; tests record them. The engine can be tested without a window. |

## Data model (schema v4)

`downloads`

| Column | Type | Notes |
| --- | --- | --- |
| id | TEXT PK | UUID v4 |
| url | TEXT | Validated http/https, no embedded credentials |
| file_name | TEXT | Sanitized for Windows |
| destination_dir | TEXT | Canonical absolute path to an existing folder |
| status | TEXT | `pending, queued, downloading, paused, completed, failed, cancelled` (CHECK constraint) |
| total_bytes | INTEGER NULL | Unknown until the server reports it |
| downloaded_bytes | INTEGER | ≥ 0 |
| error_message | TEXT NULL | |
| created_at / updated_at / completed_at | INTEGER | Unix epoch ms |
| final_url | TEXT NULL | URL after redirects (v2) |
| temp_path / saved_path | TEXT NULL | Partial file while incomplete; final file once completed (v2) |
| etag / last_modified | TEXT NULL | Remote identity checked before resuming (v2) |
| resumable | INTEGER | Server honours Range requests (v2) |
| expected_sha256 | TEXT NULL | Optional checksum, lowercase hex (v2) |

| kind | TEXT | `file` or `media` (v3) |
| title | TEXT NULL | Title reported by the site (v3) |
| media_options | TEXT NULL | JSON `MediaOptions`: video (max height, MP4/MKV), audio (MP3/M4A/Opus/original) or an exact format (v3) |
| referrer | TEXT NULL | Page that linked to the file; sent as `Referer` (v4) |

`download_segments` (v2) — `download_id → downloads ON DELETE CASCADE, idx, start_byte, end_byte (inclusive), downloaded`

`settings` — `key TEXT PK, value TEXT (JSON), updated_at INTEGER`

Keys: `theme`, `default_download_dir`, `max_concurrent_downloads` (1–16), `segments_per_download` (1–16), `confirm_before_remove`, `auto_update_yt_dlp`, `show_on_browser_add`.

### Adding a migration

1. Create `src-tauri/src/db/migrations/000N_description.sql`.
2. Append it to `MIGRATIONS` in `src-tauri/src/db/mod.rs`.
3. Never edit a migration that has shipped.

## IPC surface

| Command | Input | Output |
| --- | --- | --- |
| `get_app_info` | — | `AppInfo` |
| `list_downloads` | — | `Download[]` |
| `add_download` | `{ url, fileName?, destinationDir?, expectedSha256?, start? }` | `Download` (queued, or pending if `start: false`) |
| `resume_download` | `id` | — (starts pending; resumes paused/failed/cancelled) |
| `pause_download` | `id` | — |
| `cancel_download` | `id` | — (deletes the partial file) |
| `remove_download` | `id` (UUID) | — (deletes partial files; never finished files) |
| `get_media_tools` | — | `ToolStatus[]` (yt-dlp, FFmpeg, Deno: source, path, version) |
| `install_media_tools` | `kinds?` | `ToolStatus[]` (installs/updates BDM's copies) |
| `check_media_tool_updates` | — | `ToolKind[]` with newer releases |
| `analyze_media` | `url` | `MediaInfo` (qualities, audio, formats, playlist entries, license, DRM) |
| `add_media_downloads` | `{ items[], options, destinationDir?, start? }` | `Download[]` |
| `get_browser_integration` | — | `BrowserStatus[]` (Chrome, Edge) |
| `register_browser_integration` / `unregister_browser_integration` | — | `BrowserStatus[]` |

Browser messages (extension → host → app): `ping`, `download`, `media` (open the Video dialog), `formats` (choices for the on-video panel), `mediaDownload` (start a chosen format).
| `suggest_file_name` | `url` | `string` |
| `get_settings` | — | `Settings` |
| `update_settings` | `SettingsPatch` | `Settings` |

TypeScript mirrors: `src/lib/types.ts`; wrappers: `src/lib/api.ts`.

Events (engine → webview): `download:progress` (`{id, downloadedBytes, totalBytes, speed, etaSeconds, connections}`, every 500 ms while running), `download:updated` (full `Download` on every status change), `download:removed` (`id`), `tools:progress` (`{tool, downloadedBytes, totalBytes, stage}`), `browser:added` (`Download` sent from the browser), `browser:media` (`{url, pageTitle}`: open the Video / audio dialog). Media progress events also carry `stage` (e.g. "Merging video and audio").

## Download engine (M2)

**Scheduler** (`engine/mod.rs`): the database is the queue. Queued downloads start in priority order (high → low), then queue position, and only from queues that are running; up to `max_concurrent_downloads` run at once across all queues. Each active download has a cancellation token, a stop reason (pause / cancel / remove / requeue) and its own rate limiter. A ticker (every 15 s, `engine/schedule.rs`) applies schedule starts and stops between ticks in local time (overnight windows belong to their start day), requeues the running downloads of a queue that stops, stops scheduled queues that run dry, and fires due automatic retries. On launch, `downloading` rows become `paused` ("Interrupted…"), queued rows start again, and a queue whose window is open starts.

**Speed limits** (`engine/limiter.rs`): token buckets with at most 0.5 s of burst; every chunk waits on the global bucket and the download's own, and changes apply on the next chunk. yt-dlp gets the tighter of the two as `-r`.

**Retries**: within one attempt, transient errors retry up to `max_retries` times with backoff. A failed download whose error might be temporary (network, 5xx/429, changed file, yt-dlp failure) gets `auto_retry_count` automatic retries after `auto_retry_delay_minutes`; broken links, sign-in pages and checksum failures never do.

**Completion** (`completion.rs`): the engine reports completed / failed / all-done through `EventSink`; the app shows Windows notifications and runs the "when all done" action (sleep via `SetSuspendState`, `shutdown /s /t 60`, or exit) once, then resets it to "do nothing".

**Queues and categories** (migration 0005): `queues(id, name, schedule JSON, running, sort_order)` with a permanent `main` row; `categories(id, name, extensions, folder)`; downloads gained `queue_id, priority, position, category, speed_limit, auto_retries, retry_at`. New downloads without a chosen folder go to `<default folder>\<Category>` when sorting is on.

**Per download** (`engine/task.rs`):

1. **Probe** with `GET Range: bytes=0-0`. A `206` gives the total size and confirms Range support in one round trip; a `200` means no ranges. Redirects are followed (at most 10, http/https only) and the final URL is recorded.
2. **Identity check.** If progress exists, ETag, Last-Modified and size must all match what was stored, otherwise the partial file is discarded and the download starts over.
3. **Transfer.**
   - *Segmented* (ranges and a known size): the temp file is pre-sized and split into up to N segments (at least 1 MiB each). Each segment is fetched with `Range` plus `If-Range`. A `200` reply or a mismatched `Content-Range` means the file changed, which triggers one automatic restart. **Dynamic segmentation:** a connection that finishes takes an unclaimed range, or else the back half of the range with the most left (keeping a 256 KiB gap ahead of the connection working on it, and both parts at least the minimum size); that range's end moves earlier and its connection, which re-reads the end for every chunk, stops there. The whole layout is saved with progress, so pauses and crashes resume split ranges correctly.
   - *Single stream* (otherwise): resumes with `Range: bytes=n-` when possible, else restarts from zero.
4. **Retries**: connection errors, timeouts (20 s connect, 30 s read silence), mid-body drops, 408/425/429/5xx. Backoff is exponential from 1 s up to 30 s, or follows `Retry-After`; up to 5 attempts, and the count resets whenever bytes arrive. 401/403/404/410 fail immediately with a clear message. URLs are never included in messages.
5. **Progress accounting**: bytes count only after they've been written and flushed to the OS (128 KB buffer, or at least every 200 ms). Progress is saved every 2 s and on every stop, so saved progress never runs ahead of the file contents.
6. **Finalize**: check the size, verify the optional SHA-256 (a mismatch deletes the data), then reserve a free name with `create_new` (`name (1).ext`, …) and atomically `rename` the temp file over that placeholder. Existing files are never overwritten.

**Temp files** are `<name>.<id8>.bdmpart`, kept in the destination folder so the final rename stays on one volume.

## Video and audio engine (M3)

**Tools** (`media/tools.rs`): yt-dlp, FFmpeg and Deno (the JavaScript runtime yt-dlp needs for YouTube). BDM looks for its own copy in `%APPDATA%\com.babarify.bdm\tools\<tool>\` first, then on PATH. *Install / update* reads each project's latest GitHub release (`yt-dlp/yt-dlp`, `BtbN/FFmpeg-Builds` newest static GPL win64, `denoland/deno`), downloads the asset and verifies its SHA-256 against the digest GitHub publishes. Assets without a digest are refused. Only the needed executables are extracted (matched by file name, never by archive path). A running exe is renamed aside before replacement and cleaned up at next launch. `manifest.json` records release, asset and hash; updates compare the hash. With *Keep yt-dlp up to date* on, BDM's own yt-dlp is checked at most once a day at launch. Nothing is bundled with the app.

**Analysis** (`media/analyze.rs`): `yt-dlp -J --flat-playlist --no-playlist` with `--ignore-config`. Storyboards and DRM formats are dropped; if every format is DRM-protected the result is flagged and the UI blocks downloading. Size estimates mirror the sort order used for the actual download.

**Download** (`engine/media_task.rs`): media downloads share the queue, concurrency limit and pause/resume/cancel with file downloads. yt-dlp runs with no console window, arguments passed as a list (never a shell) and the URL after `--`:

- `-P <dest>\.bdm-media-<id8>` (a hidden temp folder) and `-o "%(title).150B [%(id)s].%(ext)s"`.
- MP4: `-f "bv*[height<=H]+ba/b[height<=H]" -S "res:H,vcodec:h264,channels:2,acodec:aac" --merge-output-format mp4`. MKV: `-S res:H` (best quality). Audio: `-x [--audio-format …]`.
- Machine-readable `--progress-template` / `--print` lines (stdout and stderr) drive progress, stream count ("stream 1 of 2") and stage labels.
- On success the produced file must be inside the temp folder; it's moved into the destination with the same never-overwrite finalization as file downloads, and the temp folder is deleted. Pause kills the process tree (`taskkill /T`) and keeps the folder so yt-dlp continues its `.part` files; cancel and remove delete it.
- Errors are mapped to plain messages (DRM, sign-in, private, unavailable, unsupported site, 403 → "update yt-dlp", 429), with URLs stripped.

## Chrome and Edge integration (M4)

```
Extension (MV3) ──sendNativeMessage──▶ bdm-native-host.exe ──named pipe + token──▶ BDM app
  downloads.onCreated                    checks caller origin            \\.\pipe\com.babarify.bdm.<user>
  context menus, popup                   strict JSON, 64 KB cap           validates again, adds/starts,
  optional media detection               starts BDM if not running        or opens the Video dialog
```

- **Extension** (`extension/`): fixed ID through the `key` in `manifest.json` (private key kept in the git-ignored `keys/`). Required permissions are only `downloads`, `contextMenus`, `nativeMessaging`, `storage` and `activeTab`; media detection asks for `webRequest` plus http/https host access as optional permissions when the user turns it on. Capture runs on `downloads.onCreated`: if the type is selected and BDM is reachable, the browser download is cancelled and erased and the URL, file name, referrer, MIME type and size are sent. Cookies are never sent. If BDM refuses, the browser downloads the file after all (marked exempt so it isn't captured again). Private-window downloads are never captured. `npm run ext:pack` builds the same zip for the Chrome Web Store and Edge Add-ons.
- **Native host** (`src-tauri/src/bin/bdm-native-host.rs`, `browser/host.rs`): only accepts the origins in `EXTENSION_IDS`; parses each message strictly (unknown types and fields are rejected) and validates URLs; never acts itself. If the app isn't listening it starts `bdm.exe` (path recorded by the app in `%APPDATA%\com.babarify.bdm\app-path`) detached and outside the browser's job object, then retries for up to 25 s. Pings never start BDM.
- **IPC** (`browser/ipc.rs`): per-user named pipe created with `first_pipe_instance` (no squatting) and `reject_remote_clients`; every request carries a fresh random token the app writes to the user's AppData at startup. Comparison is constant time; requests are capped at 64 KB and must arrive within 5 s.
- **Registration** (`browser/registry.rs`): HKCU keys for Chrome and Edge point at a manifest whose `allowed_origins` lists only BDM's extension ID(s). It needs no admin rights; the Browser integration page connects and disconnects it, and the installer will do the same in M7.
- **On-video panel** (`extension/src/video-panel.js`): off by default. Turning it on in Options requests the optional `scripting` permission and http/https site access, then the service worker registers the panel as a dynamic content script (all frames). The panel lives in a closed shadow root and picks the largest visible video. Clicking it sends `formats` (BDM runs the analysis and returns a short `MediaChoices` list: MP4 per quality with sizes, MP3, M4A, plus title and license, or a reason such as DRM or playlist); choosing an item sends `mediaDownload` with those options, and BDM queues and starts it. The service worker forwards only these two message types from pages, with an http(s) URL. The pipe handler is async because analysis runs yt-dlp.
- **App**: `tauri-plugin-single-instance` makes a second launch focus the running window. Browser downloads start immediately (with a toast and, optionally, the window brought to the front); media pages open the Video / audio dialog, which analyzes the URL right away.

## Branding

Brand assets live in `assets/brand/` (SVG mark, app icon) and come from `assets/Babarify Download Manager Brand Guide.png`. Colour tokens are in `src/styles/tokens.css` (`--brand-blue` #2563EB, `--brand-green` #10B981, `--brand-navy` #0F172A, `--brand-gray` #64748B, `--brand-light-gray` #E2E8F0). Text uses Arial (body) and Arial Bold (headings and titles), which ship with Windows, so no font is bundled. Regenerate icons with `npx tauri icon ../assets/brand/bdm-app-icon.svg -o icons` (in `src-tauri`) and `-p 16,32,48,128` for the extension.

**Testing**: `src-tauri/testsupport/server.rs` is an axum server with switchable Range support, ETags, throttling, dropped connections, stalls, redirects and error statuses. It's used by `engine/tests.rs` and by `cargo run --example test_server` for manual checks.

## Security model

- **Capabilities** (`src-tauri/capabilities/main.json`): `core:default`, `dialog:allow-open` (folder picker), and BDM's ten commands. No `fs`, `shell`, `http` or `opener` permissions are granted to the webview; all network access happens in Rust.
- **CSP**: `script-src 'self'`, no inline scripts, `object-src 'none'`, `form-action 'none'`, IPC-only `connect-src`. The theme boot script is a separate file for this reason.
- **Validation in Rust** for every command, independent of frontend checks:
  - URLs: http/https only, must have a host, ≤ 8 KB, embedded `user:pass@` rejected so credentials never reach the database.
  - Filenames: Windows-reserved characters and control characters replaced, trailing dots/spaces removed, reserved device names (`CON`, `NUL`, `COM1`…) prefixed, `.`/`..` rejected, length capped at 200 while keeping the extension, separators rejected in user-supplied names.
  - Destinations: absolute, existing directory, canonicalized.
- `freezePrototype` is enabled to harden the webview's JS prototypes.

## Frontend structure

- `App.tsx` — layout, view state, selection, keyboard shortcuts (`Ctrl+N` add, `Delete` remove, `Ctrl+A` select all, `Esc` clear).
- `hooks/useDownloads.ts` — list/add/remove with optimistic local updates from command results.
- `hooks/useSettings.ts` — load/update settings; applies the theme and follows Windows when set to System.
- `lib/views.ts` — sidebar view definitions and counts.

## Roadmap hooks for later milestones


## Security model (M6)

- **Webview:** treated as untrusted. It may only listen to BDM's events, open the file/folder picker and call BDM's
  own commands, each of which validates its input in Rust. No filesystem, shell, HTTP, window, opener or notification
  APIs; CSP limits scripts and connections to the app; prototypes are frozen; DevTools are off in release builds.
- **Network guard** (`net_guard.rs`): a DNS resolver plugged into the HTTP client drops addresses on this PC, the LAN
  (unless "Allow local network downloads" is on), cloud metadata, link-local, multicast and reserved ranges, on every
  connection including redirects. Literal-IP URLs are checked before requests and on each redirect; links are resolved
  and checked before yt-dlp runs. Proxy host names configured in Windows or the environment are let through.
- **Redirects:** at most 10, http/https only, never HTTPS → HTTP.
- **Referer:** the browser default (strict-origin-when-cross-origin): full page address to the same origin only.
- **Files:** names cleaned of reserved characters, device names and invisible formatting characters; finished files
  never overwrite; only `.bdmpart` files and `.bdm-media-*` folders are ever deleted.
- **Subprocesses:** yt-dlp gets an argument list (no shell), `--ignore-config --no-plugin-dirs`, the URL after `--`.
- **Mark of the Web:** finished files get a `Zone.Identifier` stream (zone 3, origin and path only) so Windows
  SmartScreen and Office treat them as downloaded from the internet.
- **Secrets:** URLs with credentials are refused; error messages never include URLs; there are no log files.
