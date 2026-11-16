//! Reads one field of a 64-bit Windows exe (ticket 224). Shared by the
//! `blade-lite` and `blade-dev` tests.

use std::path::Path;

/// `IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG`.
const LOAD_CONFIG: usize = 10;
/// Where `DependentLoadFlags` sits in `IMAGE_LOAD_CONFIG_DIRECTORY64`.
const DEPENDENT_LOAD_FLAGS: usize = 0x4E;

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// The file offset of a relative virtual address, from the section table.
fn file_offset(bytes: &[u8], sections: usize, count: usize, rva: usize) -> usize {
    (0..count)
        .map(|index| sections + index * 40)
        .find_map(|section| {
            let start = u32_at(bytes, section + 12);
            let size = u32_at(bytes, section + 8).max(u32_at(bytes, section + 16));
            (start..start + size)
                .contains(&rva)
                .then(|| rva - start + u32_at(bytes, section + 20))
        })
        .expect("the address is in a section")
}

/// Where the exe's `DependentLoadFlags` (two bytes) sit in the file.
/// `None` when the exe has no load config big enough to hold the field,
/// so Windows uses its default.
pub fn dependent_load_flags_offset(bytes: &[u8]) -> Option<usize> {
    let pe = u32_at(bytes, 0x3C);
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "not a PE file");
    let coff = pe + 4;
    let optional = coff + 20;
    assert_eq!(u16_at(bytes, optional), 0x20B, "not a 64-bit exe");
    let sections = optional + u16_at(bytes, coff + 16);
    let rva = u32_at(bytes, optional + 112 + LOAD_CONFIG * 8);
    if rva == 0 {
        return None;
    }
    let config = file_offset(bytes, sections, u16_at(bytes, coff + 2), rva);
    (u32_at(bytes, config) >= DEPENDENT_LOAD_FLAGS + 2).then_some(config + DEPENDENT_LOAD_FLAGS)
}

/// The exe's `DependentLoadFlags`: where Windows looks for the DLLs the
/// exe imports (`/DEPENDENTLOADFLAG`).
pub fn dependent_load_flags(exe: &Path) -> Option<usize> {
    let bytes = std::fs::read(exe).unwrap();
    dependent_load_flags_offset(&bytes).map(|at| u16_at(&bytes, at))
}
