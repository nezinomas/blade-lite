//! What each click does.

use blade_core::{
    ApplyError, Config, Controller, Lighting, Preset, Query, apply_lighting, apply_performance,
    keyboard_brightness, next_preset, set_charge_limit, set_logo, set_native_mode,
    set_refresh_rate, set_touchpad, set_vapor_light,
};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};
use ui::{DisplayCommand, KeyboardCommand, PerformanceCommand};

use super::{App, AppError};
use crate::report;

impl<C, D, R, K> App<C, D, R, K>
where
    C: Controller + Query,
    D: DisplayApi,
    R: Registry,
    K: KeyInjector,
{
    /// # Errors
    ///
    /// See [`AppError`]. The config is left as it was, except that a
    /// change that was sent but not saved is kept in memory. A click on a
    /// preset not offered on battery is refused on a weak charge; if the
    /// charger read for it found the weak charge, the battery preset that
    /// now runs is sent first.
    pub fn on_performance(&mut self, command: PerformanceCommand) -> Result<(), AppError> {
        let mut config = self.config;
        // What ran before a charger read, if the click made one.
        let mut read_from = None;
        match command {
            PerformanceCommand::SetPreset(preset) => {
                if !preset.offered_on_battery(self.model) {
                    // A charger swap gives no power event, and the last
                    // poll may be 10 s old.
                    let before = self.running_preset();
                    read_from = Some(before);
                    self.read_charger();
                    if self.battery_presets() {
                        self.send_if_read_changed(before);
                        return Err(AppError::NotOnBattery(preset));
                    }
                }
                config.preset = preset;
            }
            PerformanceCommand::SetCpuBoost(level) => config.custom.cpu_boost = level,
            PerformanceCommand::SetGpuBoost(level) => config.custom.gpu_boost = level,
            PerformanceCommand::SetFanRpm(rpm) => config.custom.fan_rpm = rpm,
            // The controller keeps the limit, so nothing is saved.
            PerformanceCommand::SetChargeLimit(on) => return self.set_charge_limit(on),
        }
        let running = self.running(config);
        let applied = apply_performance(&running, self.model, &mut self.machine.controller);
        // A failed send may have changed part of it.
        self.note_preset_send(&applied);
        // Refused before any send (a saved value out of range): the read
        // may still have changed what runs, such as a full charger found.
        let unsent = matches!(applied, Err(ApplyError::Preset(_) | ApplyError::Custom(_)));
        if let (true, Some(before)) = (unsent, read_from) {
            self.send_if_read_changed(before);
        }
        applied?;
        self.keep_sent(config)
    }

    /// Sends the preset that runs if a charger read changed it from
    /// `before`, as a power event does: no later poll sees that change, so
    /// Performance or Custom would keep running on a weak charge (or the
    /// app show Performance while Balanced runs). A failed
    /// send is logged, and marks the preset unsure, so the polls send it
    /// again; the caller's own error is the one shown.
    fn send_if_read_changed(&mut self, before: Preset) {
        if self.running_preset() == before {
            return;
        }
        let running = self.running(self.config);
        let applied = apply_performance(&running, self.model, &mut self.machine.controller);
        self.note_preset_send(&applied);
        if let Err(error) = applied {
            report(&format!("the preset after a charger read failed: {error}"));
        }
    }

    /// Steps to the next preset, as the power-mode key asks.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. Nothing is sent if the model has no key, or no
    /// other preset to step to. The config is left as it was, except that
    /// a preset that was sent but not saved is kept in memory.
    pub fn on_power_key(&mut self) -> Result<(), AppError> {
        if self.model.power_key.is_none() {
            return Ok(());
        }
        // The step depends on the charger, which the last poll may have
        // read 10 s ago.
        self.read_charger();
        let running = self.running_preset();
        match next_preset(running, self.model, self.battery_presets()) {
            Some(next) if next != running => {
                self.on_performance(PerformanceCommand::SetPreset(next))
            }
            _ => Ok(()),
        }
    }

    /// Takes the keyboard brightness the laptop runs now, while Fn is
    /// held or as it comes up: Fn + F10 or F11 set it in the laptop itself. A new level is
    /// shown and saved, and the vapor light follows it as it follows the
    /// slider. Nothing is taken while no lighting is saved: the keyboard
    /// colour is not known then, and a saved lighting would send white at
    /// the next start.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. A failed read changes nothing. A new level is
    /// kept and saved even if the vapor light send fails, since the
    /// keyboard already runs it. After a failed start the keyboard may
    /// run another level than the saved one; the next Fn press takes it.
    ///
    /// Returns whether a new level was taken, so the window is filled
    /// again only then.
    pub fn on_brightness_keys(&mut self) -> Result<bool, AppError> {
        let Some(saved) = self.config.lighting else {
            return Ok(false);
        };
        let Some(read) = keyboard_brightness(&mut self.machine.controller, self.model)? else {
            return Ok(false);
        };
        if read.percent() == saved.brightness {
            return Ok(false);
        }
        let lighting = Lighting {
            brightness: read.percent(),
            ..saved
        };
        let sent = set_vapor_light(lighting, self.model, &mut self.machine.controller);
        let kept = self.keep_sent(Config {
            lighting: Some(lighting),
            ..self.config
        });
        if let (Err(_), Err(error)) = (&sent, &kept) {
            // Only the send error is returned: log this one.
            report(&format!(
                "the new keyboard brightness was not saved: {error}"
            ));
        }
        sent?;
        kept.map(|()| true)
    }

    /// # Errors
    ///
    /// See [`AppError`]. The config is left as it was, except that a
    /// change that was sent but not saved is kept in memory.
    pub fn on_keyboard(&mut self, command: KeyboardCommand) -> Result<(), AppError> {
        let mut lighting = self.lighting();
        match command {
            KeyboardCommand::SetBrightness(brightness) => {
                lighting.brightness = brightness.percent();
            }
            KeyboardCommand::SetColour(colour) => lighting.colour = colour,
            // A model without the light is left alone.
            KeyboardCommand::SetVaporLight(_) if self.model.vapor_light.is_none() => {
                return Ok(());
            }
            KeyboardCommand::SetVaporLight(on) => lighting.vapor_light = on,
            KeyboardCommand::SetTouchpad(on) => {
                let machine = &mut self.machine;
                return Ok(set_touchpad(on, &machine.registry, &mut machine.keys)?);
            }
            // Saved, as the laptop turns the logo on again after a wake.
            KeyboardCommand::SetLogo(on) => {
                if let Err(error) = set_logo(on, &mut self.machine.controller, self.model) {
                    // A send that fails part way may still switch it. If
                    // it did, keep it, so a resume does not undo it.
                    self.read_logo();
                    if self.logo_on == Some(on)
                        && let Err(kept) = self.keep_sent(Config {
                            lid_logo: Some(on),
                            ..self.config
                        })
                    {
                        report(&format!("the lid logo was not saved: {kept}"));
                    }
                    return Err(error.into());
                }
                self.logo_on = Some(on);
                return self.keep_sent(Config {
                    lid_logo: Some(on),
                    ..self.config
                });
            }
        }
        let red = self.key_should_be_red();
        let applied = apply_lighting(lighting, self.model, &mut self.machine.controller, red);
        self.key_red = applied.is_ok().then_some(red);
        applied?;
        self.keep_sent(Config {
            lighting: Some(lighting),
            ..self.config
        })
    }

    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_display(&mut self, command: DisplayCommand) -> Result<(), AppError> {
        // A rate error held for a resend may no longer hold.
        self.held_rate_error = None;
        match command {
            DisplayCommand::SetRefreshRate(rate) => {
                set_refresh_rate(&mut self.machine.displays, rate)?;
            }
            DisplayCommand::SetNativeMode(mode) => {
                // A failed send may still have changed it: unknown until a
                // send works or a read. A refused one (nothing sent) too.
                self.native_mode = None;
                set_native_mode(mode, &mut self.machine.controller, self.model)?;
                self.native_mode = Some(mode);
            }
            DisplayCommand::SetSixtyHzOnBattery(on) => self.set_sixty_hz_on_battery(on)?,
        }
        Ok(())
    }

    /// Flips the 60 Hz on battery switch. It reads the saved value, so it
    /// works while the controller or the panel can't be read.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn toggle_sixty_hz_on_battery(&mut self) -> Result<(), AppError> {
        self.set_sixty_hz_on_battery(!self.config.sixty_hz_on_battery)
    }

    /// Saves the switch and acts on it at once. If 60 Hz can't be set on
    /// battery, the old value is saved again, so the rates don't lock while
    /// the panel runs another rate.
    pub(super) fn set_sixty_hz_on_battery(&mut self, on: bool) -> Result<(), AppError> {
        let old = self.config;
        if on == old.sixty_hz_on_battery {
            return Ok(());
        }
        self.commit(Config {
            sixty_hz_on_battery: on,
            ..old
        })?;
        if !self.on_battery() {
            return Ok(());
        }
        // As if the laptop had just gone on battery, or back on AC.
        if !on {
            return self.follow_power(PowerEvent::OnAc);
        }
        let dropped = self.follow_power(PowerEvent::OnBattery);
        if dropped.is_err() {
            // The rate error is the one to show, even if this save fails too.
            let _ = self.commit(old);
        }
        dropped
    }

    /// Sets the charge limit. If that fails, the switch shows what the
    /// controller reads back, or hides if that read fails too.
    fn set_charge_limit(&mut self, on: bool) -> Result<(), AppError> {
        let result = set_charge_limit(on, &mut self.machine.controller, self.model);
        match result {
            Ok(()) => self.charge_limit_on = Some(on),
            Err(_) => self.read_charge_limit(),
        }
        Ok(result?)
    }
}
