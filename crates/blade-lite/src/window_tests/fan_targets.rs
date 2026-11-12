//! A manual Custom's fan targets go once more about 2 s after it is sent
//! (ticket 173): sent from auto fans, the fans stall near 2900 RPM on
//! firmware 1.6 until the next target write.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::Command;

use super::{Fixture, click, fixture};

const SET_FAN_TARGET: u16 = 0x0D01;

fn manual_fan_saved(preset: Preset) -> Config {
    let mut config = Config {
        preset,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(3600);
    config
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

/// The fan target writes since `before` commands.
fn targets_since(f: &Fixture, before: usize) -> Vec<Command> {
    f.shell.app().machine().controller.inner.sent[before..]
        .iter()
        .filter(|command| command.id == SET_FAN_TARGET)
        .cloned()
        .collect()
}

/// The two fan target writes of 3600 RPM.
fn targets_3600() -> Vec<Command> {
    [1, 2]
        .map(|fan| Command {
            id: SET_FAN_TARGET,
            args: vec![1, fan, 36],
        })
        .to_vec()
}

fn tick(f: &Fixture, at: Instant) {
    f.shell.tick_at(at, SystemTime::now());
}

#[test]
fn the_fan_targets_go_once_more_two_seconds_after_a_custom_click() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let before = sent_count(&f);
    let t0 = Instant::now();

    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(1));
    assert_eq!(targets_since(&f, before), [], "not before 2 s");
    tick(&f, t0 + Duration::from_secs(2));
    assert_eq!(targets_since(&f, before), targets_3600());
    tick(&f, t0 + Duration::from_secs(4));
    tick(&f, t0 + Duration::from_secs(6));

    assert_eq!(targets_since(&f, before), targets_3600(), "once");
}

#[test]
fn the_start_with_a_manual_fan_sends_its_fan_targets_once_more() {
    let f = fixture(manual_fan_saved(Preset::Custom));
    let before = sent_count(&f);
    let t0 = Instant::now();

    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(2));

    assert_eq!(targets_since(&f, before), targets_3600());
}

#[test]
fn a_preset_click_before_the_two_seconds_sends_no_fan_targets() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    click(f.shell.window(), Button, "Balanced");
    let before = sent_count(&f);

    tick(&f, t0 + Duration::from_secs(2));
    tick(&f, t0 + Duration::from_secs(4));

    assert_eq!(targets_since(&f, before), []);
}

#[test]
fn custom_with_the_fans_on_auto_sends_no_fan_targets_again() {
    let f = fixture(Config::default());
    click(f.shell.window(), Button, "Custom");
    let before = sent_count(&f);
    let t0 = Instant::now();

    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(2));

    assert_eq!(targets_since(&f, before), []);
}

#[test]
fn the_wake_sends_the_fan_targets_once_more_after_its_own_send() {
    let f = fixture(manual_fan_saved(Preset::Custom));
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Suspending).unwrap();
    f.shell.follow_power(&receiver);
    let before = sent_count(&f);
    let t0 = Instant::now();

    // Asleep: nothing, however long.
    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(10));
    assert_eq!(targets_since(&f, before), []);
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));
    let woke = sent_count(&f);
    // The wake's own Custom, with its targets, then once more.
    assert_eq!(targets_since(&f, woke), []);
    tick(&f, t0 + Duration::from_secs(20));
    tick(&f, t0 + Duration::from_secs(22));

    assert_eq!(targets_since(&f, woke), targets_3600());
}

#[test]
fn no_fan_targets_go_after_the_quit() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    f.shell.quit();
    let before = sent_count(&f);

    tick(&f, t0 + Duration::from_secs(2));

    assert_eq!(targets_since(&f, before), []);
}

#[test]
fn a_failed_fan_target_send_is_shown_and_not_tried_again() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    f.shell.app().machine_mut().controller.inner.fails = true;
    let runs = f.shell.app().machine().controller.inner.runs;

    tick(&f, t0 + Duration::from_secs(2));
    tick(&f, t0 + Duration::from_secs(4));

    let error = f.shell.window().get_error();
    assert!(error.contains("stayed busy"), "{error}");
    let tried = f.shell.app().machine().controller.inner.runs - runs;
    assert_eq!(tried, 1, "one try, no retry");
}
