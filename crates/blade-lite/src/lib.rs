//! The Blade 18 settings window: the app's state and what each click does.

mod accent;
mod app;
mod key_reader;
mod placement;

pub use accent::accent_colour;
pub use app::{App, AppError, Machine};
pub use key_reader::{KEY_IDLE_READS, KEY_READ_WAIT, KeyReportSource, ReaderEnd, step_on_presses};
pub use placement::{Rect, flyout_position};
