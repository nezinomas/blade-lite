//! Hears Windows end the session (shut down, restart, sign out) or an
//! installer close the app, through a hidden top-level window: Windows
//! sends `WM_ENDSESSION` to each top-level window, hidden ones too, but not
//! to message-only ones. The window has its own thread, so it hears the
//! session end even while the app's thread is inside a Windows call that
//! takes sent messages, such as a rate change (ticket 159).

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WNDCLASSW,
    WS_OVERLAPPED,
};

use crate::wide;

/// What runs when the session ends. Set once per run.
static ON_END: OnceLock<fn()> = OnceLock::new();

/// The hidden window and its thread. Dropping it stops the watch.
pub struct SessionEndWatch {
    hwnd: HWND,
    thread: Option<JoinHandle<()>>,
}

impl SessionEndWatch {
    /// Makes the hidden window on a thread of its own, which reads its
    /// messages. `on_end` runs on that thread when Windows ends the session
    /// for good (not on the question before, which can still be
    /// cancelled), before it returns: Windows may end the process right
    /// after. A panic in it is caught (it can't leave the window
    /// procedure).
    ///
    /// # Errors
    ///
    /// The Windows error if the window or its thread can't be made, or an
    /// error if a watch was already started in this run (one per run).
    pub fn start(on_end: fn()) -> io::Result<Self> {
        if ON_END.get().is_some() {
            return Err(io::Error::other("the session end is already watched"));
        }
        let (made, window) = mpsc::channel();
        let (go, told) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("session end".to_owned())
            .spawn(move || {
                let hwnd = match make_window() {
                    Ok(hwnd) => hwnd,
                    Err(error) => {
                        let _ = made.send(Err(error));
                        return;
                    }
                };
                // As a number: a window handle can't cross threads.
                let _ = made.send(Ok(hwnd as isize));
                // Only once the callback is set: no message is read before.
                if told.recv() == Ok(true) {
                    read_messages();
                }
                // SAFETY: this thread's window; one Close already destroyed
                // fails harmlessly.
                unsafe { DestroyWindow(hwnd) };
            })?;
        let hwnd = window
            .recv()
            .map_err(|_| io::Error::other("the session end thread stopped"))??;
        let watch = Self {
            hwnd: hwnd as HWND,
            thread: Some(thread),
        };
        // Only now: a failed start leaves no callback behind. A start that
        // lost a race for the slot drops its window.
        if ON_END.set(on_end).is_err() {
            let _ = go.send(false);
            return Err(io::Error::other("the session end is already watched"));
        }
        let _ = go.send(true);
        Ok(watch)
    }
}

/// Makes the hidden window on this thread.
fn make_window() -> io::Result<HWND> {
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
    Ok(hwnd)
}

/// Reads this thread's messages until its window is destroyed. Windows
/// runs a sent message, such as the session end, inside the read.
fn read_messages() {
    // SAFETY: all zero is a valid message.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // 0 at the quit message, -1 on an error: both end the reads.
    // SAFETY: a message this function owns; no window: every one of this
    // thread's.
    while unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) } > 0 {
        // SAFETY: the message just read.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

impl Drop for SessionEndWatch {
    fn drop(&mut self) {
        // Only its own thread can destroy the window: Close does it there,
        // and the destroy ends the reads. A thread that has ended fails the
        // post harmlessly.
        // SAFETY: the window this watch made, or one already gone.
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
    if message == WM_DESTROY {
        // SAFETY: ends this thread's reads.
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    // SAFETY: the arguments Windows gave this procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SMTO_NORMAL, SendMessageTimeoutW, WM_ENDSESSION, WM_QUERYENDSESSION,
    };

    use super::SessionEndWatch;

    static ENDS: AtomicU32 = AtomicU32::new(0);

    fn count_end() {
        ENDS.fetch_add(1, Ordering::SeqCst);
    }

    /// Sends `message` to `hwnd` as Windows does, and whether it was
    /// answered within 2 s.
    fn answered(hwnd: isize, message: u32, wparam: usize) -> bool {
        let mut result = 0;
        // SAFETY: a window of this test; a closed one fails the call.
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd as HWND,
                message,
                wparam,
                0,
                SMTO_NORMAL,
                2000,
                &raw mut result,
            )
        };
        sent != 0
    }

    // One test: the callback can be set once per run.
    #[test]
    fn only_a_session_that_really_ends_runs_the_callback_while_the_starting_thread_is_busy() {
        let watch = SessionEndWatch::start(count_end).unwrap();
        let hwnd = watch.hwnd as isize;

        // The thread that started the watch waits here and reads no
        // messages, as the app's thread does inside a Windows call
        // (ticket 159).
        let (asked, ended_early, ended) = std::thread::spawn(move || {
            let asked = answered(hwnd, WM_QUERYENDSESSION, 0) && answered(hwnd, WM_ENDSESSION, 0);
            let ended_early = ENDS.load(Ordering::SeqCst);
            let ended = answered(hwnd, WM_ENDSESSION, 1);
            (asked, ended_early, ended)
        })
        .join()
        .unwrap();

        assert!(asked && ended);
        assert_eq!(ended_early, 0);
        assert_eq!(ENDS.load(Ordering::SeqCst), 1);
    }
}
