//! Hears Windows end the session (shut down, restart, sign out) or an
//! installer close the app, through a hidden top-level window: Windows
//! sends `WM_ENDSESSION` to each top-level window, hidden ones too, but not
//! to message-only ones.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WM_ENDSESSION, WNDCLASSW,
    WS_OVERLAPPED,
};

use crate::wide;

/// What runs when the session ends. Set once per run.
static ON_END: OnceLock<fn()> = OnceLock::new();

/// The hidden window. Dropping it stops the watch.
pub struct SessionEndWatch {
    hwnd: HWND,
}

impl SessionEndWatch {
    /// Makes the hidden window on this thread, whose messages the caller's
    /// event loop must pump. `on_end` runs on this thread when Windows
    /// ends the session for good (not on the question before, which can
    /// still be cancelled), before it returns: Windows may end the process
    /// right after. A panic in it is caught (it can't leave the window
    /// procedure).
    ///
    /// # Errors
    ///
    /// The Windows error if the window can't be made, or an error if a
    /// watch was already started in this run (one per run).
    pub fn start(on_end: fn()) -> io::Result<Self> {
        if ON_END.get().is_some() {
            return Err(io::Error::other("the session end is already watched"));
        }
        let class = wide("BladeLiteSessionEnd");
        // SAFETY: no name: this module's own handle.
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(on_message),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            // SAFETY: all zero is a valid value for the other fields.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: the class name outlives the call. A second register of
        // the same class fails harmlessly; the create below then uses it.
        unsafe { RegisterClassW(&raw const window_class) };
        // SAFETY: the class name outlives the call; every other pointer is
        // null, which the call allows. Never shown.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null_mut(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Only now: a failed start leaves no callback behind. Each
        // message comes on this thread, after this returns.
        let _ = ON_END.set(on_end);
        Ok(Self { hwnd })
    }
}

impl Drop for SessionEndWatch {
    fn drop(&mut self) {
        // SAFETY: the window this watch made. An error leaves nothing to do.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// The hidden window's procedure.
unsafe extern "system" fn on_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_ENDSESSION && wparam != 0 {
        if let Some(on_end) = ON_END.get() {
            // A panic can't unwind out of this `extern "system"` function.
            let _ = catch_unwind(AssertUnwindSafe(on_end));
        }
        return 0;
    }
    // SAFETY: the arguments Windows gave this procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageW, WM_ENDSESSION, WM_QUERYENDSESSION,
    };

    use super::SessionEndWatch;

    static ENDS: AtomicU32 = AtomicU32::new(0);

    fn count_end() {
        ENDS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn only_a_session_that_really_ends_runs_the_callback() {
        let watch = SessionEndWatch::start(count_end).unwrap();

        // SAFETY: the window this test made, on its own thread.
        unsafe {
            SendMessageW(watch.hwnd, WM_QUERYENDSESSION, 0, 0);
            SendMessageW(watch.hwnd, WM_ENDSESSION, 0, 0);
        }
        assert_eq!(ENDS.load(Ordering::SeqCst), 0);
        // SAFETY: as above.
        unsafe { SendMessageW(watch.hwnd, WM_ENDSESSION, 1, 0) };

        assert_eq!(ENDS.load(Ordering::SeqCst), 1);
    }
}
