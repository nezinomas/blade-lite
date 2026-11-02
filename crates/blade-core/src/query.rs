//! What the app reads back from the controller.

use razer_device::{Clock, Device, DeviceError, Transport};
use razer_proto::{Celsius, Command, FanRpm, ModelProfile, NativeMode};

/// Sends a command and gives back the reply's args. A trait so that tests
/// need no device.
pub trait Query {
    /// # Errors
    ///
    /// See [`DeviceError`].
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError>;
}

impl<T: Transport, C: Clock> Query for Device<T, C> {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        Ok(self.send(command.id, &command.args)?.args().to_vec())
    }
}

/// The controller's own readings. A value is `None` when it could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerReadings {
    pub cpu: Option<Celsius>,
    /// One per fan, in the model's fan order.
    pub fans: Vec<Option<FanRpm>>,
}

/// The native mode chosen for the next boot, or `None` for one the model lacks.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn chosen_native_mode(
    controller: &mut impl Query,
    model: &ModelProfile,
) -> Result<Option<NativeMode>, DeviceError> {
    let args = controller.query(&NativeMode::query())?;
    Ok(NativeMode::from_reply(&args, model))
}

/// Reads the CPU temperature and each fan. A failed read leaves only its
/// value empty.
pub fn read_controller(controller: &mut impl Query, model: &ModelProfile) -> ControllerReadings {
    let cpu = controller
        .query(&Celsius::query())
        .ok()
        .and_then(|args| Celsius::cpu_from_reply(&args));
    let fans = FanRpm::queries(model)
        .iter()
        .map(|query| {
            let args = controller.query(query).ok()?;
            FanRpm::from_reply(&args)
        })
        .collect();
    ControllerReadings { cpu, fans }
}
