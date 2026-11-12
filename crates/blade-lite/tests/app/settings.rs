//! Export, import and reset of the settings.

use std::fs;

use blade_core::{Config, CustomSettings, Lighting, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode, Rgb};

use crate::{WHITE, app, app_with, break_save, lighting_commands, logo, panel_hz, sent};

const BLUE: Rgb = Rgb::new(0x00, 0x78, 0xD4);

fn blue_lighting() -> Lighting {
    Lighting {
        brightness: 40,
        colour: BLUE,
        vapor_light: true,
    }
}

/// A config the user saved before, as a file next to `config.toml`.
fn exported(dir: &std::path::Path, config: &Config) -> std::path::PathBuf {
    let path = dir.join("exported.toml");
    config.save(&path).unwrap();
    path
}

#[test]
fn export_writes_the_settings_and_sends_nothing() {
    let saved = Config {
        preset: Preset::Silent,
        lighting: Some(blue_lighting()),
        ..Config::default()
    };
    let (app, dir, _) = app_with(saved);
    let path = dir.path().join("out.toml");

    app.export_settings(&path).unwrap();

    assert_eq!(Config::import(&path).unwrap(), saved);
    assert_eq!(sent(&app), []);
}

#[test]
fn import_sends_the_file_as_a_start_does_then_saves_it() {
    let (mut app, dir, path) = app();
    let file = Config {
        preset: Preset::Silent,
        lid_logo: Some(false),
        lighting: Some(blue_lighting()),
        ..Config::default()
    };
    let picked = exported(dir.path(), &file);

    app.import_settings(&picked).unwrap();

    let mut expected = PerfMode::Silent.commands(&BLADE_18).unwrap();
    expected.extend(logo().commands(false));
    expected.extend(lighting_commands(40, BLUE));
    assert_eq!(sent(&app), expected);
    assert_eq!(*app.config(), file);
    assert_eq!(Config::load(&path).unwrap(), file);
    assert_eq!(app.logo_on(), Some(false));
}

#[test]
fn import_of_a_broken_file_changes_nothing() {
    let (mut app, dir, path) = app();
    let picked = dir.path().join("broken.toml");
    fs::write(&picked, "preset = \"turbo\"\n").unwrap();

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(sent(&app), []);
    assert_eq!(*app.config(), Config::default());
    assert!(!path.exists());
}

#[test]
fn import_of_a_missing_file_changes_nothing() {
    let (mut app, dir, path) = app();

    assert!(app.import_settings(&dir.path().join("gone.toml")).is_err());

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn import_of_a_value_the_model_refuses_changes_nothing() {
    let (mut app, dir, path) = app();
    let picked = dir.path().join("too-fast.toml");
    fs::write(
        &picked,
        "preset = \"custom\"\n[custom]\ncpu_boost = 99\ngpu_boost = 1\n",
    )
    .unwrap();

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(sent(&app), []);
    assert_eq!(*app.config(), Config::default());
    assert!(!path.exists());
}

#[test]
fn import_on_battery_checks_the_saved_custom_too() {
    let (mut app, dir, path) = app();
    app.take_on_battery(Some(true));
    let picked = dir.path().join("too-fast.toml");
    fs::write(
        &picked,
        "preset = \"custom\"\n[custom]\ncpu_boost = 99\ngpu_boost = 1\n",
    )
    .unwrap();

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn import_on_battery_runs_the_battery_preset_and_keeps_the_saved_one() {
    let (mut app, dir, _) = app();
    app.take_on_battery(Some(true));
    let file = Config {
        preset: Preset::Performance,
        ..Config::default()
    };
    let picked = exported(dir.path(), &file);

    app.import_settings(&picked).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn import_without_lighting_or_logo_keeps_the_current_ones() {
    let (mut app, dir, _) = app_with(Config {
        lighting: Some(blue_lighting()),
        lid_logo: Some(false),
        ..Config::default()
    });
    let picked = exported(
        dir.path(),
        &Config {
            preset: Preset::Silent,
            ..Config::default()
        },
    );

    app.import_settings(&picked).unwrap();

    assert_eq!(app.config().lighting, Some(blue_lighting()));
    assert_eq!(app.config().lid_logo, Some(false));
    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn import_the_controller_refuses_keeps_the_old_settings() {
    let (mut app, dir, path) = app();
    let picked = exported(
        dir.path(),
        &Config {
            preset: Preset::Silent,
            ..Config::default()
        },
    );
    app.machine_mut().controller.inner.fails = true;

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(*app.config(), Config::default());
    assert!(!path.exists());
}

fn custom_with_fan(fan_rpm: u16) -> Config {
    Config {
        preset: Preset::Custom,
        custom: CustomSettings {
            fan_rpm: Some(fan_rpm),
            ..CustomSettings::default()
        },
        ..Config::default()
    }
}

/// Starts on AC with `shown`, then imports `file` with blue lighting, with
/// only the preset run taken. Returns the app with nothing sent yet, and
/// the import's error text.
fn import_with_a_lighting_error(shown: Config, file: Config) -> (crate::fakes::TestApp, String) {
    let (mut app, dir, _) = app_with(shown);
    app.take_on_battery(Some(false));
    let picked = exported(
        dir.path(),
        &Config {
            lighting: Some(blue_lighting()),
            ..file
        },
    );
    app.machine_mut().controller.inner.ok_runs = Some(1);
    let error = app.import_settings(&picked).unwrap_err().to_string();
    app.machine_mut().controller.inner.ok_runs = None;
    app.machine_mut().controller.inner.sent.clear();
    assert_eq!(*app.config(), shown);
    (app, error)
}

/// The preset commands `config` sends.
fn preset_sent(config: &Config) -> Vec<razer_proto::Command> {
    let mut whole = crate::fakes::FakeController::default();
    blade_core::apply_performance(config, &BLADE_18, &mut whole).unwrap();
    whole.sent
}

#[test]
fn import_of_a_new_preset_whose_lighting_fails_puts_the_old_preset_back() {
    let file = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    let (mut app, error) = import_with_a_lighting_error(Config::default(), file);

    assert!(
        error.contains("the laptop may run part of the change"),
        "{error}"
    );
    assert!(app.preset_unsure());
    assert!(app.resend_preset().unwrap());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn import_of_new_custom_values_whose_lighting_fails_puts_the_old_ones_back() {
    let (mut app, _) = import_with_a_lighting_error(custom_with_fan(3000), custom_with_fan(4000));

    assert!(app.preset_unsure());
    assert!(app.resend_preset().unwrap());
    assert_eq!(sent(&app), preset_sent(&custom_with_fan(3000)));
}

#[test]
fn import_of_the_same_preset_whose_lighting_fails_leaves_the_preset_sure() {
    let file = Config {
        custom: custom_with_fan(4000).custom,
        ..Config::default()
    };
    // Only the unused Custom values differ.
    let (mut app, error) = import_with_a_lighting_error(Config::default(), file);

    assert!(error.contains("the preset was sent"), "{error}");
    assert!(!app.preset_unsure());
    assert!(!app.resend_preset().unwrap());
}

#[test]
fn reset_whose_lighting_fails_puts_the_old_preset_back() {
    let shown = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    let (mut app, _dir, _) = app_with(shown);
    app.take_on_battery(Some(false));
    app.machine_mut().controller.inner.ok_runs = Some(1);

    assert!(app.reset_settings().is_err());
    app.machine_mut().controller.inner.ok_runs = None;
    app.machine_mut().controller.inner.sent.clear();

    assert_eq!(*app.config(), shown);
    assert!(app.resend_preset().unwrap());
    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
}

#[test]
fn import_sent_but_not_saved_is_kept() {
    let (mut app, dir, path) = app();
    break_save(&path);
    let file = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    let picked = exported(dir.path(), &file);

    let error = app.import_settings(&picked).unwrap_err();

    assert!(error.was_sent());
    assert_eq!(*app.config(), file);
}

#[test]
fn import_with_60_hz_on_battery_drops_the_panel_on_battery() {
    let (mut app, dir, _) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    let picked = exported(
        dir.path(),
        &Config {
            sixty_hz_on_battery: true,
            ..Config::default()
        },
    );

    app.import_settings(&picked).unwrap();

    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn reset_sends_and_saves_the_defaults() {
    let (mut app, _dir, path) = app_with(Config {
        preset: Preset::Performance,
        custom: CustomSettings {
            cpu_boost: 3,
            gpu_boost: 2,
            fan_rpm: Some(3000),
        },
        sixty_hz_on_battery: true,
        lighting: Some(blue_lighting()),
        lid_logo: Some(false),
    });

    app.reset_settings().unwrap();

    let defaults = Config {
        lighting: Some(Lighting {
            brightness: 100,
            colour: WHITE,
            vapor_light: true,
        }),
        lid_logo: Some(true),
        ..Config::default()
    };
    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(logo().commands(true));
    expected.extend(lighting_commands(100, WHITE));
    assert_eq!(sent(&app), expected);
    assert_eq!(*app.config(), defaults);
    assert_eq!(Config::load(&path).unwrap(), defaults);
}

#[test]
fn import_checks_the_custom_values_while_another_preset_is_picked() {
    let (mut app, dir, path) = app();
    let picked = dir.path().join("too-fast.toml");
    fs::write(
        &picked,
        "preset = \"balanced\"\n[custom]\ncpu_boost = 99\ngpu_boost = 1\n",
    )
    .unwrap();

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(sent(&app), []);
    assert_eq!(*app.config(), Config::default());
    assert!(!path.exists());
}

#[test]
fn import_with_60_hz_on_battery_that_windows_refuses_keeps_the_switch_off() {
    let (mut app, dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().displays.1 = 1;
    let picked = exported(
        dir.path(),
        &Config {
            preset: Preset::Silent,
            sixty_hz_on_battery: true,
            ..Config::default()
        },
    );

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(panel_hz(&app), 240);
    assert!(!app.config().sixty_hz_on_battery);
    assert!(!Config::load(&path).unwrap().sixty_hz_on_battery);
    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn reset_on_battery_puts_the_rate_back() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(panel_hz(&app), 60);

    app.reset_settings().unwrap();

    assert_eq!(panel_hz(&app), 240);
}
