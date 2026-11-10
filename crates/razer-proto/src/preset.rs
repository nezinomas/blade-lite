//! Performance presets, set as Synapse sets them.

use crate::{Command, ModelProfile, VARIABLE_STORAGE};

/// Set the preset of one zone. Args are `[1, zone, mode, fan]`.
pub(crate) const SET_PRESET: u16 = 0x0D02;
/// Get the preset of one zone. The reply args are `[1, zone, mode, fan]`.
const GET_PRESET: u16 = 0x0D82;
/// The fan byte: the controller picks the fan speed.
pub(crate) const FAN_AUTO: u8 = 0x00;

/// Why a preset could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetError {
    /// The model does not offer this preset.
    NotOffered(PerfMode),
}

/// The controller's mode byte of each preset. They differ between models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfModeBytes {
    pub silent: u8,
    pub balanced: u8,
    pub performance: u8,
}

/// A performance preset. The fans stay on auto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMode {
    Silent,
    Balanced,
    Performance,
}

impl PerfMode {
    const ALL: [Self; 3] = [Self::Silent, Self::Balanced, Self::Performance];

    /// The model's mode byte for it.
    fn byte(self, model: &ModelProfile) -> u8 {
        let bytes = model.mode_bytes;
        match self {
            Self::Silent => bytes.silent,
            Self::Balanced => bytes.balanced,
            Self::Performance => bytes.performance,
        }
    }

    /// The commands that set it: one per zone of the model.
    ///
    /// # Errors
    ///
    /// [`PresetError::NotOffered`] if the model does not offer it.
    pub fn commands(self, model: &ModelProfile) -> Result<Vec<Command>, PresetError> {
        if !model.presets.contains(&self) {
            return Err(PresetError::NotOffered(self));
        }
        Ok(model
            .perf_zones
            .iter()
            .map(|&zone| Command {
                id: SET_PRESET,
                args: vec![VARIABLE_STORAGE, zone, self.byte(model), FAN_AUTO],
            })
            .collect())
    }

    /// The commands that read the preset back: one per zone of the model.
    #[must_use]
    pub fn queries(model: &ModelProfile) -> Vec<Command> {
        model
            .perf_zones
            .iter()
            .map(|&zone| Command {
                id: GET_PRESET,
                args: vec![VARIABLE_STORAGE, zone, 0, 0],
            })
            .collect()
    }

    /// The preset of `zone` in a query's reply args, read with the model's
    /// mode bytes. `None` for a mode this does not know, or a reply for
    /// another storage or zone.
    #[must_use]
    pub fn from_reply(model: &ModelProfile, zone: u8, args: &[u8]) -> Option<Self> {
        match args {
            [VARIABLE_STORAGE, echoed, mode, ..] if *echoed == zone => Self::ALL
                .into_iter()
                .find(|preset| preset.byte(model) == *mode),
            _ => None,
        }
    }
}
