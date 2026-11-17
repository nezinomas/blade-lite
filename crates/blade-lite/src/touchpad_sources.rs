//! Logs what the two touchpad sources say (ticket 239): the registry value
//! the app reads, and Windows' touchpad API. The key was seen red with the
//! touchpad on, so the log keeps both, to find which one was wrong.

/// Remembers the last pair logged, so a line is written only on a change.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadSources {
    last: Option<(Option<bool>, Option<bool>)>,
}

impl TouchpadSources {
    /// The log line for the registry's and the API's state (`None`: can't
    /// be read), if the pair differs from the last one. The first pair
    /// always gives a line.
    pub fn line(&mut self, registry: Option<bool>, api: Option<bool>) -> Option<String> {
        let now = Some((registry, api));
        if self.last == now {
            return None;
        }
        self.last = now;
        let differ = if registry.is_some() && api.is_some() && registry != api {
            ", they differ"
        } else {
            ""
        };
        Some(format!(
            "touchpad: the registry says {}, Windows' touchpad API says {}{differ}",
            state(registry),
            state(api)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_pair_is_logged() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources.line(Some(true), Some(true)).as_deref(),
            Some("touchpad: the registry says on, Windows' touchpad API says on")
        );
    }

    #[test]
    fn the_same_pair_is_not_logged_again() {
        let mut sources = TouchpadSources::default();
        sources.line(Some(true), Some(true));

        assert_eq!(sources.line(Some(true), Some(true)), None);
    }

    #[test]
    fn a_change_of_either_source_is_logged() {
        let mut sources = TouchpadSources::default();
        sources.line(Some(true), Some(true));

        assert!(sources.line(Some(false), Some(true)).is_some());
        assert!(sources.line(Some(false), Some(false)).is_some());
        assert!(sources.line(Some(false), None).is_some());
    }

    #[test]
    fn sources_that_differ_say_so() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources.line(Some(false), Some(true)).as_deref(),
            Some("touchpad: the registry says off, Windows' touchpad API says on, they differ")
        );
    }

    #[test]
    fn a_source_not_read_is_named_and_not_called_a_difference() {
        let mut sources = TouchpadSources::default();

        assert_eq!(
            sources.line(Some(true), None).as_deref(),
            Some("touchpad: the registry says on, Windows' touchpad API says not read")
        );
    }
}
