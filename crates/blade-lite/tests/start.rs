//! A start that can't go on: a config that does not parse, and a
//! controller not ready yet at login.

use std::io;
use std::time::Duration;

use blade_core::{Config, Preset};
use blade_lite::{config_or_defaults, try_times};

fn dir_with(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn a_good_config_loads_and_asks_nothing() {
    let (_dir, path) = dir_with("preset = \"silent\"\n");
    let config = config_or_defaults(&path, |_| panic!("asked")).unwrap();
    assert_eq!(config.preset, Preset::Silent);
}

#[test]
fn a_bad_config_kept_aside_gives_the_defaults_when_the_user_agrees() {
    let (_dir, path) = dir_with("[custom]\ncpu_boost = 300\n");
    let mut asked = String::new();

    let config = config_or_defaults(&path, |text| {
        asked = text.to_owned();
        true
    })
    .unwrap();

    assert_eq!(config, Config::default());
    assert!(asked.contains("config.toml.bad"), "{asked}");
    assert!(!path.exists());
    let bad = path.with_file_name("config.toml.bad");
    assert_eq!(
        std::fs::read_to_string(bad).unwrap(),
        "[custom]\ncpu_boost = 300\n"
    );
}

#[test]
fn a_bad_config_the_user_keeps_stops_the_start_and_is_left_alone() {
    let (_dir, path) = dir_with("not = [toml");

    assert!(config_or_defaults(&path, |_| false).is_err());

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not = [toml");
}

#[test]
fn try_times_stops_at_the_first_that_works() {
    let mut tries = 0;
    let got = try_times(5, Duration::ZERO, || {
        tries += 1;
        if tries < 3 {
            Err(io::Error::other("not yet"))
        } else {
            Ok(tries)
        }
    });
    assert_eq!(got.unwrap(), 3);
}

#[test]
fn try_times_gives_the_last_error_after_all_tries() {
    let mut tries = 0;
    let got: io::Result<()> = try_times(5, Duration::ZERO, || {
        tries += 1;
        Err(io::Error::other(format!("try {tries}")))
    });
    assert_eq!(got.unwrap_err().to_string(), "try 5");
    assert_eq!(tries, 5);
}

#[test]
fn a_config_saved_as_utf16_is_offered_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let utf16: Vec<u8> = "preset = \"silent\"\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    std::fs::write(&path, [&[0xFF, 0xFE][..], &utf16].concat()).unwrap();

    let config = config_or_defaults(&path, |_| true).unwrap();

    assert_eq!(config, Config::default());
}

#[test]
fn an_older_kept_file_is_never_replaced() {
    let (_dir, path) = dir_with("not = [toml");
    let first = path.with_file_name("config.toml.bad");
    std::fs::write(&first, "older").unwrap();

    config_or_defaults(&path, |_| true).unwrap();

    assert_eq!(std::fs::read_to_string(first).unwrap(), "older");
    let second = path.with_file_name("config.toml.bad.1");
    assert_eq!(std::fs::read_to_string(second).unwrap(), "not = [toml");
}

#[test]
fn a_config_that_cant_be_read_stops_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    // A folder where the file should be: it can't be read as a file.
    let path = dir.path().join("config.toml");
    std::fs::create_dir(&path).unwrap();

    assert!(config_or_defaults(&path, |_| panic!("asked")).is_err());
}

#[test]
fn try_times_zero_still_tries_once() {
    let mut tries = 0;
    let _: io::Result<()> = try_times(0, Duration::ZERO, || {
        tries += 1;
        Err(io::Error::other("no"))
    });
    assert_eq!(tries, 1);
}
