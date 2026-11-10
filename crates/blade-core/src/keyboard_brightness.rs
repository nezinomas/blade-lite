//! The keyboard brightness that the laptop sets itself, on Fn + F10 or
//! F11, and the vapor light that follows it as it follows the slider.

use razer_device::DeviceError;
use razer_proto::{Brightness, ModelProfile};

use crate::{ApplyError, Controller, Lighting, Query};

/// The keyboard brightness the controller runs now. `None` if the model
/// has no brightness, or the reply is for another LED.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn keyboard_brightness(
    controller: &mut impl Query,
    model: &ModelProfile,
) -> Result<Option<Brightness>, DeviceError> {
    let Some(query) = Brightness::query(model) else {
        return Ok(None);
    };
    let args = controller.query(&query)?;
    Ok(Brightness::from_reply(model, &args))
}

/// Sends only the vapor light: the keyboard's level while it is on, 0
/// while off. Nothing if the model has no vapor light.
///
/// # Errors
///
/// See [`ApplyError`]. Nothing is sent for a brightness over 100 percent.
pub fn set_vapor_light(
    lighting: Lighting,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), ApplyError> {
    let Some(vapor) = model.vapor_light else {
        return Ok(());
    };
    let brightness = Brightness::new(lighting.brightness).map_err(ApplyError::Brightness)?;
    controller
        .run(&[vapor.command(brightness, lighting.vapor_light)])
        .map_err(ApplyError::Device)
}
