//! Tests on the real laptop. Run with `cargo hw`.

use std::cell::Cell;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use platform::{InstanceLock, WindowsInstance};
use razer_device::{Clock, Device, HidKeyReports, HidTransport, PAUSE, SystemClock};

/// The name the app locks, as `blade_core::APP_NAME` has it.
const APP: &str = "blade-lite";

/// Takes the app's one-copy lock for this test run, so the tests can't drive
/// the controller while Blade Lite runs. It is held until the run ends.
fn hold_lock() {
    static LOCK: Mutex<Option<WindowsInstance>> = Mutex::new(None);
    let mut held = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    if held.is_none() {
        let mut instance = WindowsInstance::new(APP).unwrap();
        assert!(
            instance.claim().unwrap(),
            "Blade Lite is running. Quit it from the tray first."
        );
        *held = Some(instance);
    }
}

/// Opens the controller, once the lock is held.
fn open() -> HidTransport {
    hold_lock();
    HidTransport::open(&razer_proto::BLADE_18).unwrap()
}

/// How many reads the pause tests make.
const READS: usize = 100;

#[test]
#[ignore = "needs the Blade 18"]
fn the_firmware_version_is_read() {
    let transport = open();
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
    let mut device = Device::new(open(), &clock);

    for _ in 0..READS {
        device.firmware_version().unwrap();
    }

    // Each read sleeps once, and each re-read (after Busy or another ID) once more.
    let retries = clock.sleeps.get() - READS;

    eprintln!("{PAUSE:?} pause: {retries} re-reads in {READS} reads");
    assert_eq!(retries, 0);
}

#[test]
#[ignore = "needs the Blade 18"]
fn reads_without_a_pause_fail() {
    let mut device = Device::new(open(), NoPause);

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

/// Each LED's level. A reply for another LED is an error.
fn levels<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<Vec<u8>, String> {
    LEDS.iter()
        .map(|&led| {
            let reply = device
                .send(GET_BRIGHTNESS, &[1, led, 0])
                .map_err(|error| format!("{error:?}"))?;
            match reply.args() {
                [1, echoed, level, ..] if *echoed == led => Ok(*level),
                args => Err(format!("reply for LED {led:02X}: {:02X?}", &args[..3])),
            }
        })
        .collect()
}

/// Sets the keyboard to 50% with the vapor light on or off, reads both
/// levels, then puts the old levels back.
fn set_and_read<C: Clock>(device: &mut Device<HidTransport, C>, vapor_on: bool) -> Vec<u8> {
    let model = &razer_proto::BLADE_18;
    let before = levels(device).unwrap();
    let brightness = razer_proto::Brightness::new(50).unwrap();
    let mut commands = brightness.commands(model);
    commands.push(model.vapor_light.unwrap().command(brightness, vapor_on));

    // The old levels go back before any error is raised.
    let after = device
        .run(&commands)
        .map_err(|error| format!("{error:?}"))
        .and_then(|()| levels(device));
    for (&led, &level) in LEDS.iter().zip(&before) {
        device.send(SET_BRIGHTNESS, &[1, led, level]).unwrap();
    }
    let after = after.unwrap();
    eprintln!("levels before {before:?}, at 50% with vapor {vapor_on}: {after:?}, restored");
    after
}

#[test]
#[ignore = "needs the Blade 18"]
fn brightness_is_set_and_read_back() {
    let mut device = Device::new(open(), SystemClock);

    assert_eq!(set_and_read(&mut device, true), [128, 128]);
}

#[test]
#[ignore = "needs the Blade 18"]
fn the_vapor_light_off_reads_back_level_0() {
    let mut device = Device::new(open(), SystemClock);

    assert_eq!(set_and_read(&mut device, false), [128, 0]);
}

/// Get and set the keyboard effect. Args start `[1, 5]`: storage, keyboard LED.
const GET_EFFECT: u16 = 0x0F82;
const SET_EFFECT: u16 = 0x0F02;
const EFFECT_ARGS: usize = 12;

#[test]
#[ignore = "needs the Blade 18"]
fn static_colour_is_painted_and_the_effect_restored() {
    let mut device = Device::new(open(), SystemClock);
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

    let mut device = Device::new(open(), SystemClock);
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

/// Get a boost level. Args are `[1, cluster, 0]`, the reply args `[1, cluster, level]`.
const GET_BOOST: u16 = 0x0D87;
const SET_BOOST: u16 = 0x0D07;
/// Get a fan's target. Args are `[1, fan, 0]`, the reply args `[1, fan, rpm / 100]`.
const GET_FAN: u16 = 0x0D81;
const SET_FAN: u16 = 0x0D01;
/// Get a fan's real speed. The reply args are `[1, fan, rpm / 100]`.
const GET_FAN_RPM: u16 = 0x0D88;
const CLUSTERS: [u8; 2] = [1, 2];
const FANS: [u8; 2] = [1, 2];

/// Reads `[1, index, 0]` for each index and returns the reply args.
fn read_each<C: Clock>(
    device: &mut Device<HidTransport, C>,
    id: u16,
    indexes: &[u8],
) -> Vec<Vec<u8>> {
    indexes
        .iter()
        .map(|&index| device.send(id, &[1, index, 0]).unwrap().args().to_vec())
        .collect()
}

/// The preset of each zone, both boosts and both fan targets.
fn custom_state<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<Vec<u8>> {
    let mut state = preset_args(device);
    state.extend(read_each(device, GET_BOOST, &CLUSTERS));
    state.extend(read_each(device, GET_FAN, &FANS));
    state
}

#[test]
#[ignore = "needs the Blade 18"]
fn custom_is_set_and_read_back_and_the_old_preset_restored() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);

    // Safe values: middle boosts, and a fan above what auto runs at idle.
    let custom = Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3000, &BLADE_18).unwrap()),
    };
    let set = device.run(&custom.commands(&BLADE_18));
    let manual = custom_state(&mut device);
    std::thread::sleep(Duration::from_secs(5));
    let real_rpm = read_each(&mut device, GET_FAN_RPM, &FANS);

    // Boosts and fan targets first: a mode write after them resets them.
    let boosts = &before[4..6];
    let targets = &before[6..8];
    for args in boosts {
        device.send(SET_BOOST, args).unwrap();
    }
    for args in targets {
        device.send(SET_FAN, args).unwrap();
    }
    for args in &before[..4] {
        device.send(0x0D02, args).unwrap();
    }
    let after = custom_state(&mut device);
    eprintln!(
        "before {before:02X?}\nset {set:?}, read back {manual:02X?}\nreal rpm {real_rpm:02X?}\nafter {after:02X?}"
    );
    set.unwrap();
    assert!(manual[..4].iter().all(|args| args[2..4] == [0x04, 0x01]));
    assert_eq!(manual[4..6], [vec![1, 1, 1], vec![1, 2, 1]]);
    assert_eq!(manual[6..8], [vec![1, 1, 30], vec![1, 2, 30]]);
    assert_eq!(after, before);
}

#[test]
#[ignore = "needs the Blade 18"]
fn both_fans_and_the_cpu_temperature_are_read() {
    use razer_proto::{BLADE_18, Celsius, FanRpm};

    let mut device = Device::new(open(), SystemClock);
    let fans: Vec<_> = FanRpm::queries(&BLADE_18)
        .iter()
        .map(|query| {
            let reply = device.send(query.id, &query.args).unwrap();
            FanRpm::from_reply(reply.args()).unwrap()
        })
        .collect();
    let query = Celsius::query();
    let reply = device.send(query.id, &query.args).unwrap();
    let cpu = Celsius::cpu_from_reply(reply.args());

    eprintln!(
        "fans {fans:?}, cpu {cpu:?}, temperature reply {:02X?}",
        reply.args()
    );
    assert_eq!(fans.len(), 2);
    assert!(fans.iter().all(|fan| fan.0 <= 6000), "{fans:?}");
    let cpu = cpu.expect("the controller reports the CPU temperature");
    assert!((20..=105).contains(&cpu.0), "{cpu:?}");
}

#[test]
#[ignore = "needs the Blade 18"]
fn the_key_reports_open_and_wait_quietly() {
    // Only read, so the one-copy lock is not needed.
    let mut reports = HidKeyReports::open(&razer_proto::BLADE_18).unwrap();

    // No key is pressed, so nothing comes, or only a key-up report.
    let report = reports.read(Duration::from_millis(100)).unwrap();

    if let Some(report) = report {
        let key = razer_proto::BLADE_18.power_key.unwrap();
        assert_ne!(key.held(&report), Some(true), "{report:02X?}");
    }
}
