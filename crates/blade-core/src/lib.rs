//! Decides what to apply to the Blade 18 and keeps the user's settings.

mod apply;
mod battery_rate;
mod config;
mod native_mode;
mod poll;
mod query;
mod readings;
mod refresh;
mod touchpad;

pub use apply::{
    ApplyError, Controller, RESUME_PAUSE, apply, apply_lighting, apply_performance,
    reapply_on_resume,
};
pub use battery_rate::{BATTERY_RATE, BatteryRate};
pub use config::{Config, ConfigError, CustomSettings, Lighting, Preset};
pub use native_mode::{NativeModeState, native_mode_state, set_native_mode};
pub use poll::{POLL_INTERVAL, PollSchedule, View};
pub use query::{ControllerReadings, Query, chosen_native_mode, read_controller};
pub use readings::{Readings, ReadingsSampler};
pub use refresh::{DisplayError, RefreshRate, current_rate, refresh_rates, set_refresh_rate};
pub use touchpad::{TOUCHPAD_KEY, TOUCHPAD_VALUE, TouchpadError, set_touchpad, touchpad_on};
