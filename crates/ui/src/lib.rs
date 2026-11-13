//! View models that turn the app's state into what the window and flyout show.

#![deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

mod display;
mod fan_guard;
mod header;
mod keyboard;
mod performance;
mod popup;

pub use display::{DisplayCommand, DisplayState, DisplayView, NativeOption, RateSegment};
pub use fan_guard::{FAN_GUARD_COOL_HOLD, FAN_GUARD_WAKE_HOLD, FanGuard};
pub use header::{Header, HeaderState, MISSING, PowerPill, Stat};
pub use keyboard::{KeyboardCommand, KeyboardState, KeyboardView, Swatch};
pub use performance::{
    ChargeLimitSwitch, PerformanceCommand, PerformanceState, PerformanceView, PresetOption, Slider,
    SliderKind,
};
pub use popup::{HEAT_HOLD, HeatWatch, ModeWatch, Sensor, TouchpadWatch};
