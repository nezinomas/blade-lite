//! Drops the panel to 60 Hz on battery, and puts the old rate back on AC.

use platform::{DisplayApi, ModeLife, PowerEvent};

use crate::Config;
use crate::refresh::{DisplayError, RefreshRate, current_rate, set_rate, set_refresh_rate};

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
    /// The 60 Hz is not saved, so a shutdown or a crash on battery does not
    /// leave the panel at 60 Hz after the restart.
    fn on_battery(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        if self.saved.is_some() {
            return Ok(());
        }
        let rate = current_rate(api)?;
        if rate == BATTERY_RATE {
            return Ok(());
        }
        set_rate(api, BATTERY_RATE, ModeLife::UntilRestart)?;
        self.saved = Some(rate);
        Ok(())
    }

    /// Puts back the rate that was active before battery, if there is one:
    /// on AC, and when the app quits. Nothing is sent if the panel no
    /// longer runs at 60 Hz, as the rate was then changed outside the app.
    /// Each saved rate is tried once, so a failed put-back is not retried.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn put_back(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        let Some(rate) = self.saved.take() else {
            return Ok(());
        };
        if current_rate(api)? != BATTERY_RATE {
            return Ok(());
        }
        set_refresh_rate(api, rate)
    }
}
