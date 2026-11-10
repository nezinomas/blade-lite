//! Keyboard and vapor-chamber light brightness, set as Synapse sets it.

use crate::{Command, ModelProfile};

/// Set LED brightness. Args are `[1, led, level]`.
const SET_BRIGHTNESS: u16 = 0x0303;
/// Get LED brightness. Args are `[1, led, 0]`, the reply args `[1, led, level]`.
/// The laptop answers it for the keyboard and the vapor light (`cargo hw`
/// brightness test, and `blade-dev keys` on 2026-11-10).
const GET_BRIGHTNESS: u16 = 0x0383;
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

    /// The brightness in percent, 0 to 100.
    #[must_use]
    pub fn percent(self) -> u8 {
        self.0
    }

    /// The controller's level, 0 to 255: percent × 255 / 100, rounded up.
    #[must_use]
    pub fn level(self) -> u8 {
        let level = (u16::from(self.0) * 255).div_ceil(u16::from(MAX_PERCENT));
        u8::try_from(level).unwrap_or(u8::MAX)
    }

    /// The command that reads the keyboard's level: the model's first
    /// brightness LED. `None` if the model has none.
    #[must_use]
    pub fn query(model: &ModelProfile) -> Option<Command> {
        let &led = model.brightness_leds.first()?;
        Some(Self::level_query(led))
    }

    /// The command that reads one LED's level, such as the vapor light's.
    #[must_use]
    pub fn level_query(led: u8) -> Command {
        Command {
            id: GET_BRIGHTNESS,
            args: vec![VARIABLE_STORAGE, led, 0],
        }
    }

    /// The LED's level, 0 to 255, from a [`Brightness::level_query`] reply.
    /// `None` if the reply is for another storage or LED.
    #[must_use]
    pub fn level_from_reply(led: u8, args: &[u8]) -> Option<u8> {
        match args {
            [VARIABLE_STORAGE, echoed, level, ..] if *echoed == led => Some(*level),
            _ => None,
        }
    }

    /// The keyboard's brightness from a query's reply args. `None` if the
    /// reply is for another storage or LED. The percent is the nearest one,
    /// so a level the app set reads back as the percent it came from, but
    /// at least 1 for a level above 0, so a dim keyboard is never read as
    /// off.
    #[must_use]
    pub fn from_reply(model: &ModelProfile, args: &[u8]) -> Option<Self> {
        let &led = model.brightness_leds.first()?;
        let level = Self::level_from_reply(led, args)?;
        let nearest = (u16::from(level) * u16::from(MAX_PERCENT) + 127) / 255;
        let percent = if level > 0 { nearest.max(1) } else { 0 };
        Some(Self(u8::try_from(percent).ok()?))
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

/// The light over the vapor chamber. Synapse sets it with the keyboard
/// brightness; the app can also keep it at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaporLight {
    pub led: u8,
}

impl VaporLight {
    /// The command that sets it: the keyboard's level while on, 0 while off.
    #[must_use]
    pub fn command(self, brightness: Brightness, on: bool) -> Command {
        let level = if on { brightness.level() } else { 0 };
        Command {
            id: SET_BRIGHTNESS,
            args: vec![VARIABLE_STORAGE, self.led, level],
        }
    }
}
