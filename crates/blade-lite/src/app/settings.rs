//! Export, import and reset of the settings.

use std::path::Path;

use blade_core::{Config, Controller, Query, apply, check};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};

use super::{App, AppError, DEFAULT_LIGHTING};
use crate::report;

impl<C, D, R, K> App<C, D, R, K>
where
    C: Controller + Query,
    D: DisplayApi,
    R: Registry,
    K: KeyInjector,
{
    /// Writes the settings to a file the user picked. Nothing is sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn export_settings(&self, path: &Path) -> Result<(), AppError> {
        Ok(self.config.save(path)?)
    }

    /// Reads a file the user picked, with the same checks as the config
    /// at start, then sends it as a start does and saves it. A file that
    /// can't be read, or has a value the model does not take, changes
    /// nothing. A file without lighting or lid logo keeps the current ones,
    /// as the app does not know what the keyboard or logo would show then.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn import_settings(&mut self, path: &Path) -> Result<(), AppError> {
        let file = Config::import(path)?;
        self.replace_config(Config {
            lighting: file.lighting.or(self.config.lighting),
            lid_logo: file.lid_logo.or(self.config.lid_logo),
            ..file
        })
    }

    /// Sends and saves the defaults: Balanced, the Custom defaults, the
    /// 60 Hz switch off, white keys at full brightness with the vapor light
    /// on, and the lid logo on. What the laptop or Windows keep (the charge
    /// limit, the native mode, the rate, the touchpad) is left alone.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn reset_settings(&mut self) -> Result<(), AppError> {
        self.replace_config(Config {
            lighting: Some(DEFAULT_LIGHTING),
            lid_logo: self.model.logo.map(|_| true),
            ..Config::default()
        })
    }

    /// Checks `config` and the one that runs in its place, sends it all as
    /// a start does, then keeps and saves it. Nothing is sent if the model
    /// does not take a value. A failed send keeps the old config, and the
    /// next power event sends the preset again. The 60 Hz switch acts at
    /// once, as a click on it does.
    fn replace_config(&mut self, config: Config) -> Result<(), AppError> {
        let running = self.running(config);
        check(&config, self.model)?;
        check(&running, self.model)?;
        let red = self.key_should_be_red();
        let applied = apply(&running, self.model, &mut self.machine.controller, red);
        self.key_red = applied.is_ok().then_some(red);
        self.preset_unsure = applied.is_err();
        self.read_logo();
        applied?;
        let switched = self.config.sixty_hz_on_battery != config.sixty_hz_on_battery;
        let kept = self.keep_sent(config);
        let rate = if switched && self.on_battery {
            self.follow_power(if config.sixty_hz_on_battery {
                PowerEvent::OnBattery
            } else {
                PowerEvent::OnAc
            })
        } else {
            Ok(())
        };
        if let (Err(_), Err(error)) = (&kept, &rate) {
            // Only the save error is shown, so this one is logged.
            report(&format!("the 60 Hz on battery failed too: {error}"));
        }
        kept.and(rate)
    }
}
