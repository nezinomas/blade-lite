//! One running copy, through a named mutex, and named events that a second
//! copy sets to wake the first or to ask it to quit.

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

use crate::{InstanceLock, wide};

/// A Windows handle, closed on drop.
pub(crate) struct Handle(pub(crate) *mut c_void);

// SAFETY: a kernel object handle is a plain value that any thread may use.
unsafe impl Send for Handle {}

impl Handle {
    /// Takes the handle a create call returned, or its error if it failed.
    pub(crate) fn new(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a create call and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The lock for one app name, across all sessions on this PC, so a copy
/// in another user's session can't drive the controller too. The wake and
/// the ask to quit stay in this session: a copy in another session (such
/// as an installer's service) can't reach them.
pub struct WindowsInstance {
    mutex_name: Vec<u16>,
    event_name: Vec<u16>,
    event: Handle,
    quit_name: Vec<u16>,
    quit: Handle,
    /// Set by the copy that quit, once its quit preset was sent.
    quit_sent: Handle,
    /// Kept until drop, so the claim lasts as long as this copy runs.
    mutex: Option<Handle>,
}

impl WindowsInstance {
    /// Opens the wake and quit events for `app`. Nothing is claimed yet.
    ///
    /// # Errors
    ///
    /// The Windows error if an event could not be made.
    pub fn new(app: &str) -> io::Result<Self> {
        let event_name = wide(&format!(r"Local\{app}-open"));
        let quit_name = wide(&format!(r"Local\{app}-quit"));
        let sent_name = wide(&format!(r"Local\{app}-quit-sent"));
        Ok(Self {
            mutex_name: wide(&format!(r"Global\{app}")),
            event: open_event(&event_name)?,
            event_name,
            quit: open_event(&quit_name)?,
            quit_name,
            quit_sent: open_manual_event(&sent_name)?,
            mutex: None,
        })
    }

    /// A second handle to the wake event, to wait on from another thread.
    ///
    /// # Errors
    ///
    /// The Windows error if the event could not be opened.
    pub fn listener(&self) -> io::Result<WakeListener> {
        open_event(&self.event_name).map(WakeListener)
    }

    /// A second handle to the quit event, to wait on from another thread.
    ///
    /// # Errors
    ///
    /// The Windows error if the event could not be opened.
    pub fn quit_listener(&self) -> io::Result<WakeListener> {
        open_event(&self.quit_name).map(WakeListener)
    }

    /// Says the quit preset was sent, for a `--quit` start that waits.
    ///
    /// # Errors
    ///
    /// The Windows error if the event could not be set.
    pub fn note_quit_sent(&self) -> io::Result<()> {
        set(&self.quit_sent)
    }
}

impl InstanceLock for WindowsInstance {
    fn claim(&mut self) -> io::Result<bool> {
        // SAFETY: the name ends in a nul. No attributes, and not owned.
        let raw = unsafe { CreateMutexW(ptr::null(), 0, self.mutex_name.as_ptr()) };
        // SAFETY: read straight after the call, on the same thread.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mutex = Handle::new(raw)?;
        if existed {
            return Ok(false);
        }
        self.mutex = Some(mutex);
        Ok(true)
    }

    fn wake_first(&self) -> io::Result<()> {
        // This copy was started by the user, so it may bring a window to the
        // front. It hands that right on, or the first copy's window could
        // open behind others. If that fails, the wake still goes.
        // SAFETY: no pointers; ASFW_ANY is a plain process id value.
        unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        set(&self.event)
    }

    fn ask_first_to_quit(&self) -> io::Result<()> {
        // SAFETY: the event handle is open until drop.
        if unsafe { ResetEvent(self.quit_sent.0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        set(&self.quit)
    }

    fn take_back_quit(&self) -> io::Result<bool> {
        // Auto-reset: a wait that finds it set also clears it, in one step,
        // so a copy can't take it in between.
        WakeListener::wait_on(&self.quit, Duration::ZERO)
    }

    fn quit_was_sent(&self) -> io::Result<bool> {
        WakeListener::wait_on(&self.quit_sent, Duration::ZERO)
    }
}

/// Sets `event`.
fn set(event: &Handle) -> io::Result<()> {
    // SAFETY: the event handle is open until drop.
    if unsafe { SetEvent(event.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Waits for a second copy to ask for the window, or to quit.
pub struct WakeListener(Handle);

impl WakeListener {
    /// Waits up to `timeout` for an ask. `true` means one came. Each ask
    /// is seen once. A timeout too long for Windows waits for ever.
    ///
    /// # Errors
    ///
    /// The Windows error if the wait failed.
    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        Self::wait_on(&self.0, timeout)
    }

    /// Waits up to `timeout` for `event`.
    fn wait_on(event: &Handle, timeout: Duration) -> io::Result<bool> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(INFINITE);
        // SAFETY: the event handle is open until drop.
        match unsafe { WaitForSingleObject(event.0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// Makes the auto-reset wake event, or opens it if another copy made it.
fn open_event(name: &[u16]) -> io::Result<Handle> {
    // SAFETY: the name ends in a nul. No attributes, auto-reset, not set.
    Handle::new(unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) })
}

/// Makes a manual-reset event, which stays set for every look, or opens
/// it if another copy made it.
fn open_manual_event(name: &[u16]) -> io::Result<Handle> {
    // SAFETY: the name ends in a nul. No attributes, manual reset, not set.
    Handle::new(unsafe { CreateEventW(ptr::null(), 1, 0, name.as_ptr()) })
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    use super::*;

    #[test]
    fn the_claim_holds_across_all_sessions() {
        let app = format!("blade-lite-test-global-{}", std::process::id());
        let mut instance = WindowsInstance::new(&app).unwrap();
        assert!(instance.claim().unwrap());

        let global = wide(&format!(r"Global\{app}"));
        // SAFETY: the name ends in a nul. The handle is closed on drop.
        let opened =
            Handle::new(unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, global.as_ptr()) });

        assert!(opened.is_ok(), "the mutex is not global");
    }
}
