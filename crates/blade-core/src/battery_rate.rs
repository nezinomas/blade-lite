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
    /// What is known of the power source.
    source: SourceFollow,
    /// The display list followed on battery (ticket 300).
    display: DisplayFollow,
}

/// What [`BatteryRate`] knows of the power source.
#[derive(Debug, Default)]
struct SourceFollow {
    /// Whether a known power source has come since the app started.
    seen: bool,
    /// Whether the last wake's pause ended with the source unknown, so the
    /// next battery read sets 60 Hz again as that wake would have
    /// ([`BatteryRate::on_resume_unknown`]; ticket 286).
    wake_left: bool,
}

/// What [`BatteryRate::on_displays`] keeps between polls.
#[derive(Debug, Default)]
struct DisplayFollow {
    /// The list last read by [`BatteryRate::on_displays`] on battery: each
    /// active display's name and connection, sorted (Win + P Duplicate may
    /// give two with one name). `None` off a known battery or with the
    /// switch off.
    layout: Option<Vec<(String, i32)>>,
    /// The polls left after a display list change in which a panel back at
    /// the rate saved before battery still gets 60 Hz: Windows may put its
    /// rate back after the list shows the change (not known).
    settle: u8,
    /// The settle rounds begun since the list was last quiet for
    /// [`DISPLAY_QUIET_POLLS`] polls.
    rounds: u8,
    /// The reads since the list last changed (a failed read counts none).
    quiet: u16,
    /// Whether a change the cap refused was told in this burst.
    cap_told: bool,
}

/// What a display list change did ([`BatteryRate::on_displays`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayChange {
    /// No change, a change within a round, or one already told.
    None,
    /// A change began a round.
    Began,
    /// The cap refused a change (told once a burst).
    Capped,
}

/// How many polls after a display list change still check the rate.
pub const DISPLAY_SETTLE_POLLS: u8 = 5;

/// How many settle rounds begin at most until the list is quiet for
/// [`DISPLAY_QUIET_POLLS`], so a display that keeps coming and going (a
/// flaky cable) can't set the rate and log each second for ever. A cap of
/// 5 a battery spell was too low: check 86's five GPU and Win + P switches
/// used it up, and the sixth got no 60 Hz (2026-11-19).
pub const DISPLAY_ROUNDS: u8 = 10;

/// How many polls (about a minute) the list must stay as it is before
/// rounds may begin again: a choice, not a measurement. A list that
/// changes at most once a minute (a dock that sleeps and wakes) is so
/// followed for as long as it lasts, one round and one log line a change.
pub const DISPLAY_QUIET_POLLS: u16 = 60;

impl BatteryRate {
    /// Sets the rate for a change of power source. A wake goes to
    /// [`Self::on_resume`], so `Resumed` changes nothing here, nor does a
    /// sleep (`Suspending`, the rate stays as it is), nor does any
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
                let wake_left = std::mem::take(&mut self.source.wake_left);
                self.source.seen = true;
                if self.put_back_tries {
                    self.stop_tries();
                }
                if !config.sixty_hz_on_battery {
                    return Ok(());
                }
                // A refused 60 Hz is tried again on the polls, as after a
                // wake. A rate the panel does not offer, or no panel, is
                // not, and ends a wake's tries too. After a wake with the
                // source unknown, an inactive panel is left alone with no
                // error, as at the wake.
                let set = if wake_left {
                    self.after_resume(api)
                } else {
                    self.on_battery(api)
                };
                match &set {
                    Err(error) if error.may_work_later() => self.start_tries(false),
                    Err(_) => self.stop_tries(),
                    Ok(()) => {}
                }
                set
            }
            PowerEvent::OnAc => {
                self.source.wake_left = false;
                // The put-back tries are AC's own: they go on (Windows
                // sends AC once more when the app listens, after the start).
                if self.put_back_tries {
                    return Ok(());
                }
                self.stop_tries();
                let first = !std::mem::replace(&mut self.source.seen, true);
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
            PowerEvent::Resumed | PowerEvent::Suspending | PowerEvent::ScreenChanged => Ok(()),
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
        self.source.wake_left = false;
        if !on_battery {
            return self.on_event(PowerEvent::OnAc, config, api);
        }
        self.sixty_again(config, api);
        Ok(())
    }

    /// Marks a wake whose pause ended with the source unknown: the rate was
    /// left, though Windows may have put its saved rate back (it does,
    /// 2026-11-10). The next battery read then sets 60 Hz again as the wake
    /// would have (see `after_resume`), even with a rate saved, while other
    /// battery reads keep a panel at the saved rate as the user's (ticket
    /// 286). Only the first read after it counts, and a refused set is an
    /// error with its tries, as at a switch; an AC read or a known wake
    /// ends it.
    pub fn on_resume_unknown(&mut self) {
        self.source.wake_left = true;
    }

    /// On battery, sets 60 Hz again as after a wake (see `after_resume`); a
    /// refused set starts the tries ([`Self::retry`]), which AC stops.
    fn sixty_again(&mut self, config: &Config, api: &mut impl DisplayApi) {
        self.source.seen = true;
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
    }

    /// Follows the display list on each poll (ticket 300, user pick A): when
    /// a display came or went, or one's connection changed (a GPU mode
    /// switch, ticket 282), Windows may have put its saved rate back (Win + P
    /// and a GPU mode switch did, check 79), so on a known battery with the
    /// switch on the 60 Hz goes again as after a wake ([`Self::sixty_again`]:
    /// only if the panel runs the rate saved before battery; a refused set
    /// is tried again on the polls), and so for [`DISPLAY_SETTLE_POLLS`]
    /// polls in all (a round), as Windows may put its rate back a little
    /// later. A change within a round begins none, and tries already
    /// running finish first; at most [`DISPLAY_ROUNDS`] rounds until the
    /// list stays as it is for [`DISPLAY_QUIET_POLLS`] (a cap per burst of
    /// changes, not per battery spell). A rate change alone is the user's
    /// (such as one picked in Windows Settings) and is kept. The list is
    /// read only on a known battery with the switch on (a light read, no
    /// modes); its first read there begins a round too (not counted, not a
    /// change), as a monitor that also charged may go with the switch to
    /// battery. A failed read keeps the last list. Returns what a change
    /// did, for the log: a change the cap refused is told once a burst.
    pub fn on_displays(
        &mut self,
        on_battery: bool,
        config: &Config,
        api: &mut impl DisplayApi,
    ) -> DisplayChange {
        if !on_battery || !config.sixty_hz_on_battery {
            self.display = DisplayFollow::default();
            return DisplayChange::None;
        }
        let Ok(mut layout) = api.layout() else {
            self.settle_poll(config, api);
            return DisplayChange::None;
        };
        layout.sort();
        let seen = self.display.layout.replace(layout.clone());
        if seen.as_ref() == Some(&layout) {
            self.display.quiet = self.display.quiet.saturating_add(1);
            if self.display.quiet >= DISPLAY_QUIET_POLLS {
                self.display.rounds = 0;
                self.display.cap_told = false;
            }
        } else {
            self.display.quiet = 0;
        }
        let change = match seen {
            None => {
                self.display.settle = DISPLAY_SETTLE_POLLS;
                DisplayChange::None
            }
            Some(seen) if seen != layout && self.display.settle == 0 => {
                if self.display.rounds < DISPLAY_ROUNDS {
                    self.display.rounds += 1;
                    self.display.settle = DISPLAY_SETTLE_POLLS;
                    DisplayChange::Began
                } else if std::mem::replace(&mut self.display.cap_told, true) {
                    DisplayChange::None
                } else {
                    DisplayChange::Capped
                }
            }
            Some(_) => DisplayChange::None,
        };
        self.settle_poll(config, api);
        change
    }

    /// One poll of a settle round, if one runs.
    fn settle_poll(&mut self, config: &Config, api: &mut impl DisplayApi) {
        if self.display.settle > 0 {
            self.display.settle -= 1;
            if !self.retry_pending() {
                self.sixty_again(config, api);
            }
        }
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
