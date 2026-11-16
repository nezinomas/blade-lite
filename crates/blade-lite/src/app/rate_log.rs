//! Ticket 222 (c): a log line for each set of the battery rate, to learn
//! why Windows refuses the first 60 Hz after a wake. The lines go out again
//! when ticket 222 closes (ticket 82 made the tries quiet on purpose),
//! unless the user wants them kept.

use std::io;
use std::time::{Duration, Instant};

use platform::{Display, DisplayApi, DisplayMode, ModeLife};

use crate::report;

/// The sets of the rate since the last power event or start.
#[derive(Debug, Default)]
pub(super) struct RateSets {
    /// When the last wake notice came, if the sets follow a wake.
    woke: Option<Instant>,
    /// How many sets were made since.
    count: u8,
}

impl RateSets {
    /// A new series of sets: after a wake notice (`woke`), or after a
    /// switch, a first AC or the quit (`None`).
    pub(super) fn begin(&mut self, woke: Option<Instant>) {
        *self = Self { woke, count: 0 };
    }
}

/// Passes each call on to the displays, and logs each set of a mode.
pub(super) struct RateLog<'a, D> {
    pub(super) displays: &'a mut D,
    pub(super) sets: &'a mut RateSets,
}

impl<D: DisplayApi> DisplayApi for RateLog<'_, D> {
    fn displays(&self) -> io::Result<Vec<Display>> {
        self.displays.displays()
    }

    fn set_mode(&mut self, name: &str, mode: DisplayMode, life: ModeLife) -> io::Result<()> {
        let set = self.displays.set_mode(name, mode, life);
        self.sets.count = self.sets.count.saturating_add(1);
        let since = self.sets.woke.map(|woke| woke.elapsed());
        let now = self.displays.displays().map(|displays| {
            displays
                .into_iter()
                .find(|display| display.name == name)
                .map(|display| display.current)
        });
        report(&rate_set_line(
            self.sets.count,
            since,
            mode.hz,
            &set,
            &now,
            platform::session_locked(),
        ));
        set
    }

    fn saved_mode(&self, name: &str) -> Option<DisplayMode> {
        self.displays.saved_mode(name)
    }
}

/// The log line of one set: its number since the event, the time since
/// the wake notice, the result, the mode Windows reports after it, and
/// whether the session is locked.
fn rate_set_line(
    count: u8,
    since_wake: Option<Duration>,
    hz: u32,
    set: &io::Result<()>,
    now: &io::Result<Option<DisplayMode>>,
    locked: Option<bool>,
) -> String {
    let since = since_wake.map_or_else(
        || "no wake".to_owned(),
        |since| format!("{:.1} s after the wake notice", since.as_secs_f32()),
    );
    let result = match set {
        Ok(()) => "worked".to_owned(),
        Err(error) => format!("failed: {error}"),
    };
    let now = match now {
        Ok(Some(mode)) => format!("{}x{} {} Hz", mode.width, mode.height, mode.hz),
        Ok(None) => "the panel is not active".to_owned(),
        Err(error) => format!("not read: {error}"),
    };
    let locked = match locked {
        Some(true) => "locked",
        Some(false) => "unlocked",
        None => "lock not known",
    };
    format!("rate set {count} ({since}): {hz} Hz {result}; Windows reports {now}; session {locked}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODE: DisplayMode = DisplayMode {
        width: 2560,
        height: 1600,
        hz: 240,
    };

    #[test]
    fn a_refused_set_after_a_wake_says_all_it_knows() {
        let line = rate_set_line(
            1,
            Some(Duration::from_millis(5300)),
            60,
            &Err(io::Error::other("ChangeDisplaySettingsExW returned -1")),
            &Ok(Some(MODE)),
            Some(true),
        );
        assert_eq!(
            line,
            "rate set 1 (5.3 s after the wake notice): 60 Hz failed: \
             ChangeDisplaySettingsExW returned -1; Windows reports 2560x1600 240 Hz; session locked"
        );
    }

    #[test]
    fn a_set_with_no_wake_says_so() {
        let line = rate_set_line(3, None, 60, &Ok(()), &Ok(None), None);
        assert_eq!(
            line,
            "rate set 3 (no wake): 60 Hz worked; Windows reports the panel is not active; \
             session lock not known"
        );
    }

    #[test]
    fn a_failed_read_after_the_set_is_said() {
        let line = rate_set_line(
            2,
            None,
            60,
            &Ok(()),
            &Err(io::Error::other("gone")),
            Some(false),
        );
        assert!(line.ends_with("Windows reports not read: gone; session unlocked"));
    }

    #[test]
    fn a_new_series_counts_from_one() {
        let mut sets = RateSets {
            woke: None,
            count: 4,
        };
        let woke = Instant::now();
        sets.begin(Some(woke));
        assert_eq!((sets.woke, sets.count), (Some(woke), 0));
    }
}
