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

/// A reading at or above this, in °C, is hot.
pub const HEAT_LIMIT: u32 = 95;
/// How long a sensor stays hot before the warning shows.
pub const HEAT_HOLD: Duration = Duration::from_secs(180);
/// A hot spell ends only below this, so a sensor near the limit does not
/// warn again and again.
const HEAT_COOL: u32 = 90;
/// A longer time without a reading starts the hold again: the sensor was
/// not watched in between.
const HEAT_GAP: Duration = Duration::from_secs(30);
/// Readings above this are not believed.
const HEAT_MAX: u32 = 125;

/// A sensor the heat warning watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensor {
    Cpu,
    Gpu,
}

impl Sensor {
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
    /// When it got hot, or `None` while it is not hot or was not watched.
    hot_since: Option<Instant>,
    last_reading: Option<Instant>,
    /// Whether this spell has warned. Cleared when the sensor cools down.
    warned: bool,
}

impl HeatWatch {
    /// Takes one reading of `sensor` at `now`, `None` if it could not be
    /// read. Returns the warning, such as "CPU 96 °C", when the sensor has
    /// been hot for [`HEAT_HOLD`] and has not warned in this spell.
    pub fn note(&mut self, sensor: Sensor, celsius: Option<u32>, now: Instant) -> Option<String> {
        let spell = match sensor {
            Sensor::Cpu => &mut self.cpu,
            Sensor::Gpu => &mut self.gpu,
        };
        let celsius = celsius.filter(|&celsius| celsius <= HEAT_MAX);
        spell
            .note(celsius, now)
            .then(|| format!("{} {} °C", sensor.label(), celsius.unwrap_or_default()))
    }
}

impl Spell {
    /// Whether to warn now. A reading that is unknown, under the limit, or
    /// after a gap starts the hold again.
    fn note(&mut self, celsius: Option<u32>, now: Instant) -> bool {
        let gap = self
            .last_reading
            .is_none_or(|last| now.saturating_duration_since(last) > HEAT_GAP);
        self.last_reading = Some(now);
        let Some(celsius) = celsius else {
            self.hot_since = None;
            return false;
        };
        if celsius < HEAT_COOL {
            self.warned = false;
        }
        if celsius < HEAT_LIMIT || gap {
            self.hot_since = (celsius >= HEAT_LIMIT).then_some(now);
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
