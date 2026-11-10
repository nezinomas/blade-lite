//! The touchpad key: red in the lighting frame while the touchpad is off,
//! as it looked with Synapse.

use razer_device::DeviceError;
use razer_proto::{Command, ModelProfile, Rgb, static_colour, static_colour_touchpad_off};

use crate::{Controller, Lighting};

/// Paints the lighting colour again, with the touchpad key red or not.
/// The brightness is left as it is. A model without the key is left alone.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn paint_touchpad_key(
    red: bool,
    lighting: Lighting,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), DeviceError> {
    if model.touchpad_key.is_none() {
        return Ok(());
    }
    controller.run(&colour_frame(lighting.colour, model, red))
}

/// The static colour frame, with the touchpad key red or not.
pub(crate) fn colour_frame(colour: Rgb, model: &ModelProfile, red: bool) -> Vec<Command> {
    if red {
        static_colour_touchpad_off(colour, model)
    } else {
        static_colour(colour, model)
    }
}
