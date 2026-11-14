//! The GPU read on a thread of its own (ticket 206). An NVML read can take
//! over 2 s (a GPU going to sleep or waking), and on the event loop it froze
//! the window's scroll. The loop asks the thread for a read and waits for it
//! at most [`GPU_READ_WAIT`]: nearly every read takes a few ms, so the poll
//! gets it. A slower one is a failed read for that poll, which holds an on
//! fan guard on as any failed GPU read does, and is given to the next poll
//! if it is still fresh then. Only reads: nothing is sent.

use std::io;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use platform::{BatteryHealth, CpuTimes, GpuReading, PowerReading, Sensors};

/// How long a poll waits for the read it asked for. On the laptop the reads
/// took 2 to 6 ms, the slow ones 0.4 to 2.3 s (2026-11-14).
pub const GPU_READ_WAIT: Duration = Duration::from_millis(50);

/// The oldest read a poll takes: one that began longer ago than this (by either clock, so also one that ran over a
/// sleep), is a failed read. The slow reads seen took up to 2.3 s. A read is
/// never given to a second poll, so the fan guard never takes an old
/// reading as a new one; after a wake whose first read is slow, the guard
/// keeps its full hold, not a long sleep's short one (ticket 172).
const GPU_READ_MAX_AGE: Duration = Duration::from_secs(3);

/// How long a stopped read thread waits before a new one starts, so a read
/// that panics each time can't start a thread each poll.
const RESTART_PAUSE: Duration = Duration::from_mins(1);

/// One GPU read by the thread: its power state, then its reading if awake.
#[derive(Debug, Clone)]
struct GpuSample {
    /// When the read began, so its age errs on the old side.
    at: Instant,
    wall: SystemTime,
    awake: Result<bool, String>,
    /// `None` unless it read awake.
    reading: Option<Result<GpuReading, String>>,
}

impl GpuSample {
    /// Reads the GPU as the sampler does: NVML only if it is awake, so the
    /// read does not wake it.
    fn read(sensors: &mut impl Sensors) -> Self {
        let (at, wall) = (Instant::now(), SystemTime::now());
        let awake = sensors.gpu_awake().map_err(|error| error.to_string());
        let reading =
            matches!(awake, Ok(true)).then(|| sensors.gpu().map_err(|error| error.to_string()));
        Self {
            at,
            wall,
            awake,
            reading,
        }
    }

    /// Whether it is at most [`GPU_READ_MAX_AGE`] old by both clocks: a
    /// sleep may not count in `Instant`, but it does in the wall clock. A
    /// wall clock set back counts as old.
    fn fresh(&self, now: Instant, wall: SystemTime) -> bool {
        now.saturating_duration_since(self.at) <= GPU_READ_MAX_AGE
            && wall
                .duration_since(self.wall)
                .is_ok_and(|age| age <= GPU_READ_MAX_AGE)
    }
}

/// What the thread shares with the event loop.
#[derive(Debug, Default)]
struct Shared {
    last: Option<GpuSample>,
    /// Reads done, so a poll can wait for one after its ask.
    done: u64,
    /// When the read that runs now began, if one runs.
    running_since: Option<Instant>,
}

type Slot = Arc<(Mutex<Shared>, Condvar)>;

fn lock(slot: &Slot) -> MutexGuard<'_, Shared> {
    slot.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sensors whose GPU reads run on a thread of their own, started at the
/// first read; the other reads go to `S` on the caller's thread. The
/// thread makes its own `G` (made there, so it need not be `Send`).
pub struct BackgroundGpu<S, G = S> {
    inner: S,
    asks: Option<SyncSender<()>>,
    /// When the last thread was found stopped.
    stopped_at: Option<Instant>,
    slot: Slot,
    /// The read [`gpu_awake`](Sensors::gpu_awake) took, for the
    /// [`gpu`](Sensors::gpu) after it.
    taken: Option<GpuSample>,
    /// The count of reads done when a read was last given to a poll, so no
    /// read is given twice.
    given: u64,
    make: fn() -> G,
}

impl<S: Default, G: Default> Default for BackgroundGpu<S, G> {
    fn default() -> Self {
        Self {
            inner: S::default(),
            asks: None,
            stopped_at: None,
            slot: Slot::default(),
            taken: None,
            given: 0,
            make: G::default,
        }
    }
}

impl<S, G: Sensors + 'static> BackgroundGpu<S, G> {
    /// Asks the thread for a read, starting it first if need be. An ask
    /// while one waits is dropped: that one is still to come. A stopped
    /// thread is said, and a new one starts after [`RESTART_PAUSE`].
    fn ask(&mut self, now: Instant) -> io::Result<()> {
        let asks = match &self.asks {
            Some(asks) => asks.clone(),
            None => self.start(now)?,
        };
        match asks.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => {
                self.asks = None;
                self.stopped_at = Some(now);
                lock(&self.slot).running_since = None;
                Err(io::Error::other(format!(
                    "the GPU read's thread stopped (a panic, in the log); a new one starts in {} s",
                    RESTART_PAUSE.as_secs()
                )))
            }
        }
    }

    fn start(&mut self, now: Instant) -> io::Result<SyncSender<()>> {
        if self
            .stopped_at
            .is_some_and(|at| now.saturating_duration_since(at) < RESTART_PAUSE)
        {
            return Err(io::Error::other(
                "the GPU read's thread stopped lately, so a new one waits",
            ));
        }
        let (asks, asked) = mpsc::sync_channel::<()>(1);
        let slot = Arc::clone(&self.slot);
        let make = self.make;
        std::thread::Builder::new()
            .name("GPU read".to_owned())
            .spawn(move || {
                let mut sensors = make();
                while asked.recv().is_ok() {
                    lock(&slot).running_since = Some(Instant::now());
                    let sample = GpuSample::read(&mut sensors);
                    let mut shared = lock(&slot);
                    shared.last = Some(sample);
                    shared.done = shared.done.wrapping_add(1);
                    shared.running_since = None;
                    drop(shared);
                    slot.1.notify_all();
                }
            })?;
        self.asks = Some(asks.clone());
        Ok(asks)
    }

    /// Waits at most [`GPU_READ_WAIT`] for a read to end after `done`
    /// reads, then gives the last read if no poll had it and it is fresh,
    /// else why there is none.
    fn wait_for_read(&mut self, done: u64) -> Result<GpuSample, String> {
        let shared = lock(&self.slot);
        let (shared, _) = self
            .slot
            .1
            .wait_timeout_while(shared, GPU_READ_WAIT, |shared| shared.done == done)
            .unwrap_or_else(PoisonError::into_inner);
        let (now, wall) = (Instant::now(), SystemTime::now());
        if shared.done != self.given
            && let Some(sample) = shared
                .last
                .as_ref()
                .filter(|sample| sample.fresh(now, wall))
        {
            self.given = shared.done;
            return Ok(sample.clone());
        }
        // The varying part in brackets, so the log's quiet time sees one kind.
        Err(match shared.running_since {
            Some(since) => format!(
                "the GPU read is slow, so this poll has none (it has run {} ms)",
                now.saturating_duration_since(since).as_millis()
            ),
            None => {
                "the GPU read is slow, so this poll has none (the last ran too long)".to_owned()
            }
        })
    }
}

impl<S: Sensors, G: Sensors + 'static> Sensors for BackgroundGpu<S, G> {
    fn cpu_times(&mut self) -> io::Result<CpuTimes> {
        self.inner.cpu_times()
    }

    fn power(&mut self) -> io::Result<PowerReading> {
        self.inner.power()
    }

    fn battery_health(&mut self) -> io::Result<BatteryHealth> {
        self.inner.battery_health()
    }

    /// Asks for a read and waits for it at most [`GPU_READ_WAIT`]. Its power
    /// state, or an error if no read ended in the wait.
    fn gpu_awake(&mut self) -> io::Result<bool> {
        self.taken = None;
        let done = lock(&self.slot).done;
        self.ask(Instant::now())?;
        let sample = self.wait_for_read(done).map_err(io::Error::other)?;
        let awake = sample.awake.clone().map_err(io::Error::other);
        self.taken = Some(sample);
        awake
    }

    /// The reading of the read the last [`gpu_awake`](Sensors::gpu_awake)
    /// took, once.
    fn gpu(&mut self) -> io::Result<GpuReading> {
        match self.taken.take().and_then(|sample| sample.reading) {
            Some(reading) => reading.map_err(io::Error::other),
            None => Err(io::Error::other("the GPU read asleep or was not read")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const MS: Duration = Duration::from_millis(1);

    const READING: GpuReading = GpuReading {
        celsius: 50,
        load_percent: 3,
    };

    /// A fake GPU: awake or not, its NVML read taking `DELAY_MS`, counted
    /// in `READS`, and panicking if `PANICS`.
    macro_rules! fake_gpu {
        ($name:ident, awake: $awake:expr, delay_ms: $delay:expr, reads: $reads:ident, panics: $panics:expr) => {
            static $reads: AtomicU32 = AtomicU32::new(0);

            #[derive(Default)]
            struct $name;

            impl Sensors for $name {
                fn cpu_times(&mut self) -> io::Result<CpuTimes> {
                    Err(io::Error::other("cpu"))
                }
                fn power(&mut self) -> io::Result<PowerReading> {
                    Err(io::Error::other("power"))
                }
                fn battery_health(&mut self) -> io::Result<BatteryHealth> {
                    Err(io::Error::other("health"))
                }
                fn gpu_awake(&mut self) -> io::Result<bool> {
                    assert!(!$panics, "a read that panics");
                    Ok($awake)
                }
                fn gpu(&mut self) -> io::Result<GpuReading> {
                    assert!($awake, "an NVML read on a sleeping GPU wakes it");
                    $reads.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(MS * $delay);
                    Ok(READING)
                }
            }
        };
    }

    fake_gpu!(QuickGpu, awake: true, delay_ms: 2, reads: QUICK_READS, panics: false);
    fake_gpu!(SlowGpu, awake: true, delay_ms: 300, reads: SLOW_READS, panics: false);
    fake_gpu!(CountedGpu, awake: true, delay_ms: 300, reads: COUNTED_READS, panics: false);
    fake_gpu!(AsleepGpu, awake: false, delay_ms: 0, reads: ASLEEP_READS, panics: false);
    fake_gpu!(PanicGpu, awake: true, delay_ms: 0, reads: PANIC_READS, panics: true);

    /// Polls until a read comes, for at most 5 s: the thread's start may
    /// take longer than the wait on a busy runner.
    fn warm<G: Sensors + 'static>(sensors: &mut BackgroundGpu<QuickGpu, G>) {
        let until = Instant::now() + Duration::from_secs(5);
        while sensors.gpu_awake().is_err() {
            assert!(Instant::now() < until, "no GPU read came");
            std::thread::sleep(MS * 20);
        }
    }

    #[test]
    fn a_quick_read_is_taken_by_the_poll_that_asked() {
        let mut sensors = BackgroundGpu::<QuickGpu>::default();
        warm(&mut sensors);
        assert!(sensors.gpu_awake().is_ok_and(|awake| awake));
        assert_eq!(sensors.gpu().ok(), Some(READING));
        assert!(QUICK_READS.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn a_slow_read_holds_the_caller_only_for_the_wait() {
        let mut sensors = BackgroundGpu::<QuickGpu, SlowGpu>::default();
        let began = Instant::now();
        let first = sensors.gpu_awake();
        let held = began.elapsed();
        assert!(held < MS * 250, "{held:?}");
        assert!(
            first
                .as_ref()
                .is_err_and(|error| error.to_string().contains("slow")),
            "{first:?}"
        );
        assert!(sensors.gpu().is_err());
        // The next poll takes the slow read once it is in, and the one
        // after it does not take it again.
        std::thread::sleep(MS * 600);
        assert!(sensors.gpu_awake().is_ok_and(|awake| awake));
        assert_eq!(sensors.gpu().ok(), Some(READING));
        assert!(sensors.gpu_awake().is_err());
        assert!(SLOW_READS.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn asks_while_a_read_runs_are_dropped_not_queued() {
        let mut sensors = BackgroundGpu::<QuickGpu, CountedGpu>::default();
        for _ in 0..10 {
            let _ = sensors.gpu_awake();
        }
        // The 10 asks take about 500 to 650 ms (the wait each); reads of
        // 300 ms each, one running and at most one waiting: 4 at most, not 10.
        std::thread::sleep(MS * 1500);
        let reads = COUNTED_READS.load(Ordering::SeqCst);
        assert!((1..=4).contains(&reads), "{reads}");
    }

    #[test]
    fn a_sleeping_gpu_is_never_read_through_nvml() {
        let mut sensors = BackgroundGpu::<QuickGpu, AsleepGpu>::default();
        warm(&mut sensors);
        assert!(sensors.gpu_awake().is_ok_and(|awake| !awake));
        assert!(sensors.gpu().is_err());
        assert_eq!(ASLEEP_READS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_reading_is_given_once_per_power_state_read() {
        let mut sensors = BackgroundGpu::<QuickGpu>::default();
        warm(&mut sensors);
        assert!(sensors.gpu_awake().is_ok());
        assert!(sensors.gpu().is_ok());
        assert!(sensors.gpu().is_err());
    }

    #[test]
    fn a_read_too_old_by_either_clock_is_not_fresh() {
        let (at, wall) = (Instant::now(), SystemTime::now());
        let sample = GpuSample {
            at,
            wall,
            awake: Ok(true),
            reading: None,
        };
        let max = GPU_READ_MAX_AGE;
        assert!(sample.fresh(at + max, wall + max));
        assert!(!sample.fresh(at + max + MS, wall));
        // A sleep that `Instant` did not count.
        assert!(!sample.fresh(at, wall + Duration::from_mins(10)));
        // The wall clock set back.
        assert!(!sample.fresh(at, wall - Duration::from_secs(1)));
    }

    #[test]
    fn a_stopped_thread_is_said_and_a_new_one_waits_its_pause() {
        let mut sensors = BackgroundGpu::<QuickGpu, PanicGpu>::default();
        // The thread panics at its first read.
        let _ = sensors.gpu_awake();
        let until = Instant::now() + Duration::from_secs(5);
        let stopped = loop {
            let read = sensors.gpu_awake();
            if read
                .as_ref()
                .is_err_and(|error| error.to_string().contains("thread stopped ("))
            {
                break read;
            }
            assert!(Instant::now() < until, "the stop was never said: {read:?}");
            std::thread::sleep(MS * 20);
        };
        assert!(stopped.is_err());
        assert!(sensors.asks.is_none());
        let next = sensors.gpu_awake();
        assert!(
            next.as_ref()
                .is_err_and(|error| error.to_string().contains("a new one waits")),
            "{next:?}"
        );
        // After the pause a new thread starts.
        sensors.stopped_at = Instant::now().checked_sub(RESTART_PAUSE);
        let _ = sensors.gpu_awake();
        assert!(sensors.asks.is_some());
    }

    #[test]
    fn the_other_reads_stay_on_the_callers_sensors() {
        let mut sensors = BackgroundGpu::<QuickGpu, SlowGpu>::default();
        assert!(
            sensors
                .cpu_times()
                .is_err_and(|error| error.to_string() == "cpu")
        );
        assert!(
            sensors
                .power()
                .is_err_and(|error| error.to_string() == "power")
        );
    }
}
