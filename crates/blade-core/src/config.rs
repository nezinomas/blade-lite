//! The user's saved settings, kept as TOML in `%APPDATA%`.
//!
//! The values are stored as the user picked them. They are checked against
//! the model when they are applied, not here.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::{fs, io};

use razer_proto::{PerfMode, Rgb};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The app's folder in `%APPDATA%`.
pub(crate) const APP_DIR: &str = "blade-lite";
/// The config file's name in the app's folder.
const FILE_NAME: &str = "config.toml";

/// Why the config could not be loaded or saved.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read or written.
    #[error("config file I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The file is not a valid config.
    #[error("config file is not valid: {0}")]
    Parse(#[from] toml::de::Error),
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
    /// [`ConfigError::Parse`] if it is not a valid config.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    /// Writes the config, making its folder if needed.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] if the folder or file can't be written.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
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
