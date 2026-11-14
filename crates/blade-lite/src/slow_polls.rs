//! Logs polls that hold the event loop (ticket 206). Each read on a poll
//! runs on the event-loop thread, so while one waits the window can't
//! scroll or redraw. Only logs: nothing is sent.

use std::io;
use std::time::{Duration, Instant};

use platform::{BatteryHealth, CpuTimes, GpuReading, PowerReading, Sensors};

/// A poll that runs this long, or begins this late, counts as slow: a
/// scroll stalls visibly well before it.
pub const SLOW_POLL: Duration = Duration::from_millis(100);

/// At most one line per this time, so a slow poll each second can't flood
/// the log. The line names the slowest poll since the last one.
pub const SLOW_POLL_QUIET: Duration = Duration::from_mins(1);

/// A poll this late means the laptop slept (as the watchdog's), not a busy
/// event loop. A hold this long is not logged here; one of 30 s or more is
/// the watchdog's.
const SLEPT: Duration = Duration::from_secs(15);

/// Where one poll's time went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollTime {
    /// How long the poll ran.
    pub took: Duration,
    /// How much later it began than it was due ([`poll_late`]): the event
    /// loop was busy with something else (a redraw, a click).
    pub late: Duration,
    /// The GPU's power state and NVML reads.
    pub gpu: Duration,
    /// The other Windows sensor reads: CPU load and the battery.
    pub sensors: Duration,
    /// The controller's sensor reads (CPU temperature, fans).
    pub controller: Duration,
}

impl PollTime {
    fn worst(&self) -> Duration {
        self.took.max(self.late)
    }

    /// The poll's time outside the timed reads: other controller sends and
    /// reads, the power events, the window's fill.
    fn rest(&self) -> Duration {
        self.took
            .saturating_sub(self.gpu)
            .saturating_sub(self.sensors)
            .saturating_sub(self.controller)
    }
}

/// How late a poll that began at `began` is, after the last one that began
/// at `last_began` and ended at `last_ended`, each `interval`. Slint arms a
/// repeated timer again as it fires, so a poll is due `interval` after the
/// last one began, or at its end if it ran longer: its own time is not
/// counted twice.
#[must_use]
pub fn poll_late(
    last_began: Instant,
    last_ended: Instant,
    began: Instant,
    interval: Duration,
) -> Duration {
    let due = last_began
        .checked_add(interval)
        .map_or(last_ended, |due| due.max(last_ended));
    began.saturating_duration_since(due)
}

/// The slow polls since the last line.
#[derive(Debug, Default)]
pub struct SlowPolls {
    said_at: Option<Instant>,
    count: u32,
    slowest: Option<(PollTime, Instant)>,
}

impl SlowPolls {
    /// Notes a poll that ended at `now`. A line to log, at most once per
    /// [`SLOW_POLL_QUIET`] (the first at once), if a poll since the last
    /// line was slow.
    pub fn note(&mut self, mut poll: PollTime, now: Instant) -> Option<String> {
        if poll.late >= SLEPT {
            poll.late = Duration::ZERO;
        }
        if poll.worst() >= SLOW_POLL {
            self.count = self.count.saturating_add(1);
            if self
                .slowest
                .is_none_or(|(slowest, _)| poll.worst() > slowest.worst())
            {
                self.slowest = Some((poll, now));
            }
        }
        let (slowest, at) = self.slowest?;
        if self
            .said_at
            .is_some_and(|said| now.saturating_duration_since(said) < SLOW_POLL_QUIET)
        {
            return None;
        }
        self.said_at = Some(now);
        self.slowest = None;
        let count = std::mem::take(&mut self.count);
        Some(format!(
            "the event loop was held: {count} slow poll(s) since the last such line; the slowest ({} s ago) ran {} ms (GPU {} ms, other sensors {} ms, controller sensors {} ms, the rest {} ms) and began {} ms late",
            now.saturating_duration_since(at).as_secs(),
            slowest.took.as_millis(),
            slowest.gpu.as_millis(),
            slowest.sensors.as_millis(),
            slowest.controller.as_millis(),
            slowest.rest().as_millis(),
            slowest.late.as_millis(),
        ))
    }
}

/// Sensors that keep how long their reads took, the GPU's apart.
#[derive(Debug, Default)]
pub struct TimedSensors<S> {
    inner: S,
    gpu: Duration,
    other: Duration,
}

impl<S> TimedSensors<S> {
    /// The GPU's time and the other reads' time since the last take.
    pub fn take_times(&mut self) -> (Duration, Duration) {
        (
            std::mem::take(&mut self.gpu),
            std::mem::take(&mut self.other),
        )
    }
}

/// Runs `read`, adding its time to `total`.
fn timed<T>(total: &mut Duration, read: impl FnOnce() -> T) -> T {
    let began = Instant::now();
    let value = read();
    *total = total.saturating_add(began.elapsed());
    value
}

impl<S: Sensors> Sensors for TimedSensors<S> {
    fn cpu_times(&mut self) -> io::Result<CpuTimes> {
        timed(&mut self.other, || self.inner.cpu_times())
    }

    fn power(&mut self) -> io::Result<PowerReading> {
        timed(&mut self.other, || self.inner.power())
    }

    fn battery_health(&mut self) -> io::Result<BatteryHealth> {
        timed(&mut self.other, || self.inner.battery_health())
    }

    fn gpu_awake(&mut self) -> io::Result<bool> {
        timed(&mut self.gpu, || self.inner.gpu_awake())
    }

    fn gpu(&mut self) -> io::Result<GpuReading> {
        timed(&mut self.gpu, || self.inner.gpu())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn poll(took_ms: u32, late_ms: u32) -> PollTime {
        PollTime {
            took: MS * took_ms,
            late: MS * late_ms,
            gpu: MS * (took_ms / 2),
            sensors: MS * 2,
            controller: MS * 3,
        }
    }

    #[test]
    fn quick_polls_say_nothing() {
        let mut slow = SlowPolls::default();
        let start = Instant::now();
        for second in 0..120 {
            let line = slow.note(poll(99, 99), start + Duration::from_secs(second));
            assert_eq!(line, None);
        }
    }

    #[test]
    fn the_first_slow_poll_is_said_at_once_with_its_parts() {
        let mut slow = SlowPolls::default();
        let line = slow.note(poll(3100, 0), Instant::now());
        assert_eq!(
            line.as_deref(),
            Some(
                "the event loop was held: 1 slow poll(s) since the last such line; the slowest (0 s ago) ran 3100 ms (GPU 1550 ms, other sensors 2 ms, controller sensors 3 ms, the rest 1545 ms) and began 0 ms late"
            )
        );
    }

    #[test]
    fn a_poll_of_exactly_the_limit_is_slow() {
        let mut slow = SlowPolls::default();
        assert!(slow.note(poll(100, 0), Instant::now()).is_some());
    }

    #[test]
    fn a_poll_is_due_an_interval_after_the_last_began_or_at_its_end() {
        let start = Instant::now();
        let at = |ms| start + MS * ms;
        let second = Duration::from_secs(1);
        // A quick last poll: due 1 s after it began.
        assert_eq!(poll_late(at(0), at(20), at(1000), second), Duration::ZERO);
        assert_eq!(poll_late(at(0), at(20), at(1500), second), MS * 500);
        // A last poll of 3 s: due at its end, not 2 s late.
        assert_eq!(poll_late(at(0), at(3000), at(3001), second), MS);
        // Early or at once is not late.
        assert_eq!(poll_late(at(0), at(20), at(900), second), Duration::ZERO);
    }

    #[test]
    fn a_late_poll_counts_as_slow() {
        let mut slow = SlowPolls::default();
        let line = slow.note(poll(10, 400), Instant::now());
        assert!(line.is_some_and(|line| line.ends_with("began 400 ms late")));
    }

    #[test]
    fn a_sleep_is_not_a_late_poll() {
        let mut slow = SlowPolls::default();
        assert_eq!(slow.note(poll(10, 15_000), Instant::now()), None);
    }

    #[test]
    fn later_slow_polls_wait_a_minute_and_the_slowest_is_said() {
        let mut slow = SlowPolls::default();
        let start = Instant::now();
        let at = |second| start + Duration::from_secs(second);
        assert!(slow.note(poll(200, 0), at(0)).is_some());
        assert_eq!(slow.note(poll(150, 0), at(10)), None);
        assert_eq!(slow.note(poll(900, 0), at(20)), None);
        assert_eq!(slow.note(poll(300, 0), at(30)), None);
        assert_eq!(slow.note(poll(10, 0), at(59)), None);
        // The next poll after the quiet minute says it, slow or not.
        let line = slow.note(poll(10, 0), at(60)).unwrap_or_default();
        assert!(line.contains("3 slow poll(s)"), "{line}");
        assert!(line.contains("(40 s ago) ran 900 ms"), "{line}");
        assert_eq!(slow.note(poll(10, 0), at(200)), None);
    }

    #[derive(Default)]
    struct Fake;

    impl Sensors for Fake {
        fn cpu_times(&mut self) -> io::Result<CpuTimes> {
            std::thread::sleep(MS * 5);
            Err(io::Error::other("cpu"))
        }
        fn power(&mut self) -> io::Result<PowerReading> {
            Err(io::Error::other("power"))
        }
        fn battery_health(&mut self) -> io::Result<BatteryHealth> {
            Err(io::Error::other("health"))
        }
        fn gpu_awake(&mut self) -> io::Result<bool> {
            Ok(true)
        }
        fn gpu(&mut self) -> io::Result<GpuReading> {
            std::thread::sleep(MS * 20);
            Ok(GpuReading {
                celsius: 40,
                load_percent: 1,
            })
        }
    }

    #[test]
    fn timed_sensors_pass_the_reads_on_and_keep_the_gpu_time_apart() {
        let mut sensors = TimedSensors::<Fake>::default();
        assert!(sensors.cpu_times().is_err());
        assert!(sensors.gpu_awake().is_ok_and(|awake| awake));
        assert_eq!(sensors.gpu().ok().map(|gpu| gpu.celsius), Some(40));
        let (gpu, other) = sensors.take_times();
        // The fake's GPU read sleeps 20 ms, its CPU read 5 ms and the rest
        // nothing; a busy runner only adds time, so the GPU's stays apart
        // if it is at least its own sleep and the rest well under it.
        assert!(gpu >= MS * 20 && other >= MS * 5, "{gpu:?} {other:?}");
        assert!(gpu >= other, "{gpu:?} {other:?}");
        assert_eq!(sensors.take_times(), (Duration::ZERO, Duration::ZERO));
    }
}
