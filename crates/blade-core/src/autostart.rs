//! Autostart at login, through the user's Run key. The app then starts in
//! the tray and re-applies the saved settings.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use platform::{UserBytes, UserStrings};

/// The key Windows runs at login, under `HKEY_CURRENT_USER`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Where Task Manager keeps its own on or off flag for each [`RUN_KEY`]
/// value. It can turn an app off here and leave the Run value alone.
pub const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The app's value in [`RUN_KEY`] and [`APPROVED_KEY`].
pub const RUN_VALUE: &str = "Blade Lite";

/// The argument that starts the app in the tray, with no window.
pub const TRAY_FLAG: &str = "--tray";

/// Whether the app was started with [`TRAY_FLAG`], as autostart does. Only
/// the exact flag counts, and an argument that is not text is no crash.
pub fn starts_in_tray(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter().any(|arg| arg == TRAY_FLAG)
}

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

/// The most characters of another copy's command that [`Autostart::Other`]
/// keeps, so a long Run value can't flood the log.
pub const SHOWN_COMMAND_CHARS: usize = 200;

/// What the Run key starts at login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Autostart {
    /// Nothing starts: no Run value, or Task Manager turned it off.
    Off,
    /// This exe starts in the tray.
    On,
    /// Another command starts, such as a copy in `target\debug`. It holds
    /// that command, cut to [`SHOWN_COMMAND_CHARS`], with no control
    /// characters.
    Other(String),
}

/// What the Run key starts at login, seen from this exe.
///
/// # Errors
///
/// See [`AutostartError`]. A missing value is off, not an error. A Run
/// value that Task Manager turned off is off too.
pub fn autostart(
    registry: &(impl UserStrings + UserBytes),
    exe: &Path,
) -> Result<Autostart, AutostartError> {
    let value = match registry.read_user_string(RUN_KEY, RUN_VALUE) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Autostart::Off),
        Err(error) => return Err(AutostartError::Registry(error)),
    };
    // A blank value starts nothing.
    if value.trim().is_empty() || !approved(registry)? {
        return Ok(Autostart::Off);
    }
    // Refuses a relative exe before any compare.
    command(exe)?;
    if starts_this(&value, exe) {
        return Ok(Autostart::On);
    }
    Ok(Autostart::Other(shown(&value)))
}

/// Whether the Run value `value` is this exe, quoted, then only
/// [`TRAY_FLAG`]. Spaces around them don't count.
fn starts_this(value: &str, exe: &Path) -> bool {
    let Some((path, args)) = value
        .trim()
        .strip_prefix('"')
        .and_then(|rest| rest.split_once('"'))
    else {
        return false;
    };
    args.trim() == TRAY_FLAG && same_file(Path::new(path), exe)
}

/// Whether two paths name the same file. Case doesn't count, as on
/// Windows; another way to the same file (`.`, a short name, a link)
/// counts too when both paths exist.
fn same_file(a: &Path, b: &Path) -> bool {
    let lower = |path: &Path| path.to_string_lossy().to_lowercase();
    if lower(a) == lower(b) {
        return true;
    }
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// `command` safe to show: no control characters, no characters that
/// hide or reorder text (zero-width, bidi, line separators), at most
/// [`SHOWN_COMMAND_CHARS`] long.
fn shown(command: &str) -> String {
    command
        .chars()
        .filter(|&c| !hides_text(c))
        .take(SHOWN_COMMAND_CHARS)
        .collect()
}

/// A control character, or a format character that can hide or reorder
/// the text around it.
fn hides_text(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// Whether Task Manager lets the Run value start. No flag means it does.
/// An odd first byte is off, and so is a flag with no bytes.
fn approved(registry: &impl UserBytes) -> Result<bool, AutostartError> {
    match registry.read_user_bytes(APPROVED_KEY, RUN_VALUE) {
        Ok(flag) => Ok(flag.first().is_some_and(|byte| byte & 1 == 0)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(AutostartError::Registry(error)),
    }
}

/// Turns autostart on or off for this exe.
///
/// # Errors
///
/// See [`AutostartError`]. Nothing is written for a relative path.
/// Both ways also clear Task Manager's flag: turning on so the tick is
/// kept, turning off so no flag is left behind. The Run value goes first,
/// so a failed write changes nothing.
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
    written
        .and_then(|()| registry.delete_user_value(APPROVED_KEY, RUN_VALUE))
        .map_err(AutostartError::Registry)
}

/// The quoted exe path and the tray flag, as Windows runs it.
fn command(exe: &Path) -> Result<String, AutostartError> {
    if !exe.is_absolute() {
        return Err(AutostartError::NotAbsolute(exe.to_owned()));
    }
    Ok(format!("\"{}\" {TRAY_FLAG}", exe.display()))
}
