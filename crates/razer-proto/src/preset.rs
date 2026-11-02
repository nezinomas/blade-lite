//! Performance presets, set as Synapse sets them.

use crate::{Command, ModelProfile};

/// Set the preset of one zone. Args are `[1, zone, mode, fan]`.
const SET_PRESET: u16 = 0x0D02;
/// Get the preset of one zone. The reply args are `[1, zone, mode, fan]`.
const GET_PRESET: u16 = 0x0D82;
/// The args start with 1, as Synapse sends them.
const VARIABLE_STORAGE: u8 = 0x01;
/// The fan byte: the controller picks the fan speed.
const FAN_AUTO: u8 = 0x00;
/// Position of the mode byte in the args.
const MODE_INDEX: usize = 2;

/// Why a preset could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetError {
    /// The model does not offer this preset.
    NotOffered(PerfMode),
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

    /// The controller's mode byte.
    fn byte(self) -> u8 {
        match self {
            Self::Silent => 0x05,
            Self::Balanced => 0x00,
            Self::Performance => 0x02,
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
                args: vec![VARIABLE_STORAGE, zone, self.byte(), FAN_AUTO],
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

    /// The preset in a query's reply args, or `None` for a mode this does not know.
    #[must_use]
    pub fn from_reply(args: &[u8]) -> Option<Self> {
        let mode = *args.get(MODE_INDEX)?;
        Self::ALL.into_iter().find(|preset| preset.byte() == mode)
    }
}
