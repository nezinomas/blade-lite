//! Values read from and written to the real registry.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_BINARY, REG_DWORD, REG_SZ,
    REG_VALUE_TYPE, RRF_RT_ANY, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteKeyValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
};

use crate::{RegValue, Registry, UsbInstance, UserBytes, UserStrings, wide, windows_error};

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
        let (_, bytes) = read_value(HKEY_CURRENT_USER, key, name, RRF_RT_REG_SZ)?;
        text(&bytes)
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
        read_value(HKEY_CURRENT_USER, key, name, RRF_RT_REG_BINARY).map(|(_, bytes)| bytes)
    }
}

/// The text in a `REG_SZ` value's bytes, up to its first nul.
fn text(bytes: &[u8]) -> io::Result<String> {
    let (pairs, _) = bytes.as_chunks::<2>();
    let text: Vec<u16> = pairs
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads the value `name` in `key` under `root` as its kind and raw bytes,
/// if its kind is in `types`. If the value grows between the size check and
/// the read, the read fails.
fn read_value(
    root: HKEY,
    key: &str,
    name: &str,
    types: REG_VALUE_TYPE,
) -> io::Result<(REG_VALUE_TYPE, Vec<u8>)> {
    let key = wide(key);
    let name = wide(name);
    let mut kind: REG_VALUE_TYPE = 0;
    let mut size = 0_u32;
    // SAFETY: both names end in a nul. With no buffer, Windows only writes
    // the size it needs into `size`.
    check(unsafe {
        RegGetValueW(
            root,
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
    // Windows writes no further. `kind` takes the value's kind.
    check(unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            types,
            &raw mut kind,
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    })?;
    bytes.truncate(size as usize);
    Ok((kind, bytes))
}

/// Where Windows keeps its USB devices, under `HKEY_LOCAL_MACHINE`.
const USB_DEVICES: &str = r"SYSTEM\CurrentControlSet\Enum\USB";

/// The most instance keys read under one interface. The laptop's own
/// device has one; far more is no device we know.
const MAX_INSTANCES: u32 = 16;

/// The longest key name, in UTF-16 units, closing nul included.
const MAX_KEY_NAME: u32 = 256;

/// For each USB interface `MI_00` up to `count` of the device: its instance
/// keys, each with its driver and the value `name` under `Device
/// Parameters`. Reads the registry only: the device is never opened.
#[must_use]
pub fn usb_interfaces(
    vendor_id: u16,
    product_id: u16,
    count: u8,
    name: &str,
) -> Vec<io::Result<Vec<UsbInstance>>> {
    (0..count)
        .map(|number| {
            let key = interface_key(vendor_id, product_id, number);
            let instances = subkeys(&key)?
                .iter()
                .map(|instance| usb_instance(&format!(r"{key}\{instance}"), name))
                .collect();
            Ok(instances)
        })
        .collect()
}

/// The key of one USB interface of the device, as Windows names it.
fn interface_key(vendor_id: u16, product_id: u16, number: u8) -> String {
    format!(r"{USB_DEVICES}\VID_{vendor_id:04X}&PID_{product_id:04X}&MI_{number:02X}")
}

/// One instance key's driver and the value `name` under its `Device
/// Parameters`.
fn usb_instance(key: &str, name: &str) -> UsbInstance {
    UsbInstance {
        service: read_value(HKEY_LOCAL_MACHINE, key, "Service", RRF_RT_REG_SZ)
            .and_then(|(_, bytes)| text(&bytes)),
        value: read_value(
            HKEY_LOCAL_MACHINE,
            &format!(r"{key}\Device Parameters"),
            name,
            RRF_RT_ANY,
        )
        .map(|(kind, bytes)| reg_value(kind, bytes)),
    }
}

/// A value from its kind and raw bytes.
fn reg_value(kind: REG_VALUE_TYPE, bytes: Vec<u8>) -> RegValue {
    match kind {
        REG_DWORD => match bytes.as_slice() {
            &[a, b, c, d] => RegValue::Dword(u32::from_le_bytes([a, b, c, d])),
            _ => RegValue::Other,
        },
        REG_BINARY => RegValue::Binary(bytes),
        _ => RegValue::Other,
    }
}

/// The names of the keys right under `key` in `HKEY_LOCAL_MACHINE`. More
/// than [`MAX_INSTANCES`] is an error.
fn subkeys(key: &str) -> io::Result<Vec<String>> {
    let key = MachineKey::open(key)?;
    let mut names = Vec::new();
    for index in 0..=MAX_INSTANCES {
        let mut name = [0_u16; MAX_KEY_NAME as usize];
        let mut length = MAX_KEY_NAME;
        // SAFETY: the key is open, and `length` is the buffer's length in
        // UTF-16 units, so Windows writes no further. The other pointers
        // may be null.
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name.as_mut_ptr(),
                &raw mut length,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(names);
        }
        check(status)?;
        let name = name.get(..length as usize).unwrap_or_default();
        names.push(
            String::from_utf16(name)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        );
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("more than {MAX_INSTANCES} keys"),
    ))
}

/// A key under `HKEY_LOCAL_MACHINE`, open to read, closed when dropped.
struct MachineKey(HKEY);

impl MachineKey {
    fn open(key: &str) -> io::Result<Self> {
        let key = wide(key);
        let mut opened: HKEY = ptr::null_mut();
        // SAFETY: the name ends in a nul, and Windows writes only the handle.
        check(unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                0,
                KEY_READ,
                &raw mut opened,
            )
        })?;
        Ok(Self(opened))
    }
}

impl Drop for MachineKey {
    fn drop(&mut self) {
        // SAFETY: `open` opened the key, and only this closes it.
        unsafe { RegCloseKey(self.0) };
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
/// A size too big for Windows is refused, never cut down.
fn byte_len(count: usize, size: usize) -> io::Result<u32> {
    count
        .checked_mul(size)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "too big for the registry"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_keys_are_named_as_windows_names_them() {
        assert_eq!(
            interface_key(0x1532, 0x02C7, 3),
            r"SYSTEM\CurrentControlSet\Enum\USB\VID_1532&PID_02C7&MI_03"
        );
        assert!(interface_key(0x1532, 0x02C7, 10).ends_with("MI_0A"));
    }

    #[test]
    fn a_dword_is_read_little_endian() {
        assert_eq!(reg_value(REG_DWORD, vec![1, 0, 0, 0]), RegValue::Dword(1));
        assert_eq!(
            reg_value(REG_DWORD, vec![0, 0, 0, 1]),
            RegValue::Dword(0x0100_0000)
        );
    }

    #[test]
    fn a_dword_of_the_wrong_size_is_other() {
        assert_eq!(reg_value(REG_DWORD, vec![1]), RegValue::Other);
    }

    #[test]
    fn binary_keeps_its_bytes_and_other_kinds_are_other() {
        assert_eq!(reg_value(REG_BINARY, vec![1]), RegValue::Binary(vec![1]));
        assert_eq!(reg_value(REG_SZ, vec![b'1', 0]), RegValue::Other);
    }
}
