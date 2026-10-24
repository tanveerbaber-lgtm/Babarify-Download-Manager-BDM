//! Chrome and Edge integration: native-messaging host, registration and
//! the local IPC the host uses to reach the running app.

pub mod host;
pub mod ipc;
pub mod protocol;
pub mod registry;

/// Native-messaging host name (also the registry key name).
pub const HOST_NAME: &str = "com.babarify.bdm";
/// Host executable, installed next to `bdm.exe`.
pub const HOST_EXE: &str = "bdm-native-host.exe";

/// Extensions allowed to start the host. The first is the development ID
/// fixed by the `key` in `extension/manifest.json`; store IDs are added
/// here when the extension is published.
pub const EXTENSION_IDS: &[&str] = &["emaohblojdddihemfkbcchbfgaaanjkb"];
