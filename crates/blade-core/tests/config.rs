use std::fs;
use std::path::Path;

use blade_core::{Config, ConfigError, CustomSettings, Lighting, Preset};
use razer_proto::Rgb;

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
        sixty_hz_on_battery: true,
        lighting: Some(Lighting {
            brightness: 40,
            colour: Rgb::new(0x00, 0x78, 0xD4),
            vapor_light: true,
        }),
        lid_logo: Some(false),
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
        ..Config::default()
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
fn sixty_hz_on_battery_is_off_by_default() {
    assert!(!Config::default().sixty_hz_on_battery);
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

/// The temp file `save` writes first, next to the config.
const TEMP: &str = "config.toml.tmp";

/// The file names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_save_that_fails_keeps_the_old_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let old = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    old.save(&path).unwrap();
    // A folder where the temp file goes makes the write fail.
    fs::create_dir(dir.path().join(TEMP)).unwrap();

    let new = Config {
        preset: Preset::Performance,
        ..Config::default()
    };

    assert!(matches!(new.save(&path), Err(ConfigError::Io(_))));
    assert_eq!(Config::load(&path).unwrap(), old);
}

#[test]
fn a_save_replaces_the_old_config_and_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    Config::default().save(&path).unwrap();
    let new = Config {
        preset: Preset::Performance,
        ..Config::default()
    };

    new.save(&path).unwrap();

    assert_eq!(Config::load(&path).unwrap(), new);
    assert_eq!(names_in(dir.path()), [FILE]);
}

#[test]
fn a_save_that_cannot_replace_the_config_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    // A folder in the config's place makes the rename fail.
    fs::create_dir(&path).unwrap();

    assert!(matches!(
        Config::default().save(&path),
        Err(ConfigError::Io(_))
    ));
    assert_eq!(names_in(dir.path()), [FILE]);
}

#[test]
fn path_in_appdata_is_the_app_folder() {
    let path = Config::path_in(Path::new(r"C:\Users\me\AppData\Roaming"));

    assert_eq!(
        path,
        Path::new(r"C:\Users\me\AppData\Roaming\blade-lite\config.toml")
    );
}

#[test]
fn lighting_is_unset_by_default_so_nothing_is_sent_for_it() {
    assert_eq!(Config::default().lighting, None);
}

#[test]
fn lighting_colour_is_stored_as_hex() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let config = Config {
        lighting: Some(Lighting {
            brightness: 75,
            colour: Rgb::new(0xFF, 0x8C, 0x00),
            vapor_light: true,
        }),
        ..Config::default()
    };

    config.save(&path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("colour = \"#FF8C00\""), "{text}");
    assert!(text.contains("brightness = 75"), "{text}");
}

#[test]
fn a_colour_that_is_not_hex_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    fs::write(
        &path,
        "[lighting]
brightness = 50
colour = \"red\"
",
    )
    .unwrap();

    assert!(matches!(Config::load(&path), Err(ConfigError::Parse(_))));
}

#[test]
fn a_saved_lighting_without_the_vapor_light_keeps_it_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    fs::write(
        &path,
        "[lighting]
brightness = 50
colour = \"#FFFFFF\"
",
    )
    .unwrap();

    let lighting = Config::load(&path).unwrap().lighting.unwrap();

    assert!(lighting.vapor_light);
}

#[test]
fn the_vapor_light_off_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let config = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour: Rgb::new(0xFF, 0xFF, 0xFF),
            vapor_light: false,
        }),
        ..Config::default()
    };

    config.save(&path).unwrap();

    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn the_lid_logo_is_unset_by_default_so_nothing_is_sent_for_it() {
    assert_eq!(Config::default().lid_logo, None);
}

#[test]
fn the_lid_logo_off_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    let config = Config {
        lid_logo: Some(false),
        ..Config::default()
    };

    config.save(&path).unwrap();

    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn import_reads_a_saved_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exported.toml");
    let config = Config {
        preset: Preset::Silent,
        custom: tweaked_custom(),
        ..Config::default()
    };
    config.save(&path).unwrap();

    assert_eq!(Config::import(&path).unwrap(), config);
}

#[test]
fn import_of_a_missing_file_is_an_error_not_the_defaults() {
    let dir = tempfile::tempdir().unwrap();

    assert!(matches!(
        Config::import(&dir.path().join("gone.toml")),
        Err(ConfigError::Io(_))
    ));
}

#[test]
fn import_of_a_broken_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.toml");
    fs::write(&path, "preset = \"turbo\"\n").unwrap();

    assert!(matches!(Config::import(&path), Err(ConfigError::Parse(_))));
}

#[test]
fn a_file_too_big_for_a_config_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    // Valid TOML, but far bigger than any config.
    let comment = format!("# {}\n", "x".repeat(70_000));
    fs::write(&path, comment + "preset = \"silent\"\n").unwrap();

    assert!(matches!(Config::load(&path), Err(ConfigError::TooBig)));
    assert!(matches!(Config::import(&path), Err(ConfigError::TooBig)));
}
