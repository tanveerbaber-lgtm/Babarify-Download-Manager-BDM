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
];

fn main() {
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
