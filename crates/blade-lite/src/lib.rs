//! The Blade 18 settings window: the app's state and what each click does.

#![deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

mod accent;
mod app;
mod autostart_item;
mod background_gpu;
pub mod icon;
mod key_reader;
mod placement;
mod report;
mod session_end;
mod slow_polls;
mod start;
mod usb_suspend;
mod watchdog;

pub use accent::accent_colour;
pub use app::{App, AppError, Machine, RESEND_TRIES, Session, took_effect};
pub use autostart_item::{
    AUTOSTART_LABEL, AUTOSTART_OTHER_LABEL, AUTOSTART_UNKNOWN_LABEL, AutostartItem, autostart_item,
    takeover_note,
};
pub use background_gpu::BackgroundGpu;
pub use key_reader::{
    ENTER_READ_WAIT, FnKeys, HeardReports, KEY_QUIET_SPELL, KEY_READ_WAIT, KEY_REOPEN_PAUSE,
    KeyEvent, KeyReportSource, ReaderEnd, UNHEARD_SPELLS_BEFORE_ALL, UnheardSpells, WindowsFnKeys,
    reads_on, step_on_presses,
};
pub use placement::{Rect, flyout_position, popup_position};
pub use report::{
    Reporter, SaidLately, ShownErrors, open_log, open_quiet_log, report, report_panics,
};
pub use session_end::{LoopWork, end_on_loop};
pub use slow_polls::{
    LoopSteps, PollTime, SLOW_POLL, SLOW_POLL_QUIET, SlowPolls, StepTime, TimedSensors, poll_late,
};
pub use start::{config_or_defaults, try_times, try_until_it_works};
pub use usb_suspend::{report_usb_suspend, usb_suspend_on};
pub use watchdog::{LOOK_GAP, STALL, Watchdog};
