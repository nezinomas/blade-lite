//! The header card: the model, the preset, the power pill and the live readings.

use blade_core::{Preset, Readings};
use razer_proto::{Celsius, FanRpm, ModelProfile};

/// Shown in place of a value that could not be read.
pub const MISSING: &str = "—";

/// What the header is built from.
#[derive(Debug, Clone)]
pub struct HeaderState<'a> {
    pub model: &'a ModelProfile,
    pub preset: Preset,
    pub on_battery: bool,
    pub readings: Readings,
    /// `None` when the controller does not report it.
    pub cpu_celsius: Option<Celsius>,
    /// One per fan, in the model's fan order. `None` if it could not be read.
    pub fans: Vec<Option<FanRpm>>,
}

/// Whether the laptop runs on the charger or the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPill {
    PluggedIn,
    OnBattery,
}

impl PowerPill {
    /// The pill's text.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::PluggedIn => "Plugged in",
            Self::OnBattery => "On battery",
        }
    }
}

/// One live reading, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub label: String,
    pub value: String,
}

/// The header card, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub preset: &'static str,
    pub power: PowerPill,
    /// In the order they are shown.
    pub stats: Vec<Stat>,
}

impl Header {
    /// Builds the header from the state.
    #[must_use]
    pub fn new(state: &HeaderState) -> Self {
        Self {
            title: state.model.name.to_owned(),
            preset: preset_label(state.preset),
            power: if state.on_battery {
                PowerPill::OnBattery
            } else {
                PowerPill::PluggedIn
            },
            stats: stats(state),
        }
    }
}

pub(crate) fn preset_label(preset: Preset) -> &'static str {
    match preset {
        Preset::Silent => "Silent",
        Preset::Balanced => "Balanced",
        Preset::Performance => "Performance",
        Preset::Custom => "Custom",
    }
}

/// CPU and GPU first, then the fans, then the battery.
fn stats(state: &HeaderState) -> Vec<Stat> {
    let mut all = processor_stats(state);
    all.extend(fan_stats(&state.fans));
    all.extend(battery_stats(&state.readings));
    all
}

fn processor_stats(state: &HeaderState) -> Vec<Stat> {
    let cpu_load = state.readings.cpu_load_percent;
    let gpu = state.readings.gpu;
    vec![
        stat("CPU", state.cpu_celsius.map(|c| celsius(c.0.into()))),
        stat("CPU load", cpu_load.map(|p| percent(p.into()))),
        stat("GPU", gpu.map(|g| celsius(g.celsius))),
        stat("GPU load", gpu.map(|g| percent(g.load_percent))),
    ]
}

fn fan_stats(fans: &[Option<FanRpm>]) -> impl Iterator<Item = Stat> {
    fans.iter().enumerate().map(|(index, rpm)| {
        let label = format!("Fan {}", index + 1);
        stat(&label, rpm.map(|rpm| format!("{} rpm", rpm.0)))
    })
}

fn battery_stats(readings: &Readings) -> [Stat; 2] {
    [
        stat(
            "Battery",
            readings.battery_percent.map(|p| percent(p.into())),
        ),
        stat("Power", readings.power_draw_mw.map(watts)),
    ]
}

fn stat(label: &str, value: Option<String>) -> Stat {
    Stat {
        label: label.to_owned(),
        value: value.unwrap_or_else(|| MISSING.to_owned()),
    }
}

fn celsius(degrees: u32) -> String {
    format!("{degrees} °C")
}

fn percent(value: u32) -> String {
    format!("{value}%")
}

/// Milliwatts as whole watts, rounded.
fn watts(milliwatts: u32) -> String {
    format!("{} W", (milliwatts + 500) / 1000)
}
