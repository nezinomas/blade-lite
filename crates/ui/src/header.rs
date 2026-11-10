//! The header card: the model, the preset, the power pill and the live readings.

use blade_core::{Preset, Readings};
use razer_proto::{Celsius, FanRpm, ModelProfile};

/// Shown in place of a value that could not be read.
pub const MISSING: &str = "—";

/// What the header is built from.
#[derive(Debug, Clone)]
pub struct HeaderState<'a> {
    pub model: &'a ModelProfile,
    /// The saved preset. On battery it may run as another.
    pub preset: Preset,
    pub on_battery: bool,
    pub readings: Readings,
    /// `None` when the controller does not report it.
    pub cpu_celsius: Option<Celsius>,
    /// One per fan, in the model's fan order. `None` if it could not be read.
    pub fans: Vec<Option<FanRpm>>,
}

/// Whether the laptop runs on the charger or the battery, and on the
/// charger whether the battery charges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPill {
    Charging,
    /// Such as when the charge limit is reached.
    NotCharging,
    /// On the charger, charging unknown.
    PluggedIn,
    OnBattery,
}

impl PowerPill {
    /// The pill's text.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Charging => "Plugged in \u{b7} Charging",
            Self::NotCharging => "Plugged in \u{b7} Not charging",
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
            preset: preset_label(state.preset.running(state.on_battery, state.model)),
            power: power_pill(state),
            stats: stats(state),
        }
    }
}

fn power_pill(state: &HeaderState) -> PowerPill {
    match (state.on_battery, state.readings.charging) {
        (true, _) => PowerPill::OnBattery,
        (false, Some(true)) => PowerPill::Charging,
        (false, Some(false)) => PowerPill::NotCharging,
        (false, None) => PowerPill::PluggedIn,
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
