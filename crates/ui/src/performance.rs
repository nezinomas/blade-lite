//! The Performance group: the preset and, in Custom, its three sliders.

use blade_core::{CustomSettings, Preset};
use razer_proto::{ModelProfile, PerfMode};

use crate::header::preset_label;

/// The steps the fan slider moves in, in rpm.
const RPM_STEP: u16 = 100;
/// The fan slider's lowest stop.
const AUTO: &str = "Auto";

/// What the Performance group is built from.
#[derive(Debug, Clone)]
pub struct PerformanceState<'a> {
    pub model: &'a ModelProfile,
    pub preset: Preset,
    pub custom: CustomSettings,
}

/// A change the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceCommand {
    SetPreset(Preset),
    SetCpuBoost(u8),
    SetGpuBoost(u8),
    /// `None` puts the fans on auto.
    SetFanRpm(Option<u16>),
}

/// One preset choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetOption {
    pub preset: Preset,
    pub label: &'static str,
    pub selected: bool,
}

/// What a slider sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderKind {
    CpuBoost,
    GpuBoost,
    Fan,
}

/// One Custom slider. Its stops are numbered from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slider {
    pub kind: SliderKind,
    pub label: &'static str,
    pub stops: usize,
    pub position: usize,
    /// The value at `position`, as text.
    pub text: String,
}

/// The Performance group, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceView {
    pub presets: Vec<PresetOption>,
    /// Empty unless the preset is Custom.
    pub sliders: Vec<Slider>,
    fan_min: u16,
}

impl PerformanceView {
    /// Builds the group from the state.
    #[must_use]
    pub fn new(state: &PerformanceState) -> Self {
        let sliders = if state.preset == Preset::Custom {
            sliders(state.model, state.custom)
        } else {
            Vec::new()
        };
        Self {
            presets: preset_options(state.model, state.preset),
            sliders,
            fan_min: state.model.custom.fan_rpm.min,
        }
    }

    /// The command for the preset at `index`, unless it is already chosen.
    #[must_use]
    pub fn pick_preset(&self, index: usize) -> Option<PerformanceCommand> {
        let option = self.presets.get(index).filter(|o| !o.selected)?;
        Some(PerformanceCommand::SetPreset(option.preset))
    }

    /// The command for moving the slider at `index` to `position`, unless
    /// the move changes nothing or is off the slider.
    #[must_use]
    pub fn move_slider(&self, index: usize, position: usize) -> Option<PerformanceCommand> {
        let slider = self
            .sliders
            .get(index)
            .filter(|s| s.position != position && position < s.stops)?;
        Some(match slider.kind {
            SliderKind::CpuBoost => PerformanceCommand::SetCpuBoost(level(position)),
            SliderKind::GpuBoost => PerformanceCommand::SetGpuBoost(level(position)),
            SliderKind::Fan => PerformanceCommand::SetFanRpm(fan_rpm(position, self.fan_min)),
        })
    }
}

fn preset_options(model: &ModelProfile, current: Preset) -> Vec<PresetOption> {
    model
        .presets
        .iter()
        .map(|&mode| preset(mode))
        .chain([Preset::Custom])
        .map(|preset| PresetOption {
            preset,
            label: preset_label(preset),
            selected: preset == current,
        })
        .collect()
}

fn preset(mode: PerfMode) -> Preset {
    match mode {
        PerfMode::Silent => Preset::Silent,
        PerfMode::Balanced => Preset::Balanced,
        PerfMode::Performance => Preset::Performance,
    }
}

fn sliders(model: &ModelProfile, custom: CustomSettings) -> Vec<Slider> {
    let limits = model.custom;
    vec![
        boost_slider(
            SliderKind::CpuBoost,
            "CPU boost",
            limits.cpu_max,
            custom.cpu_boost,
        ),
        boost_slider(
            SliderKind::GpuBoost,
            "GPU boost",
            limits.gpu_max,
            custom.gpu_boost,
        ),
        fan_slider(model, custom.fan_rpm),
    ]
}

fn boost_slider(kind: SliderKind, label: &'static str, max: u8, level: u8) -> Slider {
    Slider {
        kind,
        label,
        stops: usize::from(max) + 1,
        position: usize::from(level),
        text: level.to_string(),
    }
}

/// Auto first, then each speed of the model's range.
fn fan_slider(model: &ModelProfile, rpm: Option<u16>) -> Slider {
    let range = model.custom.fan_rpm;
    let speeds = usize::from((range.max - range.min) / RPM_STEP) + 1;
    let (position, text) = match rpm {
        None => (0, AUTO.to_owned()),
        Some(rpm) => (
            // A hand-edited config may hold a speed off the range.
            usize::from((rpm.clamp(range.min, range.max) - range.min) / RPM_STEP) + 1,
            format!("{rpm} rpm"),
        ),
    };
    Slider {
        kind: SliderKind::Fan,
        label: "Fan",
        stops: speeds + 1,
        position,
        text,
    }
}

fn level(position: usize) -> u8 {
    u8::try_from(position).expect("a boost slider has few stops")
}

fn fan_rpm(position: usize, min: u16) -> Option<u16> {
    let step = u16::try_from(position.checked_sub(1)?).expect("a fan slider has few stops");
    Some(min + step * RPM_STEP)
}
