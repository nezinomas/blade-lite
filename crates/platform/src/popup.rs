//! What the power mode popup and the tray flyout need from Windows: where
//! the taskbar is not.

use std::io;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_HWNDPREV, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    HWND_TOPMOST, IsWindowVisible, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, SystemParametersInfoW, WS_EX_TOPMOST,
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
/// still not topmost (ticket 181 saw that once) or a shown topmost window
/// is still above it (ticket 195).
pub fn raise_topmost(hwnd: isize) -> io::Result<()> {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    // SAFETY: a plain call on a handle; a bad handle only makes it fail.
    let done = unsafe { SetWindowPos(hwnd as HWND, HWND_TOPMOST, 0, 0, 0, 0, flags) };
    raised(done, done != 0 && is_topmost(hwnd))?;
    match covered(hwnd) {
        Some(above) => Err(io::Error::other(format!(
            "{COVERED} ({})",
            class_name(above)
        ))),
        None => Ok(()),
    }
}

/// The miss when Windows says the raise worked but the window is plain.
const NOT_TOPMOST: &str = "the window is still not topmost";

/// The miss when a shown topmost window is still over the window.
const COVERED: &str = "a topmost window is still above it";

/// The most windows the z-order walk reads. The order can change during
/// the walk, so a cap makes sure it ends.
const MAX_WINDOWS_ABOVE: usize = 10_000;

/// The first shown topmost window of another owner that is above the
/// window and meets its area. Windows that meet none of it are left out:
/// Windows keeps a shown 1x1 `ThumbnailDeviceHelperWnd` at (0, 0) above
/// every raise.
fn covered(hwnd: isize) -> Option<HWND> {
    let area = window_rect(hwnd as HWND)?;
    let mut next = hwnd as HWND;
    for _ in 0..MAX_WINDOWS_ABOVE {
        // SAFETY: a plain call on a handle; the walk ends at the top with
        // null.
        next = unsafe { GetWindow(next, GW_HWNDPREV) };
        if next.is_null() {
            return None;
        }
        // SAFETY: a plain call on a handle; a window that went meanwhile
        // only reads as unowned.
        let owner = unsafe { GetWindow(next, GW_OWNER) };
        let in_sight = shown(next) && window_rect(next).is_some_and(|above| overlap(above, area));
        if covers(in_sight, is_topmost(next as isize), owner as isize == hwnd) {
            return Some(next);
        }
    }
    None
}

/// The window's class name, so a log line says what covered the popup.
fn class_name(hwnd: HWND) -> String {
    let mut name = [0u16; 256];
    // SAFETY: the pointer and length are of a local buffer that outlives
    // the call; a bad handle only returns 0.
    let read = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), 256) };
    let len = usize::try_from(read).unwrap_or(0).min(name.len());
    String::from_utf16_lossy(&name[..len])
}

/// Whether the window can be seen: visible and not cloaked. Windows keeps
/// shell windows (such as the touch keyboard host) and windows on other
/// desktops visible but cloaked.
fn shown(hwnd: HWND) -> bool {
    // SAFETY: a plain call on a handle; a window that went meanwhile only
    // reads as hidden.
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    let mut cloaked: u32 = 0;
    // SAFETY: the pointer is to a local u32 that outlives the call, with its
    // size given; a bad handle only makes it fail.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&raw mut cloaked).cast(),
            u32::try_from(size_of::<u32>()).unwrap_or(0),
        )
    };
    read != 0 || cloaked == 0
}

/// Whether a window above the popup covers it: in sight over it, topmost,
/// and not one of the popup's own.
const fn covers(in_sight: bool, topmost: bool, owned_by_popup: bool) -> bool {
    in_sight && topmost && !owned_by_popup
}

/// The window's rect on the screen as drawn, without the unseen resize
/// borders, or `None` if Windows can't read it.
fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = NO_RECT;
    // SAFETY: the pointer is to a local RECT that outlives the call, with
    // its size given; a bad handle only makes it fail.
    let drawn = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&raw mut rect).cast(),
            u32::try_from(size_of::<RECT>()).unwrap_or(0),
        )
    };
    // SAFETY: as above.
    (drawn == 0 || unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0).then_some(rect)
}

/// Whether two rects share at least one pixel.
const fn overlap(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// The raise's result from what `SetWindowPos` returned and whether the
/// window is topmost after it.
fn raised(done: i32, topmost: bool) -> io::Result<()> {
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    if !topmost {
        return Err(io::Error::other(NOT_TOPMOST));
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
    use std::io;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GW_HWNDNEXT, GetWindow, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, WS_EX_TOPMOST, WS_POPUP,
    };

    use super::{
        COVERED, NOT_TOPMOST, covered, covers, is_topmost, overlap, raise_topmost, raised,
    };
    use crate::wide;

    /// A hidden popup, topmost or not, away from the screen's corner (where
    /// Windows keeps a 1x1 helper window above everything).
    fn window(ex_style: u32) -> isize {
        window_at(ex_style, 100, 0)
    }

    /// A hidden 100x100 popup at `(x, x)`, owned by `owner` (0 for none).
    fn window_at(ex_style: u32, x: i32, owner: isize) -> isize {
        let class = wide("STATIC");
        // SAFETY: the class name outlives the call; every other pointer is
        // null, which the call allows.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                class.as_ptr(),
                null_mut(),
                WS_POPUP,
                x,
                x,
                100,
                100,
                owner as HWND,
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

    /// Shows a window on top of the topmost windows, without the focus.
    fn show_on_top(hwnd: isize) {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
        // SAFETY: a plain call on a live window this thread made.
        unsafe { SetWindowPos(hwnd as HWND, HWND_TOPMOST, 0, 0, 0, 0, flags) };
    }

    /// Whether the raise put the window on top as far as this test can
    /// tell: a shown topmost window of the real desktop may still be over
    /// it, which is no fault of the raise.
    fn raised_here(result: &io::Result<()>) -> bool {
        result
            .as_ref()
            .map_or_else(|error| error.to_string().starts_with(COVERED), |()| true)
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
        assert!(raised_here(&result), "{result:?}");
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
                Err(error) if error.to_string() == NOT_TOPMOST && misses.len() + 1 < TRIES => {
                    misses.push(error.to_string());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                result => break result,
            }
        };

        let topmost = is_topmost(popup);
        destroy(popup);
        assert!(raised_here(&result), "{result:?} after misses: {misses:?}");
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
    fn only_a_shown_topmost_window_of_another_owner_covers() {
        // As a full-screen game that went on top again (ticket 195).
        assert!(covers(true, true, false));
        assert!(
            !covers(false, true, false),
            "a window out of sight covers nothing"
        );
        assert!(!covers(true, false, false), "a plain window is under it");
        assert!(
            !covers(true, true, true),
            "the popup's own windows are its part"
        );
    }

    #[test]
    fn rects_overlap_only_when_they_share_a_pixel() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let popup = rect(100, 100, 200, 200);
        assert!(overlap(rect(0, 0, 1920, 1080), popup), "a full screen");
        assert!(overlap(rect(199, 199, 300, 300), popup), "one corner pixel");
        assert!(!overlap(rect(0, 0, 1, 1), popup), "the corner helper");
        assert!(!overlap(rect(200, 100, 300, 200), popup), "edge to edge");
        assert!(
            !overlap(rect(100, 200, 200, 300), popup),
            "edge to edge below"
        );
    }

    #[test]
    fn a_hidden_topmost_window_above_does_not_cover() {
        // A hidden topmost window of the same thread above the popup, as
        // Windows keeps for the IME, must not count as a miss. The real
        // desktop may cover the spot, so the test compares with before.
        let popup = window(WS_EX_TOPMOST);
        let before = covered(popup);
        let game = window(WS_EX_TOPMOST);
        let game_above = above(game, popup);

        let after = covered(popup);

        destroy(game);
        destroy(popup);
        assert!(game_above, "the test needs the game above the popup");
        assert_eq!(after, before);
    }

    // The two tests below show windows for a moment, each at a spot of its
    // own (tests run at the same time), away from the hidden test windows
    // at (100, 100) that other tests raise.

    #[test]
    fn a_shown_topmost_window_over_the_popup_covers_it() {
        // As a full-screen game that went on top again (ticket 195).
        let popup = window_at(WS_EX_TOPMOST, 400, 0);
        let game = window_at(WS_EX_TOPMOST, 400, 0);
        show_on_top(game);

        let covered = covered(popup);

        destroy(game);
        destroy(popup);
        assert!(covered.is_some());
    }

    #[test]
    fn a_shown_window_of_the_popup_does_not_cover_it() {
        let popup = window_at(WS_EX_TOPMOST, 600, 0);
        let before = covered(popup);
        let own = window_at(WS_EX_TOPMOST, 600, popup);
        show_on_top(own);

        let after = covered(popup);

        destroy(own);
        destroy(popup);
        assert_eq!(after, before);
    }

    #[test]
    fn a_handle_that_is_no_window_is_an_error() {
        assert!(raise_topmost(0).is_err());
    }
}
