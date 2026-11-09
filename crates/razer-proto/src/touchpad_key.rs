//! The touchpad key: painted red in the lighting frame while the touchpad
//! is off, as it looked with Synapse.

use crate::Rgb;

/// The colour the key shows while the touchpad is off.
pub const TOUCHPAD_OFF: Rgb = Rgb::new(0xFF, 0x00, 0x00);

/// Where the model's touchpad key sits in the key grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadKey {
    pub row: u8,
    pub column: u8,
    /// Whether the key is also a numpad key while Num Lock is on (one light
    /// for both). It keeps the plain colour then.
    pub plain_with_num_lock: bool,
}
