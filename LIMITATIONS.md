# Known Limitations

What Babarify Download Manager doesn't do (yet), and where its behavior may surprise you.

**Last reviewed:** 2026-10-25 (M6).

## Platform

- **Windows 10 and 11, x64 only.** Development and testing happened on Windows 11 (build 26300). Windows 10 and
  ARM64 haven't been tested; WebView2 must be installed (it ships with Windows 11 and current Windows 10).
- **No installer or self-update yet.** Both are planned for M7. Until then BDM runs from its build folder.

## Protocols and links

- **HTTP and HTTPS only.** FTP, SFTP, BitTorrent and magnet links, `file:` and `data:` URLs are not supported.
- **No sign-in downloads.** Links that need a password, an HTTP login or the browser's cookies fail with a clear
  message. Links with a user name or password inside them (`https://user:pass@…`) are refused on purpose.
- **File names come from the link.** BDM doesn't read the server's `Content-Disposition` name; when the browser sends
  a download, the browser's chosen name is used. You can always type a name in the Add dialog.
- **HTTPS stays HTTPS.** A server that redirects from HTTPS to plain HTTP is refused.
- **Local network blocked by default.** Links to this PC (`localhost`) or your home/office network (e.g. a NAS) need
  Settings → Security → "Allow local network downloads". Cloud metadata and link-local addresses are always blocked.
  When a proxy server resolves host names for BDM, only links with a literal IP address can be checked.
- **Checksums:** SHA-256 only (MD5, SHA-1 and SHA-512 aren't accepted in the Add dialog).
- **Batch:** at most 1,000 links at once; a numbered pattern can have one range.

## Video and audio

- **Sites:** whatever yt-dlp supports. Sites change often; if one breaks, update yt-dlp in Settings → Media tools.
- **Never downloaded:** DRM-protected media (Netflix, Spotify, Disney+, Prime Video and the like).
- **Not supported yet:** live streams, private or members-only videos, anything that needs a signed-in account.
- **Playlists:** up to 1,000 entries are listed.
- **Speed limits for media** are passed to yt-dlp when the download starts. Changing a limit applies after a pause
  and resume, and the global limit isn't split exactly between several media downloads running at once.

## Browser integration

- **Chrome and Edge only.** Firefox, Opera and Brave haven't been tested; Firefox needs a separate extension.
- **The extension is loaded unpacked** until it's published to the Chrome Web Store / Edge Add-ons (M7). After first
  installing BDM, restart the browser once so it finds the helper.
- **The on-video panel** is off by default and needs site access, which you grant in the extension's options.

## Queues, schedules and completion actions

- **Schedules use the PC's local time and need the PC awake.** BDM doesn't wake the PC from sleep. A start time missed
  while BDM was closed isn't caught up, but opening BDM inside a scheduled window starts that queue.
- **"Sleep"** uses Windows' standard sleep call; on PCs set to hibernate instead, Windows may hibernate.
- **"Shut down"** waits 60 seconds; run `shutdown /a` to cancel.
- **Notifications** follow Windows' Focus Assist / Do Not Disturb settings.

## Limits

- Up to 16 downloads at once and 16 connections per download.
- Automated tests use files up to 100 MB (and 200+ MB videos have downloaded fine in real use); there's no size limit
  other than free disk space. Partial files take their full size on disk as soon as a segmented download starts.
- The download list handles 1,000+ entries smoothly; history is kept until you remove or clear it, and can't be
  exported yet.
