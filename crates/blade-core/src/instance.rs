//! One running copy: a second start opens the first copy's window and ends.

use std::io;

use platform::InstanceLock;

/// What this start of the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// No other copy runs, so this one runs.
    First,
    /// Another copy runs and was told to open its window, so this one ends.
    Second,
}

/// Claims the app, or wakes the copy that has it.
///
/// # Errors
///
/// The Windows error if the claim or the wake failed. The caller should
/// then end, since a second copy must never drive the controller.
pub fn start_instance(lock: &mut impl InstanceLock) -> io::Result<Start> {
    if lock.claim()? {
        return Ok(Start::First);
    }
    lock.wake_first()?;
    Ok(Start::Second)
}
