//! The touchpad key's light, set as Synapse set it.

use crate::Command;

/// Light the key or not. Args are `[storage, led, lit]`.
const SET_KEY: u16 = 0x0059;
/// Get the key light. The reply args are `[storage, led, lit]`.
const GET_KEY: u16 = 0x00D9;
const STORAGE: u8 = 0x00;
const UNLIT: u8 = 0x00;
const LIT: u8 = 0x01;

/// The model's touchpad key: the controller lights it while the touchpad
/// is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadKey {
    pub led: u8,
}

impl TouchpadKey {
    /// The command that lights the key or turns its light off.
    #[must_use]
    pub fn command(self, lit: bool) -> Command {
        Command {
            id: SET_KEY,
            args: vec![STORAGE, self.led, if lit { LIT } else { UNLIT }],
        }
    }

    /// The command that reads whether the key is lit.
    #[must_use]
    pub fn query(self) -> Command {
        Command {
            id: GET_KEY,
            args: vec![STORAGE, self.led, UNLIT],
        }
    }

    /// Whether the key is lit, from a query's reply args. `None` if the
    /// reply is for another storage or LED, or holds an odd value.
    #[must_use]
    pub fn from_reply(self, args: &[u8]) -> Option<bool> {
        match args {
            [STORAGE, led, UNLIT, ..] if *led == self.led => Some(false),
            [STORAGE, led, LIT, ..] if *led == self.led => Some(true),
            _ => None,
        }
    }
}
