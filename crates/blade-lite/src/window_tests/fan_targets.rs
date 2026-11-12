//! A manual Custom's fan targets go once more about 2 s after it is sent
//! (ticket 173): sent from auto fans, the fans stall near 2900 RPM on
//! firmware 1.6 until the next target write.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::Command;

use super::{
    FAKE_REOPENED, FAKE_SLEEP_NOTICE_LATE, Fixture, click, fixture, fixture_with_programs,
};
use crate::shell::Reopened;

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

fn power(f: &Fixture, event: PowerEvent) {
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(event).unwrap();
    f.shell.follow_power(&receiver);
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

    f.shell.act(blade_lite::App::start);
    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(2));

    let start_targets = targets_3600();
    let mut expected = start_targets.clone();
    expected.extend(start_targets);
    assert_eq!(
        targets_since(&f, before),
        expected,
        "the start's, then once more"
    );
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
    f.shell.act(blade_lite::App::start);
    // A late notice sends no quit preset, so the start's targets stay due
    // over the sleep.
    FAKE_SLEEP_NOTICE_LATE.set(true);
    power(&f, PowerEvent::Suspending);
    let before = sent_count(&f);
    let t0 = Instant::now();

    // Asleep: nothing, however long.
    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(10));
    assert_eq!(sent_count(&f), before);
    power(&f, PowerEvent::Resumed);
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));
    let woke = sent_count(&f);
    assert_eq!(
        targets_since(&f, before),
        targets_3600(),
        "the wake's own Custom"
    );
    // Its own 2 s, not the old send's.
    tick(&f, t0 + Duration::from_secs(20));
    assert_eq!(targets_since(&f, woke), []);
    tick(&f, t0 + Duration::from_secs(22));

    assert_eq!(targets_since(&f, woke), targets_3600());
}

#[test]
fn a_newer_custom_send_waits_its_own_two_seconds() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    // Before the next tick: Balanced, then Custom again.
    click(f.shell.window(), Button, "Balanced");
    click(f.shell.window(), Button, "Custom");
    let before = sent_count(&f);

    tick(&f, t0 + Duration::from_secs(2));
    assert_eq!(targets_since(&f, before), [], "the newer send's 2 s");
    tick(&f, t0 + Duration::from_secs(4));

    assert_eq!(targets_since(&f, before), targets_3600());
}

#[test]
fn the_fan_guard_s_full_speed_goes_once_more_too() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    f.shell.watch_heat(Some(96), None, t0);
    let before = sent_count(&f);

    tick(&f, t0 + Duration::from_secs(2));
    tick(&f, t0 + Duration::from_secs(4));

    let top = u8::try_from(razer_proto::BLADE_18.custom.fan_rpm.max / 100).unwrap();
    let speeds: Vec<u8> = targets_since(&f, before)
        .iter()
        .map(|c| c.args[2])
        .collect();
    assert_eq!(speeds, [top, top]);
}

#[test]
fn no_fan_targets_go_again_in_the_pause_of_an_open_again_but_after_its_start() {
    let f = fixture(manual_fan_saved(Preset::Balanced));
    click(f.shell.window(), Button, "Custom");
    let t0 = Instant::now();
    tick(&f, t0);
    FAKE_REOPENED.set(Reopened::Opened);
    tick(&f, t0 + Duration::from_secs(1));
    let before = sent_count(&f);

    tick(&f, t0 + Duration::from_secs(2));
    tick(&f, t0 + Duration::from_secs(3));
    assert_eq!(sent_count(&f), before, "nothing in the pause");
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));
    let started = sent_count(&f);
    tick(&f, t0 + Duration::from_secs(4));
    tick(&f, t0 + Duration::from_secs(6));

    assert_eq!(targets_since(&f, started), targets_3600());
}

#[test]
fn no_fan_targets_go_again_while_razer_software_runs() {
    let f = fixture_with_programs(
        manual_fan_saved(Preset::Balanced),
        &["Razer Synapse Service"],
    );
    click(f.shell.window(), Button, "Custom");
    let before = sent_count(&f);
    let t0 = Instant::now();

    tick(&f, t0);
    tick(&f, t0 + Duration::from_secs(2));

    assert_eq!(targets_since(&f, before), []);
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
