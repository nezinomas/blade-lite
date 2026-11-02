//! The Blade 18 settings window: the app's state and what each click does.

mod accent;
mod app;

pub use accent::accent_colour;
pub use app::{App, AppError, Machine};
