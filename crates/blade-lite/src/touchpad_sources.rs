//! Logs what the two touchpad sources say (ticket 239): the registry value
//! the app reads, and Windows' touchpad API, with Num Lock and how the app
//! last painted the touchpad key. The key was seen red with the touchpad
//! on, so the log keeps all of them, to tell a wrong source from a stale
//! paint. A read that flips each tick could fill the log, so an hour logs
//! at most [`LINES_PER_HOUR`] changes.

use std::time::{Duration, Instant};

/// The most changes logged in an hour.
pub const LINES_PER_HOUR: u32 = 30;
const HOUR: Duration = Duration::from_secs(3600);

/// What one look saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadLook {
    /// The registry's state; `None` if it can't be read.
    pub registry: Option<bool>,
    /// Windows' touchpad API's state; `None` if it can't be read.
    pub api: Option<bool>,
    /// Whether Num Lock is on: the key is then plain whatever the touchpad.
    pub num_lock: bool,
    /// Whether the app last painted the key red; `None` if not painted.
    pub key_red: Option<bool>,
}

/// Remembers the last look logged, so a line is written only on a change.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadSources {
    last: Option<TouchpadLook>,
    /// When this hour's count began, and how many changes it counted.
    hour: Option<(Instant, u32)>,
}

impl TouchpadSources {
    /// The log line for `look` at `at`, if it differs from the last look
    /// logged. The first look gives a line. Past [`LINES_PER_HOUR`] in an
    /// hour, its last line says so and the rest of that hour's changes are not
    /// logged; the first look after that hour that differs from the last
    /// one logged is.
    pub fn line(&mut self, look: TouchpadLook, at: Instant) -> Option<String> {
        if self.last == Some(look) {
            return None;
        }
        let (start, count) = match self.hour {
            Some((start, count)) if at.saturating_duration_since(start) < HOUR => (start, count),
            _ => (at, 0),
        };
        self.hour = Some((start, count.saturating_add(1)));
        if count > LINES_PER_HOUR {
            return None;
        }
        self.last = Some(look);
        let line = describe(look);
        if count == LINES_PER_HOUR {
            return Some(format!("{line}; more changes this hour are not logged"));
        }
        Some(line)
    }
}

fn describe(look: TouchpadLook) -> String {
    let differ = match (look.registry, look.api) {
        (Some(registry), Some(api)) if registry != api => ", they differ",
        _ => "",
    };
    format!(
        "touchpad: the registry says {}, Windows' touchpad API says {}{differ}; \
         Num Lock {}, the key is {}",
        state(look.registry),
        state(look.api),
        if look.num_lock { "on" } else { "off" },
        paint(look.key_red)
    )
}

fn state(on: Option<bool>) -> &'static str {
    match on {
        Some(true) => "on",
        Some(false) => "off",
        None => "not read",
    }
}

fn paint(red: Option<bool>) -> &'static str {
    match red {
        Some(true) => "painted red",
        Some(false) => "painted plain",
        None => "not painted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Instant {
        Instant::now()
    }

    fn look(registry: Option<bool>, api: Option<bool>, key_red: Option<bool>) -> TouchpadLook {
        TouchpadLook {
            registry,
            api,
            num_lock: false,
            key_red,
        }
    }

    /// Goes past this hour's share with looks that flip each time.
    fn flap_past_the_share(sources: &mut TouchpadSources, now: Instant) -> Vec<String> {
        (0..LINES_PER_HOUR + 5)
            .filter_map(|n| sources.line(look(Some(n % 2 == 0), None, None), now))
            .collect()
    }

    #[test]
    fn an_hour_logs_at_most_its_share_then_says_so_once() {
        let mut sources = TouchpadSources::default();

        let lines = flap_past_the_share(&mut sources, at());

        assert_eq!(lines.len(), LINES_PER_HOUR as usize + 1);
        assert!(
            lines
                .last()
                .is_some_and(|line| line.ends_with("; more changes this hour are not logged")),
            "{lines:?}"
        );
    }

    /// The look that spends the share is named in its line, so a state
    /// that settles there is in the log.
    #[test]
    fn the_last_line_of_the_hour_names_its_look() {
        let mut sources = TouchpadSources::default();
        let lines = flap_past_the_share(&mut sources, at());

        assert!(
            lines
                .last()
                .is_some_and(|line| line.starts_with("touchpad: the registry says ")),
            "{lines:?}"
        );
    }

    #[test]
    fn the_next_hour_logs_again() {
        let mut sources = TouchpadSources::default();
        let now = at();
        flap_past_the_share(&mut sources, now);

        assert!(
            sources
                .line(look(Some(true), Some(true), None), now + HOUR)
                .is_some()
        );
    }

    /// A state reached while the hour's share was spent is logged at the
    /// next hour, even if it has not changed since.
    #[test]
    fn a_state_settled_while_not_logged_is_logged_next_hour() {
        let mut sources = TouchpadSources::default();
        let now = at();
        flap_past_the_share(&mut sources, now);
        let settled = look(Some(false), Some(true), Some(true));
        assert_eq!(sources.line(settled, now), None);

        assert!(sources.line(settled, now + HOUR).is_some());
    }

    #[test]
    fn the_first_look_is_logged() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources
                .line(look(Some(true), Some(true), Some(false)), at())
                .as_deref(),
            Some(
                "touchpad: the registry says on, Windows' touchpad API says on; \
                 Num Lock off, the key is painted plain"
            )
        );
    }

    #[test]
    fn the_same_look_is_not_logged_again() {
        let mut sources = TouchpadSources::default();
        sources.line(look(Some(true), Some(true), Some(false)), at());

        assert_eq!(
            sources.line(look(Some(true), Some(true), Some(false)), at()),
            None
        );
    }

    #[test]
    fn a_change_of_a_source_num_lock_or_the_paint_is_logged() {
        let mut sources = TouchpadSources::default();
        sources.line(look(Some(true), Some(true), Some(false)), at());

        let changes = [
            look(Some(false), Some(true), Some(false)),
            look(Some(false), Some(false), Some(false)),
            look(Some(false), None, Some(false)),
            look(Some(false), None, Some(true)),
            TouchpadLook {
                num_lock: true,
                ..look(Some(false), None, Some(true))
            },
        ];
        for change in changes {
            assert!(sources.line(change, at()).is_some(), "{change:?}");
        }
    }

    #[test]
    fn sources_that_differ_say_so() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources
                .line(look(Some(false), Some(true), Some(true)), at())
                .as_deref(),
            Some(
                "touchpad: the registry says off, Windows' touchpad API says on, they differ; \
                 Num Lock off, the key is painted red"
            )
        );
    }

    #[test]
    fn a_source_not_read_is_named_and_not_called_a_difference() {
        let mut sources = TouchpadSources::default();
        let num_lock_on = TouchpadLook {
            num_lock: true,
            ..look(Some(true), None, None)
        };

        assert_eq!(
            sources.line(num_lock_on, at()).as_deref(),
            Some(
                "touchpad: the registry says on, Windows' touchpad API says not read; \
                 Num Lock on, the key is not painted"
            )
        );
    }
}
