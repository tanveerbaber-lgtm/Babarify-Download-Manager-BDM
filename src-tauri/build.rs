// Declaring the app commands here makes Tauri generate an `allow-<command>`
// permission for each one. Commands are then denied unless a capability
// (see capabilities/main.json) grants them explicitly.
const COMMANDS: &[&str] = &[
    "get_app_info",
    "list_downloads",
    "add_download",
    "resume_download",
    "pause_download",
    "cancel_download",
    "remove_download",
    "suggest_file_name",
    "get_settings",
    "update_settings",
    "get_media_tools",
    "install_media_tools",
    "check_media_tool_updates",
    "analyze_media",
    "add_media_downloads",
    "get_browser_integration",
    "register_browser_integration",
    "unregister_browser_integration",
    "add_batch",
    "expand_batch_pattern",
    "import_url_list",
    "find_duplicate",
    "list_queues",
    "save_queue",
    "delete_queue",
    "start_queue",
    "stop_queue",
    "list_categories",
    "save_category",
    "delete_category",
    "set_download_priority",
    "move_downloads_to_queue",
    "move_downloads_in_queue",
    "set_download_speed_limit",
    "redownload",
    "clear_finished",
    "open_download",
    "show_in_folder",
    "file_sha256",
    "bulk_action",
    "open_download_window",
    "open_legal_document",
    "refresh_download_link",
    "get_clipboard_offer",
    "accept_clipboard_offer",
    "dismiss_clipboard_offer",
];

fn main() {
    // Tauri requires the sidecar file to exist for every build. Release
    // bundles get the real helper from scripts/prepare-sidecar.mjs; other
    // builds only need a placeholder so the check passes.
    let target = std::env::var("TARGET").unwrap_or_default();
    let sidecar = std::path::Path::new("binaries").join(format!("bdm-native-host-{target}.exe"));
    if !sidecar.exists() {
        std::fs::create_dir_all("binaries").expect("create binaries/");
        std::fs::write(&sidecar, b"").expect("write sidecar placeholder");
    }
    // The app icon is embedded in the exe at build time; rebuild when it changes.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=capabilities");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
