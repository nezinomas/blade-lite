//! The power-mode key.

use blade_core::{Config, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, ModelProfile, PerfMode};

use crate::{app, app_on, app_with, performance_app, sent};

#[test]
fn the_power_key_sends_the_next_preset_and_saves_it() {
    let (mut app, _dir, path) = app();

    app.on_power_key().unwrap();

    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
    assert_eq!(Config::load(&path).unwrap().preset, Preset::Performance);
    assert_eq!(
        app.performance_view()
            .presets
            .iter()
            .find(|o| o.selected)
            .unwrap()
            .preset,
        Preset::Performance
    );
}

#[test]
fn the_power_key_wraps_from_performance_to_silent() {
    let (mut app, _dir, _) = performance_app();

    app.on_power_key().unwrap();

    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn the_power_key_steps_from_custom_to_silent() {
    let (mut app, _dir, _) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
}

#[test]
fn on_battery_the_power_key_steps_from_the_running_preset() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.sent.clear();

    // Balanced runs in place of Performance, so Silent is next.
    app.on_power_key().unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn a_power_key_step_the_controller_refuses_is_neither_kept_nor_saved() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.inner.fails = true;

    assert!(app.on_power_key().is_err());
    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(!path.exists());
}

static NO_POWER_KEY: ModelProfile = ModelProfile {
    power_key: None,
    ..BLADE_18
};

static ONE_PRESET: ModelProfile = ModelProfile {
    presets: &[PerfMode::Balanced],
    ..BLADE_18
};

#[test]
fn a_model_without_the_power_key_ignores_it() {
    let (mut app, _dir, path) = app_on(&NO_POWER_KEY, Config::default());

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_power_key_with_only_the_running_preset_sends_nothing() {
    let (mut app, _dir, path) = app_on(&ONE_PRESET, Config::default());

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}
