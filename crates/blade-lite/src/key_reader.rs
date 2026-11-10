//! Reads the power-mode key's reports and calls back on each press, and
//! while Fn is held or as it comes up. With Num Lock on the keyboard sends only Fn, so
//! while Fn is held, Enter's state is read too.

use std::io;
use std::time::{Duration, Instant};

use blade_core::PowerKeyPresses;
use razer_device::HidKeyReports;

/// How long one read waits for a key report.
pub const KEY_READ_WAIT: Duration = Duration::from_millis(100);

/// How long one read waits while Fn is held, so Enter is read often. A
/// quick tap of Enter is down for about 30 ms (read on the laptop). The
/// short reads last at most [`blade_core::FN_HOLD_LIMIT`] per Fn press.
pub const ENTER_READ_WAIT: Duration = Duration::from_millis(5);

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

/// Somewhere Enter's state comes from.
pub trait EnterKey {
    /// Whether Enter is down now.
    fn down(&mut self) -> bool;
}

/// Enter as Windows sees it now, counted only while Num Lock is on. With
/// Num Lock off the keyboard sends the power key itself, and either Enter
/// key, or one sent by another program, reads the same here, so this
/// keeps the Enter path to the one case it is for.
pub struct WindowsEnter;

impl EnterKey for WindowsEnter {
    fn down(&mut self) -> bool {
        platform::num_lock_on() && platform::enter_down()
    }
}

/// What the key reports said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// The power-mode key was pressed: step the preset.
    PowerKey,
    /// Fn is still held, and [`blade_core::BRIGHTNESS_READ_GAP`] has
    /// passed: Fn + F10 or F11 may be changing the keyboard brightness, so
    /// read it, to follow it live. Up to
    /// [`blade_core::BRIGHTNESS_HOLD_LIMIT`] of a hold.
    FnHeld,
    /// Fn came up: Fn + F10 or F11 may have changed the keyboard
    /// brightness, so read it.
    FnUp,
}

/// Why the reader loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderEnd {
    /// The reports were quiet too long: open them again.
    Reopen,
    /// A read failed, for this reason: open them again.
    Failed(String),
    /// The app is quitting.
    Quit,
}

/// Reads reports, and Enter while Fn is held, and calls `on_key` for
/// each press, every [`blade_core::BRIGHTNESS_READ_GAP`] while Fn is held,
/// and each time Fn comes up. `on_key` returns false once the
/// app is quitting.
pub fn step_on_presses(
    source: &mut impl KeyReportSource,
    enter: &mut impl EnterKey,
    mut presses: PowerKeyPresses,
    mut on_key: impl FnMut(KeyEvent) -> bool,
) -> ReaderEnd {
    let mut quiet = 0;
    while quiet < KEY_IDLE_READS {
        let watching = presses.watching_enter(Instant::now());
        let wait = if watching {
            ENTER_READ_WAIT
        } else {
            KEY_READ_WAIT
        };
        let (key_pressed, fn_up) = match source.read(wait) {
            Ok(Some(report)) => {
                quiet = 0;
                let stepped = presses.on_report(&report, Instant::now());
                (stepped, presses.fn_came_up())
            }
            Ok(None) => {
                // Short reads while Fn is held are not a quiet spell.
                if !watching {
                    quiet += 1;
                }
                (false, false)
            }
            Err(error) => return ReaderEnd::Failed(error.to_string()),
        };
        let now = Instant::now();
        let entered = presses.watching_enter(now) && presses.on_enter(enter.down(), now);
        if (key_pressed || entered) && !on_key(KeyEvent::PowerKey) {
            return ReaderEnd::Quit;
        }
        if fn_up && !on_key(KeyEvent::FnUp) {
            return ReaderEnd::Quit;
        }
        if presses.brightness_due(now) && !on_key(KeyEvent::FnHeld) {
            return ReaderEnd::Quit;
        }
    }
    ReaderEnd::Reopen
}
