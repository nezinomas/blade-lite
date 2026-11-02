//! The poll schedule, over a fake clock.

use std::time::{Duration, Instant};

use blade_core::{POLL_INTERVAL, PollSchedule, View};

/// A clock that moves only when told to, in steps of 100 ms.
struct FakeClock {
    start: Instant,
    now: Instant,
}

const TICK: Duration = Duration::from_millis(100);

impl FakeClock {
    fn new() -> Self {
        let start = Instant::now();
        Self { start, now: start }
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
