//! One running copy: a second start opens the first copy's window and ends.

use std::io;

use platform::InstanceLock;

/// What this start of the app should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// No other copy runs, so this one runs.
    First,
    /// Another copy runs, so this one ends.
    Second,
}

/// Claims the app. If another copy has it, that copy is told to open its
/// window, but only if `open_window` (a start in the tray leaves it alone).
///
/// # Errors
///
/// The Windows error if the claim or the wake failed. The caller should
/// then end, since a second copy must never drive the controller.
pub fn start_instance(lock: &mut impl InstanceLock, open_window: bool) -> io::Result<Start> {
    if lock.claim()? {
        return Ok(Start::First);
    }
    if open_window {
        lock.wake_first()?;
    }
    Ok(Start::Second)
}
