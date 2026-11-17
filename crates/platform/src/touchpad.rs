//! Windows' touchpad API (ticket 239): `SPI_GETTOUCHPADPARAMETERS`, on
//! Windows 11 24H2 and later. It reads Windows' own state, opens no device.

use std::io;

use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;

/// `SPI_GETTOUCHPADPARAMETERS` in `WinUser.h` (SDK 10.0.26100); not in
/// `windows-sys` 0.61.
const SPI_GETTOUCHPADPARAMETERS: u32 = 0xAE;

/// `TOUCHPAD_PARAMETERS_VERSION_1`, the first field of the struct.
const VERSION_1: u32 = 1;

/// `TOUCHPAD_PARAMETERS_V1`: 11 `UINT`s. The 4th holds the status bits.
type Parameters = [u32; 11];
const STATUS: usize = 3;
/// `touchpadEnabled` is the 4th bit (after `touchpadPresent`,
/// `legacyTouchpadPresent` and `externalMousePresent`).
const ENABLED_BIT: u32 = 1 << 3;

/// Whether Windows has the touchpad on, from its touchpad API.
///
/// # Errors
///
/// The Windows error if the call fails, as on Windows before 24H2.
pub fn touchpad_enabled() -> io::Result<bool> {
    let mut parameters: Parameters = [0; 11];
    parameters[0] = VERSION_1;
    let size = u32::try_from(size_of::<Parameters>()).map_err(io::Error::other)?;
    // SAFETY: the pointer is to a local array of the struct's size that
    // outlives the call, and its first field holds the version, as the
    // call needs.
    let done = unsafe {
        SystemParametersInfoW(
            SPI_GETTOUCHPADPARAMETERS,
            size,
            parameters.as_mut_ptr().cast(),
            0,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(parameters[STATUS] & ENABLED_BIT != 0)
}
