//! Reads the power-mode key's reports and calls back on each press.

use std::io;
use std::time::{Duration, Instant};

use blade_core::PowerKeyPresses;
use razer_device::HidKeyReports;

/// How long one read waits for a key report.
pub const KEY_READ_WAIT: Duration = Duration::from_millis(100);

/// Quiet reads in a row after which the reports are opened again, about a
/// minute. A handle that went stale over a sleep may never fail, so this
/// keeps the key from dying without a word.
pub const KEY_IDLE_READS: u32 = 600;

/// Somewhere key reports come from.
pub trait KeyReportSource {
    /// Waits up to `wait` for a report. `None` if none came.
    ///
    /// # Errors
    ///
    /// Any I/O error, such as the keyboard going away.
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>>;
}

impl KeyReportSource for HidKeyReports {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        HidKeyReports::read(self, wait)
    }
}

/// Why the reader loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderEnd {
    /// A read failed or the reports were quiet too long: open them again.
    Reopen,
    /// The app is quitting.
    Quit,
}

/// Reads reports and calls `on_press` for each press. `on_press` returns
/// false once the app is quitting.
pub fn step_on_presses(
    source: &mut impl KeyReportSource,
    mut presses: PowerKeyPresses,
    mut on_press: impl FnMut() -> bool,
) -> ReaderEnd {
    let mut quiet = 0;
    while quiet < KEY_IDLE_READS {
        let report = match source.read(KEY_READ_WAIT) {
            Ok(Some(report)) => report,
            Ok(None) => {
                quiet += 1;
                continue;
            }
            Err(error) => {
                eprintln!("blade-lite: the key reports stopped: {error}");
                return ReaderEnd::Reopen;
            }
        };
        quiet = 0;
        if presses.on_report(&report, Instant::now()) && !on_press() {
            return ReaderEnd::Quit;
        }
    }
    ReaderEnd::Reopen
}
