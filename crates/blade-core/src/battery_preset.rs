//! Only Silent and Balanced run on battery. The saved preset comes back on AC.

use crate::Preset;

/// What runs on battery in place of a preset not offered there.
const BATTERY_FALLBACK: Preset = Preset::Balanced;

impl Preset {
    /// Whether the user can pick it on battery.
    #[must_use]
    pub fn offered_on_battery(self) -> bool {
        matches!(self, Self::Silent | Self::Balanced)
    }

    /// The preset that runs in place of this saved one.
    #[must_use]
    pub fn running(self, on_battery: bool) -> Self {
        if on_battery && !self.offered_on_battery() {
            BATTERY_FALLBACK
        } else {
            self
        }
    }
}
