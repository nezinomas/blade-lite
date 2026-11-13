//! The fan guard fed by the sensor reads of the ticks (ticket 163): the
//! GPU comes from the fake sensors, the CPU from the fake controller.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset};
use platform::PowerEvent;
use razer_proto::BLADE_18;
use ui::PerformanceCommand;

use crate::fakes::FakeCharger;
use crate::shell::{BY_A_POLL, FULL_SPEED_SENT_AGAIN, WITH_THE_PRESET};

use super::sleep::{pause_ends, power};
use super::{FAKE_GPU, FAKE_INPUT_AGE, FakeGpu, Fixture, fixture, fixture_with_programs, log_path};

const SET_FAN_TARGET: u16 = 0x0D01;

fn manual_fan() -> Fixture {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(1600);
    fixture(config)
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

/// The fan speeds sent since `before` commands, in RPM. Each manual
/// Custom send goes once more 2 s later (ticket 173).
fn fan_speeds_since(f: &Fixture, before: usize) -> Vec<u16> {
    f.shell.app().machine().controller.inner.sent[before..]
        .iter()
        .filter(|command| command.id == SET_FAN_TARGET)
        .map(|command| u16::from(command.args[2]) * 100)
        .collect()
}

/// Ticks every second from `from` for `time`, with the CPU at `cpu` °C.
/// Returns the time after the last tick.
fn ticks(f: &Fixture, from: Instant, time: Duration, cpu: u8) -> Instant {
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(cpu);
    let mut now = from;
    while now <= from + time {
        f.shell.tick_at(now, SystemTime::now());
        now += Duration::from_secs(1);
    }
    now
}

/// [`ticks`] with the wall clock from `wall`, a second a tick.
fn ticks_at(f: &Fixture, from: Instant, wall: SystemTime, time: Duration, cpu: u8) -> Instant {
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(cpu);
    let mut now = from;
    while now <= from + time {
        f.shell
            .tick_at(now, wall + now.saturating_duration_since(from));
        now += Duration::from_secs(1);
    }
    now
}

/// How many log lines contain `text`.
fn log_lines(text: &str) -> usize {
    std::fs::read_to_string(log_path())
        .unwrap()
        .lines()
        .filter(|line| line.contains(text))
        .count()
}

#[test]
fn a_hot_gpu_read_by_the_sensors_puts_a_manual_fan_at_full_speed() {
    let f = manual_fan();
    FAKE_GPU.set(FakeGpu::Celsius(90));
    let before = sent_count(&f);

    ticks(&f, Instant::now(), Duration::from_secs(3), 60);

    let top = BLADE_18.custom.fan_rpm.max;
    assert_eq!(fan_speeds_since(&f, before), [top; 4]);
}

#[test]
fn a_sleeping_gpu_lets_a_manual_fan_go_back_once_the_cpu_cools() {
    let f = manual_fan();
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);
    let before = sent_count(&f);

    ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 2, 60);

    assert_eq!(fan_speeds_since(&f, before), [1600; 4]);
}

#[test]
fn failed_gpu_reads_hold_a_manual_fan_at_full_speed_and_say_so() {
    let f = manual_fan();
    FAKE_GPU.set(FakeGpu::Fails);
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);
    let before = sent_count(&f);
    // A CPU heat no other test uses, so these lines are this test's.
    let held = "fan guard: Full speed held only by failed GPU reads (CPU 63 °C)";
    let ended = "fan guard: failed GPU reads no longer hold a manual fan at full speed (CPU 63 °C, GPU 50 °C)";

    let now = ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 3, 63);

    assert_eq!(fan_speeds_since(&f, before), [], "held at full speed");
    assert_eq!(log_lines(held), 1);

    FAKE_GPU.set(FakeGpu::Celsius(50));
    ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 2, 63);

    assert_eq!(log_lines(ended), 1);
    assert_eq!(log_lines(held), 1);
    assert_eq!(fan_speeds_since(&f, before), [1600; 4]);
}

#[test]
fn failed_gpu_reads_say_nothing_while_razer_software_runs() {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(1600);
    let f = fixture_with_programs(config, &["Razer Synapse Service"]);
    FAKE_GPU.set(FakeGpu::Fails);
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);

    ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 3, 64);

    assert_eq!(
        log_lines("fan guard: Full speed held only by failed GPU reads (CPU 64 °C)"),
        0
    );
}

/// A held rate error can't come behind the guard's send: 60 Hz follows
/// only the battery, where Custom does not run, so such a resend does not
/// take it (the App test
/// `a_battery_resend_with_a_held_rate_error_leaves_the_full_speed_unsent`).
#[test]
fn a_poll_that_takes_the_guard_s_failed_full_speed_says_so_once() {
    let f = manual_fan();
    let start = Instant::now();
    f.shell.app().machine_mut().controller.inner.fails = true;
    f.shell.watch_heat(Some(96), None, start);
    f.shell.app().machine_mut().controller.inner.fails = false;

    ticks(&f, start, Duration::from_secs(8), 96);

    assert_eq!(sent_again(), [BY_A_POLL]);
}

/// The guard's lines that say its failed full-speed send went later.
fn sent_again() -> Vec<&'static str> {
    FULL_SPEED_SENT_AGAIN.with_borrow(Clone::clone)
}

/// A manual fan whose guard full-speed send failed, with the controller
/// working again.
fn full_speed_unsent() -> Fixture {
    let f = manual_fan();
    f.shell.app().machine_mut().controller.inner.fails = true;
    f.shell.watch_heat(Some(96), None, Instant::now());
    f.shell.app().machine_mut().controller.inner.fails = false;
    assert!(f.shell.app().boost_unsent());
    f
}

#[test]
fn a_click_that_takes_the_guard_s_failed_full_speed_says_so() {
    let f = full_speed_unsent();

    f.shell
        .user_act(|app| app.on_performance(PerformanceCommand::SetFanRpm(Some(2000))));

    assert!(!f.shell.app().boost_unsent());
    assert_eq!(sent_again(), [WITH_THE_PRESET]);
}

#[test]
fn a_wake_that_takes_the_guard_s_failed_full_speed_says_so() {
    let f = full_speed_unsent();

    power(&f, &[PowerEvent::Suspending]);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();

    assert!(!f.shell.app().boost_unsent());
    assert_eq!(sent_again(), [WITH_THE_PRESET]);
}

#[test]
fn a_failed_click_says_nothing_of_the_guard_s_full_speed() {
    let f = full_speed_unsent();
    f.shell.app().machine_mut().controller.inner.fails = true;

    f.shell
        .user_act(|app| app.on_performance(PerformanceCommand::SetFanRpm(Some(2000))));

    assert!(f.shell.app().boost_unsent());
    assert_eq!(sent_again(), [""; 0]);
}

#[test]
fn a_click_off_custom_says_nothing_of_the_guard_s_full_speed() {
    let f = full_speed_unsent();
    let before = sent_count(&f);

    f.shell
        .user_act(|app| app.on_performance(PerformanceCommand::SetPreset(Preset::Balanced)));

    assert_ne!(sent_count(&f), before, "Balanced went");
    assert!(f.shell.app().boost_unsent(), "Balanced did not carry it");
    assert_eq!(sent_again(), [""; 0]);
}

#[test]
fn the_full_charger_back_that_takes_the_guard_s_failed_full_speed_says_so() {
    let f = full_speed_unsent();
    f.shell.app().machine_mut().controller.inner.charger = FakeCharger::UsbC;
    let now = ticks(&f, Instant::now(), Duration::ZERO, 96);
    assert!(
        !f.shell.app().manual_fan_runs(),
        "Balanced on a weak charge"
    );
    assert_eq!(sent_again(), [""; 0]);
    f.shell.app().machine_mut().controller.inner.charger = FakeCharger::Razer;

    ticks(&f, now, blade_core::HEAT_INTERVAL * 3, 96);

    assert!(f.shell.app().manual_fan_runs());
    assert!(!f.shell.app().boost_unsent());
    assert_eq!(sent_again(), [WITH_THE_PRESET]);
}

#[test]
fn a_poll_that_resends_a_failed_click_says_nothing_of_the_guard() {
    let f = manual_fan();
    let start = Instant::now();
    f.shell.watch_heat(Some(96), None, start);
    f.shell.app().machine_mut().controller.inner.fails = true;
    let clicked = f
        .shell
        .app()
        .on_performance(PerformanceCommand::SetFanRpm(Some(2000)));
    assert!(clicked.is_err());
    f.shell.app().machine_mut().controller.inner.fails = false;

    ticks(&f, start, Duration::from_secs(8), 96);

    assert!(!f.shell.app().preset_unsure(), "resent");
    assert_eq!(sent_again(), [""; 0]);
}

/// The sleep notice is stamped with the real clock (`suspend`), so the
/// sleep is the wall clock of the ticks after it, measured from now.
fn after(slept: Duration) -> SystemTime {
    SystemTime::now() + slept
}

/// A manual fan held at full speed by a hot CPU, then a sleep notice and
/// its wake, read as `events`, and the wake's pause. Returns the time after
/// it, and the sent count before the wake.
fn hot_then(events: &[&[PowerEvent]]) -> (Fixture, Instant, usize) {
    let f = manual_fan();
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);
    let (wake, sleep) = events.split_last().unwrap();
    for &sleep in sleep {
        power(&f, sleep);
    }
    let before = sent_count(&f);
    power(&f, wake);
    pause_ends();
    (f, now, before)
}

/// [`hot_then`] a sleep and its wake read one by one.
fn hot_then_asleep() -> (Fixture, Instant, usize) {
    hot_then(&[&[PowerEvent::Suspending], &[PowerEvent::Resumed]])
}

/// The fan speeds sent since `before` after half a minute of cool reads
/// at the wall clock `wall`, the first `first` °C.
fn speeds_after_cool_reads(
    f: &Fixture,
    now: Instant,
    before: usize,
    wall: SystemTime,
    first: u8,
) -> Vec<u16> {
    let now = ticks_at(f, now, wall, Duration::ZERO, first);
    let wall = wall + Duration::from_secs(1);
    ticks_at(f, now, wall, Duration::from_secs(30), 60);
    fan_speeds_since(f, before)
}

/// Full speed from the wake, sent again 2 s later: two fans each.
fn top_only() -> [u16; 4] {
    [BLADE_18.custom.fan_rpm.max; 4]
}

#[test]
fn cool_reads_after_a_long_sleep_end_full_speed_after_the_short_hold() {
    let (f, now, before) = hot_then_asleep();
    let wall = after(Duration::from_secs(600));
    let now = ticks_at(&f, now, wall, Duration::ZERO, 60);

    ticks_at(
        &f,
        now,
        wall + Duration::from_secs(1),
        ui::FAN_GUARD_WAKE_HOLD + Duration::from_secs(4),
        60,
    );

    // The wake sends full speed first.
    let top = BLADE_18.custom.fan_rpm.max;
    assert_eq!(
        fan_speeds_since(&f, before),
        [top, top, top, top, 1600, 1600, 1600, 1600]
    );
    assert!(log_lines("fan guard: Back to the set speed (CPU 60 °C") >= 1);
    assert!(log_lines("fan guard: the laptop slept 10 min") >= 1);
}

#[test]
fn cool_reads_after_a_short_sleep_keep_the_usual_hold() {
    let (f, now, before) = hot_then_asleep();
    let speeds = speeds_after_cool_reads(&f, now, before, after(Duration::from_secs(60)), 60);
    assert_eq!(speeds, top_only());
}

#[test]
fn a_hot_first_read_after_a_long_sleep_keeps_the_usual_hold() {
    let (f, now, before) = hot_then_asleep();
    let speeds = speeds_after_cool_reads(&f, now, before, after(Duration::from_secs(600)), 96);
    assert_eq!(speeds, top_only());
}

#[test]
fn a_sleep_notice_read_with_its_wake_keeps_the_usual_hold() {
    let (f, now, before) = hot_then(&[&[PowerEvent::Suspending, PowerEvent::Resumed]]);
    let speeds = speeds_after_cool_reads(&f, now, before, after(Duration::from_secs(600)), 60);
    assert_eq!(speeds, top_only());
}

#[test]
fn a_clock_set_back_in_the_sleep_keeps_the_usual_hold() {
    let (f, now, before) = hot_then_asleep();
    let wall = SystemTime::now() - Duration::from_secs(600);
    let speeds = speeds_after_cool_reads(&f, now, before, wall, 60);
    assert_eq!(speeds, top_only());
}

/// A sleep that failed: the app ran on with no wake notice, so the chips
/// did not cool, however long it was.
#[test]
fn a_missed_wake_keeps_the_usual_hold() {
    let f = manual_fan();
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);
    power(&f, &[PowerEvent::Suspending]);
    FAKE_INPUT_AGE.set(Some(Duration::from_secs(1)));
    let before = sent_count(&f);
    let wall = after(Duration::from_secs(600));
    // A minute of running with input counts as a wake.
    let now = ticks_at(&f, now, wall, Duration::from_secs(60), 60);
    pause_ends();

    let speeds = speeds_after_cool_reads(&f, now, before, wall + Duration::from_secs(61), 60);

    assert_eq!(speeds, top_only());
}

#[test]
fn a_long_sleep_with_the_guard_off_sends_and_says_nothing_of_it() {
    let f = manual_fan();
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 60);
    power(&f, &[PowerEvent::Suspending]);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    let before = sent_count(&f);

    ticks_at(
        &f,
        now,
        after(Duration::from_mins(17)),
        Duration::from_secs(30),
        60,
    );

    assert_eq!(fan_speeds_since(&f, before), [1600; 2]);
    assert_eq!(log_lines("fan guard: the laptop slept 17 min"), 0);
}
