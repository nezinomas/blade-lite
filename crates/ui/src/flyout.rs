//! The tray flyout: the preset, the rate, the 60 Hz on battery switch, the
//! lighting and the touchpad, as the window's groups show them, plus
//! "Reset to Balanced" and "Open window".

use blade_core::Preset;

use crate::{
    DisplayView, KeyboardView, PerformanceCommand, PerformanceView, PresetOption, RateSegment,
};

/// The flyout, ready to show. Clicks on its preset and rate buttons go to
/// the window's group views, whose buttons are in the same order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlyoutView {
    pub presets: Vec<PresetOption>,
    pub rates: Vec<RateSegment>,
    pub rates_enabled: bool,
    pub sixty_hz_on_battery: bool,
    pub brightness: u8,
    pub brightness_text: String,
    pub touchpad_on: bool,
    /// `false` while the preset is already Balanced.
    pub reset_enabled: bool,
}

impl FlyoutView {
    /// Builds the flyout from the window's groups.
    #[must_use]
    pub fn new(
        performance: &PerformanceView,
        display: &DisplayView,
        keyboard: &KeyboardView,
    ) -> Self {
        let balanced = performance
            .presets
            .iter()
            .any(|option| option.selected && option.preset == Preset::Balanced);
        Self {
            presets: performance.presets.clone(),
            rates: display.rates.clone(),
            rates_enabled: display.rates_enabled,
            sixty_hz_on_battery: display.sixty_hz_on_battery,
            brightness: keyboard.brightness,
            brightness_text: keyboard.brightness_text.clone(),
            touchpad_on: keyboard.touchpad_on,
            reset_enabled: !balanced,
        }
    }

    /// The command for "Reset to Balanced", unless it is Balanced already.
    #[must_use]
    pub fn reset_to_balanced(&self) -> Option<PerformanceCommand> {
        self.reset_enabled
            .then_some(PerformanceCommand::SetPreset(Preset::Balanced))
    }
}
