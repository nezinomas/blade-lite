//! Sends the saved settings to the controller.

use std::time::Duration;

use razer_device::{Clock, Device, DeviceError, Transport};
use razer_proto::{
    Brightness, BrightnessError, Command, CpuBoost, Custom, CustomError, FanMode, GpuBoost,
    ModelProfile, PerfMode, PresetError, RedKeys, Rpm,
};

use crate::red_keys::colour_frame;
use crate::{Config, CustomSettings, Lighting, Query};

/// How long to wait after resume before applying again, so the controller is
/// awake. A guess: the device layer also reads again (never resends) while it replies Busy.
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
    /// The saved brightness is over 100 percent.
    #[error("the saved brightness is over 100 percent: {0:?}")]
    Brightness(BrightnessError),
    /// The controller did not take a command. The commands before it,
    /// and maybe that one too, may have run.
    #[error("the controller did not take a command, so the laptop may run part of the change: {0}")]
    Device(DeviceError),
    /// The controller took the preset, then did not take a lid logo or
    /// lighting command. The ones before it, and maybe that one too, may
    /// have run.
    #[error(
        "the preset was sent, but the controller did not take the lid logo or lighting, so the lid logo or keys may show part of the change: {0}"
    )]
    AfterPreset(DeviceError),
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

/// Sends the saved settings to the controller, with the keys in `red`
/// red. Nothing is sent if a value is outside what the model
/// takes. The preset is its own run, so a later error does not leave the
/// preset unsure.
///
/// # Errors
///
/// See [`ApplyError`]: [`ApplyError::Device`] if the preset run failed,
/// [`ApplyError::AfterPreset`] if the preset was taken and the rest failed.
pub fn apply(
    config: &Config,
    model: &ModelProfile,
    controller: &mut impl Controller,
    red: RedKeys,
) -> Result<(), ApplyError> {
    let preset = preset_commands(*config, model)?;
    let rest = rest_commands(*config, model, red)?;
    controller.run(&preset).map_err(ApplyError::Device)?;
    if rest.is_empty() {
        return Ok(());
    }
    controller.run(&rest).map_err(ApplyError::AfterPreset)
}

/// Checks that the model takes every value in `config`, as [`apply`]
/// does before it sends anything, and the Custom values too while another
/// preset is picked, as they are sent once Custom is. Nothing is sent.
///
/// # Errors
///
/// The [`ApplyError`] for a value the model does not take.
pub fn check(config: &Config, model: &ModelProfile) -> Result<(), ApplyError> {
    custom(config.custom, model).map_err(ApplyError::Custom)?;
    preset_commands(*config, model)?;
    rest_commands(*config, model, RedKeys::default()).map(drop)
}

/// Sends only the saved preset, with its Custom values.
///
/// # Errors
///
/// See [`ApplyError`].
pub fn apply_performance(
    config: &Config,
    model: &ModelProfile,
    controller: &mut impl Controller,
) -> Result<(), ApplyError> {
    let commands = preset_commands(*config, model)?;
    controller.run(&commands).map_err(ApplyError::Device)
}

/// Reads back the mode and fan byte of each zone that `config`'s preset
/// run sets (ticket 152), and names the zones that differ from what was
/// sent, such as "zone 1 reads 01 01 00 00, sent 01 01 05 00". Reads only:
/// a difference is never sent again. A value the model refuses gives no
/// commands, so nothing to read.
///
/// # Errors
///
/// The first failed read, or a reply too short to compare
/// ([`DeviceError::ShortReply`]).
pub fn preset_differs(
    config: &Config,
    model: &ModelProfile,
    controller: &mut impl Query,
) -> Result<Option<String>, DeviceError> {
    let Ok(commands) = preset_commands(*config, model) else {
        return Ok(None);
    };
    let mut differs = Vec::new();
    for (query, sent) in commands.iter().filter_map(PerfMode::read_back) {
        let read = controller.query(&query)?;
        if read.len() < sent.len() {
            return Err(DeviceError::ShortReply);
        }
        if read.get(..sent.len()) != Some(sent.as_slice()) {
            differs.push(format!(
                "zone {} reads {}, sent {}",
                sent.get(1).copied().unwrap_or_default(),
                hex(&read),
                hex(&sent)
            ));
        }
    }
    Ok((!differs.is_empty()).then(|| differs.join("; ")))
}

/// Bytes as hex, space apart.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sends only the lighting: the brightness, the vapor light, then the
/// colour on every key, with the keys in `red` red.
///
/// # Errors
///
/// See [`ApplyError`].
pub fn apply_lighting(
    lighting: Lighting,
    model: &ModelProfile,
    controller: &mut impl Controller,
    red: RedKeys,
) -> Result<(), ApplyError> {
    let commands = lighting_commands(lighting, model, red)?;
    controller.run(&commands).map_err(ApplyError::Device)
}

/// The commands sent after the preset: the saved lid logo, then the saved
/// lighting. The logo goes before the long key frame, so a device error in
/// that frame does not keep it from being sent.
fn rest_commands(
    config: Config,
    model: &ModelProfile,
    red: RedKeys,
) -> Result<Vec<Command>, ApplyError> {
    let mut commands = Vec::new();
    if let (Some(on), Some(logo)) = (config.lid_logo, model.logo) {
        commands.extend(logo.commands(on));
    }
    if let Some(lighting) = config.lighting {
        commands.extend(lighting_commands(lighting, model, red)?);
    }
    Ok(commands)
}

/// The brightness, the vapor light, then the colour on every key.
fn lighting_commands(
    lighting: Lighting,
    model: &ModelProfile,
    red: RedKeys,
) -> Result<Vec<Command>, ApplyError> {
    let brightness = Brightness::new(lighting.brightness).map_err(ApplyError::Brightness)?;
    let mut commands = brightness.commands(model);
    if let Some(vapor) = model.vapor_light {
        commands.push(vapor.command(brightness, lighting.vapor_light));
    }
    commands.extend(colour_frame(lighting.colour, model, red));
    Ok(commands)
}

/// The commands that set the saved preset.
fn preset_commands(config: Config, model: &ModelProfile) -> Result<Vec<Command>, ApplyError> {
    let Some(mode) = config.preset.perf_mode() else {
        let custom = custom(config.custom, model).map_err(ApplyError::Custom)?;
        return Ok(custom.commands(model));
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
