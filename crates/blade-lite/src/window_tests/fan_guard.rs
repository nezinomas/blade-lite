//! The fan guard fed by the sensor reads of the ticks (ticket 163): the
//! GPU comes from the fake sensors, the CPU from the fake controller.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset};
use razer_proto::BLADE_18;
use ui::PerformanceCommand;

use crate::shell::FULL_SPEED_RESENT_LINES;

use platform::PowerEvent;

use super::sleep::{pause_ends, power};
use super::{FAKE_GPU, FakeGpu, Fixture, fixture, fixture_with_programs, log_path};

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
    ticks_later(f, from, time, cpu, Duration::ZERO)
}

/// [`ticks`] with the wall clock `ahead` of the real one.
fn ticks_later(f: &Fixture, from: Instant, time: Duration, cpu: u8, ahead: Duration) -> Instant {
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(cpu);
    let mut now = from;
    while now <= from + time {
        f.shell.tick_at(now, SystemTime::now() + ahead);
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
/// only the battery, where Custom does not run (the App test covers it).
#[test]
fn a_poll_that_takes_the_guard_s_failed_full_speed_says_so_once() {
    let f = manual_fan();
    let start = Instant::now();
    f.shell.app().machine_mut().controller.inner.fails = true;
    f.shell.watch_heat(Some(96), None, start);
    f.shell.app().machine_mut().controller.inner.fails = false;

    ticks(&f, start, Duration::from_secs(8), 96);

    assert_eq!(FULL_SPEED_RESENT_LINES.get(), 1);
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
    assert_eq!(FULL_SPEED_RESENT_LINES.get(), 0);
}

/// A manual fan held at full speed by a hot CPU, then a sleep of `slept`
/// by the wall clock and its wake's pause. Returns the time after it, and
/// the sent count before the wake.
fn hot_then_asleep_for(slept: Duration) -> (Fixture, Instant, usize) {
    let f = manual_fan();
    let now = ticks(&f, Instant::now(), Duration::from_secs(3), 96);
    power(&f, &[PowerEvent::Suspending]);
    let before = sent_count(&f);
    power(&f, &[PowerEvent::Resumed]);
    pause_ends();
    // The ticks follow at the wall clock after the sleep.
    let now = ticks_later(&f, now, Duration::ZERO, 60, slept);
    (f, now, before)
}

#[test]
fn cool_reads_after_a_long_sleep_end_full_speed_after_the_short_hold() {
    let (f, now, before) = hot_then_asleep_for(Duration::from_secs(600));

    ticks_later(
        &f,
        now,
        ui::FAN_GUARD_WAKE_HOLD + Duration::from_secs(4),
        60,
        Duration::from_secs(600),
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
    let (f, now, before) = hot_then_asleep_for(Duration::from_secs(60));

    ticks_later(
        &f,
        now,
        ui::FAN_GUARD_WAKE_HOLD * 2,
        60,
        Duration::from_secs(60),
    );

    let top = BLADE_18.custom.fan_rpm.max;
    assert_eq!(fan_speeds_since(&f, before), [top; 4]);
}
