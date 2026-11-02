//! The refresh rates the laptop's own panel offers at its current resolution,
//! and switching between them.

use std::io;

use platform::{Display, DisplayApi, DisplayMode, OutputTechnology};

/// A refresh rate, in Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefreshRate(pub u32);

/// Why the refresh rates could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DisplayError {
    /// Windows could not list the displays.
    #[error("could not read the displays: {0}")]
    Query(io::Error),
    /// No active display is the laptop's own panel.
    #[error("the laptop's own panel is not active")]
    NoInternalPanel,
    /// The panel does not offer this rate at its current resolution.
    #[error("the panel does not offer {} Hz at its current resolution", .0.0)]
    Unsupported(RefreshRate),
    /// Windows did not switch the panel to the rate.
    #[error("could not set the refresh rate: {0}")]
    Set(io::Error),
}

/// The rates the panel offers at its current resolution, lowest first.
/// External monitors are never looked at.
///
/// # Errors
///
/// See [`DisplayError`].
pub fn refresh_rates(api: &impl DisplayApi) -> Result<Vec<RefreshRate>, DisplayError> {
    let displays = api.displays().map_err(DisplayError::Query)?;
    let panel = internal_panel(&displays).ok_or(DisplayError::NoInternalPanel)?;
    Ok(rates_at_current_resolution(panel))
}

/// The panel's current rate.
///
/// # Errors
///
/// See [`DisplayError`].
pub fn current_rate(api: &impl DisplayApi) -> Result<RefreshRate, DisplayError> {
    let displays = api.displays().map_err(DisplayError::Query)?;
    let panel = internal_panel(&displays).ok_or(DisplayError::NoInternalPanel)?;
    Ok(RefreshRate(panel.current.hz))
}

/// Switches the panel to `rate` and keeps its resolution. A rate the panel
/// does not offer is refused before Windows is asked.
///
/// # Errors
///
/// See [`DisplayError`].
pub fn set_refresh_rate(api: &mut impl DisplayApi, rate: RefreshRate) -> Result<(), DisplayError> {
    let displays = api.displays().map_err(DisplayError::Query)?;
    let panel = internal_panel(&displays).ok_or(DisplayError::NoInternalPanel)?;
    if !rates_at_current_resolution(panel).contains(&rate) {
        return Err(DisplayError::Unsupported(rate));
    }
    let mode = DisplayMode {
        hz: rate.0,
        ..panel.current
    };
    api.set_mode(&panel.name, mode).map_err(DisplayError::Set)
}

/// The panel's rates at its current resolution, each once, lowest first.
fn rates_at_current_resolution(panel: &Display) -> Vec<RefreshRate> {
    let mut rates: Vec<RefreshRate> = panel
        .modes
        .iter()
        .filter(|mode| (mode.width, mode.height) == (panel.current.width, panel.current.height))
        .map(|mode| RefreshRate(mode.hz))
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

/// The laptop's own panel, picked by how it is connected.
fn internal_panel(displays: &[Display]) -> Option<&Display> {
    displays
        .iter()
        .find(|display| display.technology == OutputTechnology::Internal)
}
