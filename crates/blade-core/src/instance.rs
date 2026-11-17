//! One running copy: a second start opens the first copy's window and ends.
//! An installer's `--quit` asks it to quit; `--reset` runs when none runs.

use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use platform::InstanceLock;

use crate::TRAY_FLAG;

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

/// The exit code of a `--quit` whose copy took the ask but has not ended:
/// it must not be ended, as it sends the quit preset as it ends. The
/// installer script (`installer/blade-lite.iss`) holds it too.
pub const STILL_QUITTING: u8 = 2;

/// The exit code of a task that failed (an error or a panic): while a copy
/// may be quitting on a `--quit` ask, [`STILL_QUITTING`], as it must not be
/// ended (ticket 235); else 1.
#[must_use]
pub const fn failed_task_code(may_be_quitting: bool) -> u8 {
    if may_be_quitting { STILL_QUITTING } else { 1 }
}

/// What a start of `blade-lite.exe` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Runs the app, in the tray if started with [`TRAY_FLAG`].
    App {
        /// Started with [`TRAY_FLAG`], as autostart does.
        in_tray: bool,
    },
    /// Does one task for an installer and ends.
    Task(Task),
}

/// Why the arguments name nothing the app can do.
#[derive(Debug, thiserror::Error)]
#[error("unknown arguments: Blade Lite takes none, or one of --tray, --quit and --reset")]
pub struct ArgsError;

/// What the arguments ask for. Only no argument, or one exact flag, counts;
/// anything else (a mistyped flag, two flags, a flag that is not text) is
/// refused, so a wrong command never starts the app and sends the saved
/// preset (fail safe, ticket 148).
///
/// # Errors
///
/// [`ArgsError`] for any other arguments.
pub fn launch(args: impl IntoIterator<Item = OsString>) -> Result<Launch, ArgsError> {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(Launch::App { in_tray: false }),
        [one] if one == TRAY_FLAG => Ok(Launch::App { in_tray: true }),
        [one] => [Task::Quit, Task::Reset]
            .into_iter()
            .find(|task| one == task.flag())
            .map(Launch::Task)
            .ok_or(ArgsError),
        _ => Err(ArgsError),
    }
}

/// What `--quit` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    /// No copy ran, so nobody was asked.
    NoneRan,
    /// The running copy was asked, sent its quit preset and has ended.
    Ended,
    /// The running copy was asked and has ended, but did not say its quit
    /// preset was sent (a failed send, a Razer updater, a crash).
    EndedUnsent,
    /// The running copy took the ask but had not ended within both waits,
    /// such as while a menu or a file box of its own was open. It quits
    /// once that ends: it must not be ended meanwhile, and whether its quit
    /// preset went is not known.
    Quitting,
    /// The running copy had not taken the ask within the wait (it may wait
    /// at login for the controller, or on a box); the ask was taken back.
    StillRuns,
}

/// How long `--quit` waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuitWait {
    /// For the running copy to take the ask or end.
    pub to_take: Duration,
    /// Then, once it took the ask, for it to end.
    pub to_end: Duration,
    /// How often it looks.
    pub every: Duration,
}

/// Asks the running copy to quit and waits up to `wait.to_take` for it to
/// let go of the lock, looking each `wait.every` (each pause through
/// `sleep`). An ask left untaken is then taken back, so no copy quits later
/// by surprise. A copy that took the ask is quitting: it gets `wait.to_end`
/// more, and is never reported as one that can be ended. Once it returns
/// [`Quit::NoneRan`], [`Quit::Ended`] or [`Quit::EndedUnsent`], this copy
/// holds the lock, so no new copy can start until it ends.
///
/// `may_be_quitting` is set just before the ask and cleared once no copy can
/// be quitting on it (the copy ended, or the ask was taken back), so an
/// error or a panic can be told apart (see [`failed_task_code`]).
///
/// # Errors
///
/// The Windows error if the first claim or the ask failed (nothing is asked
/// after a failed first claim), if a claim failed while the ask was taken
/// back (so no copy quits), or if the read of whether the quit preset was
/// sent failed.
pub fn quit_first(
    lock: &mut impl InstanceLock,
    wait: QuitWait,
    may_be_quitting: &AtomicBool,
    mut sleep: impl FnMut(Duration),
) -> io::Result<Quit> {
    if lock.claim()? {
        return Ok(Quit::NoneRan);
    }
    // Before the call: a failed ask may still have been set.
    may_be_quitting.store(true, Ordering::SeqCst);
    lock.ask_first_to_quit()?;
    let ended = wait_for_the_lock(lock, wait.to_take, wait.every, &mut sleep);
    // Also after an end or an error: a copy that ended without taking the
    // ask must not leave it to the next one.
    let taken_back = lock.take_back_quit();
    if matches!(ended, Ok(true)) {
        may_be_quitting.store(false, Ordering::SeqCst);
        return quit_result(lock);
    }
    if matches!(taken_back, Ok(true)) {
        may_be_quitting.store(false, Ordering::SeqCst);
        return ended.map(|_| Quit::StillRuns);
    }
    // The copy took the ask (or the look failed, so it may have): it may
    // be quitting, so even a failed claim must not report it as one to end.
    match wait_for_the_lock(lock, wait.to_end, wait.every, &mut sleep) {
        Ok(true) => {
            may_be_quitting.store(false, Ordering::SeqCst);
            quit_result(lock)
        }
        Ok(false) | Err(_) => Ok(Quit::Quitting),
    }
}

/// Whether the copy that ended said its quit preset was sent.
fn quit_result(lock: &impl InstanceLock) -> io::Result<Quit> {
    if lock.quit_was_sent()? {
        Ok(Quit::Ended)
    } else {
        Ok(Quit::EndedUnsent)
    }
}

/// Claims the lock each `every` until it works or `wait` is up: whether
/// the other copy ended.
fn wait_for_the_lock(
    lock: &mut impl InstanceLock,
    wait: Duration,
    every: Duration,
    sleep: &mut impl FnMut(Duration),
) -> io::Result<bool> {
    let looks = wait.as_millis().div_ceil(every.as_millis().max(1));
    for _ in 0..looks {
        sleep(every);
        if lock.claim()? {
            return Ok(true);
        }
    }
    Ok(false)
}
