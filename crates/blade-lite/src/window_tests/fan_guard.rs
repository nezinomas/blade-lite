//! The fan guard fed by the sensor reads of the ticks (ticket 163): the
//! GPU comes from the fake sensors, the CPU from the fake controller.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset};
use razer_proto::BLADE_18;

use super::{FAKE_GPU, FakeGpu, Fixture, fixture, log_path};

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
    let ended = "fan guard: no longer held only by failed GPU reads (CPU 63 °C, GPU 50 °C)";

    let now = ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 3, 63);

    assert_eq!(fan_speeds_since(&f, before), [], "held at full speed");
    assert_eq!(log_lines(held), 1);

    FAKE_GPU.set(FakeGpu::Celsius(50));
    ticks(&f, now, ui::FAN_GUARD_COOL_HOLD * 2, 63);

    assert_eq!(log_lines(ended), 1);
    assert_eq!(log_lines(held), 1);
    assert_eq!(fan_speeds_since(&f, before), [1600; 4]);
}
