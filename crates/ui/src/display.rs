//! The Display group: refresh rate, 60 Hz on battery, and native mode.

use blade_core::{BATTERY_RATE, NativeModeState, RefreshRate};
use razer_proto::{ModelProfile, NativeMode};

/// What the Display group is built from.
#[derive(Debug, Clone)]
pub struct DisplayState<'a> {
    pub model: &'a ModelProfile,
    /// The rates the panel offers at its current resolution.
    pub offered: Vec<RefreshRate>,
    pub current: RefreshRate,
    /// `None` if not known, such as not read while Razer's own software
    /// runs: no native mode is chosen then.
    pub native: Option<NativeModeState>,
    pub sixty_hz_on_battery: bool,
    pub on_battery: bool,
}

/// A change the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayCommand {
    SetRefreshRate(RefreshRate),
    SetSixtyHzOnBattery(bool),
    SetNativeMode(NativeMode),
}

/// One refresh-rate button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateSegment {
    pub rate: RefreshRate,
    pub label: String,
    pub selected: bool,
}

/// One native-mode choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOption {
    pub mode: NativeMode,
    pub label: String,
    pub selected: bool,
}

/// The Display group, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayView {
    pub rates: Vec<RateSegment>,
    /// `false` while the rate is locked to 60 Hz on battery.
    pub rates_enabled: bool,
    /// Under the refresh rate's title. Empty when there is nothing to say.
    pub rate_note: String,
    pub sixty_hz_on_battery: bool,
    pub native_modes: Vec<NativeOption>,
    /// Under the native mode's title.
    pub native_note: String,
    /// Shown while the chosen native mode waits for a restart.
    pub restart_infobar: bool,
}

impl DisplayView {
    /// Builds the group from the state.
    #[must_use]
    pub fn new(state: &DisplayState) -> Self {
        let locked = state.on_battery && state.sixty_hz_on_battery;
        let (chosen, pending) = match state.native {
            Some(NativeModeState::Active(mode)) => (Some(mode), false),
            Some(NativeModeState::AppliesAfterRestart(mode)) => (Some(mode), true),
            None => (None, false),
        };
        Self {
            rates: rate_segments(&state.offered, state.current),
            rates_enabled: !locked,
            rate_note: rate_note(locked),
            sixty_hz_on_battery: state.sixty_hz_on_battery,
            native_modes: native_options(state.model, chosen),
            native_note: chosen.map_or_else(|| "Unknown".into(), |c| native_note(c, pending)),
            restart_infobar: pending,
        }
    }

    /// The command for the rate button at `index`, unless it changes nothing.
    #[must_use]
    pub fn pick_rate(&self, index: usize) -> Option<DisplayCommand> {
        let segment = self.rates.get(index).filter(|s| !s.selected)?;
        self.rates_enabled
            .then_some(DisplayCommand::SetRefreshRate(segment.rate))
    }

    /// The command that flips the 60 Hz on battery switch.
    #[must_use]
    pub fn toggle_sixty_hz_on_battery(&self) -> DisplayCommand {
        DisplayCommand::SetSixtyHzOnBattery(!self.sixty_hz_on_battery)
    }

    /// The command for the native mode at `index`, unless it is already chosen.
    #[must_use]
    pub fn pick_native_mode(&self, index: usize) -> Option<DisplayCommand> {
        let option = self.native_modes.get(index).filter(|o| !o.selected)?;
        Some(DisplayCommand::SetNativeMode(option.mode))
    }
}

fn rate_segments(offered: &[RefreshRate], current: RefreshRate) -> Vec<RateSegment> {
    offered
        .iter()
        .map(|&rate| RateSegment {
            rate,
            label: format!("{} Hz", rate.0),
            selected: rate == current,
        })
        .collect()
}

fn rate_note(locked: bool) -> String {
    if locked {
        format!("Locked to {} Hz while on battery", BATTERY_RATE.0)
    } else {
        String::new()
    }
}

fn native_options(model: &ModelProfile, chosen: Option<NativeMode>) -> Vec<NativeOption> {
    model
        .native_modes
        .iter()
        .map(|&mode| NativeOption {
            mode,
            label: format!("{}, up to {} Hz", resolution(mode), mode.hz),
            selected: Some(mode) == chosen,
        })
        .collect()
}

fn native_note(chosen: NativeMode, pending: bool) -> String {
    if pending {
        format!("{} after restart", resolution(chosen))
    } else {
        resolution(chosen)
    }
}

fn resolution(mode: NativeMode) -> String {
    format!("{} × {}", mode.width, mode.height)
}
