//! Mica behind a window, as the stage 22 spike found it works.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, SetLastError, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WM_NCDESTROY, WM_STYLECHANGING,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
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

/// The styles of the close, min and max buttons.
const CAPTION_BUTTONS: u32 = WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;

/// Our id for the style hook, among the window's other subclasses.
const STYLE_HOOK: usize = 0x58;

/// Takes the system menu, minimise and maximise styles off a window without
/// a frame, and keeps them off. winit keeps them on such a window and sets
/// them again on each change of its style, and once Mica's frame covers the
/// whole window, Windows draws their buttons over the content. The window
/// loses its system menu (Alt+Space) too, so this is for a flyout. Call it on
/// the thread that made the window; calling it again does no harm.
///
/// # Errors
///
/// The Windows error if the hook could not be put on, or the style could
/// not be read or set.
pub fn hide_caption_buttons(hwnd: isize) -> io::Result<()> {
    let hwnd = hwnd as *mut c_void;
    // SAFETY: plain calls on a handle; a bad handle only makes them fail.
    // The last error is cleared first, as a style can be 0. The hook is a
    // plain function, so it outlives the window.
    unsafe {
        // Not documented to set the last error, so it gets its own.
        if SetWindowSubclass(hwnd, Some(strip_buttons), STYLE_HOOK, 0) == 0 {
            return Err(io::Error::other("the style hook could not be put on"));
        }
        SetLastError(0);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style == 0 && GetLastError() != 0 {
            return Err(io::Error::last_os_error());
        }
        // Set again, the style goes through the hook, which takes the
        // buttons out.
        SetLastError(0);
        if SetWindowLongPtrW(hwnd, GWL_STYLE, style) == 0 && GetLastError() != 0 {
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

/// The style hook: takes the button styles out of each new style before
/// Windows takes it.
unsafe extern "system" fn strip_buttons(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // WPARAM carries the GWL index, a small negative int.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let index = wparam as i32;
    if message == WM_STYLECHANGING && index == GWL_STYLE && lparam != 0 {
        // SAFETY: for WM_STYLECHANGING, LPARAM points to the STYLESTRUCT
        // that Windows reads back after this returns.
        let styles = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
        styles.styleNew &= !CAPTION_BUTTONS;
    }
    if message == WM_NCDESTROY {
        // SAFETY: the hook comes off its own window, as Windows asks.
        unsafe { RemoveWindowSubclass(hwnd, Some(strip_buttons), STYLE_HOOK) };
    }
    // SAFETY: passes the message on unchanged, as each subclass must.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
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
        CreateWindowExW, DestroyWindow, GWL_STYLE, GetWindowLongPtrW, SetWindowLongPtrW,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SIZEBOX, WS_SYSMENU,
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

        let result = hide_caption_buttons(hwnd);

        let style = style(hwnd);
        // SAFETY: the window this test made, on its own thread.
        unsafe { DestroyWindow(hwnd as _) };
        result.unwrap();
        assert_eq!(
            style & bits(WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX),
            0
        );
        assert_eq!(style & bits(WS_SIZEBOX), bits(WS_SIZEBOX));
    }

    #[test]
    fn a_later_style_change_does_not_bring_the_buttons_back() {
        let hwnd = frameless_window();
        let result = hide_caption_buttons(hwnd);

        // As winit does on a show: the whole style, buttons and all.
        let full = style(hwnd) | bits(WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
        // SAFETY: a live window this thread made.
        unsafe { SetWindowLongPtrW(hwnd as _, GWL_STYLE, full) };

        let style = style(hwnd);
        // SAFETY: the window this test made, on its own thread.
        unsafe { DestroyWindow(hwnd as _) };
        result.unwrap();
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
