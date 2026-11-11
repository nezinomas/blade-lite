//! The fan guard (ticket 129): while it is on, a manual Custom fan runs at
//! the model's top speed; the saved speed is kept and goes back after.

use blade_core::{Config, Preset};
use razer_proto::{BLADE_18, Command};
use ui::PerformanceCommand;

use crate::fakes::TestApp;
use crate::sent;

/// The fan targets sent, as speeds in RPM.
fn fan_speeds(app: &TestApp) -> Vec<u16> {
    sent(app)
        .iter()
        .filter(|command: &&Command| command.id == 0x0D01)
        .map(|command| u16::from(command.args[2]) * 100)
        .collect()
}

fn custom_app(fan_rpm: Option<u16>) -> TestApp {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = fan_rpm;
    let (mut app, _dir, _) = crate::app_with(config);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app
}

#[test]
fn the_guard_sends_the_top_speed_then_the_saved_one() {
    let mut app = custom_app(Some(1600));
    let top = BLADE_18.custom.fan_rpm.max;

    assert!(app.set_fan_boost(true).unwrap());
    assert_eq!(fan_speeds(&app), [top, top]);
    assert_eq!(app.config().custom.fan_rpm, Some(1600));

    app.machine_mut().controller.sent.clear();
    assert!(app.set_fan_boost(false).unwrap());
    assert_eq!(fan_speeds(&app), [1600, 1600]);
}

#[test]
fn the_guard_sends_nothing_again_while_it_stays_on() {
    let mut app = custom_app(Some(1600));
    app.set_fan_boost(true).unwrap();
    app.machine_mut().controller.sent.clear();

    assert!(!app.set_fan_boost(true).unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn the_guard_sends_nothing_for_fans_on_auto_or_another_preset() {
    let mut app = custom_app(None);
    assert!(!app.set_fan_boost(true).unwrap());
    assert_eq!(sent(&app), []);

    let (mut app, _dir, _) = crate::performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    assert!(!app.set_fan_boost(true).unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_fan_click_while_the_guard_is_on_saves_the_speed_but_sends_the_top_one() {
    let mut app = custom_app(Some(1600));
    app.set_fan_boost(true).unwrap();
    app.machine_mut().controller.sent.clear();
    let top = BLADE_18.custom.fan_rpm.max;

    app.on_performance(PerformanceCommand::SetFanRpm(Some(2000)))
        .unwrap();

    assert_eq!(fan_speeds(&app), [top, top]);
    assert_eq!(app.config().custom.fan_rpm, Some(2000));
}

#[test]
fn a_fan_speed_the_model_does_not_take_is_refused_while_the_guard_is_on() {
    let mut app = custom_app(Some(1600));
    app.set_fan_boost(true).unwrap();
    app.machine_mut().controller.sent.clear();

    let result = app.on_performance(PerformanceCommand::SetFanRpm(Some(9000)));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
    assert_eq!(app.config().custom.fan_rpm, Some(1600));
}

#[test]
fn a_failed_full_speed_send_is_tried_again_while_the_guard_stays_on() {
    let mut app = custom_app(Some(1600));
    app.machine_mut().controller.fails = true;
    assert!(app.set_fan_boost(true).is_err());
    // The polls use up their resends.
    for _ in 0..5 {
        let _ = app.resend_preset();
    }
    app.machine_mut().controller.fails = false;

    // The next reading keeps the guard on and arms the resends again.
    assert!(!app.set_fan_boost(true).unwrap());
    assert!(app.resend_preset().unwrap());
    let top = BLADE_18.custom.fan_rpm.max;
    assert_eq!(fan_speeds(&app), [top, top]);
}

#[test]
fn a_fan_saved_at_the_top_speed_sends_nothing_more() {
    let top = BLADE_18.custom.fan_rpm.max;
    let mut app = custom_app(Some(top));
    assert!(!app.set_fan_boost(true).unwrap());
    assert_eq!(sent(&app), []);
}
