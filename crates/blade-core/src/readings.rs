//! A snapshot of what Windows reports: CPU load, battery, power draw and GPU.

use platform::{CpuTimes, GpuReading, Sensors};

/// One snapshot. A value is `None` when it could not be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readings {
    pub cpu_load_percent: Option<u8>,
    pub battery_percent: Option<u8>,
    /// Milliwatts drawn from the battery. `None` while it is not draining.
    pub power_draw_mw: Option<u32>,
    /// `None` while the GPU sleeps, so that reading it does not wake it.
    pub gpu: Option<GpuReading>,
}

/// Takes snapshots. It keeps the last CPU times, since load is a difference.
#[derive(Debug, Default)]
pub struct ReadingsSampler {
    last_cpu: Option<CpuTimes>,
}

impl ReadingsSampler {
    /// Reads every sensor once. A failed read leaves its value empty.
    pub fn sample(&mut self, sensors: &mut impl Sensors) -> Readings {
        let power = sensors.power().unwrap_or_default();
        Readings {
            cpu_load_percent: self.cpu_load(sensors),
            battery_percent: power.battery_percent,
            power_draw_mw: power
                .battery_rate_mw
                .filter(|&rate| rate < 0)
                .map(i32::unsigned_abs),
            gpu: gpu_if_awake(sensors),
        }
    }

    /// The load since the last snapshot, or `None` on the first.
    fn cpu_load(&mut self, sensors: &mut impl Sensors) -> Option<u8> {
        let now = sensors.cpu_times().ok();
        let last = std::mem::replace(&mut self.last_cpu, now);
        load_percent(last?, now?)
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

/// The GPU's reading, but only if it is already awake.
fn gpu_if_awake(sensors: &mut impl Sensors) -> Option<GpuReading> {
    if sensors.gpu_awake().ok()? {
        sensors.gpu().ok()
    } else {
        None
    }
}
