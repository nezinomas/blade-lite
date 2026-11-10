//! The user's saved settings, kept as TOML in `%APPDATA%`.
//!
//! The values are stored as the user picked them. They are checked against
//! the model when they are applied, not here.

use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::{fs, io};

use razer_proto::{PerfMode, Rgb};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The app's folder in `%APPDATA%`.
pub(crate) const APP_DIR: &str = "blade-lite";
/// The config file's name in the app's folder.
const FILE_NAME: &str = "config.toml";
/// The largest file read as a config. A config is under 1 KiB; a bigger
/// file is not one, and is not read into memory.
const MAX_BYTES: u64 = 64 * 1024;

/// Why the config could not be loaded or saved.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read or written.
    #[error("config file I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The file is not a valid config.
    #[error("config file is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    /// The file is far too big to be a config.
    #[error("config file is too big to be a config")]
    TooBig,
    /// A picked file has no settings, or a key that is not a setting, so
    /// it is not a settings file.
    #[error("the file is not Blade Lite settings")]
    NotSettings,
    /// The config could not be turned into TOML.
    #[error("config could not be written as TOML: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// The performance preset to apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Silent,
    #[default]
    Balanced,
    Performance,
    Custom,
}

impl From<PerfMode> for Preset {
    fn from(mode: PerfMode) -> Self {
        match mode {
            PerfMode::Silent => Self::Silent,
            PerfMode::Balanced => Self::Balanced,
            PerfMode::Performance => Self::Performance,
        }
    }
}

impl Preset {
    /// The controller's mode for the preset, or `None` for Custom.
    pub(crate) fn perf_mode(self) -> Option<PerfMode> {
        match self {
            Self::Silent => Some(PerfMode::Silent),
            Self::Balanced => Some(PerfMode::Balanced),
            Self::Performance => Some(PerfMode::Performance),
            Self::Custom => None,
        }
    }
}

/// The Custom values. They are kept while another preset is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomSettings {
    pub cpu_boost: u8,
    pub gpu_boost: u8,
    /// The manual fan speed, or `None` for fans on auto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_rpm: Option<u16>,
}

impl Default for CustomSettings {
    fn default() -> Self {
        Self {
            cpu_boost: 1,
            gpu_boost: 1,
            fan_rpm: None,
        }
    }
}

/// The keyboard's lighting. The controller can't report it, so the app
/// keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lighting {
    /// In percent, 0 to 100.
    pub brightness: u8,
    /// Stored as `#RRGGBB`.
    #[serde(serialize_with = "hex", deserialize_with = "from_hex")]
    pub colour: Rgb,
    /// The vapor-chamber light at the keyboard's level, or off. A config
    /// saved before the switch existed keeps it on, as the laptop has it.
    #[serde(default = "on")]
    pub vapor_light: bool,
}

fn on() -> bool {
    true
}

/// The user's saved settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub preset: Preset,
    pub custom: CustomSettings,
    /// Drop the panel to 60 Hz on battery, and put the old rate back on AC.
    pub sixty_hz_on_battery: bool,
    /// `None` until the user sets it, so the keyboard is left as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lighting: Option<Lighting>,
    /// The lid logo on or off. `None` until the user sets it, so the logo
    /// is left as it is. The laptop turns it on again after a wake, so the
    /// app sends it at start and after each resume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lid_logo: Option<bool>,
}

impl Config {
    /// The config file's path inside the given `%APPDATA%` folder.
    #[must_use]
    pub fn path_in(appdata: &Path) -> PathBuf {
        appdata.join(APP_DIR).join(FILE_NAME)
    }

    /// Reads the config. A missing file gives the defaults.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] if the file exists but can't be read,
    /// [`ConfigError::Parse`] if it is not a valid config,
    /// [`ConfigError::TooBig`] if it is far too big to be one.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read_capped(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(ConfigError::Io(error)) if error.kind() == ErrorKind::NotFound => {
                Ok(Self::default())
            }
            Err(error) => Err(error),
        }
    }

    /// Reads a config the user picked, with the same checks as
    /// [`load`](Self::load), and more, since the user may pick the wrong
    /// file: a missing file, a file with no settings, or a key that is not
    /// a setting (such as a typo, which would quietly give a default) is
    /// an error.
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load), [`ConfigError::Io`] if the file is
    /// missing, and [`ConfigError::NotSettings`].
    pub fn import(path: &Path) -> Result<Self, ConfigError> {
        let text = read_capped(path)?;
        let table: toml::Table = toml::from_str(&text)?;
        if !has_a_setting(&table) || !only_settings(&table) {
            return Err(ConfigError::NotSettings);
        }
        Ok(toml::from_str(&text)?)
    }

    /// Writes the config, making its folder if needed.
    ///
    /// The text goes to a temp file next to the config first, which then
    /// replaces it. A save that fails part way (such as a full disk) keeps
    /// the old config whole.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] if the folder or temp file can't be written, or
    /// the old config can't be replaced (such as another program holding it
    /// open). The old config is then left as it was.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string(self)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = temp_path(path);
        write_then_replace(&temp, path, &text).inspect_err(|_| {
            // Best effort: the error that counts is the one returned.
            let _ = fs::remove_file(&temp);
        })?;
        Ok(())
    }
}

/// The config's top keys, and the keys of its two tables.
const KEYS: [&str; 5] = [
    "preset",
    "custom",
    "sixty_hz_on_battery",
    "lighting",
    "lid_logo",
];
const CUSTOM_KEYS: [&str; 3] = ["cpu_boost", "gpu_boost", "fan_rpm"];
const LIGHTING_KEYS: [&str; 3] = ["brightness", "colour", "vapor_light"];

/// Whether every key in `table` is a setting. The values are checked when
/// the config is read.
fn only_settings(table: &toml::Table) -> bool {
    let keys_in = |value: &toml::Value, keys: &[&str]| {
        value
            .as_table()
            .is_none_or(|inner| inner.keys().all(|key| keys.contains(&key.as_str())))
    };
    table.iter().all(|(key, value)| match key.as_str() {
        "custom" => keys_in(value, &CUSTOM_KEYS),
        "lighting" => keys_in(value, &LIGHTING_KEYS),
        key => KEYS.contains(&key),
    })
}

/// Whether `table` holds at least one value: an empty table is none.
fn has_a_setting(table: &toml::Table) -> bool {
    table
        .values()
        .any(|value| value.as_table().is_none_or(|inner| !inner.is_empty()))
}

/// Reads the file as text, refusing one far too big to be a config.
fn read_capped(path: &Path) -> Result<String, ConfigError> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_BYTES) {
        return Err(ConfigError::TooBig);
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(ErrorKind::InvalidData, error).into())
}

/// The temp file's path: the config's name with `.tmp` added.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Writes `text` to `temp`, flushes it to disk, then moves it over `path`.
fn write_then_replace(temp: &Path, path: &Path, text: &str) -> io::Result<()> {
    let mut file = fs::File::create(temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)
}

// serde hands the value by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn hex<S: Serializer>(colour: &Rgb, serializer: S) -> Result<S::Ok, S::Error> {
    let Rgb { red, green, blue } = colour;
    serializer.serialize_str(&format!("#{red:02X}{green:02X}{blue:02X}"))
}

fn from_hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Rgb, D::Error> {
    let text = String::deserialize(deserializer)?;
    text.parse()
        .map_err(|_| serde::de::Error::custom(format!("not a #RRGGBB colour: {text}")))
}
