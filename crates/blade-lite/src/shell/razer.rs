//! Looks for Razer's own software, such as Synapse or a Razer driver from
//! Windows Update, which would talk to the controller beside the app. While
//! it runs the background reads and the fan guard stop (the user picked
//! that over a guard that could fight Synapse, 2026-11-12); while a Razer
//! updater runs nothing at all is sent (ticket 145).

use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_core::{RESUME_PAUSE, razer_software, razer_updater, razer_warning};
use blade_lite::report;
use slint::Timer;

use super::{AppOn, Parts, Shell};

/// How often Razer software is looked for, beside the start, each wake
/// and each open again.
const RAZER_LOOK_GAP: Duration = Duration::from_secs(60);

impl<P: Parts> Shell<P> {
    /// Looks for Razer software. The next refresh shows the warning while
    /// it runs, and drops it once it is gone. A failed look is logged and
    /// keeps the warning as it was. Called at start and after each resume,
    /// before the refresh. True only the first time something is found in
    /// this run, so the caller opens the window once.
    pub fn look_for_razer(&self) -> bool {
        let found = match razer_software(&self.programs) {
            Ok(found) => found,
            Err(error) => {
                report(&format!("can't look for Razer software: {error}"));
                return false;
            }
        };
        let updater = razer_updater(&found);
        // The hold itself, as the start may have set it before the shell.
        let held = self.app.borrow().controller_held();
        if held != updater {
            report(if updater {
                "a Razer updater runs, so the app sends nothing"
            } else {
                "no Razer updater runs now, so the settings are sent again"
            });
            // A tick sends them after a pause, as the update may have
            // reset the controller and changed its firmware.
            self.updater_ended.set(!updater);
        }
        self.app.borrow_mut().hold_controller(updater);
        let warning = razer_warning(&found);
        let first = warning.is_some() && !self.razer_opened.replace(true);
        let before = self.razer_warning.replace(warning.clone());
        if warning != before {
            report(
                warning
                    .as_deref()
                    .unwrap_or("Razer software no longer runs"),
            );
        }
        first
    }

    /// After a Razer updater ended: waits the wake's pause, as the
    /// controller may restart, then reads the firmware again (the update may
    /// have changed it) and sends every setting again. Not in a pause: the
    /// last pause's end begins it (see [`Self::wait_for_updater`]).
    pub(super) fn start_after_updater(self: &Rc<Self>) {
        if self.resuming.get() > 0 || !self.updater_ended.take() {
            return;
        }
        let pause = self.begin_pause();
        let shell = Rc::clone(self);
        Timer::single_shot(RESUME_PAUSE, move || {
            shell.resuming.set(shell.resuming.get().saturating_sub(1));
            // A sleep or a newer pause since: nothing reaches the controller,
            // and the last pause reads the firmware and sends.
            if shell.superseded(pause) {
                shell.redo_at_wake.set(true);
                return;
            }
            // Begun at a wake's end: the wake's end now (ticket 179).
            if shell.wake_due.get() {
                shell.end_wake_pause();
                return;
            }
            // Begun at an open again's end: its release (ticket 179).
            if shell.redo_at_wake.take() {
                P::release_controller();
                shell.controller_gone.set(false);
            }
            shell.app.borrow_mut().check_firmware();
            // A power event in the pause was dropped.
            shell.follow_source();
            shell.app.borrow_mut().take_on_battery(P::on_battery());
            shell.act(AppOn::<P>::start);
            // No read while the updater ran, so the last is old: the fan
            // guard reads at the next tick, not one skipped as late (171).
            shell.schedule.borrow_mut().read_heat_now();
        });
    }

    /// At the last pause's end, after its look: an updater found ended in
    /// that pause or by that look gets its own full pause, as the update may
    /// have reset the controller just now (ticket 179). The updater's end
    /// then does this pause's work too (a wake's send, a release and a
    /// firmware read). True if it began, so the caller sends nothing.
    pub(super) fn wait_for_updater(self: &Rc<Self>) -> bool {
        if !self.updater_ended.get() {
            return false;
        }
        self.redo_at_wake.set(true);
        self.start_after_updater();
        true
    }

    /// Reads the firmware again if the last read failed (such as a
    /// controller still coming up at login), unless reads wait (a pause, or
    /// Razer software runs); the windows show the new warning. Called after
    /// the look and the power events, so their state is fresh.
    pub(super) fn read_firmware_if_unread(&self) {
        if !self.app.borrow().firmware_unread() || !self.heat_reads_allowed() {
            return;
        }
        self.app.borrow_mut().check_firmware();
        if !self.app.borrow().firmware_unread() {
            self.refresh(None);
        }
    }

    /// Looks for Razer software each [`RAZER_LOOK_GAP`], as it may start
    /// or end at any time; the windows are filled again on a change, and
    /// opened on the first find of the run. A firmware read that failed is
    /// tried again then too (by the tick).
    /// Returns whether it looked, so the tick tries a failed firmware read
    /// again after it has followed the power events.
    pub(super) fn look_for_razer_now_and_then(self: &Rc<Self>, now: Instant) -> bool {
        let Some(last) = self.razer_looked.replace(Some(now)) else {
            return false;
        };
        if now.saturating_duration_since(last) < RAZER_LOOK_GAP {
            self.razer_looked.set(Some(last));
            return false;
        }
        let before = self.razer_warning.borrow().clone();
        let first = self.look_for_razer();
        if first {
            self.show_window();
        } else if *self.razer_warning.borrow() != before {
            self.refresh(None);
        }
        true
    }

    #[cfg(test)]
    pub fn programs(&self) -> &P::Programs {
        &self.programs
    }
}
