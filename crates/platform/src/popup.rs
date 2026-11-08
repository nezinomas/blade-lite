//! What the power mode popup needs from Windows: where the taskbar is not,
//! and a way to show the popup without taking the focus.

use std::ffi::c_void;
use std::io;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, SystemParametersInfoW,
};

/// The main screen less the taskbar, as `(x, y, width, height)` in
/// physical pixels.
///
/// # Errors
///
/// The Windows error if the call fails.
pub fn work_area() -> io::Result<(i32, i32, i32, i32)> {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: the pointer is to a local RECT that outlives the call, which
    // is what SPI_GETWORKAREA writes.
    let done = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        area.left,
        area.top,
        area.right - area.left,
        area.bottom - area.top,
    ))
}

/// Shows a window without making it the active one, so the focus stays
/// where the user is typing, or hides it.
pub fn show_without_focus(hwnd: isize, shown: bool) {
    let command = if shown { SW_SHOWNOACTIVATE } else { SW_HIDE };
    // SAFETY: a bad handle only makes the call fail. Its result is whether
    // the window was visible before, not an error.
    unsafe {
        ShowWindow(hwnd as *mut c_void, command);
    }
}
