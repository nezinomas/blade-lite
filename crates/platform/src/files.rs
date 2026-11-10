//! The Windows file dialogs, to export and import the settings.

use std::ffi::{OsString, c_void};
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use crate::wide;

/// Which dialog to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialog {
    /// Pick a new or old file to write.
    Save,
    /// Pick a file that exists, to read.
    Open,
}

/// The longest path the dialog can give back, in UTF-16 units.
const PATH_UNITS: usize = 4096;

/// The name the save dialog suggests.
const SUGGESTED_NAME: &str = "blade-lite-settings.toml";

/// Shows the dialog over the `owner` window (which it blocks while open)
/// and waits for the user. `None` if the user cancels.
///
/// # Errors
///
/// The dialog's error code if it fails, such as for a path too long.
pub fn pick_file(owner: Option<isize>, dialog: FileDialog) -> io::Result<Option<PathBuf>> {
    let filter = wide("Settings (*.toml)\0*.toml\0All files (*.*)\0*.*\0");
    let extension = wide("toml");
    let mut path = vec![0u16; PATH_UNITS];
    if dialog == FileDialog::Save {
        let name = SUGGESTED_NAME.encode_utf16();
        path.splice(..SUGGESTED_NAME.len(), name);
    }
    let flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | match dialog {
            FileDialog::Save => OFN_OVERWRITEPROMPT,
            FileDialog::Open => OFN_FILEMUSTEXIST,
        };
    let mut info = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>()).unwrap_or(u32::MAX),
        hwndOwner: owner.unwrap_or(0) as *mut c_void,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: u32::try_from(path.len()).unwrap_or(u32::MAX),
        lpstrDefExt: extension.as_ptr(),
        Flags: flags,
        ..OPENFILENAMEW::default()
    };
    // SAFETY: `info` and the buffers it points to (`filter`, `extension`,
    // `path`, each ending in a NUL) live until the call returns, and
    // `nMaxFile` is `path`'s length, so the dialog writes inside it. A bad
    // owner handle only makes the call fail.
    let picked = unsafe {
        match dialog {
            FileDialog::Save => GetSaveFileNameW(&raw mut info),
            FileDialog::Open => GetOpenFileNameW(&raw mut info),
        }
    };
    if picked != 0 {
        return Ok(Some(until_nul(&path)));
    }
    // SAFETY: no arguments; it reads the error the dialog just left.
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        code => Err(io::Error::other(format!("the file dialog failed, code {code:#X}"))),
    }
}

/// The path in `units`, up to its first NUL.
fn until_nul(units: &[u16]) -> PathBuf {
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
    PathBuf::from(OsString::from_wide(units.get(..end).unwrap_or(units)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_ends_at_the_first_nul() {
        let units: Vec<u16> = "C:/a.toml\0junk".encode_utf16().collect();

        assert_eq!(until_nul(&units), PathBuf::from("C:/a.toml"));
    }

    #[test]
    fn a_path_without_a_nul_is_taken_whole() {
        let units: Vec<u16> = "C:/a.toml".encode_utf16().collect();

        assert_eq!(until_nul(&units), PathBuf::from("C:/a.toml"));
    }
}
