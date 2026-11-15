//! Where Windows looks for a DLL loaded by name (ticket 223).

use std::io;

use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};

/// Makes this process look for a DLL loaded by name only in the System32
/// folder (after the DLLs already loaded and Windows' known DLLs), never in
/// the app's folder, the current folder or `PATH`. Microsoft's documentation
/// says it also covers the DLLs that a DLL loads, such as those `nvml.dll`
/// loads (not tested). Call it first in
/// `main`: it can't move a DLL that is already loaded, such as those the
/// exe itself imports.
///
/// # Errors
///
/// The Windows error if the search path could not be set.
pub fn limit_dll_search() -> io::Result<()> {
    // SAFETY: the call takes a flag value only and touches no memory of ours.
    if unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
