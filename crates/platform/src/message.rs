//! A plain Windows message box, for a start that fails before any window.

use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO, MessageBoxW,
};

use crate::wide;

/// Shows `text` with an OK button, as an error. Blocks until it is closed.
pub fn show_error(title: &str, text: &str) {
    message_box(title, text, MB_OK | MB_ICONERROR);
}

/// Asks `text` with Yes and No. True only for Yes; a box that can't show
/// counts as No. Blocks until it is answered.
#[must_use]
pub fn ask_yes_no(title: &str, text: &str) -> bool {
    message_box(title, text, MB_YESNO | MB_ICONWARNING) == IDYES
}

fn message_box(title: &str, text: &str, style: u32) -> i32 {
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both strings end in a 0 and outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            style | MB_SETFOREGROUND,
        )
    }
}
