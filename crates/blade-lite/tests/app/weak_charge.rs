//! A weak charge, such as a USB-C monitor that powers the laptop, runs only
//! the battery presets, as Synapse does.

use blade_core::Preset;
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use tempfile::TempDir;
use ui::PerformanceCommand;

use crate::fakes::{FakeCharger, TestApp};
use crate::{performance_app, sent};

/// Whether every command of the preset was sent.
fn ran(app: &TestApp, mode: PerfMode) -> bool {
    let commands = mode.commands(&BLADE_18).unwrap();
    commands.iter().all(|command| sent(app).contains(command))
}

fn weak_performance_app() -> (TestApp, TempDir) {
    let (mut app, dir, _) = performance_app();
    app.machine_mut().controller.charger = FakeCharger::UsbC;
    (app, dir)
}

#[test]
fn a_start_on_a_weak_charge_runs_balanced_for_a_saved_performance() {
    let (mut app, _dir) = weak_performance_app();

    app.start().unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
    assert_eq!(app.config().preset, Preset::Performance);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_failed_charger_read_at_start_runs_balanced() {
    let (mut app, _dir) = weak_performance_app();
    app.machine_mut().controller.charger = FakeCharger::Fails;

    app.start().unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
}

#[test]
fn performance_is_refused_on_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.charger = FakeCharger::UsbC;
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
}

#[test]
fn the_performance_view_offers_only_the_battery_presets() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();

    let view = app.performance_view();

    let performance = view
        .presets
        .iter()
        .find(|option| option.preset == Preset::Performance)
        .unwrap();
    assert!(!performance.enabled);
}

#[test]
fn a_charger_change_on_a_poll_sends_the_preset_it_runs() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.machine_mut().controller.charger = FakeCharger::UsbC;
    assert!(app.follow_charger().unwrap());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());

    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::Razer;
    // One full power read is not enough: a reply may flicker in a swap.
    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
    assert!(app.follow_charger().unwrap());
    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_poll_with_the_same_charger_sends_nothing() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_charger_read_on_a_poll_keeps_the_last_charger() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::Fails;

    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn on_battery_the_charger_is_not_read() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.asked.clear();

    assert!(!app.follow_charger().unwrap());
    assert_eq!(app.machine().controller.asked, []);
}

#[test]
fn a_charger_plugged_in_on_a_weak_charge_keeps_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn the_power_key_does_not_step_to_performance_on_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.charger = FakeCharger::UsbC;
    app.start().unwrap();

    for _ in 0..4 {
        app.on_power_key().unwrap();
        assert_ne!(app.running_preset(), Preset::Performance);
    }
}

#[test]
fn a_weak_read_between_two_full_ones_keeps_the_weak_charge() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    for charger in [FakeCharger::Razer, FakeCharger::UsbC, FakeCharger::Razer] {
        app.machine_mut().controller.charger = charger;
        app.follow_charger().unwrap();
    }

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_weak_charge_coming_with_balanced_saved_is_still_reported() {
    let (mut app, _dir, _) = crate::app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    assert!(app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_charger_read_on_a_power_event_runs_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::Fails;

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_resume_on_a_weak_charge_runs_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
}

#[test]
fn a_performance_click_reads_the_charger_first() {
    let (mut app, _dir, _) = crate::app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    // Swapped to USB-C since the last poll.
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_refused_click_that_finds_a_weak_charger_sends_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    // Swapped to USB-C since the last poll, while Performance runs.
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Custom));

    assert!(result.is_err());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.running_preset(), Preset::Balanced);
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn a_refused_click_whose_charger_read_fails_sends_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::Fails;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_power_key_press_that_finds_a_weak_charger_steps_from_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    app.on_power_key().unwrap();

    assert!(!ran(&app, PerfMode::Performance));
    assert_ne!(app.running_preset(), Preset::Performance);
}

#[test]
fn the_power_key_reads_the_charger_first() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.charger = FakeCharger::UsbC;

    for _ in 0..4 {
        app.on_power_key().unwrap();
        assert_ne!(app.running_preset(), Preset::Performance);
    }
}
