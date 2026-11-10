//! What runs now: programs from a Toolhelp snapshot, services and drivers
//! from the service manager. Read only.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW, OpenSCManagerW,
    SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_DRIVER,
    SERVICE_WIN32,
};

use crate::Programs;
use crate::instance::Handle;

/// How many times to ask for the services when the list grows between the
/// size call and the read.
const SERVICE_TRIES: usize = 4;

/// What runs on this PC.
#[derive(Default)]
pub struct WindowsPrograms;

impl Programs for WindowsPrograms {
    fn running(&self) -> io::Result<Vec<String>> {
        let mut names = processes()?;
        names.extend(services()?);
        Ok(names)
    }
}

/// The file name of each running program.
fn processes() -> io::Result<Vec<String>> {
    // SAFETY: takes flags and a process ID by value.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Handle::new(raw)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut names = Vec::new();
    // SAFETY: the entry's size is set, so Windows writes inside it.
    let mut more = unsafe { Process32FirstW(snapshot.0, &raw mut entry) } != 0;
    while more {
        names.push(until_nul(&entry.szExeFile));
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &raw mut entry) } != 0;
    }
    Ok(names)
}

/// The text before the first nul, or all of it.
fn until_nul(text: &[u16]) -> String {
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    String::from_utf16_lossy(&text[..end])
}

/// The service manager, closed on drop.
struct Manager(SC_HANDLE);

impl Drop for Manager {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW and is closed once.
        unsafe { CloseServiceHandle(self.0) };
    }
}

/// The name and display name of each running service and driver.
fn services() -> io::Result<Vec<String>> {
    // SAFETY: null names mean this PC and its active database.
    let raw = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = Manager(raw);
    let mut needed = 0;
    for _ in 0..SERVICE_TRIES {
        match read_services(&manager, needed)? {
            Ok(names) => return Ok(names),
            Err(more) => needed = more,
        }
    }
    Err(io::Error::from_raw_os_error(
        i32::try_from(ERROR_MORE_DATA).unwrap_or(i32::MAX),
    ))
}

/// The names, if they fit in `bytes`, or the bytes Windows asks for.
fn read_services(manager: &Manager, bytes: u32) -> io::Result<Result<Vec<String>, u32>> {
    // u64 words, so the entries, which hold pointers, are aligned.
    let mut buffer = vec![0_u64; usize::try_from(bytes).unwrap_or(0).div_ceil(8)];
    let size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    let (mut needed, mut count, mut resume) = (0_u32, 0_u32, 0_u32);
    // SAFETY: the buffer is `size` bytes long and lives past the call;
    // Windows writes the entries and their text inside it.
    let ok = unsafe {
        EnumServicesStatusExW(
            manager.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32 | SERVICE_DRIVER,
            SERVICE_ACTIVE,
            buffer.as_mut_ptr().cast(),
            size,
            &raw mut needed,
            &raw mut count,
            &raw mut resume,
            ptr::null(),
        )
    };
    if ok == 0 {
        let error = io::Error::last_os_error();
        let more = i32::try_from(ERROR_MORE_DATA).ok();
        return if error.raw_os_error() == more && needed > bytes {
            Ok(Err(needed))
        } else {
            Err(error)
        };
    }
    let count = usize::try_from(count).unwrap_or(0);
    let fits = count * size_of::<ENUM_SERVICE_STATUS_PROCESSW>() <= buffer.len() * 8;
    if !fits {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "more services than the buffer holds",
        ));
    }
    // SAFETY: Windows wrote `count` entries at the start of the buffer,
    // which is aligned for them and holds them all (checked above).
    let entries = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
            count,
        )
    };
    let mut names = Vec::with_capacity(count * 2);
    for entry in entries {
        // SAFETY: each name is nul-ended text that Windows wrote in the
        // buffer, which is still alive.
        unsafe {
            names.push(from_wide_ptr(entry.lpServiceName));
            names.push(from_wide_ptr(entry.lpDisplayName));
        }
    }
    Ok(Ok(names))
}

/// The nul-ended text at `text`, or nothing for a null pointer.
///
/// # Safety
///
/// `text` is null or points to nul-ended UTF-16 text.
unsafe fn from_wide_ptr(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the text ends with a nul, so each read up to it is inside it.
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: the `len` units before the nul were read above.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
}
