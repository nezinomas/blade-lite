//! The panel's native mode, as Synapse's code sets it. It takes effect after
//! a restart.

use crate::{Command, ModelProfile};

/// Set the native mode. Args are `[mode]`.
const SET_NATIVE_MODE: u16 = 0x0D0E;
/// Get the chosen native mode. The reply args are `[mode]`.
const GET_NATIVE_MODE: u16 = 0x0D8E;

/// A native mode of the panel: its largest resolution and top rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeMode {
    /// The controller's mode byte.
    pub id: u8,
    pub width: u32,
    pub height: u32,
    /// The top refresh rate, in Hz.
    pub hz: u32,
}

impl NativeMode {
    /// The command that chooses it for the next boot.
    #[must_use]
    pub fn command(self) -> Command {
        Command {
            id: SET_NATIVE_MODE,
            args: vec![self.id],
        }
    }

    /// The command that reads the chosen mode.
    #[must_use]
    pub fn query() -> Command {
        Command {
            id: GET_NATIVE_MODE,
            args: vec![0],
        }
    }

    /// The model's mode in a query's reply args, or `None` for one it lacks.
    #[must_use]
    pub fn from_reply(args: &[u8], model: &ModelProfile) -> Option<Self> {
        let id = *args.first()?;
        model
            .native_modes
            .iter()
            .copied()
            .find(|mode| mode.id == id)
    }
}
