//! Drops the panel to 60 Hz on battery, and puts the old rate back on AC.

use platform::{DisplayApi, ModeLife, PowerEvent};

use crate::Config;
use crate::refresh::{DisplayError, RefreshRate, current_rate, saved_rate, set_rate};

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
    /// Whether a known power source has come since the app started.
    source_seen: bool,
}

impl BatteryRate {
    /// Sets the rate for a change of power source. A wake goes to
    /// [`Self::on_resume`], so `Resumed` changes nothing here, nor does any
    /// event while the option is off (but AC still puts a rate back). The
    /// first source since the start that is AC goes to
    /// [`Self::first_on_ac`].
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
            PowerEvent::OnBattery => {
                self.source_seen = true;
                if config.sixty_hz_on_battery {
                    self.on_battery(api)
                } else {
                    Ok(())
                }
            }
            PowerEvent::OnAc => {
                self.tries = 0;
                let first = !std::mem::replace(&mut self.source_seen, true);
                if first && self.saved.is_none() {
                    return self.first_on_ac(config, api);
                }
                self.put_back(api)
            }
            PowerEvent::Resumed => Ok(()),
        }
    }

    /// Follows the power source read at a wake: on AC as an AC event, on
    /// battery it sets 60 Hz again (see `after_resume`). A refused set on
    /// battery is not an error yet: it starts the tries ([`Self::retry`]),
    /// which AC stops.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn on_resume(
        &mut self,
        on_battery: bool,
        config: &Config,
        api: &mut impl DisplayApi,
    ) -> Result<(), DisplayError> {
        if !on_battery {
            return self.on_event(PowerEvent::OnAc, config, api);
        }
        self.source_seen = true;
        if config.sixty_hz_on_battery {
            let refused = self.after_resume(api).is_err();
            self.tries = if refused { RESUME_RATE_TRIES } else { 0 };
        }
        Ok(())
    }

    /// At the first known source since the start, when it is AC, puts
    /// back the rate Windows has saved if the panel runs 60 Hz: a crash on
    /// battery left it there, and the charger went in before the app
    /// started again (or before its first power event), so no rate is held
    /// to put back. Only with the switch on, and only a saved rate the
    /// panel offers that is not 60 Hz. It is not saved, as at a put-back on
    /// AC. That can't tell such a 60 Hz from one another program set
    /// without saving it; that one is put back too, as at a start on
    /// battery. A panel that is not active is left alone. Tried once.
    fn first_on_ac(
        &mut self,
        config: &Config,
        api: &mut impl DisplayApi,
    ) -> Result<(), DisplayError> {
        if !config.sixty_hz_on_battery {
            return Ok(());
        }
        match current_rate(api) {
            Ok(BATTERY_RATE) => {}
            Ok(_) | Err(DisplayError::NoInternalPanel) => return Ok(()),
            Err(error) => return Err(error),
        }
        match saved_rate(api) {
            Some(rate) if rate != BATTERY_RATE => set_rate(api, rate, ModeLife::UntilRestart),
            _ => Ok(()),
        }
    }

    /// Remembers the rate once, then sets 60 Hz if the panel is not there yet.
    /// The 60 Hz is not saved, so a shutdown or a crash on battery does not
    /// leave the panel at 60 Hz after the restart. A panel already at 60 Hz
    /// (left by a crash on battery) takes the rate Windows has saved to put
    /// back, unless that is 60 Hz too. That can't tell such a 60 Hz from
    /// one another program set without saving it; that one is put back too.
    /// The rate buttons are locked on battery, so the user can't pick 60 Hz
    /// by hand there.
    fn on_battery(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        if self.saved.is_some() {
            return Ok(());
        }
        let rate = current_rate(api)?;
        if rate == BATTERY_RATE {
            // A crash on battery leaves 60 Hz, not saved: the rate Windows
            // has saved is the one to put back.
            self.saved = saved_rate(api).filter(|&saved| saved != BATTERY_RATE);
            return Ok(());
        }
        set_rate(api, BATTERY_RATE, ModeLife::UntilRestart)?;
        self.saved = Some(rate);
        Ok(())
    }

    /// Sets 60 Hz again if the panel is back at the rate saved before
    /// battery: Windows puts its saved rate back in a wake (seen after a lid
    /// sleep, 2026-11-10). Any other rate was set outside the app, and is
    /// kept. With no rate saved (the charger was pulled in the sleep, the
    /// app started on battery, or the switch's 60 Hz failed) it is a switch
    /// to battery. A panel that is not active (lid shut, a monitor only) is
    /// left alone.
    fn after_resume(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        let Some(saved) = self.saved else {
            return match self.on_battery(api) {
                Err(DisplayError::NoInternalPanel) => Ok(()),
                other => other,
            };
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
    /// Each saved rate is set once, so a failed set is not retried; a failed
    /// read of the rate keeps it for the next AC event or the quit.
    /// The rate may be the one Windows has saved, taken at a start on
    /// battery at 60 Hz. It is not saved, so the rate Windows has saved
    /// stays the user's, even when the one put back was set by another
    /// program; unless Windows has saved 60 Hz (such as from a scale change
    /// in Settings on battery), so a restart does not start at 60 Hz.
    ///
    /// # Errors
    ///
    /// See [`DisplayError`].
    pub fn put_back(&mut self, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
        let Some(rate) = self.saved else {
            return Ok(());
        };
        let current = current_rate(api)?;
        self.saved = None;
        if current != BATTERY_RATE {
            return Ok(());
        }
        let life = if saved_rate(api) == Some(BATTERY_RATE) {
            ModeLife::Saved
        } else {
            ModeLife::UntilRestart
        };
        set_rate(api, rate, life)
    }
}
