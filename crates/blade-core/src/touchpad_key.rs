//! The touchpad key's light: on while the touchpad is off, as with Synapse.

use platform::Registry;
use razer_device::DeviceError;
use razer_proto::ModelProfile;

use crate::{Controller, Query, TouchpadError, touchpad_on};

/// Why the key light could not be set.
#[derive(Debug, thiserror::Error)]
pub enum TouchpadKeyError {
    /// The touchpad state could not be read, so the light is left alone.
    #[error(transparent)]
    Touchpad(#[from] TouchpadError),
    /// The controller did not take the command.
    #[error("the controller did not take the touchpad key light: {0:?}")]
    Device(DeviceError),
    /// The command was sent, but the read back failed.
    #[error("the touchpad key light was sent, but the read back failed: {0:?}")]
    ReadBack(DeviceError),
    /// The controller took the command, but reads back another state.
    #[error("the touchpad key light reads back {read:?}, not {wanted}")]
    NotConfirmed { wanted: bool, read: Option<bool> },
}

/// Lights the touchpad key if the touchpad is off, and turns the light off
/// if it is on. Nothing is sent if the key already shows the state, and
/// what is sent is read back to confirm.
///
/// # Errors
///
/// See [`TouchpadKeyError`]. Nothing is sent if the touchpad state can't
/// be read. A model without the key is left alone.
pub fn light_touchpad_key<C: Controller + Query>(
    registry: &impl Registry,
    controller: &mut C,
    model: &ModelProfile,
) -> Result<(), TouchpadKeyError> {
    let Some(key) = model.touchpad_key else {
        return Ok(());
    };
    let wanted = !touchpad_on(registry)?;
    // A failed first read only means the light is sent anyway.
    let shown = controller
        .query(&key.query())
        .ok()
        .and_then(|args| key.from_reply(&args));
    if shown == Some(wanted) {
        return Ok(());
    }
    controller
        .run(&[key.command(wanted)])
        .map_err(TouchpadKeyError::Device)?;
    let args = controller
        .query(&key.query())
        .map_err(TouchpadKeyError::ReadBack)?;
    let read = key.from_reply(&args);
    if read == Some(wanted) {
        Ok(())
    } else {
        Err(TouchpadKeyError::NotConfirmed { wanted, read })
    }
}
