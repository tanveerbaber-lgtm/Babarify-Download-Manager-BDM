# Known Limitations

What Babarify Download Manager doesn't do (yet), and where its behavior may surprise you.

**Last reviewed:** 2026-10-25 (M6).

## Platform

- **Windows 10 and 11, x64 only.** Development and testing happened on Windows 11 (build 26300). Windows 10 and
  ARM64 haven't been tested; WebView2 must be installed (it ships with Windows 11 and current Windows 10).
- **No installer or self-update yet.** Both are planned for M7. Until then BDM runs from its build folder.

## Protocols and links

- **HTTP and HTTPS only.** FTP, SFTP, BitTorrent and magnet links, `file:` and `data:` URLs are not supported.
- **Sign-in downloads need the browser.** Sites that need you signed in work once you allow them under
  "Sign-in downloads" in the extension's options; BDM then uses the browser's cookies for that site, kept in memory
  for up to 12 hours. After BDM restarts, start such downloads from the browser again. HTTP logins (a password
  prompt from the server) aren't supported. Links with a user name or password inside them (`https://user:pass@…`) are refused on purpose.
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
- **Other sites (video panel only):** when yt-dlp can't read a page, the panel tries the video the page's player is
  using: its file address, or an HLS/DASH stream (`.m3u8`/`.mpd`) the page loaded, sent with the page as referrer.
  With **media detection** on in the extension, BDM also gets the exact request headers the browser used for each
  video (not cookies or Authorization), and detected videos are ranked so the main video comes before ads (ads
  hosts last, master playlists first, a result much shorter than the video being watched is tried last). Streams
  whose addresses or tokens change on every request can still fail. When a video's server wants the cookies the
  browser sends it, the panel names that server's site and offers to allow it for sign-in downloads (often the
  player's host, not the page's site). The panel also finds players built as web components (video inside a shadow
  root) and players embedded from other sites, titling those with the page's title.
- **Never downloaded:** DRM-protected media (Netflix, Spotify, Disney+, Prime Video and the like).
- **Not supported yet:** live streams. Members-only and age-restricted videos need sign-in downloads turned on for
  the site (see above) and an account that can watch them.
- **Playlists:** up to 1,000 entries are listed.
- **Speed limits for media** are passed to yt-dlp when the download starts. Changing a limit applies after a pause
  and resume, and the global limit isn't split exactly between several media downloads running at once.

## Browser integration

- **Chrome and Edge only.** Firefox, Opera and Brave haven't been tested; Firefox needs a separate extension.
- **Edge installs the extension manually** (Load unpacked) until it's on Edge Add-ons; Chrome uses the Chrome Web Store.
  After first installing BDM, restart the browser once so it finds the helper.
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
