//! Autostart at login, through the user's Run key. The app then starts in
//! the tray and re-applies the saved settings.

use std::io;
use std::path::{Path, PathBuf};

use platform::UserStrings;

/// The key Windows runs at login, under `HKEY_CURRENT_USER`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The app's value in [`RUN_KEY`].
pub const RUN_VALUE: &str = "Blade Lite";

/// The argument that starts the app in the tray, with no window.
pub const TRAY_FLAG: &str = "--tray";

/// Why autostart could not be read or set.
#[derive(Debug, thiserror::Error)]
pub enum AutostartError {
    /// The exe path is relative, so Windows could not find it at login.
    #[error("the exe path is not absolute: {}", .0.display())]
    NotAbsolute(PathBuf),
    /// The Run key could not be read or written.
    #[error("could not use the Run key: {0}")]
    Registry(io::Error),
}

/// Whether the Run key starts this exe at login.
///
/// # Errors
///
/// See [`AutostartError`]. A missing value is off, not an error.
pub fn autostart_on(registry: &impl UserStrings, exe: &Path) -> Result<bool, AutostartError> {
    match registry.read_user_string(RUN_KEY, RUN_VALUE) {
        Ok(value) => Ok(value.eq_ignore_ascii_case(&command(exe)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AutostartError::Registry(error)),
    }
}

/// Turns autostart on or off for this exe.
///
/// # Errors
///
/// See [`AutostartError`]. Nothing is written for a relative path.
pub fn set_autostart(
    on: bool,
    registry: &mut impl UserStrings,
    exe: &Path,
) -> Result<(), AutostartError> {
    let written = if on {
        registry.write_user_string(RUN_KEY, RUN_VALUE, &command(exe)?)
    } else {
        registry.delete_user_value(RUN_KEY, RUN_VALUE)
    };
    written.map_err(AutostartError::Registry)
}

/// The quoted exe path and the tray flag, as Windows runs it.
fn command(exe: &Path) -> Result<String, AutostartError> {
    if !exe.is_absolute() {
        return Err(AutostartError::NotAbsolute(exe.to_owned()));
    }
    Ok(format!("\"{}\" {TRAY_FLAG}", exe.display()))
}
