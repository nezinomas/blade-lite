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

/// Does not wait.
struct NoPause;

impl Clock for NoPause {
    fn sleep(&self, _: Duration) {}
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

#[test]
#[ignore = "needs the Blade 18"]
fn the_pause_leaves_no_busy_replies() {
    let clock = CountedPause::default();
    let mut device = Device::new(HidTransport::open().unwrap(), &clock);

    for _ in 0..READS {
        device.firmware_version().unwrap();
    }

    // Each read sleeps once, and each Busy retry once more.
    let retries = clock.sleeps.get() - READS;

    eprintln!("{PAUSE:?} pause: {retries} busy retries in {READS} reads");
    assert_eq!(retries, 0);
}

#[test]
#[ignore = "needs the Blade 18"]
fn reads_without_a_pause_fail() {
    let mut device = Device::new(HidTransport::open().unwrap(), NoPause);

    let errors: Vec<_> = (0..READS)
        .filter_map(|_| device.firmware_version().err())
        .collect();

    eprintln!(
        "no pause: {} of {READS} reads failed, first: {:?}",
        errors.len(),
        errors.first()
    );
    assert!(!errors.is_empty(), "the pause is not needed");
}

/// Get LED brightness. Args are `[1, led, 0]`, the reply args `[1, led, level]`.
const GET_BRIGHTNESS: u16 = 0x0383;
const SET_BRIGHTNESS: u16 = 0x0303;
const LEDS: [u8; 2] = [0x05, 0x26];

fn levels<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<u8> {
    LEDS.iter()
        .map(|&led| device.send(GET_BRIGHTNESS, &[1, led, 0]).unwrap().args()[2])
        .collect()
}

#[test]
#[ignore = "needs the Blade 18"]
fn brightness_is_set_and_read_back() {
    let mut device = Device::new(HidTransport::open().unwrap(), SystemClock);
    let before = levels(&mut device);

    device
        .run(&razer_proto::Brightness::new(50).unwrap().commands(&razer_proto::BLADE_18))
        .unwrap();
    let after = levels(&mut device);

    for (&led, &level) in LEDS.iter().zip(&before) {
        device.send(SET_BRIGHTNESS, &[1, led, level]).unwrap();
    }
    eprintln!("levels before {before:?}, at 50%: {after:?}, restored");
    assert_eq!(after, [128, 128]);
}
