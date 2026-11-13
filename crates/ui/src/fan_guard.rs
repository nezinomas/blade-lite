//! The fan guard (ticket 129): a manual Custom fan holds its speed however
//! hot the laptop gets, so once the CPU or GPU reads hot the app sends the
//! model's top fan speed, and the saved speed again once both have cooled.
//! App policy, not model data: it only says when.

use std::time::{Duration, Instant};

use crate::popup::{HEAT_GAP, HEAT_MAX, Sensor};

/// How long every sensor read must stay cool before the saved speed goes
/// back.
pub const FAN_GUARD_COOL_HOLD: Duration = Duration::from_secs(60);
/// After a long sleep, how long the first readings must stay cool before
/// the saved speed goes back (ticket 172, 10 to 20 s offered: the longer
/// one, as a game run on after the wake reads cool at first).
pub const FAN_GUARD_WAKE_HOLD: Duration = Duration::from_secs(20);
/// The longest time between two readings of the short hold after a sleep:
/// three reads missed at the 2 s guard interval end it.
const WAKE_GAP: Duration = Duration::from_secs(6);

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

    /// A reading under this, in °C, is cool for the guard. The GPU's is
    /// 20 °C under its limit (ticket 175, user pick 2026-11-13): at full
    /// fans a steady GPU load settled at 74 to 76 °C, so a 10 °C margin
    /// ended the guard mid-load and the fans went full, slow, full.
    #[must_use]
    pub fn guard_cool_line(self) -> u32 {
        match self {
            Self::Cpu => 85,
            Self::Gpu => 67,
        }
    }
}

/// Whether the fans should run at full speed, from the CPU and GPU readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FanGuard {
    on: bool,
    cpu: Track,
    gpu: Track,
    /// Whether the last GPU read failed (ticket 139): unlike a GPU asleep,
    /// it may be hot.
    gpu_failing: bool,
    /// The last failed GPU read, or one past any real heat. A GPU not read
    /// lately counts as cool only [`FAN_GUARD_COOL_HOLD`] after it.
    gpu_failed_at: Option<Instant>,
    /// Whether it is on only because the GPU's reads failed lately.
    held_by_gpu_fault: bool,
    /// `Some` after a long sleep while every reading since was cool.
    wake: Option<WakeHold>,
}

/// The cool readings since a long sleep's wake: the first and the last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WakeHold {
    since: Option<Instant>,
    last: Option<Instant>,
}

/// One sensor's recent readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Track {
    /// Whether the last reading was at or above the guard limit.
    hot: bool,
    cool_since: Option<Instant>,
    last_reading: Option<Instant>,
}

impl FanGuard {
    /// Whether the fans should run at full speed now.
    #[must_use]
    pub fn on(&self) -> bool {
        self.on
    }

    /// Whether it is on only because the GPU's reads failed lately (or
    /// gave a heat past any real one): every sensor read lately is cool
    /// (ticket 163).
    #[must_use]
    pub fn held_by_gpu_fault(&self) -> bool {
        self.held_by_gpu_fault
    }

    /// The laptop woke from a long sleep (ticket 172): if on, the readings
    /// from now on end it after [`FAN_GUARD_WAKE_HOLD`], not
    /// [`FAN_GUARD_COOL_HOLD`], while each is cool: the CPU read and under
    /// its [`guard_cool_line`](Sensor::guard_cool_line), the GPU too or
    /// asleep (not read, no failed read). The first reading that is not,
    /// or a gap of over
    /// [`WAKE_GAP`], leaves the usual hold. Returns whether it is on.
    pub fn woke(&mut self) -> bool {
        self.wake = self.on.then(WakeHold::default);
        self.on
    }

    /// Notes whether the last GPU read failed, before [`note`](Self::note)
    /// takes its `None`.
    pub fn gpu_failed(&mut self, failed: bool) {
        self.gpu_failing = failed;
    }

    /// Takes one reading of each sensor at `now` (`None`: not read). Returns
    /// the new state when it changes: on at the first recent reading of a
    /// sensor at its guard limit (user pick 2026-11-12: no wait), off once
    /// every sensor read lately has been cool for [`FAN_GUARD_COOL_HOLD`]
    /// (after a long sleep, maybe [`FAN_GUARD_WAKE_HOLD`]: see
    /// [`woke`](Self::woke)). The CPU must be among
    /// them: with no CPU reading the guard stays on. A GPU not read lately
    /// holds it on only while its reads fail (see [`gpu_failed`](Self::gpu_failed)).
    pub fn note(&mut self, cpu: Option<u32>, gpu: Option<u32>, now: Instant) -> Option<bool> {
        if self.gpu_failing || gpu.is_some_and(|celsius| celsius > HEAT_MAX) {
            self.gpu_failed_at = Some(now);
        }
        self.cpu.note(cpu, Sensor::Cpu, now);
        self.gpu.note(gpu, Sensor::Gpu, now);
        self.held_by_gpu_fault = false;
        let next = if self.on && self.wake_hold_done(cpu, gpu, now) {
            false
        } else if self.on {
            let cpu_cool = self.cpu.settled(now) == Some(true);
            let gpu_settled = self.gpu.settled(now);
            // Not read lately: a GPU asleep cools, but only once no read has
            // failed for the cool hold, as one that fails may be hot.
            let gpu_fault = gpu_settled.is_none()
                && self
                    .gpu_failed_at
                    .is_some_and(|at| now.saturating_duration_since(at) < FAN_GUARD_COOL_HOLD);
            let gpu_cool = gpu_settled.unwrap_or(!gpu_fault);
            self.held_by_gpu_fault = cpu_cool && gpu_fault;
            !(cpu_cool && gpu_cool)
        } else {
            self.cpu.hot_now(now) || self.gpu.hot_now(now)
        };
        let changed = next != self.on;
        self.on = next;
        if !next {
            self.wake = None;
        }
        changed.then_some(next)
    }

    /// Takes a reading into the short hold after a long sleep, if one runs.
    /// Returns whether the readings have stayed cool for
    /// [`FAN_GUARD_WAKE_HOLD`]. A reading that is not cool, or a gap, ends
    /// the short hold.
    fn wake_hold_done(&mut self, cpu: Option<u32>, gpu: Option<u32>, now: Instant) -> bool {
        let Some(mut hold) = self.wake.take() else {
            return false;
        };
        let cool = |celsius: u32, sensor: Sensor| celsius < sensor.guard_cool_line();
        let cpu_cool = cpu.is_some_and(|celsius| cool(celsius, Sensor::Cpu));
        let gpu_cool = !self.gpu_failing && gpu.is_none_or(|celsius| cool(celsius, Sensor::Gpu));
        let gap = hold
            .last
            .is_some_and(|last| now.saturating_duration_since(last) > WAKE_GAP);
        if !cpu_cool || !gpu_cool || gap {
            return false;
        }
        let since = *hold.since.get_or_insert(now);
        hold.last = Some(now);
        if now.saturating_duration_since(since) >= FAN_GUARD_WAKE_HOLD {
            return true;
        }
        self.wake = Some(hold);
        false
    }
}

impl Track {
    fn note(&mut self, celsius: Option<u32>, sensor: Sensor, now: Instant) {
        let Some(celsius) = celsius.filter(|&celsius| celsius <= HEAT_MAX) else {
            return;
        };
        let gap = self
            .last_reading
            .is_none_or(|last| now.saturating_duration_since(last) > HEAT_GAP);
        self.last_reading = Some(now);
        if gap {
            self.cool_since = None;
        }
        self.hot = celsius >= sensor.guard_limit();
        if celsius < sensor.guard_cool_line() {
            self.cool_since.get_or_insert(now);
        } else {
            self.cool_since = None;
        }
    }

    /// Whether its last reading, taken lately, was hot: an old hot reading
    /// of a sensor no longer read does not turn the guard on.
    fn hot_now(&self, now: Instant) -> bool {
        let recent = self
            .last_reading
            .is_some_and(|last| now.saturating_duration_since(last) <= HEAT_GAP);
        recent && self.hot
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
