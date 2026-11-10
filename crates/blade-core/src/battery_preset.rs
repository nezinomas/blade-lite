//! Only the model's battery presets run on battery. The saved preset comes
//! back on AC.

use razer_proto::ModelProfile;

use crate::Preset;

/// What runs on battery in place of a preset not offered there. Every
/// profile offers it there.
const BATTERY_FALLBACK: Preset = Preset::Balanced;

impl Preset {
    /// Whether the user can pick it on battery. Custom never runs there.
    #[must_use]
    pub fn offered_on_battery(self, model: &ModelProfile) -> bool {
        self.perf_mode()
            .is_some_and(|mode| model.battery_presets.contains(&mode))
    }

    /// The preset that runs in place of this saved one.
    #[must_use]
    pub fn running(self, on_battery: bool, model: &ModelProfile) -> Self {
        if on_battery && !self.offered_on_battery(model) {
            BATTERY_FALLBACK
        } else {
            self
        }
    }
}
