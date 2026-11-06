//! Waits for a registry key under `HKEY_CURRENT_USER` to change.

use std::io;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_THREAD_AGNOSTIC,
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

use crate::instance::Handle;
use crate::{wide, windows_error};

/// A watch on one key's values. Changes made after [`RegistryWatch::open`]
/// are seen, from any thread.
pub struct RegistryWatch {
    key: HKEY,
    event: Handle,
}

// SAFETY: a registry key handle is a plain value that any thread may use,
// and the watch is thread agnostic.
unsafe impl Send for RegistryWatch {}

impl RegistryWatch {
    /// Opens `key` and starts the watch.
    ///
    /// # Errors
    ///
    /// The Windows error if the key is missing or can't be watched.
    pub fn open(key: &str) -> io::Result<Self> {
        let name = wide(key);
        let mut raw: HKEY = ptr::null_mut();
        // SAFETY: the name ends in a nul, and Windows writes only the key.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                name.as_ptr(),
                0,
                KEY_NOTIFY,
                &raw mut raw,
            )
        };
        if status != 0 {
            return Err(windows_error(status));
        }
        // SAFETY: no attributes, auto-reset, not set, no name.
        let event = match Handle::new(unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) }) {
            Ok(event) => event,
            Err(error) => {
                // SAFETY: the key was opened above and is closed once.
                unsafe { RegCloseKey(raw) };
                return Err(error);
            }
        };
        let watch = Self { key: raw, event };
        watch.arm()?;
        Ok(watch)
    }

    /// Waits up to `timeout` for a change. `true` means one came. Changes
    /// that come together are seen once. A timeout too long for Windows
    /// waits for ever.
    ///
    /// # Errors
    ///
    /// The Windows error if the wait failed or the watch can't go on.
    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(INFINITE);
        // SAFETY: the event handle is open until drop.
        match unsafe { WaitForSingleObject(self.event.0, millis) } {
            WAIT_OBJECT_0 => {
                // Each Windows watch fires once, so the next starts now.
                self.arm()?;
                Ok(true)
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Asks Windows to set the event on the next change to a value.
    fn arm(&self) -> io::Result<()> {
        // SAFETY: the key and event are open until drop. Thread agnostic,
        // so the watch stays when the thread that set it ends.
        let status = unsafe {
            RegNotifyChangeKeyValue(
                self.key,
                0,
                REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
                self.event.0,
                1,
            )
        };
        if status != 0 {
            return Err(windows_error(status));
        }
        Ok(())
    }
}

impl Drop for RegistryWatch {
    fn drop(&mut self) {
        // SAFETY: the key came from `RegOpenKeyExW` and is closed once.
        unsafe { RegCloseKey(self.key) };
    }
}
