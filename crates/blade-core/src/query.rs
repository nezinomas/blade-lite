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
    /// One line per value that could not be read, such as `fan 1: busy`.
    pub errors: Vec<String>,
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
/// value empty, and names it in the errors.
pub fn read_controller(controller: &mut impl Query, model: &ModelProfile) -> ControllerReadings {
    let mut errors = Vec::new();
    let cpu = read_value(
        controller,
        &Celsius::query(),
        Celsius::cpu_from_reply,
        "CPU temperature",
        &mut errors,
    );
    let fans = FanRpm::queries(model)
        .iter()
        .enumerate()
        .map(|(index, query)| {
            let name = format!("fan {}", index + 1);
            read_value(controller, query, FanRpm::from_reply, &name, &mut errors)
        })
        .collect();
    ControllerReadings { cpu, fans, errors }
}

/// Reads one value. A failed read or a reply not understood adds a line to
/// `errors`, starting with `name`.
fn read_value<T>(
    controller: &mut impl Query,
    query: &Command,
    parse: impl FnOnce(&[u8]) -> Option<T>,
    name: &str,
    errors: &mut Vec<String>,
) -> Option<T> {
    let value = match controller.query(query) {
        Ok(args) => parse(&args).ok_or_else(|| "reply not understood".to_owned()),
        Err(error) => Err(format!("{error:?}")),
    };
    value.map_err(|error| errors.push(format!("{name}: {error}"))).ok()
}
