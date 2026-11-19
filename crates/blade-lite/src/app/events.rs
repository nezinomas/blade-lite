//! What the app does at start, on a power change, on a touchpad switch, on
//! a Num Lock or mic mute change and on quit.

use std::time::Instant;

use blade_core::{
    ApplyError, Config, Controller, DisplayError, Preset, Query, apply, apply_lighting,
    apply_lighting_dark, apply_performance, full_power_charger, keyboard_brightness,
    paint_red_keys, touchpad_on,
};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};
use razer_proto::{Brightness, RedKeys};

use super::{
    App, AppError, CHARGER_STEADY, FLIP_CAP, FLIP_WINDOW, NOT_UNDERSTOOD, Power,
    SOURCE_LIGHTING_SPACING, Slot, SlotGuess,
};
use crate::report;

impl<C, D, R, K> App<C, D, R, K>
where
    C: Controller + Query,
    D: DisplayApi,
    R: Registry,
    K: KeyInjector,
{
    /// Reads the charge limit state, sends the saved settings, then reads
    /// the logo state they leave. The preset that runs in place of the
    /// saved one on battery is sent. The touchpad key is red in that frame
    /// if the touchpad is off and Num Lock is off. Saved lighting lights
    /// the keys again, so a level kept for a screen off is dropped then. A
    /// lighting value out of range still sends the preset, alone.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn start(&mut self) -> Result<(), AppError> {
        self.read_charge_limit();
        self.read_charger();
        let config = self.running(self.config);
        self.read_manual_fan_at_first_start(&config);
        // A start at a wake or after a pause may leave a manual fan the
        // app sent (ticket 298); a first start adds a step only over a
        // manual fan read (ticket 299).
        if let Err(error) = self.end_manual_fan(&config) {
            let cut = Err(error);
            self.note_preset_send(&config, &cut);
            self.check_preset_if_sent(&config, &cut);
            self.note_fan_targets(&config, &cut);
            // As a start cut in its first write.
            self.key_red = None;
            self.read_logo();
            return Ok(cut?);
        }
        let red = self.key_should_be_red();
        let (applied, refused) = match apply(&config, self.model, &mut self.machine.controller, red)
        {
            // A lighting value out of range, as a hand edit can leave
            // it, refuses the lighting and the logo, not the preset:
            // else another one (such as Performance on battery) keeps
            // running (ticket 261). Its error is shown.
            Err(ApplyError::Brightness(error)) => (
                apply_performance(&config, self.model, &mut self.machine.controller),
                Some(ApplyError::Brightness(error)),
            ),
            applied => (applied, None),
        };
        let whole = applied.is_ok() && refused.is_none();
        self.key_red = whole.then_some(red);
        if whole && config.lighting.is_some() {
            self.keys_lit_by_lighting();
        }
        self.note_preset_send(&config, &applied);
        self.check_preset_if_sent(&config, &applied);
        self.note_fan_targets(&config, &applied);
        self.note_manual_sent(&config, &applied);
        self.read_logo();
        if let (Err(_), Some(error)) = (&applied, &refused) {
            // The send's error is the one shown; this one is still said.
            report(&format!("the saved lighting was not sent: {error}"));
        }
        applied?;
        refused.map_or(Ok(()), |error| Err(error.into()))
    }

    /// Makes the keys dark while Windows has the screen off, and puts their
    /// level back once it is on (ticket 241). Windows turns the screen off
    /// on its idle timeout long before any sleep notice (none at all on AC
    /// with "sleep after" never), so the keys would stay lit (`NOTES.md`).
    /// Only the keyboard's level is sent; the vapor light, colour, fans
    /// and preset stay. The level is read first, so a level the user set
    /// with Fn + F10 / F11 comes back too; a failed read, a model with no
    /// level, or keys already dark send nothing. Nothing is sent if the
    /// keys already follow this state. A failed put-back is tried at a
    /// later look, as the shell spaces and caps them (ticket 253; see
    /// [`Self::screen_followed`]): the level is kept until it is back. A failed read or dark send is not: lit keys are the
    /// safe side, and a failed dark send is put back at the screen on.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn follow_screen(&mut self, screen_off: bool) -> Result<(), AppError> {
        if self.screen_followed(screen_off) {
            return Ok(());
        }
        let followed = if screen_off {
            self.darken_keys()
        } else {
            self.keys_back_on()
        };
        if screen_off || followed.is_ok() {
            self.keys_follow = Some(screen_off);
        }
        followed
    }

    /// Whether the keys already follow this screen state, so
    /// [`Self::follow_screen`] has nothing to do.
    #[must_use]
    pub fn screen_followed(&self, screen_off: bool) -> bool {
        self.keys_follow == Some(screen_off)
    }

    /// How many times the keys were set to follow no screen state (after
    /// a send of the saved lighting or a charger change), wrapping: the
    /// shell gives a failed put-back its tries again only when it changes
    /// (ticket 285).
    #[must_use]
    pub fn keys_follow_resets(&self) -> u32 {
        self.keys_follow_resets
    }

    /// The screen needs a new look: the keys follow no state now.
    pub(super) fn follow_no_screen(&mut self) {
        self.keys_follow = None;
        self.keys_follow_resets = self.keys_follow_resets.wrapping_add(1);
    }

    fn darken_keys(&mut self) -> Result<(), AppError> {
        if self.dark_keys.is_some() {
            return Ok(());
        }
        let Some(level) = keyboard_brightness(&mut self.machine.controller, self.model)? else {
            return Ok(());
        };
        if level.percent() == 0 {
            return Ok(());
        }
        // Kept before the send: one that fails may have gone in part.
        self.dark_keys = Some(level);
        self.dark_slot = if self.source_known { self.slot } else { None };
        let dark = Brightness::new(0).map_err(ApplyError::Brightness)?;
        self.machine.controller.run(&dark.commands(self.model))?;
        report(&format!(
            "the screen is off, so the keyboard is dark (it was at {} %)",
            level.percent()
        ));
        Ok(())
    }

    /// Puts back the keyboard's level kept for a screen off, if the app
    /// made the keys dark: at the screen on, and at the quit, so the keys
    /// are not left dark with no app.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. The level is kept for the next try.
    pub fn keys_back_on(&mut self) -> Result<(), AppError> {
        let (level, guessed) = match self.dark_keys {
            Some(level) => (level, false),
            None => match self.left_dark_in_use()? {
                Some(level) => (level, true),
                None => return Ok(()),
            },
        };
        self.machine.controller.run(&level.commands(self.model))?;
        self.dark_keys = None;
        // Used once: a 0 read after it is the user's own.
        if guessed {
            self.left_dark = None;
        }
        report(&format!(
            "the keyboard is lit again at {} %",
            level.percent()
        ));
        Ok(())
    }

    /// The level kept for the other slot, if the slot in use reads 0: the
    /// controller swaps on its own time, up to 0.6 s or more before or
    /// after Windows' notice (tickets 242, 243), so the slot the app made
    /// dark may still be, or already be, the one in use. A user's own 0 is
    /// lit then too (lit keys are the safe side). Never with the lighting
    /// saved: its send at the charger change puts each slot right, and a
    /// saved 0 is the user's. Nor while the shell refuses the guess (see
    /// [`Self::refuse_slot_guess`]); the level then stays kept for the next
    /// swap into its slot. The level is used once, whichever slot it lands
    /// in. A failed read is an error, so the next look reads again.
    fn left_dark_in_use(&mut self) -> Result<Option<Brightness>, AppError> {
        let Some((_, level)) = self.left_dark else {
            return Ok(None);
        };
        if self.config.lighting.is_some() || self.slot_guess == SlotGuess::Refused {
            return Ok(None);
        }
        let read = keyboard_brightness(&mut self.machine.controller, self.model)?;
        Ok(read.filter(|read| read.percent() == 0).map(|_| level))
    }

    /// Whether the put-back may guess the other slot's level from a 0 read,
    /// for the shell to set before each put-back: refused while Razer's
    /// software runs (as last looked, up to 60 s old), as a 0 read then
    /// may be its own. The level stays kept for the next swap into its
    /// slot (ticket 247), and goes in then with no read, also while
    /// Razer's software runs: a level it set in that slot meanwhile is
    /// lost, and the keys are lit (the safe side). Allowed, as the put-back
    /// of keys the app made dark goes while it runs too: both undo the
    /// app's own 0. The saved lighting's send at a charger change is
    /// dropped then, as it writes the user's whole lighting.
    pub fn refuse_slot_guess(&mut self, refuse: bool) {
        self.slot_guess = if refuse {
            SlotGuess::Refused
        } else {
            SlotGuess::Allowed
        };
    }

    /// Paints the touchpad key red if the touchpad is now off and Num Lock
    /// is off, or back in the lighting colour if not. Nothing is sent if
    /// the key already shows the state, or the user has not set the
    /// lighting, since the app then leaves the keyboard's own effect alone.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. Nothing is sent if the touchpad state can't be
    /// read.
    pub fn on_touchpad_changed(&mut self) -> Result<(), AppError> {
        if self.config.lighting.is_none() {
            return Ok(());
        }
        let red = self.red_for(touchpad_on(&self.machine.registry)?);
        self.paint_key(red)?;
        Ok(())
    }

    /// Paints the red keys again when Num Lock changes: a key that is
    /// then a numpad key keeps the plain colour. If the touchpad state
    /// can't be read, the key keeps the plain colour, as at start. Returns
    /// whether a frame was sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. The new state is kept even if the send fails, and
    /// the next change paints the key.
    pub fn on_num_lock(&mut self, on: bool) -> Result<bool, AppError> {
        if self.num_lock_on == on || self.config.lighting.is_none() {
            self.num_lock_on = on;
            return Ok(false);
        }
        self.num_lock_on = on;
        let red = self.key_should_be_red();
        self.paint_key(red)
    }

    /// Paints the mic key red when the mic is now muted and Num Lock is
    /// off, or back in the lighting colour if not. `None` is a mute that
    /// can't be read: the key is plain then. Returns whether a frame was
    /// sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. The new state is kept even if the send fails, and
    /// the next change paints the key.
    pub fn on_mic_muted(&mut self, muted: Option<bool>) -> Result<bool, AppError> {
        if self.mic_muted == muted || self.config.lighting.is_none() {
            self.mic_muted = muted;
            return Ok(false);
        }
        self.mic_muted = muted;
        let red = self.key_should_be_red();
        self.paint_key(red)
    }

    /// Paints the red keys red or plain in the saved lighting, unless the
    /// frame shows them so already or no lighting is saved. Returns whether
    /// a frame was sent.
    fn paint_key(&mut self, red: RedKeys) -> Result<bool, AppError> {
        let Some(lighting) = self.config.lighting else {
            return Ok(false);
        };
        if self.key_red == Some(red) {
            return Ok(false);
        }
        let painted = paint_red_keys(red, lighting, self.model, &mut self.machine.controller);
        self.key_red = painted.is_ok().then_some(red);
        painted?;
        Ok(true)
    }

    /// Follows the power source, and applies everything again after a
    /// resume, with 60 Hz again on battery if Windows put the rate back.
    /// On battery only Silent or Balanced runs; the saved preset comes
    /// back on AC. The caller waits [`blade_core::RESUME_PAUSE`]
    /// before passing on a resume. A sleep sends the quit preset (see
    /// [`Self::quit_preset`]; ticket 169), as the controller stays powered
    /// on Modern Standby and would run a manual fan on in the sleep; the
    /// saved settings are kept, so the wake sends them again. The rate
    /// stays.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        // The rate is not touched, so its held error stays.
        if event == PowerEvent::Suspending {
            return self.quit_preset();
        }
        // The shell follows the screen with its state (`follow_screen`).
        if event == PowerEvent::ScreenChanged {
            return Ok(());
        }
        self.held_rate_error = None;
        match event {
            // Done above.
            PowerEvent::Suspending | PowerEvent::ScreenChanged => Ok(()),
            PowerEvent::Resumed => {
                // The rate follows even if the start fails.
                let started = self.start();
                // The source read now, as the charger may have gone in or
                // out in the sleep, before its power event comes. An
                // unknown source leaves the rate alone.
                let rate = if self.source_known {
                    let displays = &mut self.machine.displays;
                    let on_battery = self.power == Power::Battery;
                    let rate = self
                        .battery_rate
                        .on_resume(on_battery, &self.config, displays)
                        .map_err(AppError::from);
                    self.quiet_if_tried_again(rate)
                } else {
                    Ok(())
                };
                self.hold_rate_error(started, rate, "the rate after the wake failed too")
            }
            PowerEvent::OnBattery | PowerEvent::OnAc => {
                // The rate follows even if the preset send fails.
                let preset = self.set_on_battery(event == PowerEvent::OnBattery);
                self.note_slot_with_lighting(event == PowerEvent::OnBattery);
                let rate = self.follow_power(event);
                let rate = self.quiet_if_tried_again(rate);
                self.hold_rate_error(preset, rate, "the rate change failed too")
            }
        }
    }

    /// Takes the power source read at the end of an open again's pause,
    /// before the start that sends every setting: only the rate follows a
    /// change now, so the preset goes once, with the start (ticket 256).
    /// `None` (unknown) counts as battery, as at a start, so a known battery
    /// read after an unknown one counts as a change too (ticket 281).
    ///
    /// # Errors
    ///
    /// See [`AppError`]: the rate change's.
    pub fn take_source_before_start(&mut self, on_battery: Option<bool>) -> Result<(), AppError> {
        let was_known = self.source_known;
        let changed = on_battery.is_some_and(|now| now != self.on_battery() || !was_known);
        self.take_on_battery(on_battery);
        if !changed {
            return Ok(());
        }
        // As `on_power`: an error held for the old source is stale.
        self.held_rate_error = None;
        let event = if self.on_battery() {
            PowerEvent::OnBattery
        } else {
            PowerEvent::OnAc
        };
        let rate = self.follow_power(event);
        self.quiet_if_tried_again(rate)
    }

    /// Whether a charger change since the last send left the saved
    /// lighting due (see [`Self::send_source_lighting`]).
    #[must_use]
    pub fn source_lighting_due(&self) -> bool {
        self.source_lighting_due.is_some()
    }

    /// Drops a due lighting send, for when the shell must not send it
    /// (Razer's software runs, or a pause whose end sends all settings).
    pub fn forget_source_lighting(&mut self) {
        self.source_lighting_due = None;
    }

    /// The controller keeps one keyboard level per power source and swaps
    /// them itself at a charger change (ticket 243): call this after a
    /// known source changed to the other known one, by an event, a read at
    /// a wake or a pause's end, or before a sleep. With the lighting saved,
    /// its send at the change or the wake puts the level back, so only a
    /// stale kept level is dropped. With none saved, the app follows its
    /// own dark: a slot it left at 0 for a screen off keeps its level for
    /// the next swap into it (ticket 245), a level kept for the new slot is
    /// the one to put back, and the screen needs a new look, which makes
    /// the new slot dark for a screen off (ticket 246) or puts that level
    /// back. A slot the app left at 0 stays so after a quit until the user
    /// lights it: the app can write only the slot in use.
    pub(super) fn follow_dark_slots(&mut self) {
        let on_battery = self.on_battery();
        let kept = self
            .left_dark
            .take_if(|(battery, _)| *battery == on_battery)
            .map(|(_, level)| level);
        if self.config.lighting.is_some() {
            return;
        }
        self.follow_no_screen();
        // Made dark while the source was unknown, the keys may be in either
        // slot: their level stays for the slot in use (lit keys are the
        // safe side; ticket 247). With a level kept for the new slot, that
        // slot holds the app's 0 (a dark read 0 sends nothing), so the
        // later dark was in the other one: it becomes the guess.
        if let Some(level) = self.dark_keys
            && self.dark_slot.is_none()
        {
            if let Some(kept) = kept {
                self.left_dark = Some((!on_battery, level));
                self.dark_keys = Some(kept);
                self.dark_slot = Some(Slot::of(on_battery));
            }
            return;
        }
        if let Some(level) = self.dark_keys {
            self.left_dark = Some((!on_battery, level));
        }
        self.dark_keys = kept;
        self.dark_slot = Some(Slot::of(on_battery));
    }

    /// Sends the saved lighting due after a charger change (ticket 244),
    /// once for any number of changes since the last send, and not before
    /// [`SOURCE_LIGHTING_SPACING`] since the last send or try: a change
    /// inside it waits for a later look. This caps the rate, not the count:
    /// a plug that bounces for hours sends 6 a minute (whether the stored
    /// writes wear the controller is not known). Keys dark for a screen off
    /// meanwhile leave the new slot lit at its own level until the send
    /// (lit keys are the safe side), and a Fn press meanwhile takes no
    /// level, so a press made then is undone by the send. Keys the app
    /// keeps dark for a screen off (ticket 241) stay dark: the colour and
    /// vapor light go with the keyboard at level 0, and the saved level is
    /// kept for the put-back. A failed send is only logged, and tried again
    /// at a later look,
    /// [`SOURCE_LIGHTING_TRIES`](super::SOURCE_LIGHTING_TRIES) times in
    /// all: the same saved lighting each time, so a send that went in part
    /// is made whole.
    pub fn send_source_lighting(&mut self, now: Instant) {
        let spaced = self
            .source_lighting_sent
            .is_none_or(|sent| now.saturating_duration_since(sent) >= SOURCE_LIGHTING_SPACING);
        if !spaced {
            return;
        }
        let Some(tries) = self.source_lighting_due.take() else {
            return;
        };
        // Due only with the lighting saved; a Reset since sends nothing.
        let Some(lighting) = self.config.lighting else {
            return;
        };
        self.source_lighting_sent = Some(now);
        let dark = self.dark_keys.is_some();
        let red = self.key_should_be_red();
        let controller = &mut self.machine.controller;
        let applied = if dark {
            apply_lighting_dark(lighting, self.model, controller, red)
        } else {
            apply_lighting(lighting, self.model, controller, red)
        };
        self.key_red = applied.is_ok().then_some(red);
        if dark && let Ok(saved) = Brightness::new(lighting.brightness) {
            self.dark_keys = Some(saved);
        }
        // As at the start: the screen needs a new look.
        if !dark && applied.is_ok() {
            self.keys_lit_by_lighting();
        }
        if let Err(error) = applied {
            report(&format!(
                "the lighting after the charger change failed: {error}"
            ));
            self.source_lighting_due = tries.checked_sub(1).filter(|&left| left > 0);
        }
    }

    /// Shows the preset error of a power event before its rate error. A
    /// rate error behind a failed preset is logged. A rate the panel does
    /// not offer is also held, if a resend of the preset is to come, to
    /// show once it works. Others are not: one tried again on the polls is
    /// quiet here and shows itself, no panel shows in the display view, and
    /// a refused put-back on AC is not tried again, as before.
    fn hold_rate_error(
        &mut self,
        preset: Result<(), AppError>,
        rate: Result<(), AppError>,
        context: &str,
    ) -> Result<(), AppError> {
        match (preset, rate) {
            (Err(preset), Err(rate)) => {
                report(&format!("{context}: {rate}"));
                let resend_due = self.preset_unsure && self.resends_left > 0;
                let lasting = matches!(rate, AppError::Display(DisplayError::Unsupported(_)));
                self.held_rate_error = (resend_due && lasting).then_some(rate);
                Err(preset)
            }
            (preset, rate) => preset.and(rate),
        }
    }

    /// Sends the preset again if the power source changes the one that
    /// runs, or the last start or switch failed. The full charger back from
    /// the battery presets waits (see [`follow_charger_back`](Self::follow_charger_back)).
    fn set_on_battery(&mut self, on_battery: bool) -> Result<(), AppError> {
        let before = self.running(self.config).preset;
        self.power = if on_battery {
            Power::Battery
        } else {
            self.power.charger_came()
        };
        self.source_known = true;
        // A full read keeps the power just set; a weak one counts at once.
        self.read_charger_with(|power| power);
        self.send_if_changed(before)
    }

    /// Ends the full charger's wait once it stayed [`CHARGER_STEADY`] from
    /// the first tick that saw it (ticket 263, user pick A): the charger is
    /// read again, and a full read sends the preset that then runs, such as
    /// Performance. A weak or failed read is a weak charge, as on a poll. A
    /// bounce ends the wait through the battery, so the next plug waits
    /// again. Returns whether the wait ended, so the windows show it.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn follow_charger_back(&mut self, now: Instant) -> Result<bool, AppError> {
        let Power::ChargerBack { since } = self.power else {
            return Ok(false);
        };
        let Some(since) = since else {
            self.power = Power::ChargerBack { since: Some(now) };
            return Ok(false);
        };
        if now.saturating_duration_since(since) < CHARGER_STEADY || self.flips_held(now) {
            return Ok(false);
        }
        // Read again: a USB-C monitor plugged in the wait is a weak charge.
        let before = self.running(self.config).preset;
        self.read_charger();
        if self.running(self.config).preset != before {
            self.flips.step_ups.push(now);
            self.send_if_changed(before)?;
        }
        Ok(true)
    }

    /// Whether the full charger's wait goes on because its read kept
    /// flipping: [`FLIP_CAP`] step ups from the battery presets in the last
    /// [`FLIP_WINDOW`], each 8 stored mode writes with the step down
    /// (ticket 294). Logged once each time it begins and ends. Never seen
    /// on the laptop (`NOTES.md`, weak charge).
    fn flips_held(&mut self, now: Instant) -> bool {
        let flips = &mut self.flips;
        flips
            .step_ups
            .retain(|&at| now.saturating_duration_since(at) < FLIP_WINDOW);
        let held = flips.step_ups.len() >= FLIP_CAP;
        if held != flips.logged {
            flips.logged = held;
            report(if held {
                "the full charger came back 3 times in 10 min (a charger read that keeps changing, or replugs), so the battery preset stays until the first of them is 10 min old"
            } else {
                "the full charger's preset may run again"
            });
        }
        held
    }

    /// Starts the full charger's wait again from the next allowed tick, for
    /// a tick that may not end it (a pause, an updater, Razer's software),
    /// so the 10 s count only time the app watched the charger.
    pub fn restart_charger_wait(&mut self) {
        if let Power::ChargerBack { .. } = self.power {
            self.power = Power::ChargerBack { since: None };
        }
    }

    /// Reads the charger again on a poll, and sends the preset if a weak
    /// charge came or went. A weak charge counts at once, the full charger
    /// only on the second read in a row. A failed read keeps the last one
    /// and is kept for the log; a reply not understood is a weak charge. On
    /// battery nothing is read. Returns whether a weak charge came or went,
    /// so the windows show it.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn follow_charger(&mut self) -> Result<bool, AppError> {
        self.follow_charger_with(true)
    }

    /// As [`follow_charger`](Self::follow_charger), but a weak charge
    /// never ends: only the step down goes, as while Razer's software runs
    /// (ticket 258, user pick A), where nothing in the background sends
    /// Performance or Custom.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn follow_charger_down_only(&mut self) -> Result<bool, AppError> {
        self.follow_charger_with(false)
    }

    fn follow_charger_with(&mut self, step_up: bool) -> Result<bool, AppError> {
        if self.on_battery() {
            return Ok(false);
        }
        let read = full_power_charger(&mut self.machine.controller, self.model).map(Some);
        let full = match self.unless_missed("charger", read) {
            None => return Ok(false),
            Some(Some(full)) => full,
            Some(None) => {
                if !self.power.is_weak() {
                    report(NOT_UNDERSTOOD);
                }
                false
            }
        };
        let (was_weak, before) = (self.power.is_weak(), self.running(self.config).preset);
        let after = self.power.after_poll(full);
        if was_weak && !after.is_weak() && !step_up {
            return Ok(false);
        }
        self.power = after;
        if self.running(self.config).preset != before {
            self.send_if_changed(before)?;
        }
        Ok(self.power.is_weak() != was_weak)
    }

    /// Sends the preset that runs again if the last start, preset change,
    /// power switch or resend failed, so the laptop does not keep running
    /// a preset the app does not show (such as Performance on battery).
    /// Called on the polls, [`RESEND_TRIES`](crate::RESEND_TRIES) times at
    /// most after each failed send. Returns whether it sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. The next poll tries again, while tries are left.
    /// A resend that works returns the rate error held behind the failed
    /// send, if any (see `hold_rate_error`), so it shows then.
    pub fn resend_preset(&mut self) -> Result<bool, AppError> {
        if !self.preset_unsure || self.resends_left == 0 {
            return Ok(false);
        }
        self.resends_left -= 1;
        let config = self.running(self.config);
        let applied = self.send_preset(&config);
        // Only the preset run is sent, so `AfterPreset` can't come here.
        match &applied {
            Ok(()) => {
                self.preset_unsure = false;
                self.note_full_speed(&config);
            }
            Err(ApplyError::Device(_)) => self.preset_send_failed(),
            Err(_) => {}
        }
        if self.resends_left == 0 && applied.is_err() {
            // No resend is left to show it.
            self.held_rate_error = None;
        }
        applied?;
        match self.held_rate_error.take() {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    /// Sends the preset that runs now if it is not `before`, or the last
    /// start or switch failed.
    fn send_if_changed(&mut self, before: Preset) -> Result<(), AppError> {
        let config = self.running(self.config);
        if config.preset != before || self.preset_unsure {
            let applied = self.send_preset(&config);
            self.note_preset_send(&config, &applied);
            applied?;
        }
        Ok(())
    }

    /// Puts back the rate the panel had before battery, if the app dropped
    /// it to 60 Hz, so the panel does not stay at 60 Hz once the app is
    /// gone. Nothing else is sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_quit(&mut self) -> Result<(), AppError> {
        Ok(self.battery_rate.put_back(&mut self.machine.displays)?)
    }

    /// Sends Balanced (Silent if Silent runs) as the app quits: no app is
    /// left to watch a manual fan or a charger pulled. Always sent, as what
    /// the controller runs may differ from the config after a failed send.
    /// If a manual fan is saved and Custom runs, the last preset send
    /// failed or the source changed at the quit or sleep (`battery_at_quit`),
    /// or a cut send may have left a manual fan the window does not show
    /// (ticket 250), Custom with the fans on auto goes first (with the lowest
    /// boosts when Custom is not what should run), the known way to end a
    /// manual fan; the preset's own mode write alone left the fans at 1600
    /// RPM (ticket 291), so every send that leaves a manual fan does the
    /// same (ticket 298, `send_preset`). The saved preset is kept, so the next start
    /// sends it again. Any fan targets due are dropped first, so none go
    /// after the quit.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. A failure stops the sends after it, and leaves the
    /// preset's unsure mark as it was (see the shell's `suspend`; ticket
    /// 191).
    pub fn quit_preset(&mut self) -> Result<(), AppError> {
        // No manual fan targets may go after the quit's sends, even when
        // the first one fails (ticket 182).
        self.forget_fan_targets();
        let running = self.running(self.config);
        let manual_saved =
            self.config.preset == Preset::Custom && self.config.custom.fan_rpm.is_some();
        // After a failed send the controller may still run the manual fan,
        // though the battery preset should run, or though the click to
        // Custom was cut (ticket 250).
        if self.manual_fan_unshown()
            || (manual_saved && (running.preset == Preset::Custom || self.preset_unsure))
        {
            let mut auto_fans = self.config;
            auto_fans.preset = Preset::Custom;
            auto_fans.custom.fan_rpm = None;
            if running.preset != Preset::Custom {
                // Custom is refused here (battery or a weak charge): the
                // lowest boosts, for the moment before the preset below.
                auto_fans.custom.cpu_boost = 0;
                auto_fans.custom.gpu_boost = 0;
            }
            // No read-back before the last send: a read that drops the
            // handle would make the quit's retry send this step again.
            let applied = apply_performance(&auto_fans, self.model, &mut self.machine.controller);
            self.note_manual_sent(&auto_fans, &applied);
            match applied {
                // A saved boost out of range: nothing was sent, so the
                // preset below still goes.
                Ok(()) | Err(ApplyError::Custom(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let preset = if running.preset == Preset::Silent {
            Preset::Silent
        } else {
            Preset::Balanced
        };
        let last = Config { preset, ..running };
        Ok(self.send_preset(&last)?)
    }

    /// Follows battery at a quit or a sleep when a read says so, but no
    /// power event was read for it (dropped in a sleep or just before; tickets
    /// 196, 199), so the quit preset ends a manual fan with the lowest boosts.
    /// Nothing is sent. The controller still runs what was sent for the
    /// charger, so a change of the preset that runs marks it unsure. Never
    /// toward the charger: a weak charge would be missed.
    pub fn battery_at_quit(&mut self) {
        // After unknown reads the app runs battery, but the slot may still
        // be the AC one (ticket 247). Noted after the power: the slots
        // follow `on_battery`.
        if self.on_battery() {
            self.note_slot(true);
            return;
        }
        let before = self.running(self.config).preset;
        self.power = Power::Battery;
        self.note_slot(true);
        if self.running(self.config).preset != before {
            report("the charger is gone, so the battery quit preset is sent");
            self.preset_unsure = true;
        }
    }

    /// At the app's start on AC, follows AC, so the rate a crash on
    /// battery left at 60 Hz is put back (see
    /// [`blade_core::BatteryRate::on_event`]). On battery or an unknown
    /// source nothing is sent: the first power event, or a known battery
    /// read after an unknown one ([`Self::rate_on_battery_read`]), takes
    /// the rate then.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn start_rate(&mut self) -> Result<(), AppError> {
        if self.on_battery() || !self.source_known {
            return Ok(());
        }
        let rate = self.follow_power(PowerEvent::OnAc);
        self.quiet_if_tried_again(rate)
    }

    /// A rate error whose set is tried again on the polls is only logged,
    /// as the polls show the last failed try.
    fn quiet_if_tried_again(&self, rate: Result<(), AppError>) -> Result<(), AppError> {
        match rate {
            Err(AppError::Display(error))
                if error.may_work_later() && self.battery_rate.retry_pending() =>
            {
                let rate = if self.battery_rate.retries_put_back() {
                    "the rate put back on AC"
                } else {
                    "the 60 Hz on battery"
                };
                report(&format!("{rate} failed, tried again: {error}"));
                Ok(())
            }
            other => other,
        }
    }

    /// Whether a wake's or a switch's 60 Hz on battery, or the put-back at
    /// the first AC, failed and is to be tried again on this source.
    #[must_use]
    pub fn battery_rate_pending(&self) -> bool {
        self.source_known
            && self.battery_rate.retry_pending()
            && self.on_battery() != self.battery_rate.retries_put_back()
    }

    /// Tries a wake's or a switch's failed 60 Hz again on battery, or the
    /// first AC's failed put-back on AC (one try per poll; see
    /// [`blade_core::BatteryRate::retry`]).
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn retry_battery_rate(&mut self) -> Result<(), AppError> {
        if !self.battery_rate_pending() {
            return Ok(());
        }
        let displays = &mut self.machine.displays;
        Ok(self.battery_rate.retry(&self.config, displays)?)
    }

    /// Takes the rate for a battery read that made the source known, as a
    /// battery event would: an unknown source counts as battery, so such a
    /// read sends no event. A charger pulled in a wake's pause sends no
    /// event again, and the pause's end read the source unknown, so it left
    /// the rate (ticket 281); a start with the source unknown gets its
    /// 60 Hz here too, if no event came first. A rate already saved (the
    /// switch to battery was seen) sends nothing, so a rate set outside the
    /// app while the source was unknown is kept; so is one Windows put
    /// back in a wake whose pause ended with the source unknown (ticket
    /// 286). A refused set is tried again from the next poll on. Nothing
    /// on AC or with the source unknown (a guard).
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn rate_on_battery_read(&mut self) -> Result<(), AppError> {
        if !self.on_battery() || !self.source_known {
            return Ok(());
        }
        // As `on_power`: an error held for the old source is stale.
        self.held_rate_error = None;
        // A panel that is not active (lid shut on a monitor) is left
        // alone, as the wake it stands in for does.
        let rate = match self.follow_power(PowerEvent::OnBattery) {
            Err(AppError::Display(DisplayError::NoInternalPanel)) => Ok(()),
            other => other,
        };
        self.quiet_if_tried_again(rate)
    }

    pub(super) fn follow_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        let displays = &mut self.machine.displays;
        Ok(self.battery_rate.on_event(event, &self.config, displays)?)
    }
}
