//! A send cut after the manual flag went may leave a manual fan running
//! that the saved config does not show (ticket 250): the fan guard, its
//! fast heat reads, a slow resend, a window warning and the quit's fans on
//! auto stay on until a send goes whole.

use std::time::{Duration, Instant};

use blade_core::{Config, CustomSettings, Preset};
use blade_lite::MANUAL_RESEND_SPACING;
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use ui::PerformanceCommand;

use tempfile::TempDir;

use crate::fakes::TestApp;
use crate::{app_with, sent};

/// The mode writes that carry the manual flag.
const SET_MODE: u16 = 0x0D02;
const SET_FAN: u16 = 0x0D01;

fn balanced_with_a_manual_fan_saved() -> (TestApp, TempDir) {
    let config = Config {
        custom: CustomSettings {
            fan_rpm: Some(1600),
            ..CustomSettings::default()
        },
        ..Config::default()
    };
    let (mut app, dir, _) = app_with(config);
    app.take_on_battery(Some(false));
    app.start().unwrap();
    (app, dir)
}

/// Clicks Custom, which the controller takes only up to its first mode
/// write (the manual flag), then every resend fails the same way.
fn cut_custom_click(app: &mut TestApp) {
    app.machine_mut().controller.inner.fails_after = Some(1);
    assert!(
        app.on_performance(PerformanceCommand::SetPreset(Preset::Custom))
            .is_err()
    );
    let mode = &sent(app).last().unwrap();
    assert_eq!(mode.id, SET_MODE, "the manual flag went");
    while app.resend_preset().is_err() {}
    app.machine_mut().controller.inner.fails_after = None;
    app.machine_mut().controller.inner.sent.clear();
}

#[test]
fn a_cut_custom_click_counts_as_a_manual_fan_running() {
    let (mut app, _dir) = balanced_with_a_manual_fan_saved();

    cut_custom_click(&mut app);

    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(app.manual_fan_runs());
    assert!(app.manual_fan_unshown());
}

#[test]
fn a_send_that_goes_whole_ends_it() {
    let (mut app, _dir) = balanced_with_a_manual_fan_saved();
    cut_custom_click(&mut app);

    app.keep_resending(Instant::now());
    assert!(app.resend_preset().unwrap());

    assert!(!app.manual_fan_runs());
    assert!(!app.manual_fan_unshown());
}

#[test]
fn a_cut_send_with_no_manual_flag_shows_nothing_new() {
    let (mut app, _dir, _) = app_with(Config::default());
    app.take_on_battery(Some(false));
    app.start().unwrap();
    app.machine_mut().controller.inner.fails_after = Some(1);

    assert!(
        app.on_performance(PerformanceCommand::SetPreset(Preset::Silent))
            .is_err()
    );

    assert!(!app.manual_fan_runs());
    assert!(!app.manual_fan_unshown());
}

/// The charger is pulled while a manual Custom runs: the battery preset's
/// send is cut, so the manual fan may still run.
#[test]
fn a_cut_battery_send_after_a_manual_custom_counts_as_a_manual_fan_running() {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(1600);
    let (mut app, _dir, _) = app_with(config);
    app.take_on_battery(Some(false));
    app.start().unwrap();
    app.machine_mut().controller.inner.fails_after = Some(1);

    assert!(app.on_power(PowerEvent::OnBattery).is_err());

    assert!(app.manual_fan_runs());
    assert!(app.manual_fan_unshown());
}

#[test]
fn the_slow_resend_comes_once_a_minute_after_the_tries_run_out() {
    let (mut app, _dir) = balanced_with_a_manual_fan_saved();
    cut_custom_click(&mut app);
    assert!(!app.resend_preset().unwrap(), "no try left");
    let start = Instant::now();

    app.keep_resending(start);
    app.machine_mut().controller.inner.fails = true;
    assert!(app.resend_preset().is_err());
    app.machine_mut().controller.inner.fails = false;
    app.keep_resending(start + MANUAL_RESEND_SPACING - Duration::from_secs(1));
    assert!(!app.resend_preset().unwrap(), "too soon");
    app.keep_resending(start + MANUAL_RESEND_SPACING);

    assert!(app.resend_preset().unwrap());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn no_slow_resend_without_a_manual_fan_unshown() {
    let (mut app, _dir, _) = crate::performance_app();
    app.machine_mut().controller.inner.fails = true;
    assert!(app.on_power(PowerEvent::OnBattery).is_err());
    while app.resend_preset().is_err() {}
    app.machine_mut().controller.inner.fails = false;

    app.keep_resending(Instant::now());

    assert!(!app.resend_preset().unwrap());
}

/// The guard sends no full speed for a Custom the window does not show:
/// it keeps the resends of the preset that should run going.
#[test]
fn the_guard_keeps_the_resends_going_and_sends_nothing_itself() {
    let (mut app, _dir) = balanced_with_a_manual_fan_saved();
    cut_custom_click(&mut app);

    assert!(!app.set_fan_boost(true).unwrap());
    assert_eq!(sent(&app), []);
    assert!(!app.boost_unsent());

    assert!(app.resend_preset().unwrap());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn the_quit_sends_the_fans_on_auto_first() {
    let (mut app, _dir) = balanced_with_a_manual_fan_saved();
    cut_custom_click(&mut app);

    app.quit_preset().unwrap();

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    let sent = sent(&app);
    let (auto_fans, last) = sent.split_at(sent.len() - balanced.len());
    assert_eq!(last, balanced);
    let modes = auto_fans.iter().filter(|c| c.id == SET_MODE);
    assert!(modes.clone().count() > 0, "{auto_fans:?}");
    assert!(modes.clone().all(|c| c.args[3] == 0), "{auto_fans:?}");
    assert!(auto_fans.iter().all(|c| c.id != SET_FAN), "{auto_fans:?}");
}
