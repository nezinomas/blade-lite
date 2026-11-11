//! The fan guard: a manual Custom fan goes to full speed while the CPU or
//! GPU stays hot, and back once both have cooled.

use std::time::{Duration, Instant};

use ui::{FAN_GUARD_COOL_HOLD, FAN_GUARD_HOLD, FanGuard};

const SECOND: Duration = Duration::from_secs(1);

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
fn a_hot_cpu_for_the_hold_turns_the_guard_on_once() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD * 2,
        Some(95),
        Some(60),
    );
    assert_eq!(changes, [true]);
    assert!(guard.on());
}

#[test]
fn a_hot_gpu_turns_it_on_too() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD,
        Some(60),
        Some(87),
    );
    assert_eq!(changes, [true]);
}

#[test]
fn a_short_hot_spell_does_nothing() {
    let mut guard = FanGuard::default();
    let start = Instant::now();
    let (now, _) = feed(
        &mut guard,
        start,
        FAN_GUARD_HOLD.saturating_sub(2 * SECOND),
        Some(99),
        None,
    );
    // A reading well under the limit starts the hold again.
    let (now, _) = feed(&mut guard, now, Duration::ZERO, Some(91), None);
    let (_, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_HOLD.saturating_sub(2 * SECOND),
        Some(99),
        None,
    );
    assert!(changes.is_empty(), "{changes:?}");
    assert!(!guard.on());
}

#[test]
fn the_guard_goes_off_only_after_both_cool_for_the_cool_hold() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD,
        Some(96),
        Some(80),
    );
    assert!(guard.on());
    // Under the limit but not cool yet (10 °C margin): stays on.
    let (now, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(90), Some(70));
    assert!(changes.is_empty(), "{changes:?}");
    // Cool, but not for long enough.
    let (now, changes) = feed(
        &mut guard,
        now,
        FAN_GUARD_COOL_HOLD.saturating_sub(2 * SECOND),
        Some(80),
        Some(70),
    );
    assert!(changes.is_empty(), "{changes:?}");
    let (_, changes) = feed(&mut guard, now, 3 * SECOND, Some(80), Some(70));
    assert_eq!(changes, [false]);
}

#[test]
fn a_warm_gpu_keeps_the_guard_on() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD,
        Some(96),
        Some(60),
    );
    let (_, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(70), Some(80));
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

#[test]
fn an_asleep_gpu_does_not_keep_it_on_but_an_unread_cpu_does() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(&mut guard, Instant::now(), FAN_GUARD_HOLD, Some(96), None);
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
fn a_cpu_swinging_around_the_limit_still_turns_it_on() {
    let mut guard = FanGuard::default();
    let mut now = Instant::now();
    let mut changes = Vec::new();
    for second in 0..=FAN_GUARD_HOLD.as_secs() * 2 {
        let cpu = if second % 2 == 0 { 96 } else { 94 };
        changes.extend(guard.note(Some(cpu), None, now));
        now += SECOND;
    }
    assert_eq!(changes, [true]);
}

#[test]
fn a_hot_gpu_that_is_no_longer_read_keeps_it_on() {
    let mut guard = FanGuard::default();
    let (now, _) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD,
        Some(60),
        Some(90),
    );
    let (_, changes) = feed(&mut guard, now, FAN_GUARD_COOL_HOLD * 2, Some(60), None);
    assert!(changes.is_empty(), "{changes:?}");
    assert!(guard.on());
}

#[test]
fn readings_far_apart_start_the_hold_again() {
    let mut guard = FanGuard::default();
    let mut now = Instant::now();
    for _ in 0..10 {
        assert_eq!(guard.note(Some(99), None, now), None);
        now += Duration::from_secs(31);
    }
    assert!(!guard.on());
}

#[test]
fn a_reading_past_any_real_heat_is_not_believed() {
    let mut guard = FanGuard::default();
    let (_, changes) = feed(
        &mut guard,
        Instant::now(),
        FAN_GUARD_HOLD * 2,
        Some(200),
        None,
    );
    assert!(changes.is_empty(), "{changes:?}");
}
