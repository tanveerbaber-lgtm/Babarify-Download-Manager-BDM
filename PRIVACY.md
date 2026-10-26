# Privacy Policy

**Babarify Download Manager (BDM) and its browser extension** · Effective 2026-10-25 · Version 1.0.0

BDM is a desktop program. It has **no accounts, no telemetry, no analytics and no advertising**, and Babarify runs no
servers that BDM talks to. Everything described below stays on your PC unless you download something.

## What BDM stores, and where

All of it lives in `%APPDATA%\com.babarify.bdm` on your PC:

| Data | Why |
| --- | --- |
| Your download list: links, file names, folders, sizes, status, dates, the page a download came from | To show, resume, retry and organise downloads, and to warn about duplicates |
| Settings, queues, schedules and categories | To remember your choices |
| yt-dlp, FFmpeg and Deno, if you install them | To download video and audio |
| A short-lived token and the app's location | So the browser helper can reach the running app |

Downloaded files are saved where you choose. BDM never uploads them anywhere.

## Network connections BDM makes

- **The servers you download from**, when you add or resume a download. They see what any download would show: your
  IP address, BDM's user agent and, for links sent by the browser, the referring site (only its origin when the
  download is on another site).
- **GitHub** (`api.github.com`, `github.com`), to install or update yt-dlp, FFmpeg and Deno, and to check for a
  newer yt-dlp when BDM starts (you can turn that off in Settings → Media tools).
- **Microsoft**, during installation only, if WebView2 needs to be installed.

BDM doesn't send your download list, settings or any identifier to Babarify or anyone else.

## The browser extension

- When **Capture downloads** is on, it passes the link, file name, size and referring page of a download you start
  to BDM on your PC (through Chrome's / Edge's native messaging). It doesn't read pages, passwords or browsing
  history, and it reads no cookies unless you turn on sign-in downloads for a site (below).
- **Sign-in downloads** are off until you add a site in the extension's options and grant cookie access for that
  site alone. Then, for requests to that site only (never from private windows), the extension sends BDM the
  cookies the browser would send to it, so downloads work while you're signed in. BDM keeps them in memory only,
  never saves them to disk except as a temporary file handed to yt-dlp and deleted when it finishes, forgets them
  after 12 hours or when it closes, and sends each cookie only to the site it belongs to. Removing the site in the
  options stops this and withdraws the permission.
- **Virus scan (BDM app):** with "Scan finished downloads for viruses" on (Settings → Security), each finished
  file is checked by Microsoft Defender on your PC (`MpCmdRun.exe`). BDM sends nothing anywhere for this; whether
  Defender itself shares samples with Microsoft is set in Windows Security.
- **Copied links (BDM app):** with "Offer to download copied links" on (Settings → Windows), BDM checks the
  clipboard when it changes. Only a single link to a file type it downloads is offered; nothing is stored, logged or
  sent anywhere, and the link goes to BDM's download list only if you click Download.
- **Video detection** also notes, for each video a page loads, the request headers the browser sent that describe
  the request (User-Agent, Referer, Origin, Accept, Accept-Language and the page's own `X-` headers; never cookies
  or Authorization). They stay in the browser's memory for that tab and go to BDM only with that video when you
  choose it; BDM keeps them in memory for up to 12 hours and passes them to yt-dlp for that video.
- **Video detection** and the **on-video panel** are off until you turn them on and grant site access. Then the
  extension looks for video on the pages you visit and, only when you click it, sends that page's address to BDM to
  list formats.
- Extension settings are kept in your browser's sync storage. The extension has no network access of its own.

## Your control

- Remove downloads from the list, or clear finished ones, at any time; files on disk stay.
- Turn off capture, video detection, the panel or yt-dlp updates in their settings.
- Uninstalling BDM removes the program and the browser registration. The uninstaller can also delete
  `%APPDATA%\com.babarify.bdm` (your list and settings). Your downloaded files are never deleted.

## Children

BDM isn't directed at children and doesn't knowingly handle children's personal data.

## Changes and contact

Changes to this policy ship with new versions of BDM and are noted in the release notes. Questions: open an issue at
https://github.com/tanveerbaber-lgtm/Babarify-Download-Manager-BDM.
