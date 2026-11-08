//! The Blade 18 settings window: the app's state and what each click does.

mod accent;
mod app;
mod autostart_item;
mod key_reader;
mod placement;
mod report;

pub use accent::accent_colour;
pub use app::{App, AppError, Machine};
pub use autostart_item::{
    AUTOSTART_LABEL, AUTOSTART_OTHER_LABEL, AUTOSTART_UNKNOWN_LABEL, AutostartItem, autostart_item,
    takeover_note,
};
pub use key_reader::{KEY_IDLE_READS, KEY_READ_WAIT, KeyReportSource, ReaderEnd, step_on_presses};
pub use placement::{Rect, flyout_position, popup_position};
pub use report::{Reporter, SaidLately, ShownErrors, open_log, report, report_panics};
