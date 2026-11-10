//! Drops the panel to 60 Hz on battery, and puts the old rate back on AC.

use platform::{DisplayApi, PowerEvent};

use crate::Config;
use crate::refresh::{DisplayError, RefreshRate, current_rate, set_refresh_rate};

/// The rate the panel runs at on battery.
pub const BATTERY_RATE: RefreshRate = RefreshRate(60);

/// Follows the power source and remembers the rate to put back on AC.
#[derive(Debug, Default)]
pub struct BatteryRate {
    /// The rate before the switch to battery, while one is to be put back.
    saved: Option<RefreshRate>,
}

impl BatteryRate {
    /// Sets the rate for a change of power source. Other events, and every
    /// event while the option is off, change nothing.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn on_event(
        &mut self,
        event: PowerEvent,
        config: &Config,
        api: &mut impl DisplayApi,
    ) -> Result<(), DisplayError> {
        match event {
            PowerEvent::OnBattery if config.sixty_hz_on_battery => self.on_battery(api),
            PowerEvent::OnAc => self.put_back(api),
            _ => Ok(()),
        }
    }

    /// Remembers the rate once, then sets 60 Hz if the panel is not there yet.
    fn on_battery(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        if self.saved.is_some() {
            return Ok(());
        }
        let rate = current_rate(api)?;
        if rate == BATTERY_RATE {
            return Ok(());
        }
        set_refresh_rate(api, BATTERY_RATE)?;
        self.saved = Some(rate);
        Ok(())
    }

    /// Puts back the rate that was active before battery, if there is one:
    /// on AC, and when the app quits. Each saved rate is sent once.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn put_back(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        match self.saved.take() {
            Some(rate) => set_refresh_rate(api, rate),
            None => Ok(()),
        }
    }
}
