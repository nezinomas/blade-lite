//! Decides what to apply to the Blade 18 and keeps the user's settings.

mod apply;
mod autostart;
mod battery_preset;
mod battery_rate;
mod charge;
mod config;
mod instance;
mod logo;
mod native_mode;
mod poll;
mod power_key;
mod query;
mod readings;
mod refresh;
mod touchpad;
mod touchpad_key;

pub use apply::{
    ApplyError, Controller, RESUME_PAUSE, apply, apply_lighting, apply_performance,
    reapply_on_resume,
};
pub use autostart::{
    APPROVED_KEY, AutostartError, RUN_KEY, RUN_VALUE, TRAY_FLAG, autostart_on, set_autostart,
    starts_in_tray,
};
pub use battery_rate::{BATTERY_RATE, BatteryRate};
pub use charge::{ChargeError, charge_limit_on, set_charge_limit};
pub use config::{Config, ConfigError, CustomSettings, Lighting, Preset};
pub use instance::{APP_NAME, Start, start_instance};
pub use logo::{LogoError, logo_on, set_logo};
pub use native_mode::{NativeModeState, native_mode_state, set_native_mode};
pub use poll::{POLL_INTERVAL, PollSchedule, View};
pub use power_key::{POWER_KEY_PAUSE, PowerKeyPresses, next_preset};
pub use query::{ControllerReadings, Query, chosen_native_mode, read_controller};
pub use readings::{Readings, ReadingsSampler};
pub use refresh::{DisplayError, RefreshRate, current_rate, refresh_rates, set_refresh_rate};
pub use touchpad::{TOUCHPAD_KEY, TOUCHPAD_VALUE, TouchpadError, set_touchpad, touchpad_on};
pub use touchpad_key::paint_touchpad_key;
