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

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW,
    WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

use crate::wide;

/// What runs when the session ends. Set once per run.
static ON_END: OnceLock<fn()> = OnceLock::new();

/// The hidden window, on its own thread. Dropping it stops the watch.
pub struct SessionEndWatch {
    hwnd: HWND,
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
        std::thread::Builder::new()
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
                // The reads end when Close has destroyed the window.
                if told.recv() == Ok(true) {
                    read_messages();
                } else {
                    // SAFETY: this thread's window, never used.
                    unsafe { DestroyWindow(hwnd) };
                }
            })?;
        let hwnd = window
            .recv()
            .map_err(|_| io::Error::other("the session end thread stopped"))??;
        // Only now: a failed start leaves no callback behind. A start that
        // lost a race for the slot drops its window.
        if ON_END.set(on_end).is_err() {
            let _ = go.send(false);
            return Err(io::Error::other("the session end is already watched"));
        }
        let _ = go.send(true);
        Ok(Self { hwnd: hwnd as HWND })
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

/// Runs the messages other threads have sent to this thread's windows,
/// and only those: a thread that waits calls it, so a Windows call that
/// sends to every window, such as a rate change, does not wait for it.
pub fn serve_sent_messages() {
    // SAFETY: all zero is a valid message.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: a message this function owns; nothing is taken off the queue.
    unsafe {
        PeekMessageW(
            &raw mut message,
            null_mut(),
            0,
            0,
            PM_NOREMOVE | PM_QS_SENDMESSAGE,
        )
    };
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
        // and the destroy ends the reads. Not waited for: a session end
        // there may wait for the event loop that has just ended, and the
        // quit after the loop must not wait for it (ticket 159 review).
        // SAFETY: the window this watch made.
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
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
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, SMTO_NORMAL, SendMessageTimeoutW, WM_ENDSESSION, WM_NULL, WM_QUERYENDSESSION,
    };

    use super::{SessionEndWatch, make_window, serve_sent_messages};

    static ENDS: AtomicU32 = AtomicU32::new(0);
    /// Whether a session end waits, as one waits for the event loop.
    static HOLD: AtomicBool = AtomicBool::new(false);

    fn count_end() {
        ENDS.fetch_add(1, Ordering::SeqCst);
        if HOLD.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1500));
        }
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

        // A session end that waits for an event loop just ended must not
        // hold the quit after it (ticket 159 review).
        HOLD.store(true, Ordering::SeqCst);
        let waiting = std::thread::spawn(move || answered(hwnd, WM_ENDSESSION, 1));
        std::thread::sleep(Duration::from_millis(200));
        let began = Instant::now();
        drop(watch);

        assert!(began.elapsed() < Duration::from_millis(500));
        assert!(waiting.join().unwrap());
    }

    #[test]
    fn a_thread_that_serves_sent_messages_answers_them_while_it_waits() {
        let (made, window) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let waiter = std::thread::spawn(move || {
            let hwnd = make_window().unwrap();
            made.send(hwnd as isize).unwrap();
            while stopped.recv_timeout(Duration::from_millis(10)).is_err() {
                serve_sent_messages();
            }
            // SAFETY: this thread's window.
            unsafe { DestroyWindow(hwnd) };
        });
        let hwnd = window.recv().unwrap();

        let served = answered(hwnd, WM_NULL, 0);

        stop.send(()).unwrap();
        waiter.join().unwrap();
        assert!(served);
    }
}
