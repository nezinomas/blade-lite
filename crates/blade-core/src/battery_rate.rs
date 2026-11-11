//! Drops the panel to 60 Hz on battery, and puts the old rate back on AC.

use platform::{DisplayApi, ModeLife, PowerEvent};

use crate::Config;
use crate::refresh::{DisplayError, RefreshRate, current_rate, saved_rate, set_rate};

/// The rate the panel runs at on battery.
pub const BATTERY_RATE: RefreshRate = RefreshRate(60);

/// How many polls try 60 Hz again after Windows refused it in a wake (it
/// did 2026-11-10; a set seconds later worked) or, not seen on the laptop,
/// at a switch to battery; or try the rate put back at the first AC again.
pub const RESUME_RATE_TRIES: u8 = 10;

/// Follows the power source and remembers the rate to put back on AC.
#[derive(Debug, Default)]
pub struct BatteryRate {
    /// The rate before the switch to battery, while one is to be put back.
    saved: Option<RefreshRate>,
    /// The tries left after a wake or a switch whose 60 Hz failed, or a
    /// first AC whose put-back failed.
    tries: u8,
    /// Whether the tries put the rate back at the first AC, not 60 Hz.
    put_back_tries: bool,
    /// Whether the next poll is skipped: a switch's tries start then, as
    /// that poll may come at once. A wake's tries wait for the pause.
    wait_a_poll: bool,
    /// Whether a known power source has come since the app started.
    source_seen: bool,
}

impl BatteryRate {
    /// Sets the rate for a change of power source. A wake goes to
    /// [`Self::on_resume`], so `Resumed` changes nothing here, nor does any
    /// event while the option is off (but AC still puts a rate back). The
    /// first source since the start that is AC goes to
    /// [`Self::first_on_ac`]. A failed 60 Hz on battery, or a failed
    /// put-back at that first AC, is an error; one that may work later (a
    /// refused set or a failed read) also starts the tries
    /// ([`Self::retry`]).
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
                if self.put_back_tries {
                    self.stop_tries();
                }
                if !config.sixty_hz_on_battery {
                    return Ok(());
                }
                // A refused 60 Hz is tried again on the polls, as after a
                // wake. A rate the panel does not offer, or no panel, is
                // not, and ends a wake's tries too.
                let set = self.on_battery(api);
                match &set {
                    Err(error) if error.may_work_later() => self.start_tries(false),
                    Err(_) => self.stop_tries(),
                    Ok(()) => {}
                }
                set
            }
            PowerEvent::OnAc => {
                // The put-back tries are AC's own: they go on (Windows
                // sends AC once more when the app listens, after the start).
                if self.put_back_tries {
                    return Ok(());
                }
                self.stop_tries();
                let first = !std::mem::replace(&mut self.source_seen, true);
                // A held rate is always put back first (none is held now,
                // as each path that holds one marks the source seen).
                if first && self.saved.is_none() {
                    // Tried again on the polls, as a switch's 60 Hz is.
                    let put = Self::first_on_ac(config, api);
                    if matches!(&put, Err(error) if error.may_work_later()) {
                        self.start_tries(true);
                    }
                    return put;
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
        if self.put_back_tries {
            self.stop_tries();
        }
        if config.sixty_hz_on_battery {
            let refused = self.after_resume(api).is_err();
            self.stop_tries();
            if refused {
                self.tries = RESUME_RATE_TRIES;
            }
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
    /// battery. A panel that is not active is left alone. A refused set or
    /// a failed read is tried again on the polls ([`Self::retry`]).
    fn first_on_ac(config: &Config, api: &mut impl DisplayApi) -> Result<(), DisplayError> {
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

    /// Starts the tries of a switch's 60 Hz, or of the first AC's put-back
    /// (`put_back`). The first poll is skipped, as it may come at once.
    fn start_tries(&mut self, put_back: bool) {
        self.tries = RESUME_RATE_TRIES;
        self.wait_a_poll = true;
        self.put_back_tries = put_back;
    }

    /// Ends the tries, with the poll a switch's tries skip.
    fn stop_tries(&mut self) {
        self.tries = 0;
        self.wait_a_poll = false;
        self.put_back_tries = false;
    }

    /// Whether a wake's or a switch's 60 Hz, or the first AC's put-back,
    /// failed and is to be tried again.
    #[must_use]
    pub fn retry_pending(&self) -> bool {
        self.tries > 0
    }

    /// Whether the tries put the rate back at the first AC (to try on AC),
    /// not 60 Hz (to try on battery).
    #[must_use]
    pub fn retries_put_back(&self) -> bool {
        self.put_back_tries
    }

    /// Tries the wake's or switch's 60 Hz, or the first AC's put-back,
    /// again, once per poll. Only the last failed try is an error, so a set
    /// refused while the panel wakes is not shown each time. A try that
    /// fails for good (such as a rate the panel no longer offers) ends the
    /// tries at once with its error; a panel gone inactive ends them with
    /// none, as it is left alone. A power event that is not the tries'
    /// source, or the option turned off, stops them.
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
            self.stop_tries();
            return Ok(());
        }
        if std::mem::take(&mut self.wait_a_poll) {
            return Ok(());
        }
        self.tries -= 1;
        let tried = if self.put_back_tries {
            Self::first_on_ac(config, api)
        } else {
            self.after_resume(api)
        };
        match tried {
            // The panel went inactive between two reads.
            Ok(()) | Err(DisplayError::NoInternalPanel) => {
                self.stop_tries();
                Ok(())
            }
            Err(error) if self.tries == 0 || !error.may_work_later() => {
                self.stop_tries();
                Err(error)
            }
            Err(_) => Ok(()),
        }
    }

    /// Puts back the rate that was active before battery, if there is one:
    /// on AC, and when the app quits. Nothing is sent if the panel no
    /// longer runs at 60 Hz, as the rate was then changed outside the app.
    /// Each saved rate is set once, so a failed set is not retried (unlike
    /// the first AC's put-back, [`Self::first_on_ac`]); a failed
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
