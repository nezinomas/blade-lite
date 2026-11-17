//! One running copy, over a fake lock.

use std::cell::Cell;
use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use blade_core::{
    Launch, Quit, QuitWait, STILL_QUITTING, Start, Task, failed_task_code, launch, quit_first,
    start_instance,
};
use platform::InstanceLock;

/// A lock that another copy may already hold.
struct FakeLock {
    held: bool,
    woken: Cell<u32>,
    /// The other copy lets go of the lock after this many claims.
    frees_after: Option<u32>,
    claims: u32,
    quits_asked: Cell<u32>,
    quits_taken_back: Cell<u32>,
    /// The other copy took the ask, so it can't be taken back.
    ask_taken: bool,
    /// The other copy said its quit preset was sent.
    quit_sent: bool,
    /// The claim with this number fails.
    fails_at: Option<u32>,
    /// A look that fails.
    look_fails: Option<Look>,
    /// The ask to quit fails.
    ask_fails: bool,
}

/// A look at the quit events.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    TakeBack,
    Sent,
}

fn failed_look() -> io::Error {
    io::ErrorKind::PermissionDenied.into()
}

impl FakeLock {
    fn new(held: bool) -> Self {
        Self {
            held,
            woken: Cell::new(0),
            frees_after: None,
            claims: 0,
            quits_asked: Cell::new(0),
            quits_taken_back: Cell::new(0),
            ask_taken: false,
            quit_sent: true,
            fails_at: None,
            look_fails: None,
            ask_fails: false,
        }
    }
}

impl InstanceLock for FakeLock {
    fn claim(&mut self) -> io::Result<bool> {
        self.claims += 1;
        if self.fails_at == Some(self.claims) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
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
        if self.ask_fails {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }

    fn take_back_quit(&self) -> io::Result<bool> {
        self.quits_taken_back.set(self.quits_taken_back.get() + 1);
        if self.look_fails == Some(Look::TakeBack) {
            return Err(failed_look());
        }
        Ok(!self.ask_taken)
    }

    fn quit_was_sent(&self) -> io::Result<bool> {
        if self.look_fails == Some(Look::Sent) {
            return Err(failed_look());
        }
        Ok(self.quit_sent)
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
    lock.fails_at = Some(1);

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
/// 10 looks to take the ask, then 20 more to end.
const WAIT: QuitWait = QuitWait {
    to_take: Duration::from_secs(1),
    to_end: Duration::from_secs(2),
    every: EVERY,
};

/// Runs `quit_first` with a sleep that only counts the sleeps.
fn quit(lock: &mut FakeLock) -> (io::Result<Quit>, u32) {
    let (quit, sleeps, _) = quit_marked(lock);
    (quit, sleeps)
}

/// As [`quit`], and whether it marked the ask as made.
fn quit_marked(lock: &mut FakeLock) -> (io::Result<Quit>, u32, bool) {
    let asked = AtomicBool::new(false);
    let mut sleeps = 0;
    let quit = quit_first(lock, WAIT, &asked, |pause| {
        assert_eq!(pause, EVERY);
        sleeps += 1;
    });
    (quit, sleeps, asked.load(Ordering::SeqCst))
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
    assert_eq!(sleeps, 3);
}

#[test]
fn a_copy_that_ended_without_taking_the_ask_leaves_none_for_the_next() {
    let mut lock = FakeLock::new(true);
    lock.frees_after = Some(1);

    quit(&mut lock).0.unwrap();

    assert_eq!(lock.quits_taken_back.get(), 1);
}

#[test]
fn a_copy_that_ended_without_saying_its_quit_preset_went_is_unsent() {
    let mut lock = FakeLock::new(true);
    lock.frees_after = Some(1);
    lock.quit_sent = false;

    assert_eq!(quit(&mut lock).0.unwrap(), Quit::EndedUnsent);
}

#[test]
fn a_copy_that_never_took_the_ask_still_runs_and_the_ask_is_taken_back() {
    let mut lock = FakeLock::new(true);

    let (quit, sleeps) = quit(&mut lock);

    assert_eq!(quit.unwrap(), Quit::StillRuns);
    assert_eq!(lock.quits_asked.get(), 1);
    assert_eq!(lock.quits_taken_back.get(), 1);
    assert_eq!(sleeps, 10);
}

#[test]
fn a_copy_that_took_the_ask_but_has_not_ended_in_both_waits_is_quitting() {
    let mut lock = FakeLock::new(true);
    lock.ask_taken = true;

    let (quit, sleeps) = quit(&mut lock);

    assert_eq!(quit.unwrap(), Quit::Quitting);
    assert_eq!(sleeps, 30);
}

#[test]
fn a_copy_that_took_the_ask_gets_the_second_wait_to_end() {
    let mut lock = FakeLock::new(true);
    lock.ask_taken = true;
    lock.frees_after = Some(25);

    assert_eq!(quit(&mut lock).0.unwrap(), Quit::Ended);
}

#[test]
fn a_failed_claim_while_the_copy_quits_is_still_quitting_never_one_to_end() {
    let mut lock = FakeLock::new(true);
    lock.ask_taken = true;
    lock.fails_at = Some(3);

    assert_eq!(quit(&mut lock).0.unwrap(), Quit::Quitting);
}

#[test]
fn a_failed_take_back_counts_as_a_copy_that_may_be_quitting() {
    let mut lock = FakeLock::new(true);
    lock.look_fails = Some(Look::TakeBack);

    // Never one to end: it may be in its quit sends.
    assert_eq!(quit(&mut lock).0.unwrap(), Quit::Quitting);
}

#[test]
fn a_copy_that_ended_is_read_even_if_the_take_back_fails() {
    let mut lock = FakeLock::new(true);
    lock.frees_after = Some(1);
    lock.look_fails = Some(Look::TakeBack);

    assert_eq!(quit(&mut lock).0.unwrap(), Quit::Ended);
}

#[test]
fn a_failed_read_of_the_quit_preset_is_an_error() {
    let mut lock = FakeLock::new(true);
    lock.frees_after = Some(1);
    lock.look_fails = Some(Look::Sent);

    assert!(quit(&mut lock).0.is_err());
}

#[test]
fn a_failed_claim_in_a_quit_is_an_error_and_asks_nobody() {
    let mut lock = FakeLock::new(true);
    lock.fails_at = Some(1);

    assert!(quit(&mut lock).0.is_err());
    assert_eq!(lock.quits_asked.get(), 0);
}

#[test]
fn a_failed_claim_after_the_ask_takes_the_ask_back() {
    let mut lock = FakeLock::new(true);
    lock.fails_at = Some(3);

    assert!(quit(&mut lock).0.is_err());
    assert_eq!(lock.quits_asked.get(), 1);
    assert_eq!(lock.quits_taken_back.get(), 1);
}

#[test]
fn a_quit_with_no_copy_running_never_marks_an_ask() {
    let mut lock = FakeLock::new(false);

    assert!(!quit_marked(&mut lock).2);
}

#[test]
fn a_failed_first_claim_never_marks_an_ask() {
    let mut lock = FakeLock::new(true);
    lock.fails_at = Some(1);

    assert!(!quit_marked(&mut lock).2);
}

#[test]
fn an_ask_is_marked_even_if_it_fails_as_it_may_have_been_set() {
    let mut lock = FakeLock::new(true);
    lock.ask_fails = true;

    let (quit, _, asked) = quit_marked(&mut lock);

    assert!(quit.is_err());
    assert!(asked);
}

#[test]
fn an_error_after_the_ask_comes_with_the_ask_marked() {
    for look in [None, Some(Look::Sent)] {
        let mut lock = FakeLock::new(true);
        lock.frees_after = look.map(|_| 1);
        lock.fails_at = look.is_none().then_some(3);
        lock.look_fails = look;

        let (quit, _, asked) = quit_marked(&mut lock);

        assert!(quit.is_err());
        assert!(asked);
    }
}

#[test]
fn a_task_that_failed_after_a_quit_ask_is_still_quitting_never_one_to_end() {
    assert_eq!(failed_task_code(true), STILL_QUITTING);
}

#[test]
fn a_task_that_failed_before_any_ask_gives_1() {
    assert_eq!(failed_task_code(false), 1);
}

fn launch_of(args: &[&str]) -> Result<Launch, String> {
    launch(args.iter().map(OsString::from)).map_err(|error| error.to_string())
}

#[test]
fn each_known_start_is_one_exact_argument_or_none() {
    assert_eq!(launch_of(&[]), Ok(Launch::App { in_tray: false }));
    assert_eq!(launch_of(&["--tray"]), Ok(Launch::App { in_tray: true }));
    assert_eq!(launch_of(&["--quit"]), Ok(Launch::Task(Task::Quit)));
    assert_eq!(launch_of(&["--reset"]), Ok(Launch::Task(Task::Reset)));
}

#[test]
fn any_other_arguments_start_nothing() {
    for args in [
        &["--quit", "--reset"][..],
        &["--quit", "--tray"],
        &["--tray", "--tray"],
        &["--quit", "--quit"],
        &["--reset", "x"],
        &["--other", "--tray"],
        &["--Quit"],
        &["/quit"],
        &["-quit"],
        &["--quit=1"],
        &["--quit "],
        &["--TRAY"],
        &["--tray=1"],
        &[""],
    ] {
        assert!(launch_of(args).is_err(), "{args:?}");
    }
}

#[test]
fn an_argument_that_is_not_text_is_refused_and_no_crash() {
    use std::os::windows::ffi::OsStringExt;
    let broken = OsString::from_wide(&[0xD800]);

    assert!(launch([broken.clone()]).is_err());
    assert!(launch([OsString::from("--quit"), broken]).is_err());
}
