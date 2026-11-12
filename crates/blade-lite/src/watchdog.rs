//! Sees a stalled event loop. Every controller send and read, and every
//! guard (the fan guard, the heat warning, Balanced on battery), runs on
//! the event-loop thread: while it waits, none of them runs. The loop's
//! poll stamps a beat; a thread of its own looks at it and logs a stall.

use std::time::{Duration, Instant};

/// How long the loop may go without a beat before it counts as stalled.
pub const STALL: Duration = Duration::from_secs(30);

/// How often the watchdog thread looks.
pub const LOOK_GAP: Duration = Duration::from_secs(5);

/// A look this much later than the last one means the laptop slept: the
/// loop did not run, but it did not stall either.
const SLEPT: Duration = Duration::from_secs(15);

/// What the watchdog last saw.
#[derive(Debug)]
pub struct Watchdog {
    last_look: Instant,
    /// The start, or the last wake: the loop is given [`STALL`] from here.
    awake_since: Instant,
    stalled: bool,
}

impl Watchdog {
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            last_look: now,
            awake_since: now,
            stalled: false,
        }
    }

    /// Looks at the loop's last `beat` (`None` before the first). A line to
    /// log when the loop stalls, and one when it beats again.
    pub fn look(&mut self, beat: Option<Instant>, now: Instant) -> Option<String> {
        if now.saturating_duration_since(self.last_look) >= SLEPT {
            self.awake_since = now;
        }
        self.last_look = now;
        let since = beat.map_or(self.awake_since, |beat| beat.max(self.awake_since));
        let quiet = now.saturating_duration_since(since);
        let stalled = quiet >= STALL;
        if stalled == self.stalled {
            return None;
        }
        self.stalled = stalled;
        Some(if stalled {
            format!(
                "the app has not run its checks for {} s: no fan guard, heat warning or battery preset while it waits",
                quiet.as_secs()
            )
        } else {
            "the app runs its checks again".to_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    /// Looks every [`LOOK_GAP`] from `from` up to `to` seconds, with the
    /// beat fixed, and keeps what was said.
    fn looks(
        dog: &mut Watchdog,
        start: Instant,
        beat: Option<Instant>,
        from: u64,
        to: u64,
    ) -> Vec<String> {
        (from..=to)
            .step_by(5)
            .filter_map(|second| dog.look(beat, start + SECOND * u32::try_from(second).unwrap()))
            .collect()
    }

    #[test]
    fn a_loop_that_beats_is_never_said() {
        let start = Instant::now();
        let mut dog = Watchdog::new(start);

        let said: Vec<_> = (5..=300)
            .step_by(5)
            .filter_map(|second| {
                let now = start + SECOND * second;
                dog.look(Some(now.checked_sub(SECOND).unwrap()), now)
            })
            .collect();

        assert!(said.is_empty(), "{said:?}");
    }

    #[test]
    fn a_stall_is_said_once_after_30_s_and_its_end_once() {
        let start = Instant::now();
        let mut dog = Watchdog::new(start);
        let beat = Some(start);

        let said = looks(&mut dog, start, beat, 5, 120);
        let back = dog.look(Some(start + SECOND * 124), start + SECOND * 125);

        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("the app has not run its checks for 30 s"),
            "{said:?}"
        );
        assert_eq!(back.as_deref(), Some("the app runs its checks again"));
    }

    #[test]
    fn no_first_beat_counts_from_the_start() {
        let start = Instant::now();
        let mut dog = Watchdog::new(start);

        assert_eq!(looks(&mut dog, start, None, 5, 25), Vec::<String>::new());
        assert_eq!(looks(&mut dog, start, None, 30, 30).len(), 1);
    }

    #[test]
    fn a_sleep_is_no_stall() {
        let start = Instant::now();
        let mut dog = Watchdog::new(start);
        let beat = Some(start + SECOND * 5);
        assert_eq!(looks(&mut dog, start, beat, 5, 5), Vec::<String>::new());

        // An hour asleep: no look, no beat. Then the loop beats within 30 s.
        let said = looks(&mut dog, start, beat, 3605, 3625);

        assert!(said.is_empty(), "{said:?}");
    }
}
