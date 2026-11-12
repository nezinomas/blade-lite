//! A sleep sends the quit preset, and the wake sends the saved settings
//! again (ticket 169): on Modern Standby the controller stays powered and
//! would run a manual fan on in the sleep.

use std::time::Duration;

use blade_core::{Config, Preset, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::{BLADE_18, Command, PerfMode};

use super::{
    FAKE_ON_BATTERY, FAKE_RELEASED, FAKE_REOPENED, FAKE_SLEEP_NOTICE_LATE, Fixture, click, fixture,
    fixture_with_programs, log_path,
};
use crate::shell::Reopened;

const SET_FAN_TARGET: u16 = 0x0D01;
const SET_MODE: u16 = 0x0D02;

fn manual_fan() -> Config {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(3500);
    config
}

/// What the controller got since `before` commands.
fn sent_since(f: &Fixture, before: usize) -> Vec<Command> {
    f.shell.app().machine().controller.inner.sent[before..].to_vec()
}

/// How many queries the controller got.
fn asked_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.asked.len()
}

const UPDATER: &str = "RazerFirmwareUpdater.exe";

/// The start of the log line for a failed sleep send.
const NOT_SENT: &str = "the quit preset was not sent before the sleep, so";

/// Whether one log line holds all of `parts`: the log is shared by the
/// tests that run at once, so one part alone may be another test's.
fn logged(parts: &[&str]) -> bool {
    log()
        .lines()
        .any(|line| parts.iter().all(|part| line.contains(part)))
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

/// Passes `events` on as one poll reads them.
fn power(f: &Fixture, events: &[PowerEvent]) {
    let (sender, receiver) = std::sync::mpsc::channel();
    for &event in events {
        sender.send(event).unwrap();
    }
    f.shell.follow_power(&receiver);
}

fn pause_ends() {
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));
}

/// Whether `sent` sets a manual fan: a fan target, or a mode write with
/// the fan byte on manual.
fn sets_a_manual_fan(sent: &[Command]) -> bool {
    sent.iter()
        .any(|c| c.id == SET_FAN_TARGET || (c.id == SET_MODE && c.args[3] != 0))
}

fn log() -> String {
    std::fs::read_to_string(log_path()).unwrap()
}

#[test]
fn a_sleep_from_a_manual_fan_sends_the_fans_on_auto_then_balanced() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    let sent = sent_since(&f, before);
    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert!(sent.ends_with(&balanced), "{sent:?}");
    let auto_fans = &sent[..sent.len() - balanced.len()];
    assert!(auto_fans.iter().any(|c| c.id == SET_MODE), "{sent:?}");
    assert!(!sets_a_manual_fan(&sent), "{sent:?}");
    assert_eq!(f.shell.app().config().preset, Preset::Custom);
    assert_eq!(f.shell.app().config().custom.fan_rpm, Some(3500));
    assert!(log().contains("the laptop goes to sleep, so the quit preset was sent"));
}

#[test]
fn a_sleep_from_performance_sends_balanced() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    assert_eq!(
        sent_since(&f, before),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn the_wake_sends_the_manual_fan_again_after_its_pause() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Resumed]);
    assert_eq!(sent_since(&f, before), [], "not in the pause");
    pause_ends();

    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_second_sleep_notice_sends_nothing() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn nothing_is_sent_or_read_between_the_sleep_and_the_wake() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let asked = asked_count(&f);

    click(f.shell.window(), Button, "Performance");
    f.shell.user_act(blade_lite::App::on_power_key);
    FAKE_ON_BATTERY.set(true);
    power(&f, &[PowerEvent::OnBattery]);
    for _ in 0..5 {
        pause_ends();
        f.shell.tick();
    }

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(asked_count(&f), asked);
}

#[test]
fn a_click_between_the_sleep_and_the_wake_says_why_nothing_happens() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);

    click(f.shell.window(), Button, "Performance");

    let error = f.shell.window().get_error();
    assert!(error.contains("has not said it woke"), "{error}");
}

#[test]
fn a_sleep_notice_windows_no_longer_waits_for_sends_nothing() {
    let f = fixture(manual_fan());
    FAKE_SLEEP_NOTICE_LATE.set(true);
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    assert_eq!(sent_since(&f, before), []);
    assert!(log().contains("read after Windows stopped waiting for it"));
    // Still nothing until the wake, which sends them.
    pause_ends();
    f.shell.tick();
    assert_eq!(sent_since(&f, before), []);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_quit_while_asleep_sends_the_quit_preset_without_a_wait() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let began = std::time::Instant::now();

    f.shell.quit();

    assert!(began.elapsed() < RESUME_PAUSE / 2);
    assert_eq!(
        sent_since(&f, before),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_sleep_in_the_wake_pause_sends_nothing_and_the_pause_end_neither() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Resumed]);
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
    assert!(log().contains("in the pause after a wake"));
    // The next wake sends them.
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_sleep_read_only_after_its_wake_sends_nothing_but_the_wake_does() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending, PowerEvent::Resumed]);
    assert_eq!(sent_since(&f, before), []);
    pause_ends();

    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_sleep_in_the_pause_of_an_open_again_leaves_its_end_to_the_wake() {
    let f = fixture(manual_fan());
    FAKE_REOPENED.set(Reopened::Opened);
    FAKE_RELEASED.set(false);
    f.shell.tick();
    let before = sent_count(&f);
    let asked = asked_count(&f);

    power(&f, &[PowerEvent::Suspending]);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(asked_count(&f), asked, "no firmware read in the sleep");
    assert!(!FAKE_RELEASED.get(), "the handle stays held");
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    assert!(FAKE_RELEASED.get());
    assert!(asked_count(&f) > asked, "the wake reads the firmware");
    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_sleep_in_the_pause_after_an_updater_leaves_its_end_to_the_wake() {
    let f = fixture(manual_fan());
    f.shell.programs().set(&[UPDATER]);
    f.shell.look_for_razer();
    f.shell.programs().set(&[]);
    f.shell.look_for_razer();
    f.shell.tick();
    let before = sent_count(&f);
    let asked = asked_count(&f);

    power(&f, &[PowerEvent::Suspending]);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(asked_count(&f), asked);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_sleep_after_the_quit_sends_nothing() {
    let f = fixture(manual_fan());
    f.shell.quit();
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_sleep_while_a_razer_updater_runs_sends_nothing_and_says_so() {
    let f = fixture_with_programs(manual_fan(), &[UPDATER]);
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Suspending]);

    assert_eq!(sent_since(&f, before), []);
    assert!(logged(&[NOT_SENT, "a Razer updater runs"]));
}

#[test]
fn a_failed_sleep_send_is_logged() {
    let f = fixture(manual_fan());
    f.shell.app().machine_mut().controller.inner.fails = true;

    power(&f, &[PowerEvent::Suspending]);

    assert!(logged(&[NOT_SENT, "stayed busy"]));
}

#[test]
fn a_sleep_while_the_app_is_busy_sends_nothing_and_says_so() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);

    {
        let _busy = f.shell.app();
        power(&f, &[PowerEvent::Suspending]);
    }

    assert_eq!(sent_since(&f, before), []);
    assert!(log().contains("the app was busy, so the quit preset was not sent before the sleep"));
}
