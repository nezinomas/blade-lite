//! The panel's native mode: chosen through the controller, live after a
//! restart.

use platform::{Display, DisplayApi};
use razer_device::DeviceError;
use razer_proto::{ModelProfile, NativeMode};

use crate::refresh::internal_panel;
use crate::{Controller, DisplayError};

/// The chosen native mode, and whether the panel runs it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeModeState {
    /// The panel runs the chosen mode.
    Active(NativeMode),
    /// The chosen mode takes effect at the next boot.
    AppliesAfterRestart(NativeMode),
}

/// Compares the controller's chosen mode with the one the panel booted in.
/// The panel booted in the mode whose resolution is its largest.
///
/// # Errors
///
/// See [`DisplayError`].
pub fn native_mode_state(
    chosen: NativeMode,
    model: &ModelProfile,
    api: &impl DisplayApi,
) -> Result<NativeModeState, DisplayError> {
    let displays = api.displays().map_err(DisplayError::Query)?;
    let panel = internal_panel(&displays).ok_or(DisplayError::NoInternalPanel)?;
    if running_mode(panel, model) == Some(chosen) {
        Ok(NativeModeState::Active(chosen))
    } else {
        Ok(NativeModeState::AppliesAfterRestart(chosen))
    }
}

/// Chooses the native mode for the next boot.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn set_native_mode(
    mode: NativeMode,
    controller: &mut impl Controller,
) -> Result<(), DeviceError> {
    controller.run(&[mode.command()])
}

/// The model's mode that matches the panel's largest resolution.
fn running_mode(panel: &Display, model: &ModelProfile) -> Option<NativeMode> {
    let largest = panel
        .modes
        .iter()
        .max_by_key(|mode| u64::from(mode.width) * u64::from(mode.height))?;
    model
        .native_modes
        .iter()
        .copied()
        .find(|mode| (mode.width, mode.height) == (largest.width, largest.height))
}
