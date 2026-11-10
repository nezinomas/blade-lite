//! A snapshot of what Windows reports: CPU load, battery, power draw and GPU.

use std::io;

use platform::{CpuTimes, GpuReading, Sensors};

/// One snapshot. A value is `None` when it could not be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readings {
    pub cpu_load_percent: Option<u8>,
    pub battery_percent: Option<u8>,
    /// Milliwatts drawn from the battery. `None` while it is not draining.
    pub power_draw_mw: Option<u32>,
    /// Whether the battery charges now. `None` with no battery, or if it
    /// could not be read.
    pub charging: Option<bool>,
    /// `None` while the GPU sleeps, so that reading it does not wake it.
    pub gpu: Option<GpuReading>,
}

/// Takes snapshots. It keeps the last CPU times, since load is a difference.
#[derive(Debug, Default)]
pub struct ReadingsSampler {
    last_cpu: Option<CpuTimes>,
    errors: Vec<String>,
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
            cpu_load_percent: self.cpu_load(sensors),
            gpu: self.gpu_if_awake(sensors),
        }
    }

    /// One line per read that failed in the last snapshot, such as
    /// `battery: <the Windows error>`.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// The load since the last snapshot, or `None` on the first.
    fn cpu_load(&mut self, sensors: &mut impl Sensors) -> Option<u8> {
        let now = self.note("CPU load", sensors.cpu_times());
        let last = std::mem::replace(&mut self.last_cpu, now);
        load_percent(last?, now?)
    }

    /// The GPU's reading, but only if it is already awake.
    fn gpu_if_awake(&mut self, sensors: &mut impl Sensors) -> Option<GpuReading> {
        if self.note("GPU power state", sensors.gpu_awake())? {
            self.note("GPU", sensors.gpu())
        } else {
            None
        }
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
