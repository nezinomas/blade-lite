use std::fs;
use std::path::Path;

use blade_core::{Config, ConfigError, CustomSettings, Preset};

const FILE: &str = "config.toml";

/// Custom values that differ from the defaults, with a manual fan.
fn tweaked_custom() -> CustomSettings {
    CustomSettings {
        cpu_boost: 3,
        gpu_boost: 2,
        fan_rpm: Some(3000),
    }
}

#[test]
fn round_trip_keeps_custom_values_while_another_preset_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let config = Config {
        preset: Preset::Performance,
        custom: tweaked_custom(),
    };

    config.save(&path).unwrap();

    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn round_trip_keeps_auto_fan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let config = Config {
        preset: Preset::Custom,
        custom: CustomSettings {
            fan_rpm: None,
            ..tweaked_custom()
        },
    };

    config.save(&path).unwrap();

    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn missing_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();

    let config = Config::load(&dir.path().join(FILE)).unwrap();

    assert_eq!(config, Config::default());
}

#[test]
fn defaults_are_balanced_with_fans_on_auto() {
    let config = Config::default();

    assert_eq!(config.preset, Preset::Balanced);
    assert_eq!(config.custom.fan_rpm, None);
}

#[test]
fn missing_keys_get_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    fs::write(&path, "preset = \"silent\"\n").unwrap();

    let config = Config::load(&path).unwrap();

    assert_eq!(config.preset, Preset::Silent);
    assert_eq!(config.custom, CustomSettings::default());
}

#[test]
fn broken_file_is_an_error_not_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    fs::write(&path, "preset = \"turbo\"\n").unwrap();

    assert!(matches!(Config::load(&path), Err(ConfigError::Parse(_))));
}

#[test]
fn save_creates_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = Config::path_in(dir.path());

    Config::default().save(&path).unwrap();

    assert!(path.is_file());
}

#[test]
fn path_in_appdata_is_the_app_folder() {
    let path = Config::path_in(Path::new(r"C:\Users\me\AppData\Roaming"));

    assert_eq!(
        path,
        Path::new(r"C:\Users\me\AppData\Roaming\blade-lite\config.toml")
    );
}
