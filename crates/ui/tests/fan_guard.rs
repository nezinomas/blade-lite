//! The fan guard: a manual Custom fan goes to full speed while the CPU or
//! GPU stays hot, and back once both have cooled.

use std::time::{Duration, Instant};

use ui::{FAN_GUARD_COOL_HOLD, FAN_GUARD_CPU_UNREAD_HOLD, FAN_GUARD_WAKE_HOLD, FanGuard, Sensor};

const SECOND: Duration = Duration::from_secs(1);
/// A hot spell long enough for any test.
const HOT_TIME: Duration = Duration::from_secs(30);

/// Feeds a reading every second for `time`, and returns each change.
fn feed(
    guard: &mut FanGuard,
    start: Instant,
    time: Duration,
    cpu: Option<u32>,
    gpu: Option<u32>,
) -> (Instant, Vec<bool>) {
    let mut changes = Vec::new();
    let mut now = start;
    while now <= start + time {
        changes.extend(guard.note(cpu, gpu, now));
        now += SECOND;
    }
    (now, changes)
}

#[test]
fn a_cpu_that_stays_hot_turns_the_guard_on_once() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(&mut guard, Instant::now(), HOT_TIME * 2, Some(95), Some(60));
    assert_eq!(changes, [true]);
    assert!(guard.on());
}

#[test]
fn a_hot_gpu_turns_it_on_too() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(&mut guard, Instant::now(), HOT_TIME, Some(60), Some(87));
    assert_eq!(changes, [true]);
}

#[test]
fn one_hot_reading_turns_the_guard_on_at_once() {
    let mut guard = FanGuard::default();

    let changed = guard.note(Some(95), None, Instant::now());

    assert_eq!(changed, Some(true));
}

#[test]
fn a_reading_under_the_limit_does_nothing() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(&mut guard, Instant::now(), HOT_TIME, Some(94), Some(86));
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn the_guard_goes_off_only_after_both_cool_for_the_cool_hold() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), Some(80));
    assert!(guard.on());
    // Under the limit but not cool yet (the cool lines): stays on.
    let (now, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(90), Some(70));
    assert!(changes.is_empty(), "{changes:?}");
    // Cool, but not for long enough.
    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(2 * SECOND),
        Some(80),
        Some(60),
    );
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(&mut guard, now, 3 * SECOND, Some(80), Some(60));
    assert_eq!(changes, [false]);
}

#[test]
fn a_warm_gpu_keeps_the_guard_on() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), Some(60));
    let (_, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(70), Some(80));
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

/// Laptop check 19B (ticket 175): at full fans a steady GPU load settled at
/// 74 to 76 °C, under the old 77 °C cool line, and the guard went off
/// mid-load.
#[test]
fn a_gpu_under_load_at_full_fans_keeps_the_guard_on() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(70), Some(87));
    let (_, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 5, Some(70), Some(74));
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

#[test]
fn the_gpu_is_cool_20_degrees_under_its_limit() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(70), Some(87));
    let (now, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(70), Some(67));
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD + SECOND,
        Some(70),
        Some(66),
    );
    assert_eq!(changes, [false]);
}

#[test]
fn each_cool_line_is_under_its_guard_limit() {
    for sensor in [Sensor::Cpu, Sensor::Gpu] {
        assert!(
            sensor.guard_cool_line() < sensor.guard_limit(),
            "{sensor:?}"
        );
    }
}

#[test]
fn an_asleep_gpu_does_not_keep_it_on_but_an_unread_cpu_does() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), None);
    // No CPU reading: it may still be hot, so the guard stays on.
    let (now, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, None, None);
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD + SECOND,
        Some(70),
        None,
    );
    assert_eq!(changes, [false]);
}

#[test]
fn a_cpu_swinging_around_the_limit_turns_it_on_once() {
    let mut guard = FanGuard::default();
    let mut now = Instant::now();
    let mut changes = Vec::new();
    for second in 0..=HOT_TIME.as_secs() * 2 {
        let cpu = if second % 2 == 0 { 96 } else { 94 };
        changes.extend(guard.note(Some(cpu), None, now));
        now += SECOND;
    }
    assert_eq!(changes, [true]);
}

#[test]
fn a_gpu_that_went_to_sleep_hot_lets_it_go_off_once_the_cpu_cools() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(60), Some(90));
    // Asleep: not read, and no read failed.
    let (_, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD + SECOND,
        Some(60),
        None,
    );
    assert_eq!(changes, [false]);
}

#[test]
fn a_gpu_whose_reads_fail_keeps_it_on_until_read_cool() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(60), Some(90));
    guard.gpu_failed(true);
    let (now, changes) = feed(&mut guard, now, Duration::from_secs(600), Some(60), None);
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());

    guard.gpu_failed(false);
    let (_, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD + SECOND,
        Some(60),
        Some(60),
    );
    assert_eq!(changes, [false]);
}

/// Not believed: it does not turn the guard on at once, but counts as no
/// reading (ticket 251).
#[test]
fn a_cpu_reading_past_any_real_heat_counts_as_no_reading() {
    let mut guard = FanGuard::default();
    let start = Instant::now();
    let (now, changes) = feed(&mut guard, start, UNREAD_LESS, Some(200), None);
    assert!(changes.is_empty(), "{changes:?}");
    assert_eq!(guard.note(Some(200), None, now), Some(true));
}

/// Just under [`FAN_GUARD_CPU_UNREAD_HOLD`] of readings a second apart.
const UNREAD_LESS: Duration = FAN_GUARD_CPU_UNREAD_HOLD.saturating_sub(SECOND);

/// Ticket 251: a guard that is off turns on when the CPU can't be read.
#[test]
fn a_cpu_not_read_for_the_unread_hold_turns_the_guard_on() {
    let mut guard = FanGuard::default();
    let (now, changes) = feed(&mut guard, Instant::now(), UNREAD_LESS, None, Some(60));
    assert!(changes.is_empty(), "{changes:?}");
    assert!(!guard.cpu_unread());

    assert_eq!(guard.note(None, Some(60), now), Some(true));
    assert!(guard.cpu_unread());
}

#[test]
fn a_cpu_reading_starts_the_unread_hold_again() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), UNREAD_LESS, None, None);
    guard.note(Some(60), None, now);

    let (_, changes) = feed(&mut guard, now + SECOND, UNREAD_LESS, None, None);

    assert!(changes.is_empty(), "{changes:?}");
}

/// After a sleep: the app read nothing for a while, so the time between
/// does not count.
#[test]
fn a_gap_in_the_readings_starts_the_unread_hold_again() {
    let mut guard = FanGuard::default();
    let start = Instant::now();
    guard.note(None, None, start);

    let later = start + FAN_GUARD_CPU_UNREAD_HOLD * 10;
    let (_, changes) = feed(&mut guard, later, UNREAD_LESS, None, None);

    assert!(changes.is_empty(), "{changes:?}");
}

/// While the app may not read the controller (a resume waits, or Razer's
/// software runs), a CPU not read is not a read that failed.
#[test]
fn a_cpu_not_tried_does_not_count_as_unread() {
    let mut guard = FanGuard::default();
    let start = Instant::now();
    let mut now = start;
    while now <= start + FAN_GUARD_CPU_UNREAD_HOLD * 3 {
        guard.cpu_tried(false);
        guard.note(None, None, now);
        now += SECOND;
    }
    assert!(!guard.on());

    let (_, changes) = feed(&mut guard, now, UNREAD_LESS, None, None);

    assert!(changes.is_empty(), "{changes:?}");
}

/// The read not tried comes just as the unread hold ends (review of 251).
#[test]
fn a_cpu_not_tried_at_the_end_of_the_unread_hold_does_not_turn_it_on() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), UNREAD_LESS, None, None);

    guard.cpu_tried(false);
    assert_eq!(guard.note(None, None, now), None);
    let (_, changes) = feed(&mut guard, now + SECOND, UNREAD_LESS, None, None);

    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn a_hot_gpu_with_an_unread_cpu_is_not_on_for_the_unread_cpu() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_CPU_UNREAD_HOLD,
        None,
        Some(90),
    );
    assert_eq!(changes, [true]);
    assert!(!guard.on_for_unread_cpu());
}

#[test]
fn a_guard_on_for_an_unread_cpu_goes_off_once_it_reads_cool() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_CPU_UNREAD_HOLD,
        None,
        None,
    );
    assert!(guard.on());
    assert!(guard.on_for_unread_cpu());

    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(2 * SECOND),
        Some(60),
        None,
    );
    assert!(changes.is_empty(), "{changes:?}");
    assert!(!guard.cpu_unread());
    let (_, changes) = feed(&mut guard, now, 3 * SECOND, Some(60), None);
    assert_eq!(changes, [false]);
}

#[test]
fn a_gpu_that_fails_then_sleeps_keeps_it_on_for_the_cool_hold() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(60), Some(90));
    guard.gpu_failed(true);
    let (now, _) = feed(&mut guard, now, Duration::from_secs(120), Some(60), None);
    // One tick says asleep: not yet cool.
    guard.gpu_failed(false);
    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(5 * SECOND),
        Some(60),
        None,
    );
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(&mut guard, now, 10 * SECOND, Some(60), None);
    assert_eq!(changes, [false]);
}

#[test]
fn a_gpu_reading_past_any_real_heat_counts_as_a_failed_read() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), Some(60));
    let (_, changes) = feed(
        &mut guard,
        now,
        Duration::from_secs(300),
        Some(60),
        Some(255),
    );
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

#[test]
fn it_says_when_only_failed_gpu_reads_hold_it_on() {
    let mut guard = FanGuard::default();
    guard.gpu_failed(true);
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), None);
    assert!(!guard.held_by_gpu_fault(), "the CPU is hot");
    let (now, _) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(SECOND),
        Some(60),
        None,
    );
    assert!(
        !guard.held_by_gpu_fault(),
        "the CPU has not cooled long enough"
    );

    let (now, _) = feed(&mut guard, now, 2 * SECOND, Some(60), None);
    assert!(guard.on());
    assert!(guard.held_by_gpu_fault());

    // Read again: held by its own cool hold now, not by the fault.
    guard.gpu_failed(false);
    guard.note(Some(60), Some(60), now);
    assert!(guard.on());
    assert!(!guard.held_by_gpu_fault());
}

#[test]
fn it_is_not_held_by_a_gpu_fault_while_off() {
    let mut guard = FanGuard::default();
    guard.gpu_failed(true);
    feed(&mut guard, Instant::now(), HOT_TIME, Some(60), None);
    assert!(!guard.on());
    assert!(!guard.held_by_gpu_fault());
}

/// A guard on from a hot CPU, then told of a long sleep (ticket 172).
fn on_and_woke() -> (FanGuard, Instant) {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), HOT_TIME, Some(96), Some(60));
    assert!(guard.woke());
    (guard, now)
}

#[test]
fn cool_readings_after_a_long_sleep_end_it_after_the_short_hold() {
    let (mut guard, now) = on_and_woke();

    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_WAKE_HOLD.saturating_sub(SECOND),
        Some(70),
        Some(60),
    );
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(&mut guard, now, Duration::ZERO, Some(70), Some(60));
    assert_eq!(changes, [false]);
}

#[test]
fn a_sleeping_gpu_counts_as_cool_after_a_long_sleep() {
    let (mut guard, now) = on_and_woke();
    let (_, changes) = feed(&mut guard, now, FAN_GUARD_WAKE_HOLD, Some(70), None);
    assert_eq!(changes, [false]);
}

/// After the wake, one reading that is not cool, then cool ones: the usual
/// cool hold.
fn usual_hold_after(cpu: Option<u32>, gpu: Option<u32>, gpu_failed: bool) {
    let (mut guard, now) = on_and_woke();
    guard.gpu_failed(gpu_failed);
    guard.note(cpu, gpu, now);
    guard.gpu_failed(false);
    let now = now + SECOND;

    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(5 * SECOND),
        Some(70),
        Some(60),
    );
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(&mut guard, now, 10 * SECOND, Some(70), Some(60));
    assert_eq!(changes, [false]);
}

#[test]
fn a_warm_cpu_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(Some(85), Some(60), false);
}

#[test]
fn a_warm_gpu_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(Some(70), Some(77), false);
}

#[test]
fn a_gpu_under_load_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(Some(70), Some(74), false);
}

#[test]
fn an_unread_cpu_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(None, Some(60), false);
}

#[test]
fn a_failed_gpu_read_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(Some(70), None, true);
}

#[test]
fn a_gpu_reading_past_any_real_heat_after_a_long_sleep_keeps_the_usual_hold() {
    usual_hold_after(Some(70), Some(255), false);
}

#[test]
fn a_gap_in_the_readings_after_a_long_sleep_keeps_the_usual_hold() {
    let (mut guard, now) = on_and_woke();
    guard.note(Some(70), Some(60), now);

    // Three reads missed at the 2 s guard interval.
    let later = now + 7 * SECOND;
    let (_, changes) = feed(&mut guard, later, FAN_GUARD_WAKE_HOLD, Some(70), Some(60));

    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

#[test]
fn reads_6_s_apart_after_a_long_sleep_keep_the_short_hold() {
    let (mut guard, now) = on_and_woke();
    let changes: Vec<bool> = (0..5)
        .filter_map(|step| guard.note(Some(70), Some(60), now + step * 6 * SECOND))
        .collect();
    assert_eq!(changes, [false]);
}

#[test]
fn a_long_sleep_does_nothing_to_a_guard_that_is_off() {
    let mut guard = FanGuard::default();
    assert!(!guard.woke());
    let (_, changes) = feed(&mut guard, Instant::now(), HOT_TIME, Some(70), Some(60));
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn a_hot_spell_after_the_short_hold_turns_it_on_again() {
    let (mut guard, now) = on_and_woke();
    let (now, _) = feed(&mut guard, now, FAN_GUARD_WAKE_HOLD, Some(70), None);
    assert!(!guard.on());

    let (_, changes) = feed(&mut guard, now, SECOND, Some(96), None);

    assert_eq!(changes, [true]);
}
