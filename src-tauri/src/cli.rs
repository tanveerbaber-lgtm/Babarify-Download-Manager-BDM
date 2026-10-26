//! Command-line arguments and `bdm://` links.
//!
//! ```text
//! bdm.exe --add <url> [--name <file>] [--folder <dir>] [--queue <name>] [--start | --later]
//! bdm.exe --media <page-url>
//! bdm.exe --import <links.txt>
//! bdm.exe <url>                      (same as --add <url>)
//! bdm.exe "bdm://add?url=<url>&name=<file>"
//! bdm.exe "bdm://media?url=<page-url>"
//! ```
//!
//! A second launch hands its arguments to the running BDM (single instance).
//! Links typed on this PC's command line are trusted: `--start` adds and
//! starts without asking. `bdm://` links can come from any web page or app,
//! so they always open the New Download dialog and can't choose a folder,
//! queue or start by themselves.

use std::path::{Path, PathBuf};

/// One thing the command line or a link asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Add {
        url: String,
        name: Option<String>,
        folder: Option<PathBuf>,
        queue: Option<String>,
        /// `Some(true)` start now, `Some(false)` add without starting, `None` ask.
        start: Option<bool>,
        /// Typed on this PC's command line, not a `bdm://` link.
        trusted: bool,
    },
    /// Open the video dialog for a page.
    Media { url: String },
    /// Open the batch dialog with the links in a text file.
    Import { path: PathBuf },
}

/// Parses BDM's arguments (without the program name). Problems are returned
/// alongside whatever could be understood, so one typo doesn't drop the rest.
pub fn parse(args: &[String], cwd: &Path) -> (Vec<Action>, Vec<String>) {
    let mut urls: Vec<String> = Vec::new();
    let mut actions = Vec::new();
    let mut problems = Vec::new();
    let (mut name, mut folder, mut queue, mut start) = (None, None, None, None);
    let absolute = |p: &str| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { cwd.join(p) }
    };

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            let v = it.next().cloned();
            if v.is_none() {
                problems.push(format!("{flag} needs a value"));
            }
            v
        };
        match arg.as_str() {
            "--add" => urls.extend(value("--add")),
            "--media" => {
                if let Some(url) = value("--media") {
                    actions.push(Action::Media { url });
                }
            }
            "--import" => {
                if let Some(path) = value("--import") {
                    actions.push(Action::Import { path: absolute(&path) });
                }
            }
            "--name" => name = value("--name"),
            "--folder" => folder = value("--folder").map(|f| absolute(&f)),
            "--queue" => queue = value("--queue"),
            "--start" => start = Some(true),
            "--later" => start = Some(false),
            // Start with Windows: handled by the window setup.
            crate::desktop::AUTOSTART_ARG => {}
            link if link.to_ascii_lowercase().starts_with("bdm:") => match parse_link(link) {
                Ok(action) => actions.push(action),
                Err(e) => problems.push(e),
            },
            url if url.starts_with("http://") || url.starts_with("https://") => urls.push(url.to_owned()),
            other => problems.push(format!("unknown argument {other:?}")),
        }
    }
    // --name only makes sense for one file.
    if urls.len() > 1 && name.is_some() {
        problems.push("--name is ignored with more than one link".into());
        name = None;
    }
    for url in urls {
        actions.push(Action::Add { url, name: name.clone(), folder: folder.clone(), queue: queue.clone(), start, trusted: true });
    }
    (actions, problems)
}

/// `bdm://add?url=…&name=…` or `bdm://media?url=…`.
pub fn parse_link(link: &str) -> Result<Action, String> {
    let parsed = url::Url::parse(link).map_err(|_| "that bdm:// link isn't valid".to_string())?;
    // "bdm://add?…" has "add" as its host; "bdm:add?…" as its path.
    let what = parsed.host_str().filter(|h| !h.is_empty()).unwrap_or_else(|| parsed.path().trim_matches('/')).to_ascii_lowercase();
    let param = |key: &str| parsed.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned()).filter(|v| !v.trim().is_empty());
    let url = param("url").ok_or("the bdm:// link has no url")?;
    match what.as_str() {
        "add" | "download" => Ok(Action::Add { url, name: param("name"), folder: None, queue: None, start: None, trusted: false }),
        "media" | "video" => Ok(Action::Media { url }),
        other => Err(format!("unknown bdm:// action {other:?}")),
    }
}

// ------------------------------------------------------------------ running

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::commands::{self, BrowserProposal};
use crate::models::{Download, NewDownloadRequest};
use crate::state::AppState;

/// Asks the main window to open the batch dialog with these links.
pub const EVENT_IMPORT_LINKS: &str = "app:import-links";

/// What the main window is asked to show, in the order it arrived.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiRequest {
    /// The New Download dialog.
    Confirm { proposal: Box<BrowserProposal> },
    /// The video dialog for a page.
    Media { url: String },
    /// The batch dialog with these links.
    Import { links: Vec<String> },
    /// Added and maybe started without asking (a toast).
    Added { download: Box<Download> },
}

/// Requests that arrive before the main window has loaded wait here; the
/// window collects them once it's ready (first launch with a link).
#[derive(Default)]
pub struct StartupRequests {
    ready: std::sync::atomic::AtomicBool,
    waiting: Mutex<Vec<UiRequest>>,
}

impl StartupRequests {
    /// Everything that arrived so far; later requests go straight to the window.
    pub fn take(&self) -> Vec<UiRequest> {
        self.ready.store(true, std::sync::atomic::Ordering::Release);
        std::mem::take(&mut *self.waiting.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn deliver(app: &AppHandle, request: UiRequest) {
    let startup = app.state::<StartupRequests>();
    if !startup.ready.load(std::sync::atomic::Ordering::Acquire) {
        startup.waiting.lock().unwrap_or_else(|e| e.into_inner()).push(request);
        return;
    }
    let _ = match &request {
        UiRequest::Confirm { proposal } => app.emit(crate::EVENT_BROWSER_CONFIRM, proposal),
        UiRequest::Media { url } => app.emit(crate::EVENT_BROWSER_MEDIA, crate::BrowserMedia { url: url.clone(), page_title: None }),
        UiRequest::Import { links } => app.emit(EVENT_IMPORT_LINKS, links),
        UiRequest::Added { download } => app.emit(crate::EVENT_BROWSER_ADDED, download),
    };
}

fn complain(app: &AppHandle, message: &str) {
    eprintln!("[bdm] command line: {message}");
    let _ = app.notification().builder().title("BDM Couldn't Do That").body(message).show();
}

/// Carries out what the command line or a link asked for.
pub fn run(app: &AppHandle, args: &[String], cwd: &Path) {
    let (actions, problems) = parse(args, cwd);
    for p in problems {
        eprintln!("[bdm] command line: {p}");
    }
    if actions.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else { return };
    for action in actions {
        let result = match action {
            Action::Add { url, name, folder, queue, start: Some(start), trusted: true } => {
                let queue_id = queue.as_deref().map(|q| commands::queue_id_by_name(&state, q)).transpose();
                queue_id.and_then(|queue_id| {
                    let request = NewDownloadRequest {
                        url,
                        file_name: name,
                        destination_dir: folder.map(|f| f.to_string_lossy().into_owned()),
                        queue_id,
                        start: Some(start),
                        ..Default::default()
                    };
                    commands::add_file(&state, &request).map(|d| deliver(app, UiRequest::Added { download: Box::new(d) }))
                })
            }
            // Everything else asks first: the New Download dialog.
            Action::Add { url, name, .. } => commands::propose_file(&state, &url, name.as_deref(), None, None)
                .map(|proposal| deliver(app, UiRequest::Confirm { proposal: Box::new(proposal) })),
            Action::Media { url } => crate::validation::validate_url(&url).map(|u| deliver(app, UiRequest::Media { url: u.to_string() })),
            Action::Import { path } => commands::read_url_list(&path).map(|links| deliver(app, UiRequest::Import { links })),
        };
        if let Err(e) = result {
            complain(app, &e.to_string());
        }
    }
    crate::desktop::show_main_window(app);
}

/// What arrived before the window loaded (and from now on, nothing waits).
#[tauri::command]
pub async fn take_startup_requests(startup: tauri::State<'_, StartupRequests>) -> crate::error::AppResult<Vec<UiRequest>> {
    Ok(startup.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    const CWD: &str = r"C:\Users\me\scripts";

    #[test]
    fn adds_links_with_options() {
        let (actions, problems) = parse(
            &args(&["--add", "https://e.com/a.zip", "--name", "A.zip", "--folder", "dl", "--queue", "Overnight", "--start"]),
            Path::new(CWD),
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            actions,
            [Action::Add {
                url: "https://e.com/a.zip".into(),
                name: Some("A.zip".into()),
                folder: Some(PathBuf::from(r"C:\Users\me\scripts\dl")),
                queue: Some("Overnight".into()),
                start: Some(true),
                trusted: true,
            }]
        );
    }

    #[test]
    fn bare_links_media_and_import() {
        let (actions, problems) =
            parse(&args(&["https://e.com/1.iso", "https://e.com/2.iso", "--later", "--media", "https://v.example/watch", "--import", "links.txt"]), Path::new(CWD));
        assert!(problems.is_empty(), "{problems:?}");
        assert!(matches!(&actions[0], Action::Media { url } if url == "https://v.example/watch"));
        assert!(matches!(&actions[1], Action::Import { path } if path == Path::new(r"C:\Users\me\scripts\links.txt")));
        assert!(actions[2..].iter().all(|a| matches!(a, Action::Add { start: Some(false), trusted: true, .. })));
        assert_eq!(actions.len(), 4);
    }

    #[test]
    fn reports_problems_but_keeps_the_rest() {
        let (actions, problems) = parse(&args(&["--bogus", "https://e.com/a.zip", "--name"]), Path::new(CWD));
        assert_eq!(actions.len(), 1);
        assert_eq!(problems.len(), 2, "{problems:?}");
        let (actions, problems) = parse(&args(&["https://e.com/a", "https://e.com/b", "--name", "x"]), Path::new(CWD));
        assert!(actions.iter().all(|a| matches!(a, Action::Add { name: None, .. })));
        assert_eq!(problems.len(), 1);
        assert!(parse(&args(&[crate::desktop::AUTOSTART_ARG]), Path::new(CWD)).0.is_empty());
    }

    #[test]
    fn links_always_ask_and_cant_choose_a_folder() {
        let link = "bdm://add?url=https%3A%2F%2Fe.com%2Ffile.zip&name=My%20File.zip&folder=C%3A%5CWindows&start=1";
        assert_eq!(
            parse_link(link).unwrap(),
            Action::Add { url: "https://e.com/file.zip".into(), name: Some("My File.zip".into()), folder: None, queue: None, start: None, trusted: false }
        );
        assert!(matches!(parse_link("bdm://media?url=https://v.example/w").unwrap(), Action::Media { .. }));
        assert!(matches!(parse_link("bdm:add?url=https://e.com/a").unwrap(), Action::Add { trusted: false, .. }), "no slashes");
        assert!(parse_link("bdm://add").is_err(), "no url");
        assert!(parse_link("bdm://delete?url=https://e.com/a").is_err());
        let (actions, _) = parse(&args(&["BDM://ADD?url=https://e.com/a"]), Path::new(CWD));
        assert!(matches!(&actions[0], Action::Add { trusted: false, .. }), "links stay untrusted on the command line");
    }
}
