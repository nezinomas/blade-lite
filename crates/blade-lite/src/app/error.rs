//! Why a click or a view failed.

use blade_core::{
    ApplyError, ChargeError, ConfigError, DisplayError, LogoError, NativeModeError, Preset,
    TouchpadError,
};
use razer_device::DeviceError;

/// Why a click or a view failed.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Display(#[from] DisplayError),
    #[error(transparent)]
    Touchpad(#[from] TouchpadError),
    #[error(transparent)]
    Logo(#[from] LogoError),
    #[error(transparent)]
    Charge(#[from] ChargeError),
    #[error(transparent)]
    NativeMode(#[from] NativeModeError),
    #[error("the controller did not answer: {0}")]
    Device(DeviceError),
    #[error("the controller chose a native mode the model lacks")]
    UnknownNativeMode,
    #[error("this preset is not offered on battery: {0:?}")]
    NotOnBattery(Preset),
    /// The laptop took the change, and the app keeps it, but the config
    /// file could not be written.
    #[error("sent, but not saved: {0}")]
    NotSaved(ConfigError),
}

impl AppError {
    /// True only for [`NotSaved`](Self::NotSaved): the laptop took the
    /// change, though it was not saved. False does not prove that nothing
    /// was sent (a send may fail part way).
    #[must_use]
    pub fn was_sent(&self) -> bool {
        matches!(self, Self::NotSaved(_))
    }
}

/// Whether the laptop took the change: it worked, or it was sent but not
/// saved. A new power mode then shows the popup.
#[must_use]
pub fn took_effect(result: &Result<(), AppError>) -> bool {
    result.as_ref().map_or_else(AppError::was_sent, |()| true)
}

impl From<DeviceError> for AppError {
    fn from(error: DeviceError) -> Self {
        Self::Device(error)
    }
}
