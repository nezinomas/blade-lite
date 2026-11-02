//! The refresh rates the laptop's own panel offers at its current resolution.

use std::io;

use platform::{Display, DisplayApi, OutputTechnology};

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
    let mut rates: Vec<RefreshRate> = panel
        .modes
        .iter()
        .filter(|mode| (mode.width, mode.height) == (panel.current.width, panel.current.height))
        .map(|mode| RefreshRate(mode.hz))
        .collect();
    rates.sort_unstable();
    rates.dedup();
    Ok(rates)
}

/// The laptop's own panel, picked by how it is connected.
fn internal_panel(displays: &[Display]) -> Option<&Display> {
    displays
        .iter()
        .find(|display| display.technology == OutputTechnology::Internal)
}
