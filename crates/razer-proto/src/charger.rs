//! Which charger powers the laptop, read as Synapse reads it.

use crate::Command;

/// Get the charger. The reply has two args: the charger that powers the
/// laptop now, then one that stayed the same in every read (its meaning is
/// not known).
const GET_CHARGER: u16 = 0x078C;

/// The model's charger read: the reply that means its own full power
/// charger. Any other reply, such as a USB-C source, is a weak charge: the
/// second byte's meaning is not known, so a reply never seen is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charger {
    pub full_power: [u8; 2],
}

impl Charger {
    /// The command that reads the charger.
    #[must_use]
    pub fn query() -> Command {
        Command {
            id: GET_CHARGER,
            args: vec![0x00, 0x00],
        }
    }

    /// Whether the full power charger powers the laptop, from a query's
    /// reply args. `None` if the reply is not two bytes.
    #[must_use]
    pub fn full_power(self, args: &[u8]) -> Option<bool> {
        let [now, other] = *args else {
            return None;
        };
        Some([now, other] == self.full_power)
    }
}
