//! Logs what the two touchpad sources say (ticket 239): the registry value
//! the app reads, and Windows' touchpad API, with how the app last painted
//! the touchpad key. The key was seen red with the touchpad on, so the log
//! keeps all three, to tell a wrong source from a stale paint. A read that
//! flips each tick could fill the log, so an hour logs at most
//! [`LINES_PER_HOUR`] changes.

use std::time::{Duration, Instant};

/// The most changes logged in an hour.
pub const LINES_PER_HOUR: u32 = 30;
const HOUR: Duration = Duration::from_secs(3600);

/// What one look saw: the registry, the API (`None`: can't be read), and
/// whether the key was last painted red (`None`: not painted).
type Seen = (Option<bool>, Option<bool>, Option<bool>);

/// Remembers the last look logged, so a line is written only on a change.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadSources {
    last: Option<Seen>,
    /// When this hour's count began, and how many changes it logged.
    hour: Option<(Instant, u32)>,
}

impl TouchpadSources {
    /// The log line for the registry's and the API's state and the key's
    /// paint at `at`, if any of them differs from the last look. The first
    /// look gives a line. Past [`LINES_PER_HOUR`] in an hour, one line says
    /// so and the rest of that hour's changes are not logged.
    pub fn line(
        &mut self,
        registry: Option<bool>,
        api: Option<bool>,
        key_red: Option<bool>,
        at: Instant,
    ) -> Option<String> {
        let now = Some((registry, api, key_red));
        if self.last == now {
            return None;
        }
        self.last = now;
        let (start, count) = match self.hour {
            Some((start, count)) if at.saturating_duration_since(start) < HOUR => (start, count),
            _ => (at, 0),
        };
        self.hour = Some((start, count.saturating_add(1)));
        if count == LINES_PER_HOUR {
            return Some("touchpad: more changes this hour are not logged".to_owned());
        }
        if count > LINES_PER_HOUR {
            return None;
        }
        let differ = if registry.is_some() && api.is_some() && registry != api {
            ", they differ"
        } else {
            ""
        };
        Some(format!(
            "touchpad: the registry says {}, Windows' touchpad API says {}{differ}; the key is {}",
            state(registry),
            state(api),
            paint(key_red)
        ))
    }
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

    #[test]
    fn an_hour_logs_at_most_its_share_then_says_so_once() {
        let mut sources = TouchpadSources::default();
        let lines: Vec<_> = (0..LINES_PER_HOUR + 5)
            .filter_map(|n| sources.line(Some(n % 2 == 0), None, None, at()))
            .collect();

        assert_eq!(lines.len(), LINES_PER_HOUR as usize + 1);
        assert_eq!(
            lines.last().map(String::as_str),
            Some("touchpad: more changes this hour are not logged")
        );
    }

    #[test]
    fn the_next_hour_logs_again() {
        let mut sources = TouchpadSources::default();
        for n in 0..=LINES_PER_HOUR {
            sources.line(Some(n % 2 == 0), None, None, at());
        }

        let later = at() + HOUR;

        assert!(sources.line(Some(true), Some(true), None, later).is_some());
    }

    #[test]
    fn the_first_look_is_logged() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources
                .line(Some(true), Some(true), Some(false), at())
                .as_deref(),
            Some(
                "touchpad: the registry says on, Windows' touchpad API says on; \
                 the key is painted plain"
            )
        );
    }

    #[test]
    fn the_same_look_is_not_logged_again() {
        let mut sources = TouchpadSources::default();
        sources.line(Some(true), Some(true), Some(false), at());

        assert_eq!(
            sources.line(Some(true), Some(true), Some(false), at()),
            None
        );
    }

    #[test]
    fn a_change_of_either_source_or_the_paint_is_logged() {
        let mut sources = TouchpadSources::default();
        sources.line(Some(true), Some(true), Some(false), at());

        assert!(
            sources
                .line(Some(false), Some(true), Some(false), at())
                .is_some()
        );
        assert!(
            sources
                .line(Some(false), Some(false), Some(false), at())
                .is_some()
        );
        assert!(sources.line(Some(false), None, Some(false), at()).is_some());
        assert!(sources.line(Some(false), None, Some(true), at()).is_some());
    }

    #[test]
    fn sources_that_differ_say_so() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources
                .line(Some(false), Some(true), Some(true), at())
                .as_deref(),
            Some(
                "touchpad: the registry says off, Windows' touchpad API says on, they differ; \
                 the key is painted red"
            )
        );
    }

    #[test]
    fn a_source_not_read_is_named_and_not_called_a_difference() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources.line(Some(true), None, None, at()).as_deref(),
            Some(
                "touchpad: the registry says on, Windows' touchpad API says not read; \
                 the key is not painted"
            )
        );
    }
}
