//! Presets on battery, and the charge limit.

use blade_core::Preset;
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use ui::PerformanceCommand;

use crate::{
    LIMIT_OFF, LIMIT_ON, SET_CHARGE_LIMIT, app, charge_limit, charge_limit_on, performance_app,
    sent,
};

#[test]
fn on_battery_performance_runs_as_balanced_and_comes_back_on_ac() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Performance);

    app.on_power(PowerEvent::OnAc).unwrap();
    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(PerfMode::Performance.commands(&BLADE_18).unwrap());
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_power_event_that_keeps_the_running_preset_sends_nothing() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn performance_is_refused_on_battery() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_resume_on_battery_applies_balanced_for_a_saved_performance() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn start_reads_the_charge_limit_for_the_performance_view() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_failed_charge_limit_read_hides_the_switch_and_keeps_the_start() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), None);
    assert_ne!(sent(&app), []);
}

#[test]
fn a_resume_reads_the_charge_limit_again() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(charge_limit_on(&app), Some(false));
}

#[test]
fn the_charge_limit_switch_sends_its_command_and_saves_nothing() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(false))
        .unwrap();

    assert_eq!(sent(&app), [charge_limit().command(false).unwrap()]);
    assert_eq!(charge_limit_on(&app), Some(false));
    assert!(!path.exists());
}

#[test]
fn a_failed_charge_limit_switch_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_that_does_not_take_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.machine_mut().controller.charge_stuck = true;
    app.start().unwrap();

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_switch_does_not_touch_the_preset() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);
    app.start().unwrap();
    let before = *app.config();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(true))
        .unwrap();

    assert!(sent(&app).iter().all(|c| c.id == SET_CHARGE_LIMIT));
    assert_eq!(*app.config(), before);
}

#[test]
fn the_running_preset_is_the_one_the_power_source_lets_run() {
    let (mut app, _dir, _) = performance_app();
    assert_eq!(app.running_preset(), Preset::Performance);

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(app.running_preset(), Preset::Balanced);
}
