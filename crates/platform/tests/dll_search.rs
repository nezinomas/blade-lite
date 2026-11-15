//! The DLL search path (ticket 223). Its own test program: the change is
//! for the whole process and can't be undone.

#![allow(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_AS_DATAFILE, LoadLibraryExW};

/// A copy of a System32 DLL under a name Windows has never seen.
const PROBE: &str = "blade_lite_dll_search_probe.dll";

/// Whether a DLL loaded by `name` is found. Loaded as data: its code
/// never runs.
fn found(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` ends with a nul and outlives the call.
    let module = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE,
        )
    };
    if module.is_null() {
        return false;
    }
    // SAFETY: `module` was just loaded and is not used after.
    unsafe { FreeLibrary(module) };
    true
}

#[test]
fn a_dll_loaded_by_name_is_found_in_system32_and_not_in_the_app_folder() {
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let probe = std::env::current_exe().unwrap().with_file_name(PROBE);
    fs::copy(system.join("version.dll"), &probe).unwrap();
    let found_before = found(PROBE);

    let limited = platform::limit_dll_search();
    let found_after = found(PROBE);
    let system_found = found("version.dll");
    fs::remove_file(&probe).unwrap();

    limited.unwrap();
    assert!(found_before, "the app folder is not searched by default");
    assert!(!found_after, "the app folder is searched after the limit");
    assert!(system_found, "System32 is not searched after the limit");
}
