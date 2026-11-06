//! The lid logo light, set as Synapse sets it.

use crate::Command;

/// Set the logo on or off. Args are `[storage, led, on]`.
const SET_LOGO: u16 = 0x0300;
/// Get the logo state. The reply args are `[storage, led, on]`.
const GET_LOGO: u16 = 0x0380;
/// Set the logo effect. Args are `[storage, led, effect]`.
const SET_LOGO_EFFECT: u16 = 0x0302;
/// Synapse sends 0 for the logo, not the 1 it sends for the keyboard. On
/// the laptop a read with 0 gives the state just set; a read with 1 gives
/// another stored value that the set does not change.
const STORAGE: u8 = 0x00;
const STATIC: u8 = 0x00;
const OFF: u8 = 0x00;
const ON: u8 = 0x01;

/// The model's lid logo: one single-colour LED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Logo {
    pub led: u8,
}

impl Logo {
    /// The commands that turn it on or off: static effect first, then the
    /// switch, as Synapse sends them.
    #[must_use]
    pub fn commands(self, on: bool) -> Vec<Command> {
        vec![
            Command {
                id: SET_LOGO_EFFECT,
                args: vec![STORAGE, self.led, STATIC],
            },
            Command {
                id: SET_LOGO,
                args: vec![STORAGE, self.led, if on { ON } else { OFF }],
            },
        ]
    }

    /// The command that reads whether it is on.
    #[must_use]
    pub fn query(self) -> Command {
        Command {
            id: GET_LOGO,
            args: vec![STORAGE, self.led, OFF],
        }
    }

    /// Whether it is on, from a query's reply args. `None` if the reply is
    /// for another LED or holds a value other than on or off.
    #[must_use]
    pub fn from_reply(self, args: &[u8]) -> Option<bool> {
        match args {
            [_, led, OFF, ..] if *led == self.led => Some(false),
            [_, led, ON, ..] if *led == self.led => Some(true),
            _ => None,
        }
    }
}
