//! The real lock and wake, under a name that each test makes its own.

use std::time::Duration;

use platform::{InstanceLock, WindowsInstance};

const WAIT: Duration = Duration::from_secs(2);
const NO_WAIT: Duration = Duration::from_millis(50);

fn name(test: &str) -> String {
    format!("blade-lite-test-{test}-{}", std::process::id())
}

#[test]
fn only_the_first_copy_gets_the_claim() {
    let name = name("claim");
    let mut first = WindowsInstance::new(&name).unwrap();
    let mut second = WindowsInstance::new(&name).unwrap();

    assert!(first.claim().unwrap());
    assert!(!second.claim().unwrap());
}

#[test]
fn the_claim_is_free_again_once_the_first_copy_ends() {
    let name = name("free");
    let mut first = WindowsInstance::new(&name).unwrap();
    assert!(first.claim().unwrap());
    drop(first);

    assert!(WindowsInstance::new(&name).unwrap().claim().unwrap());
}

#[test]
fn a_wake_from_the_second_copy_reaches_the_first() {
    let name = name("wake");
    let mut first = WindowsInstance::new(&name).unwrap();
    assert!(first.claim().unwrap());
    let listener = first.listener().unwrap();
    let waiting = std::thread::spawn(move || listener.wait(WAIT).unwrap());

    WindowsInstance::new(&name).unwrap().wake_first().unwrap();

    assert!(waiting.join().unwrap());
}

#[test]
fn no_wake_means_the_wait_times_out() {
    let name = name("quiet");
    let mut first = WindowsInstance::new(&name).unwrap();
    assert!(first.claim().unwrap());

    assert!(!first.listener().unwrap().wait(NO_WAIT).unwrap());
}

#[test]
fn one_wake_is_seen_once() {
    let name = name("once");
    let first = WindowsInstance::new(&name).unwrap();
    let listener = first.listener().unwrap();
    first.wake_first().unwrap();

    assert!(listener.wait(NO_WAIT).unwrap());
    assert!(!listener.wait(NO_WAIT).unwrap());
}
