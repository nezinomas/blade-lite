//! Decides what to apply to the Blade 18 and keeps the user's settings.

mod apply;
mod config;

pub use apply::{ApplyError, Controller, RESUME_PAUSE, apply, reapply_on_resume};
pub use config::{Config, ConfigError, CustomSettings, Preset};
