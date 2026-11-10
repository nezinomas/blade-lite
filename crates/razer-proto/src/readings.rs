//! Fan speeds and the CPU temperature, read from the controller.

use crate::{Command, ModelProfile, VARIABLE_STORAGE};

/// Get a fan's speed. Args are `[1, fan, 0]`, the reply's `[1, fan, rpm/100]`.
const GET_FAN_RPM: u16 = 0x0D88;
/// Get the temperatures. The reply's args are `[2, cpu, gpu, 0]`.
const GET_TEMPERATURES: u16 = 0x0D85;
/// Position of the CPU temperature in a temperature reply.
const CPU_INDEX: usize = 1;
/// Byte 0 of every temperature reply read so far (`docs/NOTES.md`).
const REPLY_KIND: u8 = 0x02;

/// A measured fan speed. Unlike a target `Rpm`, any value can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanRpm(pub u16);

impl FanRpm {
    /// The commands that read each fan of the model, in its fan order.
    #[must_use]
    pub fn queries(model: &ModelProfile) -> Vec<Command> {
        model
            .custom
            .fan_zones
            .iter()
            .map(|&fan| Command {
                id: GET_FAN_RPM,
                args: vec![VARIABLE_STORAGE, fan, 0],
            })
            .collect()
    }

    /// The speed of `fan` in a query's reply args. `None` if they are too
    /// short, or for another storage or fan.
    #[must_use]
    pub fn from_reply(fan: u8, args: &[u8]) -> Option<Self> {
        match args {
            [VARIABLE_STORAGE, echoed, hundreds, ..] if *echoed == fan => {
                Some(Self(u16::from(*hundreds) * 100))
            }
            _ => None,
        }
    }
}

/// A temperature in degrees Celsius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Celsius(pub u8);

impl Celsius {
    /// The command that reads the temperatures. Synapse never sends it;
    /// these are r-helper's args.
    #[must_use]
    pub fn query() -> Command {
        Command {
            id: GET_TEMPERATURES,
            args: vec![0x00, 0x01, 0x00, 0x00],
        }
    }

    /// The CPU temperature in a query's reply args. `None` if the controller
    /// does not report it: the args are too short, byte 0 is not the `02`
    /// every reply has had, or it reads 0.
    #[must_use]
    pub fn cpu_from_reply(args: &[u8]) -> Option<Self> {
        if args.first() != Some(&REPLY_KIND) {
            return None;
        }
        let celsius = *args.get(CPU_INDEX)?;
        (celsius != 0).then_some(Self(celsius))
    }
}
