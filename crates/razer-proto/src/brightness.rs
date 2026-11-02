//! Keyboard brightness, set as Synapse sets it.

use crate::{Command, ModelProfile};

/// Set LED brightness. Args are `[1, led, level]`.
const SET_BRIGHTNESS: u16 = 0x0303;
/// The args start with 1, as Synapse sends them.
const VARIABLE_STORAGE: u8 = 0x01;

const MAX_PERCENT: u8 = 100;

/// Why a brightness could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrightnessError {
    /// More than 100 percent.
    OutOfRange(u8),
}

/// Keyboard brightness in percent, 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brightness(u8);

impl Brightness {
    /// # Errors
    ///
    /// [`BrightnessError::OutOfRange`] above 100 percent.
    pub fn new(percent: u8) -> Result<Self, BrightnessError> {
        if percent > MAX_PERCENT {
            return Err(BrightnessError::OutOfRange(percent));
        }
        Ok(Self(percent))
    }

    /// The controller's level, 0 to 255: percent × 255 / 100, rounded up.
    #[must_use]
    pub fn level(self) -> u8 {
        let level = (u16::from(self.0) * 255).div_ceil(u16::from(MAX_PERCENT));
        u8::try_from(level).unwrap_or(u8::MAX)
    }

    /// The commands that set it: one per brightness LED of the model.
    #[must_use]
    pub fn commands(self, model: &ModelProfile) -> Vec<Command> {
        model
            .brightness_leds
            .iter()
            .map(|&led| Command {
                id: SET_BRIGHTNESS,
                args: vec![VARIABLE_STORAGE, led, self.level()],
            })
            .collect()
    }
}
