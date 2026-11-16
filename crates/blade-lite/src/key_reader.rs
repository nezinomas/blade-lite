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

/// How long the reports may be quiet before a quiet spell is counted, the
/// clock of ticket 215's way back ([`UnheardSpells`]). The same handle is
/// read on after it: a handle kept over a sleep and a hibernate heard the
/// key (ticket 221, check 61), so the reports are opened again only after
/// a read error or for the read-all spell. With one collection read, one
/// read waits it all, so the thread does not wake to count quiet reads (a
/// key report ends the wait at once; ticket 188). Any other report
/// neither ends the wait nor counts as a report (ticket 203).
pub const KEY_QUIET_SPELL: Duration = Duration::from_secs(120);

/// How many quiet spells in a row may pass with no key report heard before
/// one spell reads all key collections (about 1 h). That spell wakes as
/// before ticket 209, each [`KEY_READ_WAIT`], and lasts one
/// [`KEY_QUIET_SPELL`] per collection (one wait counts once for all of
/// them), so 4 min with two collections. So it is rare, and a key whose
/// collection moved is dead for up to about 1 h (ticket 215).
pub const UNHEARD_SPELLS_BEFORE_ALL: u32 = 30;

/// The quiet spells in a row with no key report heard. The profile names
/// the one key collection read before any report (ticket 209); if new
/// firmware moves the keys to another one, the key would stay dead for
/// good, so now and then all are read for one spell (ticket 215).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UnheardSpells(u32);

impl UnheardSpells {
    /// Whether the reports are opened again after a quiet spell, on an
    /// open that read all collections (`read_all`) or not: after the
    /// read-all spell, to go back to one collection, and when the read-all
    /// spell is due. Else the same handle is read on (ticket 221).
    #[must_use]
    pub fn opens_again(self, read_all: bool) -> bool {
        read_all || self.0 >= UNHEARD_SPELLS_BEFORE_ALL
    }

    /// Whether the next open reads all key collections, for one spell. The
    /// count starts again at once, so an open that fails or ends short
    /// does not make every later open read all (ticket 215 review).
    pub fn take_read_all(&mut self) -> bool {
        let read_all = self.opens_again(false);
        if read_all {
            *self = Self::default();
        }
        read_all
    }

    /// The count after a real quiet spell. `heard`: a key report was read
    /// since the last spell ([`HeardReports`]).
    #[must_use]
    pub fn after_spell(self, heard: bool) -> Self {
        if heard {
            Self::default()
        } else {
            Self(self.0 + 1)
        }
    }
}

/// Whether the reader reads on with the same handle after a run of
/// [`step_on_presses`] that ended with `end` after `ran`, on an open that
/// reads all collections (`read_all`) or not, with a key report `heard`
/// in that run; `unheard` counts a real quiet spell. Only a real quiet
/// spell reads on, and not when the reports open again for or after the
/// read-all spell ([`UnheardSpells::opens_again`]). A failure or a short
/// run (a broken handle, ticket 131) opens again and counts nothing.
#[must_use]
pub fn reads_on(
    end: &ReaderEnd,
    ran: Duration,
    read_all: bool,
    heard: bool,
    unheard: &mut UnheardSpells,
) -> bool {
    if !end.keeps_key_state(ran) {
        return false;
    }
    *unheard = unheard.after_spell(heard);
    !unheard.opens_again(read_all)
}

/// Key reports that note when one was read, so a quiet spell knows
/// whether the key was heard since the last one: about an hour with no
/// key report, the read-all spell opens the reports again, the way back
/// if a handle ever goes dead with no error (tickets 215, 230).
pub struct HeardReports<S> {
    reports: S,
    heard: bool,
}

impl<S> HeardReports<S> {
    pub fn new(reports: S) -> Self {
        Self {
            reports,
            heard: false,
        }
    }

    /// Whether a key report was read since the last take.
    pub fn take_heard(&mut self) -> bool {
        std::mem::take(&mut self.heard)
    }

    /// The key reports read.
    pub fn reports(&mut self) -> &mut S {
        &mut self.reports
    }
}

impl<S: KeyReportSource> KeyReportSource for HeardReports<S> {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        let read = self.reports.read(wait);
        self.heard |= matches!(read, Ok(Some(_)));
        read
    }

    fn single(&self) -> bool {
        self.reports.single()
    }
}

/// Somewhere key reports come from.
pub trait KeyReportSource {
    /// Waits up to `wait` for a key report. `None` if none came.
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
    /// The reports were quiet for a whole [`KEY_QUIET_SPELL`] (or reads
    /// that did not wait added up to one, a broken handle: see
    /// [`ReaderEnd::keeps_key_state`]).
    QuietSpell,
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
    /// `ran` (its whole run, not only the quiet spell). A quiet spell that
    /// opens again (for or after the read-all spell, see [`reads_on`])
    /// does so at once, so no press is lost in the pause. A run shorter than
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
        *self == Self::QuietSpell && ran >= KEY_QUIET_SPELL / 2
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
            presses.following_brightness(Instant::now()),
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
    ReaderEnd::QuietSpell
}

/// How long the next read waits: very short while Enter is watched, the
/// rest of the quiet spell `left` with one collection and the brightness
/// not followed, else [`KEY_READ_WAIT`] at most (under
/// [`blade_core::BRIGHTNESS_READ_GAP`], so the follow keeps its pace).
fn read_wait(watching: bool, following: bool, single: bool, left: Duration) -> Duration {
    if watching {
        ENTER_READ_WAIT
    } else if single && !following {
        left
    } else {
        KEY_READ_WAIT.min(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Duration = Duration::from_secs(50);

    #[test]
    fn a_single_collection_waits_out_the_spell_only_with_no_fn_held() {
        assert_eq!(read_wait(true, true, true, LEFT), ENTER_READ_WAIT);
        // Fn held past the Enter limit: the brightness follow still reads
        // each gap (ticket 188 review).
        assert_eq!(read_wait(false, true, true, LEFT), KEY_READ_WAIT);
        assert_eq!(read_wait(false, false, true, LEFT), LEFT);
    }

    /// After this many quiet spells with no key report heard.
    fn unheard(spells: u32) -> UnheardSpells {
        (0..spells).fold(UnheardSpells::default(), |unheard, _| {
            unheard.after_spell(false)
        })
    }

    #[test]
    fn all_collections_are_read_after_enough_unheard_spells() {
        assert!(!unheard(0).take_read_all());
        assert!(!unheard(UNHEARD_SPELLS_BEFORE_ALL - 1).take_read_all());
        assert!(unheard(UNHEARD_SPELLS_BEFORE_ALL).take_read_all());
    }

    #[test]
    fn all_are_read_for_one_open_only_however_it_ends() {
        let mut unheard = unheard(UNHEARD_SPELLS_BEFORE_ALL);

        assert!(unheard.take_read_all());
        // The open failed or ended short: nothing counted, still not again.
        assert!(!unheard.take_read_all());
        assert_eq!(unheard, UnheardSpells::default());
    }

    const SPELL: Duration = KEY_QUIET_SPELL;

    #[test]
    fn a_real_quiet_spell_reads_on_and_counts() {
        let mut unheard = UnheardSpells::default();
        assert!(reads_on(
            &ReaderEnd::QuietSpell,
            SPELL,
            false,
            false,
            &mut unheard
        ));
        assert_eq!(unheard, self::unheard(1));
    }

    #[test]
    fn a_heard_key_report_starts_the_count_again_and_reads_on() {
        let mut unheard = unheard(5);
        assert!(reads_on(
            &ReaderEnd::QuietSpell,
            SPELL,
            false,
            true,
            &mut unheard
        ));
        assert_eq!(unheard, UnheardSpells::default());
    }

    #[test]
    fn an_hour_of_unheard_spells_opens_again_to_read_all() {
        let mut unheard = unheard(UNHEARD_SPELLS_BEFORE_ALL - 1);
        assert!(!reads_on(
            &ReaderEnd::QuietSpell,
            SPELL,
            false,
            false,
            &mut unheard
        ));
        assert!(unheard.take_read_all());
    }

    #[test]
    fn the_read_all_spell_opens_again_when_it_ends() {
        let mut unheard = UnheardSpells::default();
        assert!(!reads_on(
            &ReaderEnd::QuietSpell,
            SPELL,
            true,
            true,
            &mut unheard
        ));
    }

    #[test]
    fn a_short_run_or_a_failure_opens_again_and_counts_nothing() {
        let mut unheard = unheard(3);
        let short = Duration::from_secs(1);
        assert!(!reads_on(
            &ReaderEnd::QuietSpell,
            short,
            false,
            false,
            &mut unheard
        ));
        let failed = ReaderEnd::Failed("gone".into());
        assert!(!reads_on(&failed, SPELL, false, false, &mut unheard));
        assert!(!reads_on(
            &ReaderEnd::Quit,
            SPELL,
            false,
            false,
            &mut unheard
        ));
        assert_eq!(unheard, self::unheard(3));
    }

    #[test]
    fn a_quiet_spell_reads_on_on_the_same_handle() {
        assert!(!UnheardSpells::default().opens_again(false));
        assert!(!unheard(UNHEARD_SPELLS_BEFORE_ALL - 1).opens_again(false));
    }

    #[test]
    fn a_quiet_spell_opens_again_when_all_are_due_to_be_read() {
        assert!(unheard(UNHEARD_SPELLS_BEFORE_ALL).opens_again(false));
    }

    #[test]
    fn a_read_all_spell_opens_again_when_it_ends() {
        assert!(UnheardSpells::default().opens_again(true));
    }

    #[test]
    fn a_key_report_heard_starts_the_count_again() {
        let mut heard = unheard(UNHEARD_SPELLS_BEFORE_ALL - 1).after_spell(true);

        assert_eq!(heard, UnheardSpells::default());
        assert!(!heard.take_read_all());
    }

    #[test]
    fn more_than_one_collection_waits_a_short_read_at_most() {
        assert_eq!(read_wait(false, false, false, LEFT), KEY_READ_WAIT);
        let short = Duration::from_millis(30);
        assert_eq!(read_wait(false, false, false, short), short);
    }
}
