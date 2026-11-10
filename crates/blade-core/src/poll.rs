//! When to read the sensors: once a second while a view is visible, and only
//! the temperatures, for the heat warning, every ten seconds while none is.

use std::time::{Duration, Instant};

/// The time between two polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The time between two temperature reads while no view is visible.
pub const HEAT_INTERVAL: Duration = Duration::from_secs(10);

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
    /// The next temperature read while no view is visible, `None` when one
    /// is due at once.
    next_heat: Option<Instant>,
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

    /// Whether to read only the temperatures now: never while a view polls,
    /// since each poll reads them. A read due moves the next one
    /// [`HEAT_INTERVAL`] on. A read more than one interval late is skipped,
    /// and the next waits one interval: the event loop stood still, as in
    /// a sleep, so the controller may not be awake and the wake not known.
    pub fn heat_due(&mut self, now: Instant) -> bool {
        if self.next.is_some() {
            self.next_heat = None;
            return false;
        }
        let Some(next) = self.next_heat else {
            self.next_heat = Some(now + HEAT_INTERVAL);
            return true;
        };
        if now < next {
            return false;
        }
        self.next_heat = Some(now + HEAT_INTERVAL);
        now <= next + HEAT_INTERVAL
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
