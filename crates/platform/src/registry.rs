//! Values read from and written to the real registry.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use crate::{Registry, UserBytes, UserStrings, wide, windows_error};

/// The longest text value the app writes, in UTF-16 units. A Run command
/// is a path and a flag, far below this.
const MAX_TEXT: usize = 32 * 1024;

/// The registry of the user running the app.
pub struct WindowsRegistry;

impl Registry for WindowsRegistry {
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32> {
        let key = wide(key);
        let name = wide(name);
        let mut value = 0_u32;
        let mut size = byte_len(1, size_of::<u32>())?;
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
        let bytes = read_value(key, name, RRF_RT_REG_SZ)?;
        let text: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        String::from_utf16(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn write_user_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        let key = wide(key);
        let name = wide(name);
        let value = wide(value);
        if value.len() > MAX_TEXT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the text is too long for the registry",
            ));
        }
        let size = byte_len(value.len(), size_of::<u16>())?;
        // SAFETY: the names end in a nul, and the size is the value's
        // length in bytes, closing nul included, as REG_SZ wants.
        check(unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                size,
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

impl UserBytes for WindowsRegistry {
    fn read_user_bytes(&self, key: &str, name: &str) -> io::Result<Vec<u8>> {
        read_value(key, name, RRF_RT_REG_BINARY)
    }
}

/// Reads the value `name` in `key` as raw bytes, if its type is in `types`.
/// If the value grows between the size check and the read, the read fails.
fn read_value(key: &str, name: &str, types: REG_VALUE_TYPE) -> io::Result<Vec<u8>> {
    let key = wide(key);
    let name = wide(name);
    let mut size = 0_u32;
    // SAFETY: both names end in a nul. With no buffer, Windows only writes
    // the size it needs into `size`.
    check(unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            types,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut size,
        )
    })?;
    let mut bytes = vec![0_u8; size as usize];
    // SAFETY: as above, and `size` is the buffer's length in bytes, so
    // Windows writes no further.
    check(unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            types,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    })?;
    bytes.truncate(size as usize);
    Ok(bytes)
}

/// The error for a status that is not success.
fn check(status: u32) -> io::Result<()> {
    if status != ERROR_SUCCESS {
        return Err(windows_error(status));
    }
    Ok(())
}

/// The size in bytes of `count` items of `size` bytes, as Windows wants it.
/// A size too big for Windows is refused, never cut down.
fn byte_len(count: usize, size: usize) -> io::Result<u32> {
    count
        .checked_mul(size)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "too big for the registry"))
}
