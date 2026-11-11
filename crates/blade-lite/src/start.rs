//! A start that can't go on as it is (ticket 133): a config that does not
//! parse, and a controller not ready yet at login.

use std::error::Error;
use std::path::Path;
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
        Err(error @ (ConfigError::Parse(_) | ConfigError::TooBig)) => error,
        loaded => return Ok(loaded?),
    };
    let bad = path.with_file_name("config.toml.bad");
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
