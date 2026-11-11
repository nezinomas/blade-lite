//! The fan guard (ticket 129): a manual Custom fan holds its speed however
//! hot the laptop gets, so while the CPU or GPU stays hot the app sends the
//! model's top fan speed, and the saved speed again once both have cooled.
//! App policy, not model data: it only says when.

use std::time::{Duration, Instant};

use crate::popup::{HEAT_GAP, HEAT_MAX, Sensor};

/// How long a sensor stays at its guard limit before the fans go to full.
pub const FAN_GUARD_HOLD: Duration = Duration::from_secs(30);
/// How long every sensor read must stay cool before the saved speed goes
/// back.
pub const FAN_GUARD_COOL_HOLD: Duration = Duration::from_secs(60);
/// A sensor is cool this far under its guard limit.
const COOL_MARGIN: u32 = 10;

impl Sensor {
    /// A reading at or above this, in °C, is hot for the guard (user pick,
    /// 2026-11-11): under the heat warning's limits, so the fans speed up
    /// before the chips slow themselves down.
    #[must_use]
    pub fn guard_limit(self) -> u32 {
        match self {
            Self::Cpu => 95,
            Self::Gpu => 87,
        }
    }
}

/// Whether the fans should run at full speed, from the CPU and GPU readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FanGuard {
    on: bool,
    cpu: Track,
    gpu: Track,
}

/// One sensor's recent readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Track {
    hot_since: Option<Instant>,
    cool_since: Option<Instant>,
    last_reading: Option<Instant>,
}

impl FanGuard {
    /// Whether the fans should run at full speed now.
    #[must_use]
    pub fn on(&self) -> bool {
        self.on
    }

    /// Takes one reading of each sensor at `now` (`None`: not read). Returns
    /// the new state when it changes: on once a sensor has been at its
    /// guard limit for [`FAN_GUARD_HOLD`], off once every sensor read lately
    /// has been cool for [`FAN_GUARD_COOL_HOLD`]. The CPU must be among
    /// them: with no CPU reading the guard stays on. A GPU that is not read
    /// (asleep) does not hold it on.
    pub fn note(&mut self, cpu: Option<u32>, gpu: Option<u32>, now: Instant) -> Option<bool> {
        self.cpu.note(cpu, Sensor::Cpu.guard_limit(), now);
        self.gpu.note(gpu, Sensor::Gpu.guard_limit(), now);
        let next = if self.on {
            let cpu_cool = self.cpu.settled(now) == Some(true);
            let gpu_cool = self.gpu.settled(now) != Some(false);
            !(cpu_cool && gpu_cool)
        } else {
            self.cpu.hot_for(now) || self.gpu.hot_for(now)
        };
        let changed = next != self.on;
        self.on = next;
        changed.then_some(next)
    }
}

impl Track {
    fn note(&mut self, celsius: Option<u32>, limit: u32, now: Instant) {
        let Some(celsius) = celsius.filter(|&celsius| celsius <= HEAT_MAX) else {
            return;
        };
        let gap = self
            .last_reading
            .is_none_or(|last| now.saturating_duration_since(last) > HEAT_GAP);
        self.last_reading = Some(now);
        if gap {
            self.hot_since = None;
            self.cool_since = None;
        }
        if celsius >= limit {
            self.hot_since.get_or_insert(now);
        } else {
            self.hot_since = None;
        }
        if celsius < limit.saturating_sub(COOL_MARGIN) {
            self.cool_since.get_or_insert(now);
        } else {
            self.cool_since = None;
        }
    }

    /// Whether it has been hot for [`FAN_GUARD_HOLD`].
    fn hot_for(&self, now: Instant) -> bool {
        self.hot_since
            .is_some_and(|since| now.saturating_duration_since(since) >= FAN_GUARD_HOLD)
    }

    /// Whether it has been cool for [`FAN_GUARD_COOL_HOLD`]; `None` if it
    /// was not read lately.
    fn settled(&self, now: Instant) -> Option<bool> {
        let recent = self
            .last_reading
            .is_some_and(|last| now.saturating_duration_since(last) <= HEAT_GAP);
        recent.then(|| {
            self.cool_since
                .is_some_and(|since| now.saturating_duration_since(since) >= FAN_GUARD_COOL_HOLD)
        })
    }
}
