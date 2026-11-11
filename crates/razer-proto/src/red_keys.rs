//! Keys painted red in the lighting frame: the touchpad key while the
//! touchpad is off, as it looked with Synapse, and the mic key while the
//! mic is muted.

use crate::Rgb;

/// The colour a red key shows.
pub const TOUCHPAD_OFF: Rgb = Rgb::new(0xFF, 0x00, 0x00);

/// Where a key that can turn red sits in the key grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedKey {
    pub row: u8,
    pub column: u8,
    /// Whether the key is also a numpad key while Num Lock is on (one light
    /// for both). It keeps the plain colour then.
    pub plain_with_num_lock: bool,
}

/// Which of the model's keys a frame paints red.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RedKeys {
    /// The touchpad key ([`crate::ModelProfile::touchpad_key`]).
    pub touchpad: bool,
    /// The mic key ([`crate::ModelProfile::mic_key`]).
    pub mic: bool,
}
