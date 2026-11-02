//! Mica behind a window, as the stage 22 spike found it works.

use std::ffi::c_void;
use std::io;

use windows_sys::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::core::HRESULT;

/// Fills the window with Mica, tinted dark or light. The window must be
/// drawn transparent, or Mica is hidden behind it.
///
/// # Errors
///
/// The Windows error if a call fails, such as for a handle that is not a
/// live window.
pub fn set_mica(hwnd: isize, dark: bool) -> io::Result<()> {
    let hwnd = hwnd as *mut c_void;
    // -1 on every side: the frame covers the whole window.
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let dark = i32::from(dark);
    let backdrop = DWMSBT_MAINWINDOW;
    // SAFETY: each pointer is to a local that outlives its call, and each
    // size is that local's size. A bad handle only makes the call fail.
    unsafe {
        check(DwmExtendFrameIntoClientArea(hwnd, &raw const margins))?;
        check(DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            size_of_u32::<i32>(),
        ))?;
        check(DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const backdrop).cast(),
            size_of_u32::<i32>(),
        ))
    }
}

/// Gives a window without a frame the rounded corners that framed windows
/// get on Windows 11.
///
/// # Errors
///
/// The Windows error if the call fails.
pub fn round_corners(hwnd: isize) -> io::Result<()> {
    let round = DWMWCP_ROUND;
    // SAFETY: the pointer is to a local that outlives the call, and the size
    // is its size. A bad handle only makes the call fail.
    unsafe {
        check(DwmSetWindowAttribute(
            hwnd as *mut c_void,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const round).cast(),
            size_of_u32::<i32>(),
        ))
    }
}

fn check(result: HRESULT) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::from_raw_os_error(result))
    } else {
        Ok(())
    }
}

fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}
