//! The popup: it shows only when the running preset or the touchpad state
//! changes, or when the CPU or GPU stays hot.

use std::time::{Duration, Instant};

use blade_core::Preset;

use crate::header::preset_label;

/// Remembers the running preset, to tell when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeWatch {
    last: Preset,
}

impl ModeWatch {
    /// Starts from the preset that runs now, which shows no popup.
    #[must_use]
    pub fn new(running: Preset) -> Self {
        Self { last: running }
    }

    /// The name of the new mode if `running` is not the last one seen and
    /// the change `worked`. A failed change may leave the controller in the
    /// old mode, so it shows nothing, and is not shown later either.
    pub fn changed(&mut self, running: Preset, worked: bool) -> Option<&'static str> {
        let last = std::mem::replace(&mut self.last, running);
        (worked && last != running).then(|| preset_label(running))
    }
}

/// Remembers the touchpad state, to tell when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadWatch {
    last: Option<bool>,
}

impl TouchpadWatch {
    /// Starts from the state now, which shows no popup. `None` if it can't
    /// be read.
    #[must_use]
    pub fn new(on: Option<bool>) -> Self {
        Self { last: on }
    }

    /// "On" or "Off" if `on` differs from the last state read. A change
    /// from or to a state that can't be read is not known, so it shows
    /// nothing.
    pub fn changed(&mut self, on: Option<bool>) -> Option<&'static str> {
        let last = std::mem::replace(&mut self.last, on);
        match (last, on) {
            (Some(last), Some(on)) if last != on => Some(if on { "On" } else { "Off" }),
            _ => None,
        }
    }
}

/// How long a sensor stays hot before the warning shows.
pub const HEAT_HOLD: Duration = Duration::from_secs(180);
/// A hot spell ends only this far below the limit, so a sensor near the
/// limit does not warn again and again.
const HEAT_COOL_MARGIN: u32 = 5;
/// A longer time without a reading starts the hold again: the sensor was
/// not watched in between (failed reads, or the event loop stood still).
pub(crate) const HEAT_GAP: Duration = Duration::from_secs(30);
/// Readings above this, in °C, are not believed.
pub(crate) const HEAT_MAX: u32 = 125;

/// A sensor the heat warning watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensor {
    Cpu,
    Gpu,
}

impl Sensor {
    /// A reading at or above this, in °C, is hot: where each chip slows
    /// itself down. The CPU sits near 100 °C under a long full load on
    /// Performance; the RTX 5090 Laptop GPU holds 87 °C (its target) and
    /// slows down at 89 °C (`nvidia-smi -q -d TEMPERATURE`). App policy, not
    /// model data: it says when to tell the user, and sends nothing.
    #[must_use]
    pub fn limit(self) -> u32 {
        match self {
            Self::Cpu => 100,
            Self::Gpu => 89,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
        }
    }
}

/// Watches the CPU and GPU temperatures, to warn once per hot spell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatWatch {
    cpu: Spell,
    gpu: Spell,
}

/// One sensor's hot spell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Spell {
    /// When it got hot, or `None` while it is not hot.
    hot_since: Option<Instant>,
    /// When it was last read.
    last_reading: Option<Instant>,
    /// Whether this spell has warned. Cleared when the sensor cools down.
    warned: bool,
}

impl HeatWatch {
    /// Takes one reading of `sensor` at `now`, `None` if it could not be
    /// read. Returns the warning, such as "CPU 96°", when the sensor has
    /// been hot for [`HEAT_HOLD`] and has not warned in this spell.
    pub fn note(&mut self, sensor: Sensor, celsius: Option<u32>, now: Instant) -> Option<String> {
        let spell = match sensor {
            Sensor::Cpu => &mut self.cpu,
            Sensor::Gpu => &mut self.gpu,
        };
        let celsius = celsius.filter(|&celsius| celsius <= HEAT_MAX)?;
        spell
            .note(celsius, sensor.limit(), now)
            .then(|| format!("{} {celsius}°", sensor.label()))
    }
}

impl Spell {
    /// Whether to warn now. A reading under the limit, or one after a gap,
    /// starts the hold again; cooling down ends the spell.
    fn note(&mut self, celsius: u32, limit: u32, now: Instant) -> bool {
        let gap = self
            .last_reading
            .is_none_or(|last| now.saturating_duration_since(last) > HEAT_GAP);
        self.last_reading = Some(now);
        if celsius < limit.saturating_sub(HEAT_COOL_MARGIN) {
            self.warned = false;
        }
        if gap {
            self.hot_since = None;
        }
        if celsius < limit {
            self.hot_since = None;
            return false;
        }
        let since = *self.hot_since.get_or_insert(now);
        if self.warned || now.saturating_duration_since(since) < HEAT_HOLD {
            return false;
        }
        self.warned = true;
        true
    }
}
