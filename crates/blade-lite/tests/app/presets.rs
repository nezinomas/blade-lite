//! Presets and custom values, and changes sent but not saved.

use blade_core::{Config, Preset};
use blade_lite::took_effect;
use platform::PowerEvent;
use razer_proto::{BLADE_18, Brightness, PerfMode};
use ui::{KeyboardCommand, PerformanceCommand};

use crate::{app, app_with, break_save, sent};

#[test]
fn start_applies_the_saved_settings() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn picking_a_preset_sends_it_saves_it_and_shows_it() {
    let (mut app, _dir, path) = app();

    app.on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(Config::load(&path).unwrap().preset, Preset::Silent);
    let view = app.performance_view();
    let selected = view.presets.iter().find(|option| option.selected).unwrap();
    assert_eq!(selected.preset, Preset::Silent);
}

#[test]
fn a_preset_the_controller_refuses_is_neither_kept_nor_saved() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.inner.fails = true;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Silent));

    let error = result.unwrap_err();
    assert!(!error.was_sent());
    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(!path.exists());
}

#[test]
fn a_preset_sent_but_not_saved_is_kept_and_shown() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    let error = app
        .on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap_err();

    assert!(error.was_sent());
    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(app.running_preset(), Preset::Silent);
    let view = app.performance_view();
    let selected = view.presets.iter().find(|option| option.selected).unwrap();
    assert_eq!(selected.preset, Preset::Silent);
}

#[test]
fn a_preset_not_saved_is_sent_again_at_resume_and_back_on_ac() {
    let (mut app, _dir, path) = app();
    break_save(&path);
    let _ = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));
    app.machine_mut().controller.inner.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();
    assert!(sent(&app).starts_with(&PerfMode::Performance.commands(&BLADE_18).unwrap()));

    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.on_power(PowerEvent::OnAc).unwrap();
    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn the_popup_follows_a_change_the_laptop_took() {
    let (mut app, _dir, path) = app();
    assert!(took_effect(&Ok(())));
    app.machine_mut().controller.inner.fails = true;
    let refused = app.on_performance(PerformanceCommand::SetPreset(Preset::Silent));
    assert!(!took_effect(&refused));

    app.machine_mut().controller.inner.fails = false;
    break_save(&path);
    let not_saved = app.on_performance(PerformanceCommand::SetPreset(Preset::Silent));

    assert!(took_effect(&not_saved));
}

#[test]
fn a_power_key_preset_not_saved_is_kept_for_the_next_press() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    assert!(app.on_power_key().unwrap_err().was_sent());
    let first = app.running_preset();
    assert_ne!(first, Preset::Balanced);
    assert!(app.on_power_key().unwrap_err().was_sent());

    assert_ne!(app.running_preset(), first);
}

#[test]
fn a_custom_value_sent_but_not_saved_is_kept() {
    let (mut app, _dir, path) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    break_save(&path);

    assert!(
        app.on_performance(PerformanceCommand::SetCpuBoost(3))
            .unwrap_err()
            .was_sent()
    );

    assert_eq!(app.config().custom.cpu_boost, 3);
}

#[test]
fn lighting_sent_but_not_saved_is_kept() {
    let (mut app, _dir, path) = app();
    break_save(&path);
    let half = Brightness::new(50).unwrap();

    let error = app
        .on_keyboard(KeyboardCommand::SetBrightness(half))
        .unwrap_err();

    assert!(error.was_sent());
    assert_eq!(app.keyboard_view().unwrap().brightness, 50);
}

#[test]
fn the_error_says_the_change_was_sent_but_not_saved() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    let error = app
        .on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap_err();

    assert!(error.to_string().starts_with("sent, but not saved: "));
}

#[test]
fn a_custom_slider_sends_the_custom_values() {
    let (mut app, _dir, path) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });

    app.on_performance(PerformanceCommand::SetCpuBoost(3))
        .unwrap();

    assert_eq!(Config::load(&path).unwrap().custom.cpu_boost, 3);
    // The four zones, then the CPU boost first among the boosts.
    assert_eq!(sent(&app)[4].args, [1, 1, 3]);
}
