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
    /// have changed it) and sends every setting again. Not in a wake's
    /// pause: the wake's own send covers it.
    pub(super) fn start_after_updater(self: &Rc<Self>) {
        if self.resuming.get() > 0 || !self.updater_ended.take() {
            return;
        }
        self.begin_pause();
        let shell = Rc::clone(self);
        Timer::single_shot(RESUME_PAUSE, move || {
            shell.resuming.set(shell.resuming.get().saturating_sub(1));
            shell.app.borrow_mut().check_firmware();
            // A power event in the pause was dropped.
            shell.follow_source();
            shell.app.borrow_mut().take_on_battery(P::on_battery());
            shell.act(AppOn::<P>::start);
        });
    }

    /// Looks for Razer software each [`RAZER_LOOK_GAP`], as it may start
    /// or end at any time; the windows are filled again on a change, and
    /// opened on the first find of the run.
    pub(super) fn look_for_razer_now_and_then(self: &Rc<Self>, now: Instant) {
        let Some(last) = self.razer_looked.replace(Some(now)) else {
            return;
        };
        if now.saturating_duration_since(last) < RAZER_LOOK_GAP {
            self.razer_looked.set(Some(last));
            return;
        }
        let before = self.razer_warning.borrow().clone();
        let first = self.look_for_razer();
        if first {
            self.show_window();
        } else if *self.razer_warning.borrow() != before {
            self.refresh(None);
        }
    }

    #[cfg(test)]
    pub fn programs(&self) -> &P::Programs {
        &self.programs
    }
}
