//! The DLLs the exe imports are looked for only in System32 (ticket 224).

#[path = "../../blade-lite/tests/support/pe.rs"]
mod pe;

use std::path::Path;

/// `LOAD_LIBRARY_SEARCH_SYSTEM32`.
const SYSTEM32_ONLY: usize = 0x800;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_blade-dev"))
}

#[test]
fn the_exe_looks_for_its_imports_only_in_system32() {
    assert_eq!(pe::dependent_load_flags(exe()), Some(SYSTEM32_ONLY));
}
