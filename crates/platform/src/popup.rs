//! What the power mode popup and the tray flyout need from Windows: where
//! the taskbar is not.

use std::io;

use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

const NO_RECT: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

/// The main screen less the taskbar, as `(x, y, width, height)` in
/// physical pixels.
///
/// # Errors
///
/// The Windows error if the call fails.
pub fn work_area() -> io::Result<(i32, i32, i32, i32)> {
    let mut area = NO_RECT;
    // SAFETY: the pointer is to a local RECT that outlives the call, which
    // is what SPI_GETWORKAREA writes.
    let done = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sides(area))
}

/// The screen at `(x, y)` less its taskbars, as `(x, y, width, height)` in
/// physical pixels. A point off every screen gets the nearest one.
///
/// # Errors
///
/// The Windows error if the call fails.
pub fn work_area_at((x, y): (i32, i32)) -> io::Result<(i32, i32, i32, i32)> {
    // SAFETY: takes the point by value; with DEFAULTTONEAREST it returns a
    // monitor whenever one exists.
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return Err(io::Error::other("no monitor"));
    }
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).map_err(io::Error::other)?,
        rcMonitor: NO_RECT,
        rcWork: NO_RECT,
        dwFlags: 0,
    };
    // SAFETY: the monitor came from Windows just now, and the pointer is to
    // a local MONITORINFO that outlives the call, with its size set.
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sides(info.rcWork))
}

/// A Windows rect as `(x, y, width, height)`.
const fn sides(area: RECT) -> (i32, i32, i32, i32) {
    (
        area.left,
        area.top,
        area.right - area.left,
        area.bottom - area.top,
    )
}
