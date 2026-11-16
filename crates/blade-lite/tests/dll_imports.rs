//! The DLLs the exe imports are looked for only in System32 (ticket 224).
//! The exe is read, never started: a start sends the saved preset.

#[path = "support/pe.rs"]
mod pe;

use std::path::Path;

/// `LOAD_LIBRARY_SEARCH_SYSTEM32`.
const SYSTEM32_ONLY: usize = 0x800;

#[test]
fn the_exe_looks_for_its_imports_only_in_system32() {
    let exe = Path::new(env!("CARGO_BIN_EXE_blade-lite"));
    assert_eq!(pe::dependent_load_flags(exe), Some(SYSTEM32_ONLY));
}
