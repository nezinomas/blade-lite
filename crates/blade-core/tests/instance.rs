//! One running copy, over a fake lock.

use std::cell::Cell;
use std::io;

use blade_core::{Start, start_instance};
use platform::InstanceLock;

/// A lock that another copy may already hold.
struct FakeLock {
    held: bool,
    fails: bool,
    woken: Cell<u32>,
}

impl FakeLock {
    fn new(held: bool) -> Self {
        Self {
            held,
            fails: false,
            woken: Cell::new(0),
        }
    }
}

impl InstanceLock for FakeLock {
    fn claim(&mut self) -> io::Result<bool> {
        if self.fails {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let first = !self.held;
        self.held = true;
        Ok(first)
    }

    fn wake_first(&self) -> io::Result<()> {
        self.woken.set(self.woken.get() + 1);
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
