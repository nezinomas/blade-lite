//! Drops the panel to 60 Hz on battery, and puts the old rate back on AC.

use platform::{DisplayApi, ModeLife, PowerEvent};

use crate::Config;
use crate::refresh::{DisplayError, RefreshRate, current_rate, set_rate, set_refresh_rate};

/// The rate the panel runs at on battery.
pub const BATTERY_RATE: RefreshRate = RefreshRate(60);

/// How many polls try 60 Hz again after Windows refused it in a wake (it
/// did 2026-11-10; a set seconds later worked).
pub const RESUME_RATE_TRIES: u8 = 10;

/// Follows the power source and remembers the rate to put back on AC.
#[derive(Debug, Default)]
pub struct BatteryRate {
    /// The rate before the switch to battery, while one is to be put back.
    saved: Option<RefreshRate>,
    /// The tries left after a wake whose 60 Hz failed.
    tries: u8,
}

impl BatteryRate {
    /// Sets the rate for a change of power source, and sets 60 Hz again
    /// after a wake on battery. A refused set after a wake is not an error
    /// yet: it starts the tries ([`Self::retry`]), which AC stops. Other
    /// events, and every event while the option is off, change nothing.
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
            PowerEvent::OnAc => {
                self.tries = 0;
                self.put_back(api)
            }
            PowerEvent::Resumed if config.sixty_hz_on_battery => {
                let refused = self.after_resume(api).is_err();
                self.tries = if refused { RESUME_RATE_TRIES } else { 0 };
                Ok(())
            }
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

    /// Sets 60 Hz again if the panel is back at the rate saved before
    /// battery: Windows puts its saved rate back in a wake (seen after a lid
    /// sleep, 2026-11-10). Any other rate was set outside the app, and is
    /// kept. A panel that is not active (lid shut, a monitor only) is left
    /// alone.
    fn after_resume(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        let Some(saved) = self.saved else {
            return Ok(());
        };
        match current_rate(api) {
            Ok(rate) if rate == saved => {}
            Ok(_) | Err(DisplayError::NoInternalPanel) => return Ok(()),
            Err(error) => return Err(error),
        }
        set_rate(api, BATTERY_RATE, ModeLife::UntilRestart)
    }

    /// Whether a wake's 60 Hz failed and is to be tried again.
    #[must_use]
    pub fn retry_pending(&self) -> bool {
        self.tries > 0
    }

    /// Tries the wake's 60 Hz again, once per poll. Only the last failed
    /// try is an error, so a set refused while the panel wakes is not shown
    /// each time. AC or the option turned off stops the tries.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn retry(
        &mut self,
        config: &Config,
        api: &mut impl DisplayApi,
    ) -> Result<(), DisplayError> {
        if self.tries == 0 || !config.sixty_hz_on_battery {
            self.tries = 0;
            return Ok(());
        }
        self.tries -= 1;
        match self.after_resume(api) {
            Ok(()) => {
                self.tries = 0;
                Ok(())
            }
            Err(error) if self.tries == 0 => Err(error),
            Err(_) => Ok(()),
        }
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
