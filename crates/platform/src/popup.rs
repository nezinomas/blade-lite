//! What the power mode popup and the tray flyout need from Windows: where
//! the taskbar is not.

use std::io;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowPos, SystemParametersInfoW, WS_EX_TOPMOST,
};

const NO_RECT: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

/// The main screen less the taskbar, as `(x, y, width, height)` in
/// physical pixels (in a DPI-aware process, as the app is).
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
/// physical pixels (in a DPI-aware process, as the app is). A point off
/// every screen gets the nearest one.
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

/// Puts a window at the top of the topmost windows, without the focus.
/// Topmost alone is not enough: a full-screen game that made itself
/// topmost later stays above it (ticket 155).
///
/// # Errors
///
/// The Windows error if the call fails, such as for a handle that is not a
/// live window, or an error if the call says it worked but the window is
/// still not topmost (ticket 181 saw that once).
pub fn raise_topmost(hwnd: isize) -> io::Result<()> {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    // SAFETY: a plain call on a handle; a bad handle only makes it fail.
    let done = unsafe { SetWindowPos(hwnd as HWND, HWND_TOPMOST, 0, 0, 0, 0, flags) };
    raised(done, done != 0 && is_topmost(hwnd))
}

/// The raise's result from what `SetWindowPos` returned and whether the
/// window is topmost after it.
fn raised(done: i32, topmost: bool) -> io::Result<()> {
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    if !topmost {
        return Err(io::Error::other("the window is still not topmost"));
    }
    Ok(())
}

/// Whether the window has `WS_EX_TOPMOST`. A window Windows can't read
/// counts as not topmost.
fn is_topmost(hwnd: isize) -> bool {
    // SAFETY: a plain call on a handle; a bad handle only returns 0.
    let ex_style = unsafe { GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE) };
    isize::try_from(WS_EX_TOPMOST).is_ok_and(|bit| ex_style & bit != 0)
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

#[cfg(test)]
mod tests {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GW_HWNDNEXT, GetWindow, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetWindowPos, WS_EX_TOPMOST, WS_POPUP,
    };

    use super::{is_topmost, raise_topmost, raised};
    use crate::wide;

    /// A hidden popup, topmost or not.
    fn window(ex_style: u32) -> isize {
        let class = wide("STATIC");
        // SAFETY: the class name outlives the call; every other pointer is
        // null, which the call allows.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                class.as_ptr(),
                null_mut(),
                WS_POPUP,
                0,
                0,
                100,
                100,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        assert!(!hwnd.is_null(), "no test window");
        hwnd as isize
    }

    /// Whether `upper` is above `lower` in the z-order.
    fn above(upper: isize, lower: isize) -> bool {
        let mut next = upper as HWND;
        while !next.is_null() {
            // SAFETY: a plain call on a live window; the walk ends at the
            // bottom with null.
            next = unsafe { GetWindow(next, GW_HWNDNEXT) };
            if next as isize == lower {
                return true;
            }
        }
        false
    }

    fn destroy(hwnd: isize) {
        // SAFETY: a window this test made, on its own thread.
        unsafe { DestroyWindow(hwnd as _) };
    }

    #[test]
    fn a_topmost_window_goes_above_a_later_topmost_one() {
        // As a full-screen game that made itself topmost after the popup.
        let popup = window(WS_EX_TOPMOST);
        let game = window(WS_EX_TOPMOST);
        // The popup goes just under the game, whatever other topmost
        // windows did meanwhile.
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // SAFETY: a plain call on live windows this thread made.
        unsafe { SetWindowPos(popup as HWND, game as HWND, 0, 0, 0, 0, flags) };
        let before = above(popup, game);

        let result = raise_topmost(popup);

        let after = above(popup, game);
        destroy(game);
        destroy(popup);
        result.unwrap();
        assert!(!before, "the test starts with the popup under the game");
        assert!(after);
    }

    #[test]
    fn a_plain_window_becomes_topmost() {
        // Topmost is desktop-wide. This failed twice in full workspace runs;
        // the failure seen was a call that reported success but left the
        // window plain (ticket 181; not seen in 830 runs alone). The raise
        // now says so with an error (ticket 190), so the test tries again on
        // that miss only. A fail names every miss; a pass after misses stays
        // quiet (the crate bans prints, and cargo test hides them anyway).
        const TRIES: usize = 5;
        let popup = window(0);

        let mut misses = Vec::new();
        let result = loop {
            match raise_topmost(popup) {
                // A miss is our own error; a Windows error has a code.
                Err(error) if error.raw_os_error().is_none() && misses.len() + 1 < TRIES => {
                    misses.push(error.to_string());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                result => break result,
            }
        };

        let topmost = is_topmost(popup);
        destroy(popup);
        assert!(result.is_ok(), "{result:?} after misses: {misses:?}");
        assert!(topmost, "not topmost after misses: {misses:?}");
    }

    #[test]
    fn a_raise_that_leaves_the_window_plain_is_an_error() {
        // Windows can say the call worked yet leave the window plain
        // (ticket 181).
        let error = raised(1, false).unwrap_err();
        assert!(error.to_string().contains("not topmost"), "{error}");
    }

    #[test]
    fn a_raise_that_made_the_window_topmost_is_ok() {
        assert!(raised(1, true).is_ok());
    }

    #[test]
    fn a_handle_that_is_no_window_is_an_error() {
        assert!(raise_topmost(0).is_err());
    }
}
