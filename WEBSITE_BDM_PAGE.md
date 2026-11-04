# BDM page for babarify.com — brief for the website chat

Paste everything below the line into the chat that built babarify.com. Items marked **[decide]** need your answer
first.

---

Add a product page for **Babarify Download Manager (BDM)** at `/bdm` (link it from the main navigation and the
home page's products section), plus a privacy page at `/bdm/privacy`. Use the site's existing layout, header and
footer. Content and design notes follow.

## Brand

- Name: **Babarify Download Manager** (short: **BDM**). Tagline: **"Faster Downloads. More Possibilities."**
- Colours: Babarify blue `#2563EB`, green `#10B981` (gradients run blue → green), navy `#0F172A` for dark panels,
  slate `#64748B` for secondary text.
- Typography: Arial, as in the app. Headings bold.
- Logo/icon: the BDM app icon (blue-to-green download arrow), supplied as files.

## Page sections

### 1. Hero

- Heading: **Faster Downloads. More Possibilities.**
- Text: "Babarify Download Manager speeds up your downloads with up to 16 connections per file, resumes where you
  left off, and saves video and audio from thousands of sites, right from Chrome and Edge."
- Buttons: **Download for Windows** (primary) and **See features** (scrolls down).
- Small line under the button: "Version 1.0.0 · Windows 10 and 11 (64-bit) · 3.4 MB · **[decide: Free / price]**"
- Image: the main window screenshot (download list with LED speed meters).

### 2. Feature highlights (cards, 3 per row)

1. **Up to 16× connections** — splits each file into parts and downloads them at once; finished parts help the
   slow ones.
2. **Pause and resume anytime** — survives restarts, crashes and dropped Wi-Fi; checks the file is unchanged before
   continuing.
3. **Video and audio** — MP4 up to 4K, MKV, or MP3/M4A audio from thousands of sites, including whole playlists,
   with subtitles, chapters and cover art.
4. **Works with Chrome and Edge** — captures downloads, adds "Download with BDM" to the right-click menu, and shows
   a download button on videos.
5. **Live speed meters** — LED bars on every download, and a digital readout with the peak speed in each download's
   window.
6. **Queues and schedules** — run downloads overnight, by priority, on chosen days, then sleep or shut down the PC.
7. **Speed limits** — for all downloads or just one, changed instantly.
8. **Batch downloads** — numbered patterns like `photo[01-50].jpg`, pasted lists or a `.txt` file.
9. **Virus scan** — each finished file is checked by Microsoft Defender.
10. **Disk-space guard** — checks free space before starting and pauses before the drive fills.
11. **Refresh expired links** — continue an unfinished download from a new link to the same file.
12. **Tray and clipboard** — keeps running in the tray and offers to download links you copy.

### 3. "Downloads from your browser" (image left, text right)

- Text: "Install the free BDM extension for Chrome or Edge. Downloads you start go straight to BDM, the right-click
  menu sends any link, and a 'Download With BDM' button appears on videos. Pick a quality and it starts."
- Points: capture by file type · excluded sites · sign-in downloads for sites you choose · nothing reads your
  passwords or history.
- Note: "Chrome Web Store and Edge Add-ons listing coming soon." **[decide: remove once published]**
- Image: the extension popup and the on-video panel screenshots.

### 4. "Every download, in detail" (text left, image right)

- Text: "Open any download to see its live speed meter, time left, and every connection's progress on a map.
  Give it its own speed limit or choose what happens when it finishes."
- Image: the download window screenshot (navy speed panel, connections table).

### 5. Privacy and safety (3 short columns)

- **No accounts, no tracking** — no telemetry, analytics or ads. Your download list stays on your PC.
- **Safe by default** — blocks downloads aimed at your own PC or local network, never downgrades HTTPS, and marks
  downloaded files so Windows checks them before opening.
- **Respects creators** — DRM-protected media is refused. "Download only content you have the right to download."
- Link: "Read the privacy policy" → `/bdm/privacy`.

### 6. Download section (id `download`)

- Heading: "Get BDM for Windows"
- Button: **Download BDM 1.0.0 (64-bit installer, 3.4 MB)** → the installer link under Download links below
- Second button: **Browser extension (Chrome / Edge)** → store links when published; until then the extension zip.
- Requirements: Windows 10 or 11, 64-bit · WebView2 (built into Windows 11; the installer adds it on Windows 10 if
  needed) · about 10 MB of disk space (the optional video tools add about 250 MB).
- Install steps:
  1. Run the installer. It installs for your Windows user, with no admin prompt.
  2. If Windows says "Windows protected your PC", choose **More info → Run anyway**. (The installer isn't
     code-signed yet.)
  3. Add the browser extension and restart the browser once.
  4. Optional: Settings → Media tools → **Install** for video and audio downloads.
- SHA-256 checksum line in small monospace text: **[decide: paste from SHA256SUMS.txt of the final release]**

### 7. FAQ (accordion)

- **Is BDM free?** **[decide]**
- **Which browsers work?** Chrome and Microsoft Edge on Windows.
- **Why does Windows show a SmartScreen warning?** BDM's installer isn't code-signed yet. Choose More info → Run
  anyway. Signing is planned.
- **Can it download from YouTube and other video sites?** Yes, from sites supported by the open-source yt-dlp,
  which BDM installs for you on request. DRM-protected services (such as Netflix) aren't supported.
- **Do I keep my downloads when I update?** Yes. Install the new version over the old one; your list and settings
  stay.
- **Does BDM collect my data?** No. See the privacy policy.
- **How do I uninstall?** Windows Settings → Apps → Babarify Download Manager → Uninstall.

### 8. Coming next (small strip, optional)

"Coming soon: **Babarify Player**, a video and audio player built into BDM, with preview while downloading."

## Privacy page `/bdm/privacy`

Use the text of `PRIVACY.md` from the BDM repository as is (headings, tables and lists), with the title
"Babarify Download Manager — Privacy Policy". The Chrome Web Store and Edge Add-ons listings link to this page, so
keep the URL stable.

## SEO

- Title: "Babarify Download Manager — Fast Download Manager for Windows"
- Description: "Speed up downloads with up to 16 connections, resume anytime, and save video and audio from
  Chrome and Edge. Free for Windows 10 and 11." **[decide: drop "Free" if it isn't]**
- Open Graph image: the main window screenshot, 1200 × 630.

## Files to upload to the website chat

- BDM icon: `src-tauri/icons/icon.png` (and `128x128@2x.png`) from the BDM repository.
- Screenshots: the 19 images in `website-assets/` (2x, sample data). `website-assets/README.md` lists which section
  each belongs to, with a caption. Main picks: hero `01-main-window.png`, section 3 `18-video-panel.png` and
  `16-extension-popup.png`, section 4 `03-download-window.png`, section 5 `12b-settings-security.png`; the
  feature cards can each use the matching screenshot (e.g. batch → `09-batch-download.png`).

## Download links

The BDM repository is public, so these always point to the newest GitHub release:

- Installer: https://github.com/tanveerbaber-lgtm/Babarify-Download-Manager-BDM/releases/latest/download/BabarifyDownloadManager_1.0.0_x64-setup.exe
- Browser extension: https://github.com/tanveerbaber-lgtm/Babarify-Download-Manager-BDM/releases/latest/download/BabarifyDownloadManager_1.0.0_browser-extension.zip
