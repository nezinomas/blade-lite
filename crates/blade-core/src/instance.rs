//! One running copy: a second start opens the first copy's window and ends.
//! An installer's `--quit` asks it to quit; `--reset` runs when none runs.

use std::ffi::OsString;
use std::io;
use std::time::Duration;

use platform::InstanceLock;

/// The name that keeps the app, and the dev tools, to one copy that
/// drives the controller.
pub const APP_NAME: &str = "blade-lite";

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

/// A start that does one task for an installer and ends (ticket 148).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// `--quit`: asks the running copy to quit, as the tray's Quit does
    /// (so it sends the quit preset), and waits for it to end.
    Quit,
    /// `--reset`: when no copy runs, ends a manual fan a crash may have
    /// left, then sends Balanced.
    Reset,
}

impl Task {
    /// The argument that starts this task.
    #[must_use]
    pub const fn flag(self) -> &'static str {
        match self {
            Self::Quit => "--quit",
            Self::Reset => "--reset",
        }
    }
}

/// Why the arguments name no task that can run.
#[derive(Debug, thiserror::Error)]
#[error("--quit or --reset must be the only argument")]
pub struct TaskError;

/// The task the arguments name, or `None` for a start of the app. A task
/// must be the only argument: anything beside it is refused, so a mistyped
/// command does nothing (fail safe).
///
/// # Errors
///
/// [`TaskError`] if a task's flag comes with any other argument.
pub fn task(args: impl IntoIterator<Item = OsString>) -> Result<Option<Task>, TaskError> {
    let args: Vec<OsString> = args.into_iter().collect();
    let named = [Task::Quit, Task::Reset]
        .into_iter()
        .find(|task| args.iter().any(|arg| arg == task.flag()));
    match (named, args.len()) {
        (None, _) => Ok(None),
        (Some(task), 1) => Ok(Some(task)),
        (Some(_), _) => Err(TaskError),
    }
}

/// What `--quit` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    /// No copy ran, so nobody was asked.
    NoneRan,
    /// The running copy was asked and has ended.
    Ended,
    /// The running copy was asked but had not ended within the wait; the
    /// ask was taken back.
    StillRuns,
}

/// Asks the running copy to quit and waits up to `wait` for it to let go
/// of the lock, looking each `every` (each pause through `sleep`). Once it
/// returns [`Quit::NoneRan`] or [`Quit::Ended`], this copy holds the lock,
/// so no new copy can start until it ends. If the copy has not ended in
/// time, the ask is taken back, so it does not quit later by surprise.
///
/// # Errors
///
/// The Windows error if a claim, the ask, or taking it back failed. Nothing
/// is asked after a failed claim.
pub fn quit_first(
    lock: &mut impl InstanceLock,
    wait: Duration,
    every: Duration,
    mut sleep: impl FnMut(Duration),
) -> io::Result<Quit> {
    if lock.claim()? {
        return Ok(Quit::NoneRan);
    }
    lock.ask_first_to_quit()?;
    let looks = wait.as_millis().div_ceil(every.as_millis().max(1));
    for _ in 0..looks {
        sleep(every);
        if lock.claim()? {
            return Ok(Quit::Ended);
        }
    }
    lock.withdraw_quit()?;
    Ok(Quit::StillRuns)
}
