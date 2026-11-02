//! Windows APIs behind small traits, so the rest can be tested with fakes.

/// Something Windows tells us about power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// The laptop woke from sleep or hibernation.
    Resumed,
}

/// Power events from Windows, one at a time.
pub trait PowerEvents {
    /// Waits for the next event. `None` means no more events will come.
    fn next_event(&mut self) -> Option<PowerEvent>;
}
