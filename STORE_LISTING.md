# Publishing the BDM extension on the Chrome Web Store and Edge Add-ons

A step-by-step guide for listing the Babarify Download Manager extension as a developer. Requirements were checked
against Google's and Microsoft's documentation on 2026-10-26; check the dashboards for anything that changed since.

## Before you start (BDM-specific)

1. **A public privacy policy URL.** Both stores require one, because the extension handles browsing data (download
   and page addresses, and sign-in cookies for sites you allow). `PRIVACY.md` is in a private repository, so publish
   it somewhere public first: a page on babarify.com, or GitHub Pages from a small public repository.
2. **A public website or support contact** (optional but recommended): where users can get BDM itself, since the
   extension does nothing without the desktop app.
3. **The extension ID will change.** Unpacked, the extension has the ID `emaohblojdddihemfkbcchbfgaaanjkb` (from the
   `key` in `extension/manifest.json`). Each store gives its copy a new ID, and BDM's browser helper only talks to
   IDs it knows (`EXTENSION_IDS` in `src-tauri/src/browser/mod.rs`). So:
   - Upload a draft to each store first (step 2 below) to get its ID **before** publishing.
   - Add both store IDs to `EXTENSION_IDS`, build and release a BDM version with them, and only then publish the
     extension. Otherwise the store copy shows "BDM isn't connected" for everyone.
   - Chrome can keep your current development ID instead: the dashboard's **Package → View public key** gives a key
     you can put in the local manifest, so development and store copies share one ID. Edge always assigns its own.
4. **Remove the `key` field from the uploaded copy.** The stores manage the key themselves; keep it only in the
   development manifest.
5. **Bump the extension version** (`version` in `manifest.json`) for every upload; stores refuse the same version
   twice. (BDM stays 1.0.0 until you decide to release.)

### Make the upload ZIP

The ZIP must have `manifest.json` at its root and nothing BDM doesn't ship (no `tests` folder):

```powershell
$src = "E:\Babarify Download Manager\extension"; $out = "E:\BDM Store\extension-store"
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $out | Out-Null
Copy-Item "$src\*" $out -Recurse -Exclude tests
# Remove the "key" line from the store copy's manifest, then:
Compress-Archive -Path "$out\*" -DestinationPath "E:\BDM Store\bdm-extension.zip" -Force
```

### Images to prepare

| Asset | Chrome Web Store | Edge Add-ons |
| --- | --- | --- |
| Icon / logo | 128×128 PNG (96×96 artwork, 16 px transparent padding), already in `extension/icons` | 300×300 recommended (min 128×128), 1:1 |
| Screenshots | at least 1, 1280×800 (or 640×400) | up to 6, 1280×800 or 640×480 (optional) |
| Small promo tile | 440×280 (optional) | 440×280 (optional) |
| Large promo / marquee | 1400×560 (optional) | 1400×560 (optional) |

Good screenshots: the "Download With BDM" panel on a video with its formats menu; the popup showing "Connected to
BDM"; the New Download dialog in BDM after clicking a download link; the extension's options page.

## Chrome Web Store

1. **Register** at the [Chrome Web Store Developer Dashboard](https://chrome.google.com/webstore/devconsole) with a
   Google account: accept the developer agreement and pay the one-time registration fee (US$5 at the time of
   writing). New publishers can publish up to two extensions at first.
2. **Add new item** → upload the ZIP. Open the **Package** tab and note the **Item ID** (this is the store
   extension ID for step 3 above).
3. **Store listing:** name and short description come from the manifest; add a detailed description, category
   (Productivity / Tools), language, screenshots and the icon. Say clearly that the extension **requires the
   Babarify Download Manager desktop app for Windows** and link to where to get it.
4. **Privacy tab:**
   - *Single purpose:* "Sends downloads, links and videos the user chooses to the Babarify Download Manager desktop
     app on the same PC."
   - *Permission justifications* (one per permission, see the table below).
   - *Remote code:* **No** (Manifest V3; all code is in the package).
   - *Data usage:* declare what applies and certify the limited-use policy. Honest, conservative choices for BDM:
     **Web history** (addresses of downloads and pages the user sends to BDM) and **Authentication information**
     (sign-in cookies, only for sites the user allows), noting that this data only goes to the BDM app on the same
     PC and is never sold or sent to Babarify.
   - *Privacy policy URL:* the public copy of `PRIVACY.md`.
5. **Distribution:** free; choose countries; **Public** (or **Unlisted** for a soft launch).
6. **Test instructions:** reviewers can't install a Windows app from the dashboard, so explain: "Requires the
   Babarify Download Manager desktop app (installer link). Without it the popup shows 'BDM isn't connected'; capture
   falls back to the browser." Offer a test installer download link.
7. **Submit for review.** Review time varies; broad optional host access, `nativeMessaging` and cookie access can
   mean a longer in-depth review. After approval you have 30 days to publish (or it publishes automatically, as you
   choose).

## Microsoft Edge Add-ons

1. **Register** at [Partner Center](https://partner.microsoft.com/dashboard/microsoftedge/public/login?ref=dd) with
   a personal Microsoft account (or a personal GitHub account). There is no registration fee; work or school
   accounts aren't accepted.
2. **Create new extension** → upload the same ZIP (with its own version). Note the **extension ID** shown on the
   overview after upload (step 3 above).
3. **Availability:** Public or Hidden; markets.
4. **Properties:** category (Productivity), website, support contact.
5. **Privacy:** single purpose, a justification per permission, remote code **No**, data usage (as for Chrome) and
   the privacy policy URL. Microsoft treats incomplete or inaccurate disclosures as a policy violation.
6. **Store listings:** description (250–10,000 characters, must describe all functionality), logo, optional
   screenshots, promo tiles and YouTube video, up to 7 search terms.
7. **Notes for certification:** the same explanation as Chrome's test instructions, with a link to the installer.
8. **Publish.** Certification can take up to seven business days.

## Permission justifications (copy into both stores)

| Permission | Why BDM needs it |
| --- | --- |
| `nativeMessaging` | Talks to the Babarify Download Manager app installed on the same PC; this is the extension's whole purpose. |
| `downloads` | Notices downloads the user starts so they can be handed to BDM instead (by file type, chosen in options). |
| `contextMenus` | Adds "Download link with BDM" and "Download video or audio with BDM" to the right-click menu. |
| `storage` | Saves the extension's own settings (file types, excluded sites, switches). |
| `activeTab` | Reads the current tab's address and title when the user clicks the toolbar button or a menu item. |
| `webRequest` (optional) | Only if the user turns on "Find video and audio on pages": lists media the page loads, and the headers needed to fetch it the same way. |
| `scripting` (optional) | Only if the user turns on the "Download With BDM" panel: shows the button on videos. |
| `cookies` (optional) | Only for sites the user adds under "Sign-in downloads": passes that site's sign-in to BDM so member downloads work. |
| Host access `http://*/*`, `https://*/*` (optional) | Needed by the three optional features above, requested only when the user turns one on. |

## After the listings are live

- Release BDM with both store IDs in `EXTENSION_IDS` (see "Before you start"), or users of the store copy can't
  connect.
- Update BDM's **Browser Integration** page and the README: replace the "Load unpacked" steps with links to the two
  store listings.
- Every extension update: raise `version`, upload a new ZIP to both dashboards, and keep the privacy answers in
  step with any new permission.

Sources: [Chrome: register](https://developer.chrome.com/docs/webstore/register),
[Chrome: publish](https://developer.chrome.com/docs/webstore/publish),
[Chrome: images](https://developer.chrome.com/docs/webstore/images),
[Chrome: privacy tab](https://developer.chrome.com/docs/webstore/cws-dashboard-privacy),
[Chrome: manifest key](https://developer.chrome.com/docs/extensions/reference/manifest/key),
[Edge: developer account](https://learn.microsoft.com/en-us/microsoft-edge/extensions/publish/create-dev-account),
[Edge: publish](https://learn.microsoft.com/en-us/microsoft-edge/extensions/publish/publish-extension).
