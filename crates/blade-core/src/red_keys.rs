//! The red keys: the touchpad key while the touchpad is off, as it looked
//! with Synapse, and the mic key while the mic is muted.

use razer_device::DeviceError;
use razer_proto::{Command, ModelProfile, RedKeys, Rgb, static_colour_red};

use crate::{Controller, Lighting};

/// Paints the lighting colour again, with the keys in `red` red. The
/// brightness is left as it is. A model with neither key is left alone.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn paint_red_keys(
    red: RedKeys,
    lighting: Lighting,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), DeviceError> {
    if model.touchpad_key.is_none() && model.mic_key.is_none() {
        return Ok(());
    }
    controller.run(&colour_frame(lighting.colour, model, red))
}

/// The static colour frame, with the keys in `red` red.
pub(crate) fn colour_frame(colour: Rgb, model: &ModelProfile, red: RedKeys) -> Vec<Command> {
    static_colour_red(colour, model, red)
}
