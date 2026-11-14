//! The GPU read on a thread of its own (ticket 206). An NVML read can take
//! over 2 s (a GPU going to sleep or waking), and on the event loop it froze
//! the window's scroll. The loop now asks for a read and takes the last one
//! the thread made, so it never waits. Only reads: nothing is sent.

use std::io;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use platform::{BatteryHealth, CpuTimes, GpuReading, PowerReading, Sensors};

/// The oldest GPU read still used: longer than the slowest poll that reads
/// the GPU ([`blade_core::HEAT_INTERVAL`], 10 s), so each poll uses the read
/// its last poll asked for. An older one, or none, counts as a failed read,
/// which holds the fan guard on as any failed GPU read does.
pub const GPU_READ_MAX_AGE: Duration = Duration::from_secs(15);

/// One GPU read by the thread: its power state, then its reading if awake.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GpuSample {
    at: Instant,
    awake: Result<bool, String>,
    /// `None` unless it read awake.
    reading: Option<Result<GpuReading, String>>,
}

impl GpuSample {
    /// Reads the GPU as the sampler does: NVML only if it is awake, so the
    /// read does not wake it.
    fn read(sensors: &mut impl Sensors) -> Self {
        let awake = sensors.gpu_awake().map_err(|error| error.to_string());
        let reading =
            matches!(awake, Ok(true)).then(|| sensors.gpu().map_err(|error| error.to_string()));
        Self {
            at: Instant::now(),
            awake,
            reading,
        }
    }
}

/// The last read the thread made, shared with the event loop.
type Last = Arc<Mutex<Option<GpuSample>>>;

/// Sensors whose GPU reads run on a thread of their own, started at the
/// first read; the other reads go to `S` on the caller's thread. The
/// thread makes its own `G` (made there, so it need not be `Send`).
pub struct BackgroundGpu<S, G = S> {
    inner: S,
    asks: Option<SyncSender<()>>,
    last: Last,
    /// The read [`gpu_awake`](Sensors::gpu_awake) took, for the
    /// [`gpu`](Sensors::gpu) after it.
    taken: Option<GpuSample>,
    make: fn() -> G,
}

impl<S: Default, G: Default> Default for BackgroundGpu<S, G> {
    fn default() -> Self {
        Self::with_maker(G::default)
    }
}

impl<S: Default, G> BackgroundGpu<S, G> {
    fn with_maker(make: fn() -> G) -> Self {
        Self {
            inner: S::default(),
            asks: None,
            last: Arc::default(),
            taken: None,
            make,
        }
    }
}

impl<S, G: Sensors + 'static> BackgroundGpu<S, G> {
    /// Asks the thread for a read, starting it first if need be. An ask
    /// while one waits is dropped: that one is still to come.
    fn ask(&mut self) -> io::Result<()> {
        let asks = match &self.asks {
            Some(asks) => asks.clone(),
            None => self.start()?,
        };
        match asks.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => {
                Err(io::Error::other("the GPU read's thread stopped"))
            }
        }
    }

    fn start(&mut self) -> io::Result<SyncSender<()>> {
        let (asks, asked) = mpsc::sync_channel::<()>(1);
        let last = Arc::clone(&self.last);
        let make = self.make;
        std::thread::Builder::new()
            .name("GPU read".to_owned())
            .spawn(move || {
                let mut sensors = make();
                while asked.recv().is_ok() {
                    let sample = GpuSample::read(&mut sensors);
                    *last.lock().unwrap_or_else(PoisonError::into_inner) = Some(sample);
                }
            })?;
        self.asks = Some(asks.clone());
        Ok(asks)
    }

    /// The thread's last read, if it is at most [`GPU_READ_MAX_AGE`] old.
    fn fresh(&self, now: Instant) -> Option<GpuSample> {
        let last = self
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        last.filter(|sample| now.saturating_duration_since(sample.at) <= GPU_READ_MAX_AGE)
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

    /// The power state of the thread's last read, and asks for the next.
    /// No read in the last [`GPU_READ_MAX_AGE`] is an error.
    fn gpu_awake(&mut self) -> io::Result<bool> {
        let asked = self.ask();
        self.taken = self.fresh(Instant::now());
        let Some(sample) = &self.taken else {
            asked?;
            return Err(io::Error::other(format!(
                "no GPU read in the last {} s yet (it runs in the background)",
                GPU_READ_MAX_AGE.as_secs()
            )));
        };
        sample.awake.clone().map_err(io::Error::other)
    }

    /// The reading of the read [`gpu_awake`](Sensors::gpu_awake) took.
    fn gpu(&mut self) -> io::Result<GpuReading> {
        match self
            .taken
            .as_ref()
            .and_then(|sample| sample.reading.clone())
        {
            Some(reading) => reading.map_err(io::Error::other),
            None => Err(io::Error::other("the GPU read asleep or was not read")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;

    const MS: Duration = Duration::from_millis(1);

    thread_local! {
        static GPU_READS: Cell<u32> = const { Cell::new(0) };
    }

    /// A GPU awake at 50 °C whose NVML read takes 300 ms.
    #[derive(Default)]
    struct SlowGpu;

    impl Sensors for SlowGpu {
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
            Ok(true)
        }
        fn gpu(&mut self) -> io::Result<GpuReading> {
            GPU_READS.set(GPU_READS.get() + 1);
            std::thread::sleep(MS * 300);
            Ok(GpuReading {
                celsius: 50,
                load_percent: 3,
            })
        }
    }

    /// A GPU asleep: its NVML read must never run.
    #[derive(Default)]
    struct AsleepGpu;

    impl Sensors for AsleepGpu {
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
            Ok(false)
        }
        fn gpu(&mut self) -> io::Result<GpuReading> {
            panic!("an NVML read on a sleeping GPU wakes it");
        }
    }

    /// Asks until the thread's read is in, for at most 5 s.
    fn awake_when_read<G: Sensors + 'static>(sensors: &mut BackgroundGpu<SlowGpu, G>) -> bool {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(awake) = sensors.gpu_awake() {
                return awake;
            }
            assert!(Instant::now() < until, "the thread never read the GPU");
            std::thread::sleep(MS * 20);
        }
    }

    #[test]
    fn a_slow_gpu_read_does_not_hold_the_caller() {
        let mut sensors = BackgroundGpu::<SlowGpu>::default();
        let began = Instant::now();
        let first = sensors.gpu_awake();
        assert!(began.elapsed() < MS * 100, "{:?}", began.elapsed());
        assert!(
            first
                .as_ref()
                .is_err_and(|error| error.to_string().contains("no GPU read")),
            "{first:?}"
        );
        assert!(sensors.gpu().is_err());
    }

    #[test]
    fn the_last_read_is_taken_after_the_thread_made_it() {
        let mut sensors = BackgroundGpu::<SlowGpu>::default();
        assert!(awake_when_read(&mut sensors));
        assert_eq!(
            sensors.gpu().ok(),
            Some(GpuReading {
                celsius: 50,
                load_percent: 3
            })
        );
    }

    static COUNTED_READS: AtomicU32 = AtomicU32::new(0);

    /// [`SlowGpu`], counting its NVML reads across threads.
    #[derive(Default)]
    struct CountedGpu(SlowGpu);

    impl Sensors for CountedGpu {
        fn cpu_times(&mut self) -> io::Result<CpuTimes> {
            self.0.cpu_times()
        }
        fn power(&mut self) -> io::Result<PowerReading> {
            self.0.power()
        }
        fn battery_health(&mut self) -> io::Result<BatteryHealth> {
            self.0.battery_health()
        }
        fn gpu_awake(&mut self) -> io::Result<bool> {
            self.0.gpu_awake()
        }
        fn gpu(&mut self) -> io::Result<GpuReading> {
            COUNTED_READS.fetch_add(1, Ordering::SeqCst);
            self.0.gpu()
        }
    }

    #[test]
    fn asks_while_a_read_runs_are_dropped_not_queued() {
        let mut sensors = BackgroundGpu::<SlowGpu, CountedGpu>::default();
        for _ in 0..20 {
            let _ = sensors.gpu_awake();
        }
        // The read that runs and at most one waiting (300 ms each), not 20.
        std::thread::sleep(MS * 1500);
        let reads = COUNTED_READS.load(Ordering::SeqCst);
        assert!((1..=2).contains(&reads), "{reads}");
        assert_eq!(
            GPU_READS.with(Cell::get),
            0,
            "no read on the caller's thread"
        );
    }

    #[test]
    fn a_sleeping_gpu_is_never_read_through_nvml() {
        let mut sensors = BackgroundGpu::<SlowGpu, AsleepGpu>::default();
        let until = Instant::now() + Duration::from_secs(5);
        let awake = loop {
            if let Ok(awake) = sensors.gpu_awake() {
                break awake;
            }
            assert!(Instant::now() < until, "the thread never read the GPU");
            std::thread::sleep(MS * 20);
        };
        assert!(!awake);
        assert!(sensors.gpu().is_err());
    }

    #[test]
    fn an_old_read_counts_as_none() {
        let sensors = BackgroundGpu::<SlowGpu>::default();
        let at = Instant::now();
        *sensors.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(GpuSample {
            at,
            awake: Ok(true),
            reading: None,
        });
        assert!(sensors.fresh(at + GPU_READ_MAX_AGE).is_some());
        assert!(sensors.fresh(at + GPU_READ_MAX_AGE + MS).is_none());
    }

    #[test]
    fn the_other_reads_stay_on_the_callers_sensors() {
        let mut sensors = BackgroundGpu::<SlowGpu>::default();
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
