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

/// Extensions allowed to start the host: the development ID fixed by the
/// `key` in `extension/manifest.json`, then the Chrome Web Store's ID.
pub const EXTENSION_IDS: &[&str] = &["emaohblojdddihemfkbcchbfgaaanjkb", "kamcoajmedllmkajbgjenlbajkbilgie"];

/// The extension's Chrome Web Store listing.
pub const CHROME_STORE_URL: &str = "https://chromewebstore.google.com/detail/babarify-download-manager/kamcoajmedllmkajbgjenlbajkbilgie";

#[cfg(test)]
mod store_tests {
    use super::{CHROME_STORE_URL, EXTENSION_IDS};

    #[test]
    fn the_store_link_points_at_an_allowed_extension() {
        let id = CHROME_STORE_URL.rsplit('/').next().unwrap();
        assert!(EXTENSION_IDS.contains(&id));
    }
}
