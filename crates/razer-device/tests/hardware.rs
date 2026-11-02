//! Tests on the real laptop, one at a time so they do not share the controller:
//! `cargo test -p razer-device --test hardware -- --ignored --test-threads=1`.

use std::cell::Cell;
use std::time::Duration;

use razer_device::{Clock, Device, HidTransport, PAUSE, SystemClock};

/// How many reads the pause tests make.
const READS: usize = 100;

#[test]
#[ignore = "needs the Blade 18"]
fn the_firmware_version_is_read() {
    let transport = HidTransport::open().unwrap();
    eprintln!("opened {:?}", transport.info());

    let version = Device::new(transport, SystemClock)
        .firmware_version()
        .unwrap();

    eprintln!("firmware {}.{}", version.major, version.minor);
    assert!(version.major >= 1, "{version:?}");
}

/// Counts the sleeps and does not wait. Each read sleeps once, each Busy retry once more.
#[derive(Default)]
struct NoPause {
    sleeps: Cell<usize>,
}

impl Clock for &NoPause {
    fn sleep(&self, _: Duration) {
        self.sleeps.set(self.sleeps.get() + 1);
    }
}

/// Counts the sleeps and waits for real.
#[derive(Default)]
struct CountedPause {
    sleeps: Cell<usize>,
}

impl Clock for &CountedPause {
    fn sleep(&self, duration: Duration) {
        self.sleeps.set(self.sleeps.get() + 1);
        std::thread::sleep(duration);
    }
}

/// Busy retries in `READS` firmware reads: the sleeps past one per read.
fn busy_retries<C: Clock>(clock: C, sleeps: impl Fn() -> usize) -> usize {
    let mut device = Device::new(HidTransport::open().unwrap(), clock);
    for _ in 0..READS {
        device.firmware_version().unwrap();
    }
    sleeps() - READS
}

#[test]
#[ignore = "needs the Blade 18"]
fn the_pause_leaves_no_busy_replies() {
    let clock = CountedPause::default();

    let retries = busy_retries(&clock, || clock.sleeps.get());

    eprintln!("{PAUSE:?} pause: {retries} busy retries in {READS} reads");
    assert_eq!(retries, 0);
}

#[test]
#[ignore = "needs the Blade 18"]
fn busy_replies_without_a_pause_are_counted() {
    let clock = NoPause::default();

    let retries = busy_retries(&clock, || clock.sleeps.get());

    eprintln!("no pause: {retries} busy retries in {READS} reads");
}
