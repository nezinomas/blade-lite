//! Reads the power-mode key's reports and calls back on each press, and
//! while Fn is held or as it comes up. With Num Lock on the keyboard sends only Fn, so
//! while Fn is held, Enter's state is read too. Fn + mic sends only Fn
//! too, and Win + Alt + K to Windows, so their state is read as well.

use std::io;
use std::time::{Duration, Instant};

use blade_core::PowerKeyPresses;
use razer_device::HidKeyReports;

/// How long one read waits for a key report while more than one key
/// collection is read in turn (before one sends), so a press on another
/// collection is not held back long.
pub const KEY_READ_WAIT: Duration = Duration::from_millis(100);

/// How long one read waits while Fn is held, so Enter is read often. A
/// quick tap of Enter is down for about 30 ms (read on the laptop). The
/// short reads last at most [`blade_core::FN_HOLD_LIMIT`] per Fn press.
pub const ENTER_READ_WAIT: Duration = Duration::from_millis(5);

/// How long the reports may be quiet before they are opened again. A
/// handle that went stale over a sleep may never fail, so this keeps the
/// key from dying without a word. With one collection read, one read
/// waits it all, so the thread does not wake to count quiet reads (a
/// report ends the wait at once; ticket 188).
pub const KEY_QUIET_SPELL: Duration = Duration::from_secs(60);

/// Somewhere key reports come from.
pub trait KeyReportSource {
    /// Waits up to `wait` for a report. `None` if none came.
    ///
    /// # Errors
    ///
    /// Any I/O error, such as the keyboard going away.
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>>;

    /// Whether only one collection is read, so a long wait holds no other
    /// one's report back.
    fn single(&self) -> bool;
}

impl KeyReportSource for HidKeyReports {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        HidKeyReports::read(self, wait)
    }

    fn single(&self) -> bool {
        HidKeyReports::single(self)
    }
}

/// Somewhere the state of the keys read while Fn is held comes from.
pub trait FnKeys {
    /// Whether Enter is down now.
    fn enter_down(&mut self) -> bool;
    /// Whether the keys Fn + mic sends (Win + Alt + K) are down now.
    fn mic_down(&mut self) -> bool;
}

/// The keys as Windows sees them now.
pub struct WindowsFnKeys;

impl FnKeys for WindowsFnKeys {
    /// Enter, counted only while Num Lock is on. With Num Lock off the
    /// keyboard sends the power key itself, and either Enter key, or one
    /// sent by another program, reads the same here, so this keeps the
    /// Enter path to the one case it is for.
    fn enter_down(&mut self) -> bool {
        platform::num_lock_on() && platform::enter_down()
    }

    fn mic_down(&mut self) -> bool {
        platform::mic_keys_down()
    }
}

/// What the key reports said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// The power-mode key was pressed: step the preset.
    PowerKey,
    /// Fn + mic was pressed: mute or unmute the mic.
    MicKey,
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

/// How long to wait before opening the key reports again, after they
/// fail, or come back quiet too soon to be a real quiet spell.
pub const KEY_REOPEN_PAUSE: Duration = Duration::from_secs(5);

impl ReaderEnd {
    /// How long to wait before the next open, after a reader that ran for
    /// `ran` (its whole run, not only the quiet spell). A quiet spell opens
    /// again at once, so no press is lost in the pause. A run shorter than
    /// half its quiet reads' waits had reads that did not wait, a broken
    /// handle, so it pauses, as a failure does. A handle that breaks late
    /// in a long run gets one open at once; the next, short run pauses.
    #[must_use]
    pub fn pause_before_open(&self, ran: Duration) -> Duration {
        if self.keeps_key_state(ran) {
            Duration::ZERO
        } else {
            KEY_REOPEN_PAUSE
        }
    }

    /// Whether this was a real quiet spell, after a reader that ran for
    /// `ran`, so the next open keeps the key as last seen (see
    /// [`blade_core::PowerKeyPresses::after_quiet_spell`]). Not after a
    /// failure, or a run too short for its quiet reads to have waited (a
    /// broken handle): either may hide a gap, such as a sleep, so the
    /// state starts unknown again. Either way a first report listing the
    /// key counts once (ticket 174).
    #[must_use]
    pub fn keeps_key_state(&self, ran: Duration) -> bool {
        *self == Self::Reopen && ran >= KEY_QUIET_SPELL / 2
    }

    /// The key state for the next open, after a reader that ran for `ran`
    /// with `presses`: kept after a real quiet spell, else new.
    #[must_use]
    pub fn next_presses(&self, ran: Duration, presses: &PowerKeyPresses) -> PowerKeyPresses {
        if self.keeps_key_state(ran) {
            presses.after_quiet_spell()
        } else {
            presses.fresh()
        }
    }
}

/// Reads reports, and Enter and the mic keys while Fn is held, and calls
/// `on_key` for each press, every [`blade_core::BRIGHTNESS_READ_GAP`] while Fn is held,
/// and each time Fn comes up. `on_key` returns false once the
/// app is quitting. `presses` keeps the key state for the next run (see
/// [`ReaderEnd::keeps_key_state`]).
pub fn step_on_presses(
    source: &mut impl KeyReportSource,
    keys: &mut impl FnKeys,
    presses: &mut PowerKeyPresses,
    mut on_key: impl FnMut(KeyEvent) -> bool,
) -> ReaderEnd {
    // The quiet spell so far: the sum of the quiet reads' waits.
    let mut quiet = Duration::ZERO;
    while quiet < KEY_QUIET_SPELL {
        let watching = presses.watching_enter(Instant::now());
        let wait = read_wait(
            watching,
            source.single(),
            KEY_QUIET_SPELL.saturating_sub(quiet),
        );
        let (key_pressed, fn_up) = match source.read(wait) {
            Ok(Some(report)) => {
                quiet = Duration::ZERO;
                let stepped = presses.on_report(&report, Instant::now());
                (stepped, presses.fn_came_up())
            }
            Ok(None) => {
                // Short reads while Fn is held are not a quiet spell.
                if !watching {
                    quiet += wait;
                }
                (false, false)
            }
            Err(error) => return ReaderEnd::Failed(error.to_string()),
        };
        let now = Instant::now();
        let fn_held = presses.watching_enter(now);
        let entered = fn_held && presses.on_enter(keys.enter_down(), now);
        if (key_pressed || entered) && !on_key(KeyEvent::PowerKey) {
            return ReaderEnd::Quit;
        }
        let mic = fn_held && presses.on_mic(keys.mic_down(), now);
        if mic && !on_key(KeyEvent::MicKey) {
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

/// How long the next read waits: short while Fn is held, the rest of the
/// quiet spell `left` with one collection, else [`KEY_READ_WAIT`] at most.
fn read_wait(watching: bool, single: bool, left: Duration) -> Duration {
    if watching {
        ENTER_READ_WAIT
    } else if single {
        left
    } else {
        KEY_READ_WAIT.min(left)
    }
}
