//! The power-mode key steps to the next preset, as it did with Synapse.

use std::time::{Duration, Instant};

use razer_proto::{ModelProfile, PowerKey};

use crate::Preset;

/// The shortest time between two steps. Presses closer than this count
/// once, so a bouncing or hammered key can't flood the controller.
pub const POWER_KEY_PAUSE: Duration = Duration::from_millis(300);

/// The preset after `running` in the model's presets, wrapping round. On
/// battery only the presets offered there count. Custom is never stepped
/// to, since it may hold a manual fan speed; from Custom the first preset
/// comes. `None` if there is no preset to step to.
#[must_use]
pub fn next_preset(running: Preset, model: &ModelProfile, on_battery: bool) -> Option<Preset> {
    let cycle: Vec<Preset> = model
        .presets
        .iter()
        .map(|&mode| Preset::from(mode))
        .filter(|preset| !on_battery || preset.offered_on_battery())
        .collect();
    let after = cycle
        .iter()
        .position(|&preset| preset == running)
        .map_or(0, |index| index + 1);
    cycle.get(after).or_else(|| cycle.first()).copied()
}

/// Turns the key reports into presses.
#[derive(Debug, Clone)]
pub struct PowerKeyPresses {
    key: PowerKey,
    held: bool,
    last_step: Option<Instant>,
}

impl PowerKeyPresses {
    #[must_use]
    pub fn new(key: PowerKey) -> Self {
        Self {
            key,
            held: false,
            last_step: None,
        }
    }

    /// True if this report is a new press that should step the preset.
    pub fn on_report(&mut self, report: &[u8], now: Instant) -> bool {
        let Some(held) = self.key.held(report) else {
            return false;
        };
        let pressed = held && !self.held;
        self.held = held;
        if !pressed || self.too_soon(now) {
            return false;
        }
        self.last_step = Some(now);
        true
    }

    fn too_soon(&self, now: Instant) -> bool {
        self.last_step
            .is_some_and(|last| now.saturating_duration_since(last) < POWER_KEY_PAUSE)
    }
}
