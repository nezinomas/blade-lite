//! The power mode popup: it shows only when the running preset changes.

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
