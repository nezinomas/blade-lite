//! The DLLs the exe imports are looked for only in System32 (ticket 224).

#![allow(unsafe_code)]

#[path = "../../blade-lite/tests/support/pe.rs"]
mod pe;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};

/// `LOAD_LIBRARY_SEARCH_SYSTEM32`.
const SYSTEM32_ONLY: usize = 0x800;
/// `STATUS_ENTRYPOINT_NOT_FOUND`: an imported function is missing.
const ENTRY_POINT_NOT_FOUND: u32 = 0xC000_0139;
/// Not a command, so the exe only prints its usage: no lock, no device.
const NOT_A_COMMAND: &str = "--dll-import-test";

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_blade-dev"))
}

#[test]
fn the_exe_looks_for_its_imports_only_in_system32() {
    assert_eq!(pe::dependent_load_flags(exe()), Some(SYSTEM32_ONLY));
}

/// Starts a copy of the exe, with `DependentLoadFlags` set to `flags`,
/// beside a fake `hid.dll` (one it imports, not a known DLL). The fake is a
/// copy of `version.dll`, which lacks the HID functions, so Windows stops
/// the start if it takes the fake.
fn start_beside_a_fake_hid_dll(flags: usize) -> Output {
    let flags = u16::try_from(flags).unwrap();
    let dir = exe().with_file_name(format!(
        "dll_imports_plant_{flags:x}_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let mut bytes = fs::read(exe()).unwrap();
    let at = pe::dependent_load_flags_offset(&bytes).unwrap();
    bytes[at..at + 2].copy_from_slice(&flags.to_le_bytes());
    let copy = dir.join("blade-dev.exe");
    fs::write(&copy, bytes).unwrap();
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    fs::copy(system.join("version.dll"), dir.join("hid.dll")).unwrap();

    let output = Command::new(&copy).arg(NOT_A_COMMAND).output().unwrap();
    // A virus scan may still hold the new files; a folder left in `target`
    // does no harm.
    let _ = fs::remove_dir_all(&dir);
    output
}

#[test]
fn a_dll_of_the_same_name_beside_the_exe_is_not_loaded() {
    // A failed start shows a box and waits for a click, unless the error
    // mode says not to; the copies take this process's mode.
    // SAFETY: the call takes a flag value only and touches no memory of ours.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };

    let without = start_beside_a_fake_hid_dll(0);
    assert_eq!(
        without.status.code().map(i32::cast_unsigned),
        Some(ENTRY_POINT_NOT_FOUND),
        "without the flag the fake must be taken, or this test shows nothing"
    );

    let with = start_beside_a_fake_hid_dll(SYSTEM32_ONLY);
    let stderr = String::from_utf8_lossy(&with.stderr);
    assert_eq!(with.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("usage: blade-dev"), "{stderr}");
}
