//! Values read from the real registry.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

use crate::{Registry, windows_error};

/// The registry of the user running the app.
pub struct WindowsRegistry;

impl Registry for WindowsRegistry {
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32> {
        let key = wide(key);
        let name = wide(name);
        let mut value = 0_u32;
        let mut size = u32::try_from(size_of::<u32>()).unwrap_or(u32::MAX);
        // SAFETY: both names end in a nul, and the type flag lets Windows
        // write only a DWORD, which fits the 4 bytes `size` gives it.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&raw mut value).cast(),
                &raw mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(windows_error(status));
        }
        Ok(value)
    }
}

/// The text as UTF-16 with a closing nul, as Windows wants it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
