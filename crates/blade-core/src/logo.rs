//! The lid logo light. The controller reports its state, but turns it on
//! again after a wake, so the app saves it and sends it again.

use razer_device::DeviceError;
use razer_proto::ModelProfile;

use crate::{Controller, Query};

/// Why the logo could not be set.
#[derive(Debug, thiserror::Error)]
pub enum LogoError {
    /// The model has no lid logo.
    #[error("the model has no lid logo")]
    NoLogo,
    /// The controller did not take a command.
    #[error("the controller did not take a command: {0}")]
    Device(DeviceError),
}

/// Whether the logo is on. `None` if the model has no logo, or the
/// controller's reply is not on or off.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn logo_on(
    controller: &mut impl Query,
    model: &ModelProfile,
) -> Result<Option<bool>, DeviceError> {
    let Some(logo) = model.logo else {
        return Ok(None);
    };
    let args = controller.query(&logo.query())?;
    Ok(logo.from_reply(&args))
}

/// Turns the logo on or off.
///
/// # Errors
///
/// See [`LogoError`]. Nothing is sent if the model has no logo.
pub fn set_logo(
    on: bool,
    controller: &mut impl Controller,
    model: &ModelProfile,
) -> Result<(), LogoError> {
    let logo = model.logo.ok_or(LogoError::NoLogo)?;
    controller
        .run(&logo.commands(on))
        .map_err(LogoError::Device)
}
