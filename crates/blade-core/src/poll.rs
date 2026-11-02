//! When to read the sensors: once a second, only while a view is visible.

use std::time::{Duration, Instant};

/// The time between two polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A view that shows readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Window,
    Flyout,
}

/// Tracks the visible views and when the next poll is due.
#[derive(Debug, Default)]
pub struct PollSchedule {
    window: bool,
    flyout: bool,
    /// `None` while no view is visible.
    next: Option<Instant>,
}

impl PollSchedule {
    /// Marks a view visible. The first visible view makes a poll due at once.
    pub fn show(&mut self, view: View, now: Instant) {
        if self.next.is_none() {
            self.next = Some(now);
        }
        *self.visible(view) = true;
    }

    /// Marks a view hidden. Polling stops when no view is left.
    pub fn hide(&mut self, view: View) {
        *self.visible(view) = false;
        if !self.window && !self.flyout {
            self.next = None;
        }
    }

    /// Whether to poll now. A `true` moves the next poll one interval on.
    pub fn poll_due(&mut self, now: Instant) -> bool {
        let due = self.next.is_some_and(|next| now >= next);
        if due {
            self.next = Some(now + POLL_INTERVAL);
        }
        due
    }

    /// When the next poll is due, or `None` while polling is stopped.
    #[must_use]
    pub fn next_poll(&self) -> Option<Instant> {
        self.next
    }

    fn visible(&mut self, view: View) -> &mut bool {
        match view {
            View::Window => &mut self.window,
            View::Flyout => &mut self.flyout,
        }
    }
}
