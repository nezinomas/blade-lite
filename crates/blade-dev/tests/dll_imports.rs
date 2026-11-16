//! The DLLs the exe imports are looked for only in System32 (ticket 224).

#[path = "../../blade-lite/tests/support/pe.rs"]
mod pe;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `LOAD_LIBRARY_SEARCH_SYSTEM32`.
const SYSTEM32_ONLY: usize = 0x800;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_blade-dev"))
}

#[test]
fn the_exe_looks_for_its_imports_only_in_system32() {
    assert_eq!(pe::dependent_load_flags(exe()), Some(SYSTEM32_ONLY));
}

/// Whether a copy of the exe, with `DependentLoadFlags` set to `flags`,
/// reaches `main` beside a fake `hid.dll` (one it imports, not a known
/// DLL). The fake is a copy of `version.dll`, which lacks the HID
/// functions, so Windows stops the start if it takes the fake. Started with
/// no arguments, the exe only prints its usage: no lock, no device.
fn reaches_main_beside_a_fake_hid_dll(flags: usize) -> bool {
    let flags = u16::try_from(flags).unwrap();
    let dir = exe().with_file_name(format!("dll_imports_plant_{flags:x}"));
    fs::create_dir_all(&dir).unwrap();
    let mut bytes = fs::read(exe()).unwrap();
    let at = pe::dependent_load_flags_offset(&bytes).unwrap();
    bytes[at..at + 2].copy_from_slice(&flags.to_le_bytes());
    let copy = dir.join("blade-dev.exe");
    fs::write(&copy, bytes).unwrap();
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    fs::copy(system.join("version.dll"), dir.join("hid.dll")).unwrap();

    let output = Command::new(&copy).output().unwrap();
    fs::remove_dir_all(&dir).unwrap();
    String::from_utf8_lossy(&output.stderr).contains("usage: blade-dev")
}

#[test]
fn a_dll_of_the_same_name_beside_the_exe_is_not_loaded() {
    assert!(
        !reaches_main_beside_a_fake_hid_dll(0),
        "without the flag the fake is not taken, so this test shows nothing"
    );
    assert!(reaches_main_beside_a_fake_hid_dll(SYSTEM32_ONLY));
}
