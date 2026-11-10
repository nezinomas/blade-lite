//! What the app does at start, on a power change, on a touchpad switch, on
//! a Num Lock change and on quit.

use blade_core::{Controller, Query, apply, apply_performance, paint_touchpad_key, touchpad_on};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};

use super::{App, AppError};

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
    /// if the touchpad is off and Num Lock is off.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn start(&mut self) -> Result<(), AppError> {
        self.read_charge_limit();
        let config = self.running(self.config);
        let red = self.key_should_be_red();
        let applied = apply(&config, self.model, &mut self.machine.controller, red);
        self.key_red = applied.is_ok().then_some(red);
        self.preset_unsure = applied.is_err();
        self.read_logo();
        Ok(applied?)
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

    /// Paints the touchpad key again when Num Lock changes: a key that is
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

    /// Paints the touchpad key red or plain in the saved lighting, unless
    /// it shows that already or no lighting is saved. Returns whether a
    /// frame was sent.
    fn paint_key(&mut self, red: bool) -> Result<bool, AppError> {
        let Some(lighting) = self.config.lighting else {
            return Ok(false);
        };
        if self.key_red == Some(red) {
            return Ok(false);
        }
        let painted = paint_touchpad_key(red, lighting, self.model, &mut self.machine.controller);
        self.key_red = painted.is_ok().then_some(red);
        painted?;
        Ok(true)
    }

    /// Follows the power source, and applies everything again after a
    /// resume, with 60 Hz again on battery if Windows put the rate back.
    /// On battery only Silent or Balanced runs; the saved preset comes
    /// back on AC. The caller waits [`blade_core::RESUME_PAUSE`]
    /// before passing on a resume.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        match event {
            PowerEvent::Resumed => {
                // The rate follows even if the start fails.
                let started = self.start();
                // Only on battery: the charger may have gone in during the
                // sleep, before its power event comes.
                let rate = if self.on_battery {
                    self.follow_power(event)
                } else {
                    Ok(())
                };
                if let (Err(_), Err(error)) = (&started, &rate) {
                    // Only the start error is shown, so this one is logged.
                    crate::report(&format!("the 60 Hz after the wake failed too: {error}"));
                }
                started.and(rate)
            }
            PowerEvent::OnBattery | PowerEvent::OnAc => {
                // The rate follows even if the preset send fails.
                let preset = self.set_on_battery(event == PowerEvent::OnBattery);
                let rate = self.follow_power(event);
                if let (Err(_), Err(error)) = (&preset, &rate) {
                    // Only the preset error is shown, so this one is logged.
                    crate::report(&format!("the 60 Hz on battery failed too: {error}"));
                }
                preset.and(rate)
            }
        }
    }

    /// Sends the preset again if the power source changes the one that
    /// runs, or the last start or switch failed.
    fn set_on_battery(&mut self, on_battery: bool) -> Result<(), AppError> {
        let before = self.running(self.config).preset;
        self.on_battery = on_battery;
        let config = self.running(self.config);
        if config.preset != before || self.preset_unsure {
            let applied = apply_performance(&config, self.model, &mut self.machine.controller);
            self.preset_unsure = applied.is_err();
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

    /// Whether a wake's 60 Hz on battery failed and is to be tried again.
    #[must_use]
    pub fn battery_rate_pending(&self) -> bool {
        self.on_battery && self.battery_rate.retry_pending()
    }

    /// Tries a wake's failed 60 Hz again, on battery only (one try per
    /// poll; see [`blade_core::BatteryRate::retry`]).
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn retry_battery_rate(&mut self) -> Result<(), AppError> {
        if !self.on_battery {
            return Ok(());
        }
        let displays = &mut self.machine.displays;
        Ok(self.battery_rate.retry(&self.config, displays)?)
    }

    pub(super) fn follow_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        let displays = &mut self.machine.displays;
        Ok(self.battery_rate.on_event(event, &self.config, displays)?)
    }
}
