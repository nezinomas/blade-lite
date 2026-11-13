//! When to read the sensors: once a second while a view is visible, and only
//! the temperatures, for the heat warning, every ten seconds while none is.

use std::time::{Duration, Instant, SystemTime};

/// The time between two polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The time between two temperature reads while no view is visible.
pub const HEAT_INTERVAL: Duration = Duration::from_secs(10);

/// How often the temperatures are read while a manual Custom fan runs, so
/// the fan guard acts within seconds (ticket 161). Each read also reads
/// the charger and may send the preset again, as a view's poll does.
pub const GUARD_HEAT_INTERVAL: Duration = Duration::from_secs(2);

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
    /// The last temperature read (or skip) while no view is visible.
    /// `None` when one is due at once.
    last_heat: Option<LastHeat>,
}

/// A temperature read, or one skipped as too late.
#[derive(Debug, Clone, Copy)]
struct LastHeat {
    at: Instant,
    /// The wall clock then, which counts a sleep.
    wall: SystemTime,
    /// Whether it was skipped: the next then waits [`HEAT_INTERVAL`].
    skipped: bool,
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

    /// Whether to read only the temperatures now: never while a view
    /// polls, since each poll reads them. A read is due `interval`
    /// ([`HEAT_INTERVAL`] or [`GUARD_HEAT_INTERVAL`]) after the last one,
    /// so a switch to the short one reads within it. A read more than
    /// [`HEAT_INTERVAL`] late by `now` or by the wall clock `wall` is
    /// skipped, and the next waits [`HEAT_INTERVAL`] whatever `interval` is:
    /// the laptop slept or the event loop stood still, so the controller may
    /// not be awake and the wake not known yet. The wall clock counts a sleep
    /// that `Instant` may not; a wall clock set back skips nothing.
    pub fn heat_due(&mut self, now: Instant, wall: SystemTime, interval: Duration) -> bool {
        if self.next.is_some() {
            self.last_heat = None;
            return false;
        }
        let Some(last) = self.last_heat else {
            self.last_heat = Some(LastHeat {
                at: now,
                wall,
                skipped: false,
            });
            return true;
        };
        let gap = if last.skipped {
            HEAT_INTERVAL
        } else {
            interval
        };
        let next = last.at + gap;
        if now < next {
            return false;
        }
        let late = now > next + HEAT_INTERVAL;
        let slept = wall
            .duration_since(last.wall)
            .is_ok_and(|passed| passed > gap + HEAT_INTERVAL);
        let skipped = late || slept;
        self.last_heat = Some(LastHeat {
            at: now,
            wall,
            skipped,
        });
        !skipped
    }

    /// Makes a temperature read due at once, not skipped as late: for the
    /// end of a wake's pause, when the wake is known (ticket 171).
    pub fn read_heat_now(&mut self) {
        self.last_heat = None;
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
