//! The touchpad switch: the state comes from the registry, and Windows
//! toggles it on Win+Ctrl+F24.

use std::io;

use platform::{Key, KeyInjector, Registry};

/// Where Windows keeps the touchpad state, under `HKEY_CURRENT_USER`.
pub const TOUCHPAD_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad\Status";

/// The value in [`TOUCHPAD_KEY`]: 1 is on, 0 is off.
pub const TOUCHPAD_VALUE: &str = "Enabled";

/// The shortcut that Windows toggles the touchpad on.
const TOGGLE_KEYS: [Key; 3] = [Key::Win, Key::Ctrl, Key::F24];

/// Why the touchpad state could not be read or set.
#[derive(Debug, thiserror::Error)]
pub enum TouchpadError {
    /// The registry value could not be read.
    #[error("could not read the touchpad state: {0}")]
    Read(io::Error),
    /// The registry value is not 0 or 1.
    #[error("the touchpad state is not 0 or 1: {0}")]
    Unknown(u32),
    /// The shortcut could not be sent.
    #[error("could not send the touchpad shortcut: {0}")]
    Keys(io::Error),
}

/// Whether the touchpad is on, as Windows says.
///
/// # Errors
///
/// See [`TouchpadError`]. A value that can't be read is never guessed.
pub fn touchpad_on(registry: &impl Registry) -> Result<bool, TouchpadError> {
    match registry
        .read_user_dword(TOUCHPAD_KEY, TOUCHPAD_VALUE)
        .map_err(TouchpadError::Read)?
    {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(TouchpadError::Unknown(other)),
    }
}

/// Turns the touchpad on or off. The state is read first, so the shortcut
/// is sent only when it is needed and the switch can't get out of sync.
///
/// # Errors
///
/// See [`TouchpadError`]. Nothing is sent if the state can't be read.
pub fn set_touchpad(
    on: bool,
    registry: &impl Registry,
    keys: &mut impl KeyInjector,
) -> Result<(), TouchpadError> {
    if touchpad_on(registry)? == on {
        return Ok(());
    }
    keys.press(&TOGGLE_KEYS).map_err(TouchpadError::Keys)
}
