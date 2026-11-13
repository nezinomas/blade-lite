//! When to read the sensors: once a second while a view is visible, and only
//! the temperatures, for the heat warning, every ten seconds while none is.
//! A view is not visible while the screen is off, nor the window while it
//! is minimised (ticket 188).

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

/// Whether the window is closed, shown, or shown but minimised.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum WindowState {
    #[default]
    Closed,
    Shown,
    Minimized,
}

/// Tracks the visible views and when the next poll is due.
#[derive(Debug, Default)]
pub struct PollSchedule {
    window: WindowState,
    flyout: bool,
    screen_off: bool,
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
    /// Marks a view shown, the window as not minimised (showing it brings
    /// it back). The first visible view makes a poll due at once.
    pub fn show(&mut self, view: View, now: Instant) {
        match view {
            View::Window => self.window = WindowState::Shown,
            View::Flyout => self.flyout = true,
        }
        self.follow(now);
    }

    /// Marks a view hidden. Polling stops when no view is left.
    pub fn hide(&mut self, view: View) {
        match view {
            View::Window => self.window = WindowState::Closed,
            View::Flyout => self.flyout = false,
        }
        self.stop_if_unseen();
    }

    /// Marks the window minimised or not. Nothing for a closed window.
    pub fn set_window_minimized(&mut self, minimized: bool, now: Instant) {
        if self.window == WindowState::Closed {
            return;
        }
        self.window = if minimized {
            WindowState::Minimized
        } else {
            WindowState::Shown
        };
        self.follow(now);
    }

    /// Marks the screen off or on. While it is off no view is visible.
    pub fn set_screen_off(&mut self, off: bool, now: Instant) {
        self.screen_off = off;
        self.follow(now);
    }

    /// Whether a view can be seen now.
    fn seen(&self) -> bool {
        !self.screen_off && (self.window == WindowState::Shown || self.flyout)
    }

    /// Makes a poll due at once if a view just came into sight, or stops
    /// polling if none is left.
    fn follow(&mut self, now: Instant) {
        if self.seen() {
            self.next.get_or_insert(now);
        }
        self.stop_if_unseen();
    }

    fn stop_if_unseen(&mut self) {
        if !self.seen() {
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
}
