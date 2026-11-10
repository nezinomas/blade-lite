//! The user-tweakable Custom preset: CPU boost, GPU boost and fan.

use crate::preset::{FAN_AUTO, SET_PRESET, VARIABLE_STORAGE};
use crate::{Command, ModelProfile};

/// Set a boost level. Args are `[1, cluster, level]`.
const SET_BOOST: u16 = 0x0D07;
/// Set a fan's target speed. Args are `[1, fan, rpm / 100]`.
const SET_FAN: u16 = 0x0D01;
/// The preset mode byte of Custom.
const CUSTOM_MODE: u8 = 0x04;
/// The fan byte: the fan follows the target set with [`SET_FAN`].
const FAN_MANUAL: u8 = 0x01;
const CPU_CLUSTER: u8 = 0x01;
const GPU_CLUSTER: u8 = 0x02;
/// The controller takes fan speeds in steps of this many RPM.
const RPM_STEP: u16 = 100;

/// What the model accepts in Custom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomLimits {
    /// The highest CPU boost level. The lowest is 0.
    pub cpu_max: u8,
    /// The highest GPU boost level. The lowest is 0.
    pub gpu_max: u8,
    /// The manual fan speeds the model takes.
    pub fan_rpm: RpmRange,
    /// The fans a manual speed sets, in the order Synapse sets them.
    pub fan_zones: &'static [u8],
}

/// The lowest and highest manual fan speed, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpmRange {
    pub min: u16,
    pub max: u16,
}

/// Why a Custom value could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomError {
    /// The boost level is above the model's highest.
    BoostTooHigh(u8),
    /// The fan speed is outside the model's range.
    RpmOutOfRange(u16),
    /// The fan speed is not a whole number of hundreds.
    RpmNotHundreds(u16),
}

/// A CPU boost level the model accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuBoost(u8);

impl CpuBoost {
    /// # Errors
    ///
    /// [`CustomError::BoostTooHigh`] if the model has no such level.
    pub fn new(level: u8, model: &ModelProfile) -> Result<Self, CustomError> {
        checked_boost(level, model.custom.cpu_max).map(Self)
    }
}

/// A GPU boost level the model accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuBoost(u8);

impl GpuBoost {
    /// # Errors
    ///
    /// [`CustomError::BoostTooHigh`] if the model has no such level.
    pub fn new(level: u8, model: &ModelProfile) -> Result<Self, CustomError> {
        checked_boost(level, model.custom.gpu_max).map(Self)
    }
}

fn checked_boost(level: u8, max: u8) -> Result<u8, CustomError> {
    if level > max {
        return Err(CustomError::BoostTooHigh(level));
    }
    Ok(level)
}

/// A manual fan speed the model accepts, kept as the byte the controller
/// takes: rpm / 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rpm(u8);

impl Rpm {
    /// # Errors
    ///
    /// [`CustomError::RpmNotHundreds`] if it is not whole hundreds,
    /// [`CustomError::RpmOutOfRange`] if it is outside the model's range or
    /// does not fit in the byte.
    pub fn new(rpm: u16, model: &ModelProfile) -> Result<Self, CustomError> {
        if rpm % RPM_STEP != 0 {
            return Err(CustomError::RpmNotHundreds(rpm));
        }
        let range = model.custom.fan_rpm;
        if !(range.min..=range.max).contains(&rpm) {
            return Err(CustomError::RpmOutOfRange(rpm));
        }
        u8::try_from(rpm / RPM_STEP)
            .map(Self)
            .map_err(|_| CustomError::RpmOutOfRange(rpm))
    }

    /// The byte the controller takes: rpm / 100.
    fn byte(self) -> u8 {
        self.0
    }
}

/// How the fans run in Custom. A manual fan exists only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanMode {
    /// The controller picks the speed.
    Auto,
    /// Every fan runs at this speed.
    Manual(Rpm),
}

impl FanMode {
    fn byte(self) -> u8 {
        match self {
            Self::Auto => FAN_AUTO,
            Self::Manual(_) => FAN_MANUAL,
        }
    }
}

/// The Custom preset. To put the fans back on auto, set it again with
/// [`FanMode::Auto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Custom {
    pub cpu: CpuBoost,
    pub gpu: GpuBoost,
    pub fan: FanMode,
}

impl Custom {
    /// The commands that set it, in Synapse's order: the mode on each zone,
    /// the boosts, then the fan targets. A mode write after a fan target
    /// would reset the target.
    #[must_use]
    pub fn commands(self, model: &ModelProfile) -> Vec<Command> {
        let mut commands = self.mode_commands(model);
        commands.push(boost_command(CPU_CLUSTER, self.cpu.0));
        commands.push(boost_command(GPU_CLUSTER, self.gpu.0));
        if let FanMode::Manual(rpm) = self.fan {
            commands.extend(fan_commands(rpm, model));
        }
        commands
    }

    fn mode_commands(self, model: &ModelProfile) -> Vec<Command> {
        model
            .perf_zones
            .iter()
            .map(|&zone| Command {
                id: SET_PRESET,
                args: vec![VARIABLE_STORAGE, zone, CUSTOM_MODE, self.fan.byte()],
            })
            .collect()
    }
}

fn boost_command(cluster: u8, level: u8) -> Command {
    Command {
        id: SET_BOOST,
        args: vec![VARIABLE_STORAGE, cluster, level],
    }
}

fn fan_commands(rpm: Rpm, model: &ModelProfile) -> impl Iterator<Item = Command> + '_ {
    model.custom.fan_zones.iter().map(move |&fan| Command {
        id: SET_FAN,
        args: vec![VARIABLE_STORAGE, fan, rpm.byte()],
    })
}
