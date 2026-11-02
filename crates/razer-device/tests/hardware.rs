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
    let transport = HidTransport::open(&razer_proto::BLADE_18).unwrap();
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
    let mut device = Device::new(HidTransport::open(&razer_proto::BLADE_18).unwrap(), &clock);

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
    let mut device = Device::new(HidTransport::open(&razer_proto::BLADE_18).unwrap(), NoPause);

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
    let mut device = Device::new(
        HidTransport::open(&razer_proto::BLADE_18).unwrap(),
        SystemClock,
    );
    let before = levels(&mut device);

    device
        .run(
            &razer_proto::Brightness::new(50)
                .unwrap()
                .commands(&razer_proto::BLADE_18),
        )
        .unwrap();
    let after = levels(&mut device);

    for (&led, &level) in LEDS.iter().zip(&before) {
        device.send(SET_BRIGHTNESS, &[1, led, level]).unwrap();
    }
    eprintln!("levels before {before:?}, at 50%: {after:?}, restored");
    assert_eq!(after, [128, 128]);
}

/// Get and set the keyboard effect. Args start `[1, 5]`: storage, keyboard LED.
const GET_EFFECT: u16 = 0x0F82;
const SET_EFFECT: u16 = 0x0F02;
const EFFECT_ARGS: usize = 12;

#[test]
#[ignore = "needs the Blade 18"]
fn static_colour_is_painted_and_the_effect_restored() {
    let mut device = Device::new(
        HidTransport::open(&razer_proto::BLADE_18).unwrap(),
        SystemClock,
    );
    let mut query = [0; EFFECT_ARGS];
    query[..2].copy_from_slice(&[1, 5]);
    let before = device.send(GET_EFFECT, &query).unwrap().args().to_vec();

    let painted = device.run(&razer_proto::static_colour(
        razer_proto::Rgb::new(0xFF, 0, 0),
        &razer_proto::BLADE_18,
    ));
    let during = device.send(GET_EFFECT, &query).unwrap().args().to_vec();
    std::thread::sleep(Duration::from_secs(5));

    let restored = device.send(SET_EFFECT, &before).map(|_| ());
    let after = device.send(GET_EFFECT, &query).unwrap().args().to_vec();
    eprintln!(
        "effect before {before:02X?}, painted {painted:?}, during {during:02X?}, restored {restored:?}, after {after:02X?}"
    );
    painted.unwrap();
    restored.unwrap();
    assert_eq!(after, before);
}

/// Reads each zone's preset args, as the controller sends them back.
fn preset_args<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<Vec<u8>> {
    razer_proto::PerfMode::queries(&razer_proto::BLADE_18)
        .iter()
        .map(|query| device.send(query.id, &query.args).unwrap().args().to_vec())
        .collect()
}

#[test]
#[ignore = "needs the Blade 18"]
fn each_preset_is_set_and_read_back_and_the_old_one_restored() {
    use razer_proto::PerfMode;

    let mut device = Device::new(
        HidTransport::open(&razer_proto::BLADE_18).unwrap(),
        SystemClock,
    );
    let before = preset_args(&mut device);

    let read_back: Vec<_> = [PerfMode::Silent, PerfMode::Balanced, PerfMode::Performance]
        .into_iter()
        .map(|preset| {
            device
                .run(&preset.commands(&razer_proto::BLADE_18).unwrap())
                .unwrap();
            let zones: Vec<_> = preset_args(&mut device)
                .iter()
                .map(|args| PerfMode::from_reply(args))
                .collect();
            (preset, zones)
        })
        .collect();

    for args in &before {
        device.send(0x0D02, args).unwrap();
    }
    let after = preset_args(&mut device);
    eprintln!("before {before:02X?}, read back {read_back:?}, after {after:02X?}");
    for (preset, zones) in read_back {
        assert!(zones.iter().all(|zone| *zone == Some(preset)), "{preset:?}");
    }
    assert_eq!(after, before);
}
