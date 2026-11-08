//! Mica behind a window, as the stage 22 spike found it works.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
};
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

/// The styles of the min, max and close buttons. Low bits, so they fit.
#[allow(clippy::cast_possible_wrap)]
const CAPTION_BUTTONS: isize = (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;

/// Takes the system menu, minimise and maximise styles off a window without
/// a frame. winit keeps them on such a window, and once Mica's frame covers
/// the whole window, Windows draws their buttons over the content.
///
/// # Errors
///
/// The Windows error if the style could not be read or set.
pub fn hide_caption_buttons(hwnd: isize) -> io::Result<()> {
    let hwnd = hwnd as *mut c_void;
    // SAFETY: plain calls on a handle; a bad handle only makes them fail.
    // The last error is cleared first, as a style can be 0.
    unsafe {
        SetLastError(0);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style == 0 && GetLastError() != 0 {
            return Err(io::Error::last_os_error());
        }
        SetLastError(0);
        if SetWindowLongPtrW(hwnd, GWL_STYLE, style & !CAPTION_BUTTONS) == 0 && GetLastError() != 0
        {
            return Err(io::Error::last_os_error());
        }
        // Windows reads a changed style only on a frame change.
        let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        if SetWindowPos(hwnd, null_mut(), 0, 0, 0, 0, flags) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::ptr::null_mut;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GWL_STYLE, GetWindowLongPtrW, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_POPUP, WS_SIZEBOX, WS_SYSMENU,
    };

    use super::hide_caption_buttons;
    use crate::wide;

    /// A hidden popup with every button style, as winit makes a frameless one.
    fn frameless_window() -> isize {
        let class = wide("STATIC");
        let style = WS_POPUP | WS_SIZEBOX | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
        // SAFETY: the class name outlives the call; every other pointer is null,
        // which the call allows.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null_mut(),
                style,
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

    fn style(hwnd: isize) -> isize {
        // SAFETY: a live window this thread made.
        unsafe { GetWindowLongPtrW(hwnd as _, GWL_STYLE) }
    }

    fn bits(style: u32) -> isize {
        isize::try_from(style).unwrap()
    }

    #[test]
    fn the_buttons_go_and_the_rest_of_the_style_stays() {
        let hwnd = frameless_window();

        hide_caption_buttons(hwnd).unwrap();

        let style = style(hwnd);
        // SAFETY: the window this test made, on its own thread.
        unsafe { DestroyWindow(hwnd as _) };
        assert_eq!(
            style & bits(WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX),
            0
        );
        assert_eq!(style & bits(WS_SIZEBOX), bits(WS_SIZEBOX));
    }

    #[test]
    fn a_handle_that_is_no_window_is_an_error() {
        assert!(hide_caption_buttons(0).is_err());
    }
}
