//! One running copy, over a fake lock.

use std::cell::Cell;
use std::ffi::OsString;
use std::io;
use std::time::Duration;

use blade_core::{Quit, Start, Task, quit_first, start_instance, task};
use platform::InstanceLock;

/// A lock that another copy may already hold.
struct FakeLock {
    held: bool,
    fails: bool,
    woken: Cell<u32>,
    /// The other copy lets go of the lock after this many claims.
    frees_after: Option<u32>,
    claims: u32,
    quits_asked: Cell<u32>,
    quits_withdrawn: Cell<u32>,
}

impl FakeLock {
    fn new(held: bool) -> Self {
        Self {
            held,
            fails: false,
            woken: Cell::new(0),
            frees_after: None,
            claims: 0,
            quits_asked: Cell::new(0),
            quits_withdrawn: Cell::new(0),
        }
    }
}

impl InstanceLock for FakeLock {
    fn claim(&mut self) -> io::Result<bool> {
        if self.fails {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.claims += 1;
        if self.frees_after.is_some_and(|after| self.claims > after) {
            self.held = false;
        }
        let first = !self.held;
        self.held = true;
        Ok(first)
    }

    fn wake_first(&self) -> io::Result<()> {
        self.woken.set(self.woken.get() + 1);
        Ok(())
    }

    fn ask_first_to_quit(&self) -> io::Result<()> {
        self.quits_asked.set(self.quits_asked.get() + 1);
        Ok(())
    }

    fn withdraw_quit(&self) -> io::Result<()> {
        self.quits_withdrawn.set(self.quits_withdrawn.get() + 1);
        Ok(())
    }
}

#[test]
fn the_first_start_runs_and_wakes_nobody() {
    let mut lock = FakeLock::new(false);

    assert_eq!(start_instance(&mut lock, true).unwrap(), Start::First);
    assert_eq!(lock.woken.get(), 0);
}

#[test]
fn a_second_start_tells_the_first_to_open_its_window() {
    let mut lock = FakeLock::new(true);

    assert_eq!(start_instance(&mut lock, true).unwrap(), Start::Second);
    assert_eq!(lock.woken.get(), 1);
}

#[test]
fn a_failed_claim_is_an_error_so_no_second_copy_runs() {
    let mut lock = FakeLock::new(false);
    lock.fails = true;

    assert!(start_instance(&mut lock, true).is_err());
    assert_eq!(lock.woken.get(), 0);
}

#[test]
fn a_second_start_in_the_tray_ends_and_leaves_the_window_alone() {
    let mut lock = FakeLock::new(true);

    assert_eq!(start_instance(&mut lock, false).unwrap(), Start::Second);
    assert_eq!(lock.woken.get(), 0);
}

const EVERY: Duration = Duration::from_millis(100);
const WAIT: Duration = Duration::from_secs(1);

/// Runs `quit_first` with a sleep that only counts the sleeps.
fn quit(lock: &mut FakeLock) -> (io::Result<Quit>, u32) {
    let mut sleeps = 0;
    let quit = quit_first(lock, WAIT, EVERY, |pause| {
        assert_eq!(pause, EVERY);
        sleeps += 1;
    });
    (quit, sleeps)
}

#[test]
fn a_quit_with_no_copy_running_asks_nobody() {
    let mut lock = FakeLock::new(false);

    let (quit, sleeps) = quit(&mut lock);

    assert_eq!(quit.unwrap(), Quit::NoneRan);
    assert_eq!((lock.quits_asked.get(), sleeps), (0, 0));
}

#[test]
fn a_quit_asks_the_running_copy_and_waits_until_it_ends() {
    let mut lock = FakeLock::new(true);
    lock.frees_after = Some(3);

    let (quit, sleeps) = quit(&mut lock);

    assert_eq!(quit.unwrap(), Quit::Ended);
    assert_eq!(lock.quits_asked.get(), 1);
    assert_eq!(lock.quits_withdrawn.get(), 0);
    assert_eq!(sleeps, 3);
}

#[test]
fn a_copy_that_does_not_end_in_time_still_runs_and_the_ask_is_withdrawn() {
    let mut lock = FakeLock::new(true);

    let (quit, sleeps) = quit(&mut lock);

    assert_eq!(quit.unwrap(), Quit::StillRuns);
    assert_eq!(lock.quits_asked.get(), 1);
    // A late quit, after the caller gave up, would surprise the user.
    assert_eq!(lock.quits_withdrawn.get(), 1);
    assert_eq!(sleeps, 10);
}

#[test]
fn a_failed_claim_in_a_quit_is_an_error_and_asks_nobody() {
    let mut lock = FakeLock::new(true);
    lock.fails = true;

    assert!(quit(&mut lock).0.is_err());
    assert_eq!(lock.quits_asked.get(), 0);
}

fn task_of(args: &[&str]) -> Result<Option<Task>, String> {
    task(args.iter().map(OsString::from)).map_err(|error| error.to_string())
}

#[test]
fn quit_and_reset_are_tasks_and_other_starts_run_the_app() {
    assert_eq!(task_of(&[]), Ok(None));
    assert_eq!(task_of(&["--tray"]), Ok(None));
    assert_eq!(task_of(&["--quit"]), Ok(Some(Task::Quit)));
    assert_eq!(task_of(&["--reset"]), Ok(Some(Task::Reset)));
}

#[test]
fn a_task_must_come_alone() {
    for args in [
        &["--quit", "--reset"][..],
        &["--reset", "--quit"],
        &["--quit", "--tray"],
        &["--tray", "--reset"],
        &["--quit", "--quit"],
        &["--reset", "x"],
    ] {
        assert!(task_of(args).is_err(), "{args:?}");
    }
}

#[test]
fn a_task_beside_an_argument_that_is_not_text_is_refused() {
    use std::os::windows::ffi::OsStringExt;
    let broken = OsString::from_wide(&[0xD800]);

    assert!(task([OsString::from("--quit"), broken]).is_err());
}
