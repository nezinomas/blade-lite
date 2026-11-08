//! The battery charge limit. The controller keeps it and reports it.

use razer_device::DeviceError;
use razer_proto::{ChargeLimit, ChargeLimitError, ModelProfile};

use crate::{Controller, Query};

/// Why the limit could not be set.
#[derive(Debug, thiserror::Error)]
pub enum ChargeError {
    /// The model has no charge limit.
    #[error("the model has no charge limit")]
    NoLimit,
    /// The model's percent is outside the safe range.
    #[error("the charge limit is not safe: {0:?}")]
    Unsafe(ChargeLimitError),
    /// The controller did not take the command.
    #[error("the controller did not take the charge limit: {0}")]
    Device(DeviceError),
    /// The command was sent, but the read back failed.
    #[error("the charge limit was sent, but the read back failed: {0}")]
    ReadBack(DeviceError),
    /// The controller took the command, but reads back another state.
    #[error("the charge limit reads back {read:?}, not {wanted}")]
    NotConfirmed { wanted: bool, read: Option<bool> },
}

/// Whether the limit is on. `None` if the model has none, or the
/// controller's reply is for another percent.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn charge_limit_on(
    controller: &mut impl Query,
    model: &ModelProfile,
) -> Result<Option<bool>, DeviceError> {
    let Some(limit) = model.charge_limit else {
        return Ok(None);
    };
    let args = controller.query(&ChargeLimit::query())?;
    Ok(limit.from_reply(&args))
}

/// Turns the limit on or off, then reads it back to confirm.
///
/// # Errors
///
/// See [`ChargeError`]. Nothing is sent if the model has no limit or its
/// percent is not safe.
pub fn set_charge_limit<C: Controller + Query>(
    on: bool,
    controller: &mut C,
    model: &ModelProfile,
) -> Result<(), ChargeError> {
    let limit = model.charge_limit.ok_or(ChargeError::NoLimit)?;
    let command = limit.command(on).map_err(ChargeError::Unsafe)?;
    controller
        .run(std::slice::from_ref(&command))
        .map_err(ChargeError::Device)?;
    let read = charge_limit_on(controller, model).map_err(ChargeError::ReadBack)?;
    if read == Some(on) {
        Ok(())
    } else {
        Err(ChargeError::NotConfirmed { wanted: on, read })
    }
}
