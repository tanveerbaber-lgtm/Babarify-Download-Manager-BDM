# Chrome Web Store listing for the BDM extension

Everything to upload or paste. Permission justifications and more detail are in `STORE_LISTING.md`.

## Upload

| Dashboard field | Use |
| --- | --- |
| Package | `bdm-extension-1.0.0-store.zip` (not `extension.zip` in the project root) |
| Screenshots | `screenshot-1-video-panel.jpg` to `screenshot-5-download-window.jpg` (1280×800) |
| Small promo tile | `promo-tile-440x280.jpg` |
| Store icon | `extension/icons/icon-128.png` |
| Category | Tools (under Productivity) |
| Language | English |

## Detailed description

Paste this. Replace the two links once they exist.

```text
Send downloads, links and videos from Chrome to Babarify Download Manager (BDM), a fast download manager for Windows.

REQUIRES THE DESKTOP APP
This extension works together with the Babarify Download Manager app for Windows 10 and 11. Without the app, downloads simply stay in the browser. Get BDM: [DOWNLOAD LINK]

WHAT IT DOES
• Capture downloads: files you download go to BDM, by file type, with sites you can exclude
• Right-click menu: "Download link with BDM" and "Download video or audio with BDM"
• Optional "Download With BDM" button on videos: pick a quality or audio only
• Optional media detection: lists the video and audio streams a page loads
• Optional sign-in downloads: for sites you add, one at a time, BDM can use your browser's sign-in

WHAT BDM ADDS
• Up to 16 connections per file, pause and resume, and automatic retries
• Video and audio from sites supported by the open-source yt-dlp tool
• Queues, schedules, speed limits, batch downloads and a virus scan with Microsoft Defender

YOUR PRIVACY
No accounts, no analytics and no ads. Addresses you send go only to the BDM app on your own PC, never to Babarify. Optional features stay off until you turn them on and grant permission. Privacy policy: [PRIVACY LINK]

USE IT RESPONSIBLY
Only download content you have the right to save. DRM-protected media is refused.
```

## Privacy tab

- **Single purpose:** Sends downloads, links and videos the user chooses to the Babarify Download Manager desktop app on the same PC.
- **Remote code:** No.
- **Data usage:** Web history and Authentication information, as explained in `STORE_LISTING.md`.
- **Privacy policy URL:** `https://babarify.com/bdm/privacy`, once that page is live.

## Test instructions for the reviewer

Replace the link once it exists.

```text
This extension requires the Babarify Download Manager desktop app for Windows: [DOWNLOAD LINK]. Install it, then reload the extension. The popup should say "Connected to BDM". Without the app, the popup says "BDM isn't connected" and downloads stay in the browser. To try the video button, turn on "Show Download With BDM on videos" in the extension's options, and allow it when Chrome asks.
```
