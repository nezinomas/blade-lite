//! The Blade 18 settings window: the app's state and what each click does.

#![deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

mod accent;
mod app;
mod autostart_item;
pub mod icon;
mod key_reader;
mod placement;
mod report;

pub use accent::accent_colour;
pub use app::{App, AppError, Machine, took_effect};
pub use autostart_item::{
    AUTOSTART_LABEL, AUTOSTART_OTHER_LABEL, AUTOSTART_UNKNOWN_LABEL, AutostartItem, autostart_item,
    takeover_note,
};
pub use key_reader::{
    ENTER_READ_WAIT, FnKeys, KEY_IDLE_READS, KEY_READ_WAIT, KeyEvent, KeyReportSource, ReaderEnd,
    WindowsFnKeys, step_on_presses,
};
pub use placement::{Rect, flyout_position, popup_position};
pub use report::{
    Reporter, SaidLately, ShownErrors, open_log, open_quiet_log, report, report_panics,
};
