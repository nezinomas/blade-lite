//! The poll schedule, over a fake clock.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{GUARD_HEAT_INTERVAL, HEAT_INTERVAL, POLL_INTERVAL, PollSchedule, View};

/// A clock that moves only when told to, in steps of 100 ms.
struct FakeClock {
    start: Instant,
    now: Instant,
    /// How far the wall clock ran ahead of `now`, as in a sleep that
    /// `Instant` did not count.
    wall_ahead: Duration,
}

const TICK: Duration = Duration::from_millis(100);

impl FakeClock {
    /// The wall clock: it moves with `now`, unless a test moves it apart.
    fn wall(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(1_000_000)
            + (self.now - self.start)
            + self.wall_ahead
    }

    fn new() -> Self {
        let start = Instant::now();
        Self {
            start,
            now: start,
            wall_ahead: Duration::ZERO,
        }
    }

    /// Ticks for `duration`, and returns when the schedule polled, from the start.
    fn run(&mut self, schedule: &mut PollSchedule, duration: Duration) -> Vec<Duration> {
        let end = self.now + duration;
        let mut polls = Vec::new();
        while self.now < end {
            if schedule.poll_due(self.now) {
                polls.push(self.now - self.start);
            }
            self.now += TICK;
        }
        polls
    }
}

fn secs(values: &[u64]) -> Vec<Duration> {
    values.iter().map(|&s| Duration::from_secs(s)).collect()
}

#[test]
fn the_interval_is_one_second() {
    assert_eq!(POLL_INTERVAL, Duration::from_secs(1));
}

#[test]
fn nothing_polls_while_no_view_is_visible() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();

    assert_eq!(clock.run(&mut schedule, Duration::from_secs(5)), secs(&[]));
    assert_eq!(schedule.next_poll(), None);
}

#[test]
fn a_visible_view_polls_at_once_then_once_a_second() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();

    schedule.show(View::Window, clock.now);

    assert_eq!(
        clock.run(&mut schedule, Duration::from_secs(4)),
        secs(&[0, 1, 2, 3])
    );
}

#[test]
fn a_second_view_does_not_poll_more_often() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Window, clock.now);
    clock.run(&mut schedule, Duration::from_millis(500));

    schedule.show(View::Flyout, clock.now);

    assert_eq!(
        clock.run(&mut schedule, Duration::from_millis(2500)),
        secs(&[1, 2])
    );
}

#[test]
fn polling_goes_on_while_one_view_is_still_visible() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Window, clock.now);
    schedule.show(View::Flyout, clock.now);
    clock.run(&mut schedule, Duration::from_secs(1));

    schedule.hide(View::Window);

    assert_eq!(
        clock.run(&mut schedule, Duration::from_secs(2)),
        secs(&[1, 2])
    );
}

#[test]
fn polling_stops_when_both_views_are_closed() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Window, clock.now);
    schedule.show(View::Flyout, clock.now);
    clock.run(&mut schedule, Duration::from_secs(1));

    schedule.hide(View::Window);
    schedule.hide(View::Flyout);

    assert_eq!(clock.run(&mut schedule, Duration::from_secs(5)), secs(&[]));
    assert_eq!(schedule.next_poll(), None);
}

#[test]
fn reopening_a_view_polls_at_once() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Flyout, clock.now);
    clock.run(&mut schedule, Duration::from_millis(300));
    schedule.hide(View::Flyout);
    clock.run(&mut schedule, Duration::from_millis(400));

    schedule.show(View::Flyout, clock.now);

    assert_eq!(schedule.next_poll(), Some(clock.now));
    assert_eq!(
        clock.run(&mut schedule, Duration::from_millis(1500)),
        vec![Duration::from_millis(700), Duration::from_millis(1700)]
    );
}

#[test]
fn a_late_poll_does_not_catch_up_with_a_burst() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Window, clock.now);
    assert!(schedule.poll_due(clock.now));

    clock.now += Duration::from_secs(10);

    assert!(schedule.poll_due(clock.now));
    assert!(!schedule.poll_due(clock.now));
    assert_eq!(schedule.next_poll(), Some(clock.now + POLL_INTERVAL));
}

/// Ticks for `duration`, and returns when a heat read was due, from `start`.
fn heat_reads(
    clock: &mut FakeClock,
    schedule: &mut PollSchedule,
    duration: Duration,
) -> Vec<Duration> {
    let end = clock.now + duration;
    let mut reads = Vec::new();
    while clock.now < end {
        if schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL) {
            reads.push(clock.now - clock.start);
        }
        clock.now += TICK;
    }
    reads
}

#[test]
fn the_heat_interval_is_ten_seconds() {
    assert_eq!(HEAT_INTERVAL, Duration::from_secs(10));
}

#[test]
fn with_no_view_visible_the_heat_is_read_at_once_then_every_ten_seconds() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();

    assert_eq!(
        heat_reads(&mut clock, &mut schedule, Duration::from_secs(25)),
        secs(&[0, 10, 20])
    );
}

#[test]
fn no_heat_read_while_a_view_polls() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Flyout, clock.now);

    assert_eq!(
        heat_reads(&mut clock, &mut schedule, Duration::from_secs(25)),
        secs(&[])
    );
}

#[test]
fn heat_reads_start_when_the_last_view_closes() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    schedule.show(View::Window, clock.now);
    heat_reads(&mut clock, &mut schedule, Duration::from_secs(15));

    schedule.hide(View::Window);

    assert_eq!(
        heat_reads(&mut clock, &mut schedule, Duration::from_secs(11)),
        secs(&[15, 25])
    );
}

#[test]
fn a_heat_read_long_overdue_waits_one_more_interval() {
    // The event loop stood still, as in a sleep: the controller may not be
    // awake yet, and the wake may not be known yet.
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));

    clock.now += Duration::from_secs(60);

    assert!(!schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));
    assert_eq!(
        heat_reads(&mut clock, &mut schedule, Duration::from_secs(11)),
        secs(&[70])
    );
}

#[test]
fn a_heat_read_a_little_late_still_runs() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));

    clock.now += HEAT_INTERVAL * 2;

    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));
}

#[test]
fn a_heat_read_after_a_sleep_instant_did_not_count_waits_one_more_interval() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));

    clock.now += HEAT_INTERVAL;
    clock.wall_ahead = Duration::from_secs(600);

    assert!(!schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));
    assert_eq!(
        heat_reads(&mut clock, &mut schedule, Duration::from_secs(11)),
        secs(&[20])
    );
}

#[test]
fn a_wall_clock_set_back_does_not_stop_heat_reads() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    clock.wall_ahead = Duration::from_secs(600);
    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));

    clock.now += HEAT_INTERVAL;
    clock.wall_ahead = Duration::ZERO;

    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));
}

#[test]
fn a_switch_to_the_guard_interval_reads_within_it() {
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    assert!(schedule.heat_due(clock.now, clock.wall(), HEAT_INTERVAL));
    clock.now += Duration::from_secs(5);

    // A manual fan starts: the read 2 s after the last one is due now.
    assert!(schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));
    clock.now += GUARD_HEAT_INTERVAL;
    assert!(schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));
}

#[test]
fn a_skipped_guard_read_still_waits_the_long_interval() {
    // After a sleep the wake event gets the same time as before to come.
    let mut clock = FakeClock::new();
    let mut schedule = PollSchedule::default();
    assert!(schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));
    clock.now += Duration::from_secs(60);
    assert!(!schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));

    clock.now += GUARD_HEAT_INTERVAL;
    assert!(!schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));
    clock.now += HEAT_INTERVAL.saturating_sub(GUARD_HEAT_INTERVAL);
    assert!(schedule.heat_due(clock.now, clock.wall(), GUARD_HEAT_INTERVAL));
}
