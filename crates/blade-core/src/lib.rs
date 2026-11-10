//! Decides what to apply to the Blade 18 and keeps the user's settings.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::print_stderr
)]

mod apply;
mod autostart;
mod battery_preset;
mod battery_rate;
mod charge;
mod charger;
mod config;
mod instance;
mod keyboard_brightness;
mod log;
mod logo;
mod native_mode;
mod poll;
mod power_key;
mod query;
mod razer_software;
mod readings;
mod refresh;
mod touchpad;
mod touchpad_key;

pub use apply::{
    ApplyError, Controller, RESUME_PAUSE, apply, apply_lighting, apply_performance, check,
};
pub use autostart::{
    APPROVED_KEY, Autostart, AutostartError, RUN_KEY, RUN_VALUE, SHOWN_COMMAND_CHARS, TRAY_FLAG,
    autostart, set_autostart, starts_in_tray,
};
pub use battery_rate::{BATTERY_RATE, BatteryRate, RESUME_RATE_TRIES};
pub use charge::{ChargeError, charge_limit_on, set_charge_limit};
pub use charger::full_power_charger;
pub use config::{Config, ConfigError, CustomSettings, Lighting, Preset};
pub use instance::{APP_NAME, Start, start_instance};
pub use keyboard_brightness::{keyboard_brightness, set_vapor_light};
pub use log::{LOG_FILES, LOG_LINE_MAX, LOG_MAX_BYTES, Log, log_line};
pub use logo::{LogoError, logo_on, set_logo};
pub use native_mode::{NativeModeError, NativeModeState, native_mode_state, set_native_mode};
pub use poll::{HEAT_INTERVAL, POLL_INTERVAL, PollSchedule, View};
pub use power_key::{
    BRIGHTNESS_HOLD_LIMIT, BRIGHTNESS_READ_GAP, FN_HOLD_LIMIT, POWER_KEY_PAUSE, PowerKeyPresses,
    next_preset,
};
pub use query::{
    ControllerReadings, Query, chosen_native_mode, read_controller, read_cpu_temperature,
};
pub use razer_software::{razer_software, razer_warning};
pub use readings::{Readings, ReadingsSampler};
pub use refresh::{DisplayError, RefreshRate, current_rate, refresh_rates, set_refresh_rate};
pub use touchpad::{TOUCHPAD_KEY, TOUCHPAD_VALUE, TouchpadError, set_touchpad, touchpad_on};
pub use touchpad_key::paint_touchpad_key;
