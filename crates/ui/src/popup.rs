//! The popup: it shows only when the running preset or the touchpad state
//! changes.

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
