//! Sends the saved settings to the controller, and again after each resume.

use std::time::Duration;

use platform::{PowerEvent, PowerEvents};
use razer_device::{Clock, Device, DeviceError, Transport};
use razer_proto::{
    Command, CpuBoost, Custom, CustomError, FanMode, GpuBoost, ModelProfile, PerfMode, PresetError,
    Rpm,
};

use crate::{Config, CustomSettings, Preset};

/// How long to wait after resume before applying again, so the controller is
/// awake. A guess: the device layer also retries while it replies Busy.
pub const RESUME_PAUSE: Duration = Duration::from_secs(2);

/// Why the settings could not be applied.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// The model does not offer the saved preset.
    #[error("the model does not offer this preset: {0:?}")]
    Preset(PresetError),
    /// A saved Custom value is outside what the model takes.
    #[error("a Custom value is outside what the model takes: {0:?}")]
    Custom(CustomError),
    /// The controller did not take a command.
    #[error("the controller did not take a command: {0:?}")]
    Device(DeviceError),
}

/// Runs commands on the controller. A trait so that tests need no device.
pub trait Controller {
    /// Sends each command in order, and stops at the first that fails.
    ///
    /// # Errors
    ///
    /// See [`DeviceError`].
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError>;
}

impl<T: Transport, C: Clock> Controller for Device<T, C> {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        Device::run(self, commands)
    }
}

/// Sends the saved settings to the controller. Nothing is sent if a value
/// is outside what the model takes.
///
/// # Errors
///
/// See [`ApplyError`].
pub fn apply(
    config: &Config,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), ApplyError> {
    let commands = commands(*config, model)?;
    controller.run(&commands).map_err(ApplyError::Device)
}

/// Applies the settings again after each resume, once [`RESUME_PAUSE`] has
/// passed. Returns when no more events will come.
///
/// # Errors
///
/// Stops at the first re-apply that fails and returns its [`ApplyError`].
pub fn reapply_on_resume(
    events: &mut impl PowerEvents,
    clock: &impl Clock,
    config: &Config,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), ApplyError> {
    while let Some(event) = events.next_event() {
        if event == PowerEvent::Resumed {
            clock.sleep(RESUME_PAUSE);
            apply(config, model, controller)?;
        }
    }
    Ok(())
}

/// The commands that set the saved preset.
fn commands(config: Config, model: &ModelProfile) -> Result<Vec<Command>, ApplyError> {
    let mode = match config.preset {
        Preset::Silent => PerfMode::Silent,
        Preset::Balanced => PerfMode::Balanced,
        Preset::Performance => PerfMode::Performance,
        Preset::Custom => {
            let custom = custom(config.custom, model).map_err(ApplyError::Custom)?;
            return Ok(custom.commands(model));
        }
    };
    mode.commands(model).map_err(ApplyError::Preset)
}

/// The saved Custom values, checked against the model.
fn custom(saved: CustomSettings, model: &ModelProfile) -> Result<Custom, CustomError> {
    let fan = match saved.fan_rpm {
        None => FanMode::Auto,
        Some(rpm) => FanMode::Manual(Rpm::new(rpm, model)?),
    };
    Ok(Custom {
        cpu: CpuBoost::new(saved.cpu_boost, model)?,
        gpu: GpuBoost::new(saved.gpu_boost, model)?,
        fan,
    })
}
