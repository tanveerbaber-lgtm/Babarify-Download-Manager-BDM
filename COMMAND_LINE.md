# Command line and bdm:// links

BDM can be given links from scripts, other programs and web pages. If BDM is already running, the request goes to
the running copy; otherwise BDM starts and handles it once its window has loaded.

## Command line

```text
bdm.exe --add <url> [--name <file>] [--folder <dir>] [--queue <name>] [--start | --later]
bdm.exe <url> [<url> …]            same as --add for each link
bdm.exe --media <page-url>         open the video dialog for a page
bdm.exe --import <links.txt>       open the batch dialog with the links in a text file
```

- `bdm.exe` is in `%LOCALAPPDATA%\Babarify Download Manager\` after a normal install.
- Without `--start` or `--later`, each link opens the **New Download** dialog (name, category, folder).
- `--start` adds and starts the download without asking; `--later` adds it without starting.
- `--folder` and `--import` accept paths relative to the current folder.
- `--queue` takes a queue's name as shown in BDM (for example `Overnight`).
- `--name` only applies when one link is given.
- Problems (an unknown option, a queue that doesn't exist, a link BDM refuses) show a notification; the rest of the
  command still runs.

Examples:

```powershell
& "$env:LOCALAPPDATA\Babarify Download Manager\bdm.exe" --add https://example.com/file.zip --queue Overnight --later
& "$env:LOCALAPPDATA\Babarify Download Manager\bdm.exe" --import .\links.txt
```

## bdm:// links

```text
bdm://add?url=<url-encoded link>&name=<file name>
bdm://media?url=<url-encoded page link>
```

Installed copies of BDM register `bdm://` for the current user (no admin), and the uninstaller removes it. Any web
page or app can open such a link, and the browser asks before handing it to BDM. Because of that, a `bdm://` link
**always** opens the New Download dialog (or the video dialog): it can suggest a file name but can never choose a
folder or queue, or start a download by itself. Links get the same checks as everything else (http/https only, no
local-network addresses unless allowed in Settings).
