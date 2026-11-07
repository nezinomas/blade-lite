//! The touchpad key: red in the lighting frame while the touchpad is off,
//! as it looked with Synapse.

use razer_device::DeviceError;
use razer_proto::{ModelProfile, static_colour, static_colour_touchpad_off};

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
    let commands = if red {
        static_colour_touchpad_off(lighting.colour, model)
    } else {
        static_colour(lighting.colour, model)
    };
    controller.run(&commands)
}
