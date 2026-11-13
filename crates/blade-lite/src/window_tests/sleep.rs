//! A sleep sends the quit preset, and the wake sends the saved settings
//! again (ticket 169): on Modern Standby the controller stays powered and
//! would run a manual fan on in the sleep.

use std::time::{Duration, SystemTime};

use blade_core::{Config, Preset, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::{BLADE_18, Command, FirmwareVersion, PerfMode};

use super::{
    FAKE_INPUT_AGE, FAKE_ON_BATTERY, FAKE_RELEASED, FAKE_REOPENED, FAKE_SLEEP_NOTICE_LATE, Fixture,
    click, fixture, fixture_with_programs, log_path,
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
pub(super) fn power(f: &Fixture, events: &[PowerEvent]) {
    let (sender, receiver) = std::sync::mpsc::channel();
    for &event in events {
        sender.send(event).unwrap();
    }
    f.shell.follow_power(&receiver);
}

pub(super) fn pause_ends() {
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

#[test]
fn a_sleep_and_a_wake_inside_a_wake_pause_leave_the_send_to_the_last_pause() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let asked = asked_count(&f);

    power(&f, &[PowerEvent::Resumed]);
    step(1000);
    power(&f, &[PowerEvent::Suspending]);
    power(&f, &[PowerEvent::Resumed]);
    // The first wake's pause ends now, inside the second's.
    step(1500);

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(asked_count(&f), asked);
    step(600);
    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

/// Advances the mock clock by `millis`.
fn step(millis: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(millis));
}

/// Starts the pause after an updater ended, as the tick that saw it end.
fn updater_ends(f: &Fixture) {
    f.shell.programs().set(&[UPDATER]);
    f.shell.look_for_razer();
    f.shell.programs().set(&[]);
    f.shell.look_for_razer();
    f.shell.tick();
}

/// Starts the pause after an open again, as the tick that saw it.
fn opened_again(f: &Fixture) {
    FAKE_REOPENED.set(Reopened::Opened);
    FAKE_RELEASED.set(false);
    f.shell.tick();
}

/// How many firmware reads the controller got.
fn firmware_reads(f: &Fixture) -> usize {
    let id = FirmwareVersion::query().id;
    f.shell
        .app()
        .machine()
        .controller
        .inner
        .asked
        .iter()
        .filter(|c| c.id == id)
        .count()
}

/// Ticket 162: the older pause ends 0.5 s after the newer one began, and
/// sends and reads nothing then; the last pause sends once, and reads the
/// firmware once if `reads` (an open again or an updater before it).
fn only_the_last_pause_sends(f: &Fixture, before: usize, reads: bool) {
    let asked = asked_count(f);
    let firmware = firmware_reads(f);
    step(600);
    assert_eq!(sent_since(f, before), [], "the older pause sends nothing");
    assert_eq!(asked_count(f), asked, "nor reads");
    step(1500);
    let targets = sent_since(f, before)
        .iter()
        .filter(|c| c.id == SET_FAN_TARGET)
        .count();
    assert_eq!(
        targets,
        BLADE_18.custom.fan_zones.len(),
        "one send of the fans"
    );
    assert_eq!(firmware_reads(f) - firmware, usize::from(reads));
}

#[test]
fn a_wake_in_the_pause_of_an_open_again_leaves_the_send_to_the_wake() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);
    opened_again(&f);
    step(1500);

    power(&f, &[PowerEvent::Resumed]);

    only_the_last_pause_sends(&f, before, true);
    assert!(FAKE_RELEASED.get(), "the wake releases the handle");
}

#[test]
fn a_wake_in_the_pause_after_an_updater_leaves_the_send_to_the_wake() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);
    updater_ends(&f);
    step(1500);

    power(&f, &[PowerEvent::Resumed]);

    only_the_last_pause_sends(&f, before, true);
}

#[test]
fn an_open_again_in_a_wake_pause_leaves_the_send_to_its_own_pause() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Resumed]);
    let before = sent_count(&f);
    step(1500);

    opened_again(&f);

    only_the_last_pause_sends(&f, before, true);
    assert!(FAKE_RELEASED.get());
}

#[test]
fn an_open_again_after_an_updater_leaves_nothing_for_a_later_wake() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);
    updater_ends(&f);
    step(1500);
    opened_again(&f);
    only_the_last_pause_sends(&f, before, true);
    let firmware = firmware_reads(&f);
    FAKE_RELEASED.set(false);

    power(&f, &[PowerEvent::Resumed]);
    pause_ends();

    assert!(!FAKE_RELEASED.get(), "no release left for the wake");
    assert_eq!(firmware_reads(&f), firmware, "nor a firmware read");
}

#[test]
fn two_wakes_under_the_pause_apart_send_once() {
    let f = fixture(manual_fan());
    let before = sent_count(&f);
    power(&f, &[PowerEvent::Resumed]);
    step(1500);

    power(&f, &[PowerEvent::Resumed]);

    only_the_last_pause_sends(&f, before, false);
}

/// Ticks each second from `wall` for `seconds`, as a running app does.
fn tick_for(f: &Fixture, wall: SystemTime, seconds: u64) {
    let now = std::time::Instant::now();
    for second in 0..=seconds {
        f.shell.tick_at(
            now + Duration::from_secs(second),
            wall + Duration::from_secs(second),
        );
    }
}

/// A sleep notice, then input from the user a second ago from now on.
fn asleep_with_input(f: &Fixture) {
    power(f, &[PowerEvent::Suspending]);
    FAKE_INPUT_AGE.set(Some(Duration::from_secs(1)));
}

#[test]
fn a_minute_of_running_and_input_with_no_wake_notice_counts_as_a_wake() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let before = sent_count(&f);
    let wall = SystemTime::now();

    tick_for(&f, wall, 59);
    assert_eq!(sent_since(&f, before), [], "not before a minute");
    tick_for(&f, wall + Duration::from_secs(60), 0);
    assert_eq!(sent_since(&f, before), [], "not in the wake's pause");
    pause_ends();

    assert!(sets_a_manual_fan(&sent_since(&f, before)));
    assert!(logged(&[
        "input from the user and no wake notice",
        "counts as a wake"
    ]));
}

#[test]
fn with_no_input_since_the_notice_the_sleep_goes_on() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let wall = SystemTime::now();

    // Unknown, then older than the run: the lid may be shut on a game.
    tick_for(&f, wall, 120);
    FAKE_INPUT_AGE.set(Some(Duration::from_secs(200)));
    tick_for(&f, wall + Duration::from_secs(121), 60);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
    let error = {
        click(f.shell.window(), Button, "Performance");
        f.shell.window().get_error()
    };
    assert!(error.contains("has not said it woke"), "{error}");
}

#[test]
fn a_gap_in_the_ticks_starts_the_minute_again() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let before = sent_count(&f);
    let wall = SystemTime::now();

    // Frozen for 10 min in the sleep, then 59 s of ticks.
    tick_for(&f, wall, 30);
    tick_for(&f, wall + Duration::from_secs(630), 59);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_clock_set_back_starts_the_minute_again() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let before = sent_count(&f);
    let wall = SystemTime::now();

    tick_for(&f, wall, 40);
    tick_for(&f, wall + Duration::from_secs(38), 32);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn ticks_5_s_apart_are_one_run() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let before = sent_count(&f);
    let wall = SystemTime::now();
    let now = std::time::Instant::now();

    for step in 0..=12 {
        let gap = Duration::from_secs(5 * step);
        f.shell.tick_at(now + gap, wall + gap);
    }
    pause_ends();

    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn a_new_sleep_notice_starts_the_minute_again() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let wall = SystemTime::now();
    tick_for(&f, wall, 60);
    pause_ends();
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);

    tick_for(&f, wall + Duration::from_secs(61), 58);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_sleep_notice_while_asleep_starts_the_minute_again() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let before = sent_count(&f);
    let wall = SystemTime::now();
    tick_for(&f, wall, 55);

    // A real sleep after one that Windows cancelled with no wake notice.
    power(&f, &[PowerEvent::Suspending]);
    tick_for(&f, wall + Duration::from_secs(56), 10);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_wake_notice_after_the_missed_one_sends_the_settings_once_more() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    tick_for(&f, SystemTime::now(), 60);
    pause_ends();
    let before = sent_count(&f);

    power(&f, &[PowerEvent::Resumed]);
    pause_ends();

    assert!(sets_a_manual_fan(&sent_since(&f, before)));
}

#[test]
fn input_from_before_the_notice_does_not_count() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let wall = SystemTime::now();
    tick_for(&f, wall, 59);

    // Such as the Start menu's Sleep click, just before the notice.
    FAKE_INPUT_AGE.set(Some(Duration::from_secs(61)));
    tick_for(&f, wall + Duration::from_secs(60), 0);
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn input_in_the_first_30_s_after_the_notice_does_not_count() {
    let f = fixture(manual_fan());
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    let wall = SystemTime::now();
    tick_for(&f, wall, 59);

    // A touch 2 s in, as the screen goes dark; then exactly 30 s in.
    for age in [58, 30] {
        FAKE_INPUT_AGE.set(Some(Duration::from_secs(age)));
        tick_for(&f, wall + Duration::from_secs(60), 0);
    }
    pause_ends();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_wake_notice_in_the_tick_of_the_minute_wakes_once() {
    let f = fixture(manual_fan());
    asleep_with_input(&f);
    let wall = SystemTime::now();
    tick_for(&f, wall, 59);
    let before = sent_count(&f);
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.listen_for_power(receiver);

    tick_for(&f, wall + Duration::from_secs(60), 0);
    pause_ends();

    // As many sends as one wake from the notice alone.
    let sent = sent_since(&f, before).len();
    assert!(sent > 0);
    power(&f, &[PowerEvent::Suspending]);
    let again = sent_count(&f);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    assert_eq!(sent, sent_since(&f, again).len());
}

/// Whether `sent` puts a fan at full speed.
fn sends_full_speed(sent: &[Command]) -> bool {
    let top = BLADE_18.custom.fan_rpm.max;
    sent.iter()
        .any(|c| c.id == SET_FAN_TARGET && u16::from(c.args[2]) * 100 == top)
}

/// A heat read, then a sleep and a wake an hour later by the wall clock,
/// with the CPU hot since. Returns the `Instant` of the read and the wall
/// clock at the wake.
fn hot_after_an_hour_asleep(f: &Fixture) -> (std::time::Instant, SystemTime) {
    let now = std::time::Instant::now();
    let wall = SystemTime::now();
    f.shell.tick_at(now, wall);
    power(f, &[PowerEvent::Suspending]);
    power(f, &[PowerEvent::Resumed]);
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(96);
    (now, wall + Duration::from_secs(3600))
}

#[test]
fn the_fan_guard_reads_at_the_first_tick_after_the_wake_pause() {
    // Ticket 171: the wall clock says the laptop slept, but the wake is
    // known once its pause ends, so the read is not skipped as late.
    let f = fixture(manual_fan());
    let (now, wall) = hot_after_an_hour_asleep(&f);
    pause_ends();
    let before = sent_count(&f);

    f.shell.tick_at(now + Duration::from_secs(3), wall);

    assert!(sends_full_speed(&sent_since(&f, before)));
}

#[test]
fn nothing_is_read_in_a_sleep_begun_in_the_wake_pause_and_the_next_wake_reads_at_once() {
    let f = fixture(manual_fan());
    let (now, wall) = hot_after_an_hour_asleep(&f);
    power(&f, &[PowerEvent::Suspending]);
    pause_ends();
    let asked = asked_count(&f);

    f.shell.tick_at(now + Duration::from_secs(3), wall);

    assert_eq!(asked_count(&f), asked, "nothing read while asleep");
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    let before = sent_count(&f);
    f.shell.tick_at(now + Duration::from_secs(4), wall);
    assert!(
        sends_full_speed(&sent_since(&f, before)),
        "the next wake reads"
    );
}

#[test]
fn the_fan_guard_reads_at_the_first_tick_after_an_updater_s_pause() {
    // No read while the updater runs, so the last one is minutes old.
    let f = fixture(manual_fan());
    let now = std::time::Instant::now();
    let wall = SystemTime::now();
    f.shell.tick_at(now, wall);
    f.shell.programs().set(&[UPDATER]);
    f.shell.look_for_razer();
    f.shell.programs().set(&[]);
    f.shell.look_for_razer();
    let later = Duration::from_secs(300);
    f.shell.tick_at(now + later, wall + later);
    pause_ends();
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(96);
    let before = sent_count(&f);

    let later = later + Duration::from_secs(3);
    f.shell.tick_at(now + later, wall + later);

    assert!(sends_full_speed(&sent_since(&f, before)));
}
