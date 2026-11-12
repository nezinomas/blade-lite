//! A snapshot of what Windows reports: CPU load, battery, power draw and GPU.

use std::io;

use platform::{BatteryHealth, CpuTimes, GpuReading, Sensors};

/// One snapshot. A value is `None` when it could not be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readings {
    pub cpu_load_percent: Option<u8>,
    pub battery_percent: Option<u8>,
    /// Milliwatts drawn from the battery. `None` while it is not draining.
    pub power_draw_mw: Option<u32>,
    /// Whether the battery charges now. `None` with no battery, off AC, or
    /// if it could not be read.
    pub charging: Option<bool>,
    /// Read once per [`HEALTH_EVERY`] snapshots, as it changes slowly.
    pub battery_health: BatteryHealth,
    /// `None` while the GPU sleeps, so that reading it does not wake it.
    pub gpu: Option<GpuReading>,
}

/// How many snapshots share one battery health read: a minute at the 1 s
/// poll.
pub const HEALTH_EVERY: usize = 60;

/// Takes snapshots. It keeps the last CPU times, since load is a difference,
/// and the last battery health.
#[derive(Debug, Default)]
pub struct ReadingsSampler {
    last_cpu: Option<CpuTimes>,
    health: BatteryHealth,
    /// Snapshots until the next battery health read; 0 reads it now.
    health_in: usize,
    errors: Vec<String>,
    /// Whether the last GPU read failed (its power state or NVML), as
    /// against a GPU asleep: the fan guard counts only a sleeping GPU cool.
    gpu_failed: bool,
}

impl ReadingsSampler {
    /// Reads every sensor once. A failed read leaves its value empty and
    /// is named in [`errors`](Self::errors).
    pub fn sample(&mut self, sensors: &mut impl Sensors) -> Readings {
        self.errors.clear();
        let power = self.note("battery", sensors.power()).unwrap_or_default();
        Readings {
            battery_percent: power.battery_percent,
            power_draw_mw: power
                .battery_rate_mw
                .filter(|&rate| rate < 0)
                .map(i32::unsigned_abs),
            charging: power.charging,
            battery_health: self.battery_health(sensors),
            cpu_load_percent: self.cpu_load(sensors),
            gpu: self.gpu_if_awake(sensors),
        }
    }

    /// Reads only the GPU's temperature, for the heat warning, and only if
    /// the GPU is already awake. A failed read is `None`, named in
    /// [`errors`](Self::errors).
    pub fn gpu_celsius(&mut self, sensors: &mut impl Sensors) -> Option<u32> {
        self.errors.clear();
        self.gpu_if_awake(sensors).map(|gpu| gpu.celsius)
    }

    /// Whether the last GPU read failed. A GPU asleep is no failure.
    #[must_use]
    pub fn gpu_failed(&self) -> bool {
        self.gpu_failed
    }

    /// One line per read that failed in the last read, such as
    /// `battery: <the Windows error>`.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// The battery health, read on the first snapshot and then once per
    /// [`HEALTH_EVERY`]. A failed read keeps the last one.
    fn battery_health(&mut self, sensors: &mut impl Sensors) -> BatteryHealth {
        if self.health_in == 0 {
            self.health_in = HEALTH_EVERY;
            if let Some(health) = self.note("battery health", sensors.battery_health()) {
                self.health = health;
            }
        }
        self.health_in -= 1;
        self.health
    }

    /// The load since the last snapshot, or `None` on the first.
    fn cpu_load(&mut self, sensors: &mut impl Sensors) -> Option<u8> {
        let now = self.note("CPU load", sensors.cpu_times());
        let last = std::mem::replace(&mut self.last_cpu, now);
        load_percent(last?, now?)
    }

    /// The GPU's reading, but only if it is already awake.
    fn gpu_if_awake(&mut self, sensors: &mut impl Sensors) -> Option<GpuReading> {
        self.gpu_failed = true;
        let reading = if self.note("GPU power state", sensors.gpu_awake())? {
            self.note("GPU", sensors.gpu())?
        } else {
            self.gpu_failed = false;
            return None;
        };
        self.gpu_failed = false;
        Some(reading)
    }

    /// The value, or `None` with a line in the errors that starts with `name`.
    fn note<T>(&mut self, name: &str, read: io::Result<T>) -> Option<T> {
        read.map_err(|error| self.errors.push(format!("{name}: {error}")))
            .ok()
    }
}

/// The busy share of the CPU time between two readings, in percent.
fn load_percent(last: CpuTimes, now: CpuTimes) -> Option<u8> {
    let total = now
        .total
        .checked_sub(last.total)
        .filter(|&total| total > 0)?;
    let idle = now.idle.checked_sub(last.idle)?.min(total);
    u8::try_from((total - idle) * 100 / total).ok()
}
