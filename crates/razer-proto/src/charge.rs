//! The battery charge limit, set as Synapse sets it.

use std::ops::RangeInclusive;

use crate::Command;

/// Set the limit. One arg: bit 7 turns it on, the low 7 bits are the percent.
const SET_CHARGE_LIMIT: u16 = 0x0712;
/// Get the limit. The reply arg has the same layout as the set.
const GET_CHARGE_LIMIT: u16 = 0x0792;
const ON: u8 = 0x80;
const PERCENT_MASK: u8 = 0x7F;
/// A safe range from the open-source tools (Synapse offers 50 to 80 on
/// other Blades). The capture of this model saw only 80. Anything else is
/// refused.
const SAFE_PERCENT: RangeInclusive<u8> = 50..=80;

/// Why a charge-limit command was not built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeLimitError {
    /// The percent is outside 50 to 80.
    UnsafePercent(u8),
}

/// The model's charge limit: charging stops at `percent` while it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeLimit {
    pub percent: u8,
}

impl ChargeLimit {
    /// The command that turns the limit on or off.
    ///
    /// # Errors
    ///
    /// [`ChargeLimitError::UnsafePercent`] if the percent is outside 50 to 80.
    pub fn command(self, on: bool) -> Result<Command, ChargeLimitError> {
        if !SAFE_PERCENT.contains(&self.percent) {
            return Err(ChargeLimitError::UnsafePercent(self.percent));
        }
        let flag = if on { ON } else { 0 };
        Ok(Command {
            id: SET_CHARGE_LIMIT,
            args: vec![flag | self.percent],
        })
    }

    /// The command that reads the limit.
    #[must_use]
    pub fn query() -> Command {
        Command {
            id: GET_CHARGE_LIMIT,
            args: vec![0x00],
        }
    }

    /// Whether the limit is on, from a query's reply args. `None` if the
    /// reply holds another percent than the model's.
    #[must_use]
    pub fn from_reply(self, args: &[u8]) -> Option<bool> {
        let [value, ..] = *args else {
            return None;
        };
        (value & PERCENT_MASK == self.percent).then_some(value & ON != 0)
    }
}
