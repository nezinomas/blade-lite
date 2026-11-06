//! Values read from and written to the real registry.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};

use crate::{Registry, UserStrings, wide, windows_error};

/// The registry of the user running the app.
pub struct WindowsRegistry;

impl Registry for WindowsRegistry {
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32> {
        let key = wide(key);
        let name = wide(name);
        let mut value = 0_u32;
        let mut size = byte_len(1, size_of::<u32>());
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
        check(status)?;
        Ok(value)
    }
}

impl UserStrings for WindowsRegistry {
    fn read_user_string(&self, key: &str, name: &str) -> io::Result<String> {
        let key = wide(key);
        let name = wide(name);
        let mut size = 0_u32;
        // SAFETY: both names end in a nul. With no buffer, Windows only
        // writes the size it needs into `size`.
        check(unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut size,
            )
        })?;
        let mut text = vec![0_u16; (size as usize).div_ceil(2)];
        size = byte_len(text.len(), size_of::<u16>());
        // SAFETY: as above, and `size` is the buffer's length in bytes, so
        // Windows writes no further. It ends the text in a nul.
        check(unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                text.as_mut_ptr().cast(),
                &raw mut size,
            )
        })?;
        let end = text
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(text.len());
        String::from_utf16(&text[..end])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn write_user_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        let key = wide(key);
        let name = wide(name);
        let value = wide(value);
        // SAFETY: the names end in a nul, and the size is the value's
        // length in bytes, closing nul included, as REG_SZ wants.
        check(unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                byte_len(value.len(), size_of::<u16>()),
            )
        })
    }

    fn delete_user_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        let key = wide(key);
        let name = wide(name);
        // SAFETY: both names end in a nul.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }
}

/// The error for a status that is not success.
fn check(status: u32) -> io::Result<()> {
    if status != ERROR_SUCCESS {
        return Err(windows_error(status));
    }
    Ok(())
}

/// The size in bytes of `count` items of `size` bytes, as Windows wants it.
fn byte_len(count: usize, size: usize) -> u32 {
    u32::try_from(count * size).unwrap_or(u32::MAX)
}
