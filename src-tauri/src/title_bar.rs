//! Paints windows' title bars in Babarify navy with white text (Windows 11).
//! Windows 10 ignores these attributes and keeps its standard title bar.

/// Babarify navy (#0F172A) and white, as Windows COLORREF values (0x00BBGGRR).
#[cfg(windows)]
const CAPTION: u32 = 0x002A_170F;
#[cfg(windows)]
const TEXT: u32 = 0x00FF_FFFF;

pub fn paint(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        // DWMWINDOWATTRIBUTE values from dwmapi.h.
        const DWMWA_BORDER_COLOR: u32 = 34;
        const DWMWA_CAPTION_COLOR: u32 = 35;
        const DWMWA_TEXT_COLOR: u32 = 36;
        #[link(name = "dwmapi")]
        unsafe extern "system" {
            fn DwmSetWindowAttribute(hwnd: *mut c_void, attribute: u32, value: *const c_void, size: u32) -> i32;
        }
        let Ok(hwnd) = window.hwnd() else { return };
        for (attribute, color) in [(DWMWA_CAPTION_COLOR, CAPTION), (DWMWA_BORDER_COLOR, CAPTION), (DWMWA_TEXT_COLOR, TEXT)] {
            // SAFETY: `hwnd` is this window's live handle; `color` outlives the call and its size is passed.
            unsafe {
                DwmSetWindowAttribute(hwnd.0, attribute, (&raw const color).cast(), size_of::<u32>() as u32);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}
