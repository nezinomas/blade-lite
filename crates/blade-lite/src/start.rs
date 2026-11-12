//! A start that can't go on as it is (ticket 133): a config that does not
//! parse, and a controller not ready yet at login.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use blade_core::{Config, ConfigError};

use crate::report;

/// The config at `path`. If it is not a valid config (bad TOML, or a value
/// such as `cpu_boost = 300` or a preset name this version does not know),
/// `ask` gets a question for the user: on yes the file is kept aside as
/// `config.toml.bad` and the defaults are used; on no the start stops with
/// the error. Other errors (the file can't be read) stop the start.
///
/// # Errors
///
/// The config error, or a failed rename.
pub fn config_or_defaults(
    path: &Path,
    ask: impl FnOnce(&str) -> bool,
) -> Result<Config, Box<dyn Error>> {
    let error = match Config::load(path) {
        Err(error) if not_a_config(&error) => error,
        loaded => return Ok(loaded?),
    };
    let bad = free_bad_name(path).ok_or(error.to_string())?;
    let question = format!(
        "Blade Lite can't read its settings ({error}).\n\nStart with the default settings? The old file is kept as {}.",
        bad.display()
    );
    if !ask(&question) {
        return Err(error.into());
    }
    std::fs::rename(path, &bad)?;
    report(&format!(
        "the settings could not be read, so the defaults run; the old file is {}: {error}",
        bad.display()
    ));
    Ok(Config::default())
}

/// Whether the file is there but is no config: bad TOML or values, far too
/// big, or not UTF-8 (such as a file saved as UTF-16). Other errors, such
/// as access denied, are not the file's content.
fn not_a_config(error: &ConfigError) -> bool {
    match error {
        ConfigError::Parse(_) | ConfigError::TooBig => true,
        ConfigError::Io(io) => io.kind() == std::io::ErrorKind::InvalidData,
        _ => false,
    }
}

/// `config.toml.bad` beside the config, or `config.toml.bad.1` to `.9`
/// if those are taken, so an older kept file is never replaced. `None` if
/// all are taken.
fn free_bad_name(path: &Path) -> Option<PathBuf> {
    let first = path.with_file_name("config.toml.bad");
    std::iter::once(first.clone())
        .chain((1..=9).map(|n| path.with_file_name(format!("config.toml.bad.{n}"))))
        .find(|name| !name.exists())
}

/// Calls `attempt` up to `times` times, `pause` apart, until it works.
///
/// # Errors
///
/// The last attempt's error.
pub fn try_times<T, E>(
    times: u32,
    pause: Duration,
    mut attempt: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut left = times.max(1);
    loop {
        left -= 1;
        match attempt() {
            Err(_) if left > 0 => thread::sleep(pause),
            result => return result,
        }
    }
}

/// Waits `pause`, then calls `attempt`, `pause` apart, until it works.
/// Each error text is logged once, so a long wait leaves one line per
/// reason, also when two reasons take turns.
pub fn try_until_it_works<T, E: std::fmt::Display>(
    pause: Duration,
    mut attempt: impl FnMut() -> Result<T, E>,
) -> T {
    let mut said = std::collections::HashSet::new();
    loop {
        thread::sleep(pause);
        match attempt() {
            Ok(value) => return value,
            Err(error) => {
                let text = error.to_string();
                if said.insert(text.clone()) {
                    report(&format!("still trying: {text}"));
                }
            }
        }
    }
}
