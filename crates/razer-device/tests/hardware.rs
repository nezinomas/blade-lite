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

const SET_BRIGHTNESS: u16 = 0x0303;
const LEDS: [u8; 2] = [0x05, 0x26];

/// Each LED's level. A reply for another LED is an error.
fn levels<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<Vec<u8>, String> {
    LEDS.iter()
        .map(|&led| {
            let query = razer_proto::Brightness::level_query(led);
            let reply = device
                .send(query.id, &query.args)
                .map_err(|error| format!("{error:?}"))?;
            let args = reply.args();
            razer_proto::Brightness::level_from_reply(led, args)
                .ok_or_else(|| format!("reply for LED {led:02X}: {args:02X?}"))
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
                .zip(razer_proto::BLADE_18.perf_zones)
                .map(|(args, &zone)| PerfMode::from_reply(&razer_proto::BLADE_18, zone, args))
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
/// The fan byte of a mode reply: auto, or manual (the fan follows its target).
const FAN_AUTO: u8 = 0x00;
const FAN_MANUAL: u8 = 0x01;

/// The preset of each zone, both boosts and both fan targets.
fn custom_state<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<Vec<u8>> {
    try_custom_state(device).unwrap()
}

#[test]
#[ignore = "needs the Blade 18"]
fn custom_is_set_and_read_back_and_the_old_preset_restored() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);

    // Safe values: middle boosts, and a fan above what auto runs at idle.
    let custom = Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3000, &BLADE_18).unwrap()),
    };

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let set = device.run(&custom.commands(&BLADE_18));
    let manual = set
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|()| try_custom_state(&mut device));
    let real_rpm = manual.as_ref().map_err(Clone::clone).and_then(|_| {
        std::thread::sleep(Duration::from_secs(5));
        try_read_each(&mut device, GET_FAN_RPM, &FANS)
    });

    // Boosts and fan targets first, as razer-laptop-control says a mode
    // write after them resets them (not seen on 1.6, ticket 144).
    let put_back = put_back_custom(&mut device, &before);
    let after = try_custom_state(&mut device);
    eprintln!(
        "before {before:02X?}\nset {set:?}, read back {manual:02X?}\nreal rpm {real_rpm:02X?}\nput back {put_back:?}\nafter {after:02X?}"
    );
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, Ok(before));
    set.unwrap();
    let manual = manual.unwrap();
    real_rpm.unwrap();
    assert!(manual[..4].iter().all(|args| args[2..4] == [0x04, 0x01]));
    assert_eq!(manual[4..6], [vec![1, 1, 1], vec![1, 2, 1]]);
    assert_eq!(manual[6..8], [vec![1, 1, 30], vec![1, 2, 30]]);
}

#[test]
#[ignore = "needs the Blade 18"]
fn both_fans_and_the_cpu_temperature_are_read() {
    use razer_proto::{BLADE_18, Celsius, FanRpm};

    let mut device = Device::new(open(), SystemClock);
    let fans: Vec<_> = FanRpm::queries(&BLADE_18)
        .iter()
        .zip(BLADE_18.custom.fan_zones)
        .map(|(query, &fan)| {
            let reply = device.send(query.id, &query.args).unwrap();
            FanRpm::from_reply(fan, reply.args()).unwrap()
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

/// What [`custom_state`] reads, keeping the first error.
fn try_custom_state<C: Clock>(
    device: &mut Device<HidTransport, C>,
) -> Result<Vec<Vec<u8>>, String> {
    let mut state = razer_proto::PerfMode::queries(&razer_proto::BLADE_18)
        .iter()
        .map(|query| {
            device
                .send(query.id, &query.args)
                .map(|reply| reply.args().to_vec())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    state.extend(try_read_each(device, GET_BOOST, &CLUSTERS)?);
    state.extend(try_read_each(device, GET_FAN, &FANS)?);
    Ok(state)
}

/// Reads `[1, index, 0]` for each index, keeping the first error.
fn try_read_each<C: Clock>(
    device: &mut Device<HidTransport, C>,
    id: u16,
    indexes: &[u8],
) -> Result<Vec<Vec<u8>>, String> {
    indexes
        .iter()
        .map(|&index| {
            device
                .send(id, &[1, index, 0])
                .map(|reply| reply.args().to_vec())
                .map_err(|error| error.to_string())
        })
        .collect()
}

#[test]
#[ignore = "needs the Blade 18, and runs the fans on manual for 10 s"]
fn a_custom_mode_write_alone_and_what_the_fans_do() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    // The fans go to the target already set, and the put-back writes the
    // state as read: refuse one the model does not take.
    refuse_unknown_state(&before);

    // Only the mode writes of a manual Custom (fan byte manual), as a kill
    // between them and the fan targets would leave it (ticket 144).
    let custom = Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3000, &BLADE_18).unwrap()),
    };
    let modes: Vec<_> = custom.commands(&BLADE_18)[..BLADE_18.perf_zones.len()].to_vec();
    assert!(
        modes
            .iter()
            .all(|command| command.id == 0x0D02 && command.args[2..4] == [BLADE_18.custom.mode, 1])
    );

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let set = device.run(&modes);
    let mut seen = Vec::new();
    let mut read_error = Ok(());
    if set.is_ok() {
        for second in 1..=10 {
            std::thread::sleep(Duration::from_secs(1));
            let reads = try_read_each(&mut device, GET_FAN, &FANS)
                .and_then(|target| Ok((target, try_read_each(&mut device, GET_FAN_RPM, &FANS)?)));
            match reads {
                Ok((target, real)) => seen.push((second, target, real)),
                Err(error) => {
                    read_error = Err(error);
                    break;
                }
            }
        }
    }
    let modes_read: Result<Vec<_>, String> = razer_proto::PerfMode::queries(&BLADE_18)
        .iter()
        .map(|query| {
            device
                .send(query.id, &query.args)
                .map(|reply| reply.args().to_vec())
                .map_err(|error| error.to_string())
        })
        .collect();

    // Put back as the Custom test does: boosts and fan targets, then modes.
    // Every send is tried, so a failed one does not keep the fans manual.
    let put_back: Vec<_> = before[4..6]
        .iter()
        .map(|args| (SET_BOOST, args))
        .chain(before[6..8].iter().map(|args| (SET_FAN, args)))
        .chain(before[..4].iter().map(|args| (0x0D02, args)))
        .map(|(id, args)| {
            device
                .send(id, args)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .collect();
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}\nset {set:?}\nmodes after the writes {modes_read:02X?}");
    for (second, target, real) in &seen {
        eprintln!("{second:>2} s: target {target:02X?}, real {real:02X?}");
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    set.unwrap();
    read_error.unwrap();
    // What ticket 144 measured: the mode took, the target was kept, and
    // the fans ran.
    assert!(
        modes_read
            .unwrap()
            .iter()
            .all(|args| args[2..4] == [BLADE_18.custom.mode, 1])
    );
    assert_eq!(seen.len(), 10);
    assert!(seen.iter().all(|(_, target, _)| target[..] == before[6..8]));
    let (_, _, last) = &seen[9];
    assert!(last.iter().all(|args| args[2] > 0), "{last:02X?}");
}

/// Refuses to run when the state read before the test is not one the
/// put-back can safely write: each mode reply must echo its zone and hold a
/// mode the model offers and an auto or manual fan byte, each boost reply
/// must echo its cluster and hold a level the model takes, and each fan
/// target must be in the model's range (0 would stop the fans: the
/// put-back writes it while the fans are still manual).
fn refuse_unknown_state(before: &[Vec<u8>]) {
    use razer_proto::{BLADE_18, CpuBoost, GpuBoost};

    let zones = BLADE_18.perf_zones;
    assert!(
        before.len() == zones.len() + CLUSTERS.len() + FANS.len(),
        "state {before:02X?} not understood: not run"
    );
    let bytes = BLADE_18.mode_bytes;
    let modes = [
        bytes.silent,
        bytes.balanced,
        bytes.performance,
        BLADE_18.custom.mode,
    ];
    for (args, &zone) in before[..4].iter().zip(zones) {
        assert!(
            matches!(args[..], [1, echoed, mode, FAN_AUTO | FAN_MANUAL]
                if echoed == zone && modes.contains(&mode)),
            "zone {zone} mode reply {args:02X?} not understood: not run"
        );
    }
    let boosts: [fn(u8) -> bool; 2] = [
        |level| CpuBoost::new(level, &BLADE_18).is_ok(),
        |level| GpuBoost::new(level, &BLADE_18).is_ok(),
    ];
    for ((args, cluster), takes) in before[4..6].iter().zip(CLUSTERS).zip(boosts) {
        assert!(
            matches!(args[..], [1, echoed, level] if echoed == cluster && takes(level)),
            "cluster {cluster} boost reply {args:02X?} not understood: not run"
        );
    }
    let range = BLADE_18.custom.fan_rpm;
    for (args, fan) in before[6..8].iter().zip(FANS) {
        assert!(
            args.len() >= 3 && args[0] == 1 && args[1] == fan,
            "fan {fan} target reply {args:02X?} not understood: not run"
        );
        let rpm = u16::from(args[2]) * 100;
        assert!(
            (range.min..=range.max).contains(&rpm),
            "fan target {rpm} RPM is outside the model's range: not run"
        );
    }
}

/// Puts back what [`custom_state`] read, as the Custom test does: boosts and
/// fan targets, then modes. The targets go with the fans as the test left
/// them (manual, unless it ended after a quit preset; a target is
/// range-checked either way) and the modes (fan byte as read) last; a reset
/// of a target by the mode write shows in the caller's after-state check. Every send is tried, so a failed one does not
/// keep the fans manual.
fn put_back_custom<C: Clock>(
    device: &mut Device<HidTransport, C>,
    before: &[Vec<u8>],
) -> Vec<Result<(), String>> {
    before[4..6]
        .iter()
        .map(|args| (SET_BOOST, args))
        .chain(before[6..8].iter().map(|args| (SET_FAN, args)))
        .chain(before[..4].iter().map(|args| (0x0D02, args)))
        .map(|(id, args)| {
            device
                .send(id, args)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .collect()
}

/// The fan targets and the real speeds, one reply's args per fan.
type FanReads = (Vec<Vec<u8>>, Vec<Vec<u8>>);

/// Reads the fan targets and real speeds each 5 s for 30 s, or until a
/// read fails.
fn watch_fans<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<Vec<FanReads>, String> {
    let mut seen = Vec::new();
    for _ in 0..6 {
        std::thread::sleep(Duration::from_secs(5));
        let target = try_read_each(device, GET_FAN, &FANS)?;
        let real = try_read_each(device, GET_FAN_RPM, &FANS)?;
        seen.push((target, real));
    }
    Ok(seen)
}

#[test]
#[ignore = "needs the Blade 18, and runs the fans on manual for about 70 s"]
fn custom_from_balanced_and_again_and_what_the_fans_do() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, PerfMode, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    // A full Custom with a manual fan, as the app sends it on a click from
    // Balanced and again on a slider release (ticket 173: about 2900 RPM
    // after the click, more after a slider move).
    let custom = Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3600, &BLADE_18).unwrap()),
    }
    .commands(&BLADE_18);
    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let from_balanced = device.run(&balanced).and_then(|()| {
        std::thread::sleep(Duration::from_secs(5));
        device.run(&custom)
    });
    let first = from_balanced
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|()| watch_fans(&mut device));
    let again = first
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|_| device.run(&custom).map_err(|error| error.to_string()))
        .and_then(|()| watch_fans(&mut device));

    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (name, seen) in [("from Balanced", &first), ("sent again", &again)] {
        match seen {
            Ok(seen) => {
                for (step, (target, real)) in seen.iter().enumerate() {
                    eprintln!(
                        "{name} {:>2} s: target {target:02X?}, real {real:02X?}",
                        (step + 1) * 5
                    );
                }
            }
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    first.unwrap();
    again.unwrap();
}

/// Watches the fans for `seconds`, reading the targets and real speeds
/// each 5 s. With `app_reads`, it also reads each second what the app's
/// open window reads: the temperatures, each fan's speed and the charger.
fn watch_fans_for<C: Clock>(
    device: &mut Device<HidTransport, C>,
    seconds: u64,
    app_reads: bool,
) -> Result<Vec<FanReads>, String> {
    use razer_proto::{BLADE_18, Celsius, Charger, FanRpm};

    let mut polls = vec![Celsius::query()];
    polls.extend(FanRpm::queries(&BLADE_18));
    polls.push(Charger::query());
    let mut seen = Vec::new();
    for second in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));
        if app_reads {
            for query in &polls {
                device
                    .send(query.id, &query.args)
                    .map_err(|error| error.to_string())?;
            }
        }
        if second % 5 == 0 {
            let target = try_read_each(device, GET_FAN, &FANS)?;
            let real = try_read_each(device, GET_FAN_RPM, &FANS)?;
            seen.push((target, real));
        }
    }
    Ok(seen)
}

#[test]
#[ignore = "needs the Blade 18, and runs the fans on manual for about 2.5 min"]
fn the_users_custom_with_and_without_the_app_reads() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, PerfMode, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    // The user's saved Custom (ticket 173): both boosts at the top, 3600 RPM,
    // sent from Performance as the app's click from it does.
    let custom = Custom {
        cpu: CpuBoost::new(3, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(3, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3600, &BLADE_18).unwrap()),
    }
    .commands(&BLADE_18);
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    let from_performance = |device: &mut Device<HidTransport, SystemClock>, app_reads| {
        device
            .run(&performance)
            .and_then(|()| {
                std::thread::sleep(Duration::from_secs(10));
                device.run(&custom)
            })
            .map_err(|error| error.to_string())
            .and_then(|()| watch_fans_for(device, 60, app_reads))
    };

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let quiet = from_performance(&mut device, false);
    let polled = quiet
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|_| from_performance(&mut device, true));

    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (name, seen) in [("no app reads", &quiet), ("app reads", &polled)] {
        match seen {
            Ok(seen) => {
                for (step, (target, real)) in seen.iter().enumerate() {
                    eprintln!(
                        "{name} {:>2} s: target {target:02X?}, real {real:02X?}",
                        (step + 1) * 5
                    );
                }
            }
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    quiet.unwrap();
    polled.unwrap();
}

#[test]
#[ignore = "needs the Blade 18, and runs the fans on manual for about 3 min"]
fn which_boost_holds_a_manual_fan_back() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    // CPU boost, GPU boost, fan RPM (ticket 173: 3 and 3 held 3600 at 2900).
    let cases = [(3, 1, 3600), (1, 3, 3600), (2, 2, 3600), (3, 3, 4500)];

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let mut seen = Vec::new();
    for (cpu, gpu, rpm) in cases {
        let custom = Custom {
            cpu: CpuBoost::new(cpu, &BLADE_18).unwrap(),
            gpu: GpuBoost::new(gpu, &BLADE_18).unwrap(),
            fan: FanMode::Manual(Rpm::new(rpm, &BLADE_18).unwrap()),
        }
        .commands(&BLADE_18);
        let watched = device
            .run(&custom)
            .map_err(|error| error.to_string())
            .and_then(|()| watch_fans_for(&mut device, 40, false));
        let failed = watched.is_err();
        seen.push(((cpu, gpu, rpm), watched));
        if failed {
            break;
        }
    }

    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (case, watched) in &seen {
        match watched {
            Ok(reads) => {
                for (step, (target, real)) in reads.iter().enumerate() {
                    eprintln!(
                        "cpu, gpu, rpm {case:?} {:>2} s: target {target:02X?}, real {real:02X?}",
                        (step + 1) * 5
                    );
                }
            }
            Err(error) => eprintln!("cpu, gpu, rpm {case:?}: {error}"),
        }
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    assert_eq!(seen.len(), cases.len());
    for (_, watched) in seen {
        watched.unwrap();
    }
}

#[test]
#[ignore = "needs the Blade 18, and runs the fans on manual for about 3.5 min"]
fn what_frees_a_manual_fan_stalled_after_a_custom_from_auto() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, PerfMode, Rpm};

    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    // The user's Custom (ticket 173): from auto it stalls near 2900 RPM
    // until the next Custom send.
    let custom = Custom {
        cpu: CpuBoost::new(3, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(3, &BLADE_18).unwrap(),
        fan: FanMode::Manual(Rpm::new(3600, &BLADE_18).unwrap()),
    }
    .commands(&BLADE_18);
    let targets = custom[custom.len() - FANS.len()..].to_vec();
    assert!(targets.iter().all(|command| command.id == SET_FAN));
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    // What goes again, and after how many seconds.
    let cases = [
        ("fan targets after 2 s", &targets, 2),
        ("full Custom after 2 s", &custom, 2),
        ("fan targets after 30 s", &targets, 30),
    ];

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let mut seen = Vec::new();
    for (name, again, after) in cases {
        let watched = device
            .run(&performance)
            .and_then(|()| {
                std::thread::sleep(Duration::from_secs(10));
                device.run(&custom)
            })
            .map_err(|error| error.to_string())
            .and_then(|()| {
                std::thread::sleep(Duration::from_secs(after));
                device.run(again).map_err(|error| error.to_string())
            })
            .and_then(|()| watch_fans_for(&mut device, 60 - after, false));
        let failed = watched.is_err();
        seen.push((name, after, watched));
        if failed {
            break;
        }
    }

    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (name, sent_at, watched) in &seen {
        match watched {
            Ok(reads) => {
                for (step, (target, real)) in reads.iter().enumerate() {
                    eprintln!(
                        "{name} {:>2} s: target {target:02X?}, real {real:02X?}",
                        sent_at + (step as u64 + 1) * 5
                    );
                }
            }
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    assert_eq!(seen.len(), cases.len());
    for (_, _, watched) in seen {
        watched.unwrap();
    }
}

/// The CPU temperature at or above which the load test stops at once. Read
/// each second: the first run climbed 16 °C in 5 s at every core.
const LOAD_STOP_CELSIUS: u8 = 95;
/// The CPU temperature under which the load test puts the state back.
const COOL_CELSIUS: u8 = 70;

/// Fan targets, real speeds and the CPU temperature, one read.
type LoadRead = (Vec<Vec<u8>>, Vec<Vec<u8>>, u8);

/// Reads the CPU temperature each second for `seconds`, and the fan targets
/// and real speeds each 5 s, kept with it in `seen`. Stops with an error at
/// [`LOAD_STOP_CELSIUS`] or a CPU temperature that can't be read.
fn watch_under_load<C: Clock>(
    device: &mut Device<HidTransport, C>,
    seconds: u64,
    seen: &mut Vec<LoadRead>,
) -> Result<(), String> {
    for second in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));
        let cpu = read_cpu(device)?;
        if cpu >= LOAD_STOP_CELSIUS {
            return Err(format!("CPU at {cpu} °C: stopped"));
        }
        if second % 5 == 0 {
            let target = try_read_each(device, GET_FAN, &FANS)?;
            let real = try_read_each(device, GET_FAN_RPM, &FANS)?;
            seen.push((target, real, cpu));
        }
    }
    Ok(())
}

/// Whether both fans' last read real speed is within 300 RPM of `rpm`,
/// each reply echoing its fan.
fn fans_near(seen: &[LoadRead], rpm: u16) -> bool {
    seen.last().is_some_and(|(_, real, _)| {
        real.len() == FANS.len()
            && real.iter().zip(FANS).all(|(args, fan)| {
                args.len() >= 3
                    && args[0] == 1
                    && args[1] == fan
                    && u16::from(args[2]) * 100 + 300 >= rpm
            })
    })
}

/// Whether the load test was asked for (`BLADE_LOAD_TEST=1`; it heats the
/// CPU, so not in every `cargo hw`). Panics, sending nothing, if not on AC.
fn load_test_asked_on_ac() -> bool {
    if std::env::var_os("BLADE_LOAD_TEST").is_none_or(|value| value != "1") {
        eprintln!("skipped: set BLADE_LOAD_TEST=1 to run it");
        return false;
    }
    assert_eq!(
        platform::on_battery(),
        Some(false),
        "not on AC (or unknown): not run"
    );
    true
}

/// After the load: keeps the fans as they are until the CPU is under
/// [`COOL_CELSIUS`] (or 60 s), as the state put back may be a slow manual
/// fan. Hot is no reason to stop here; only a failed read is.
fn cool_down<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<LoadRead> {
    let mut cooling = Vec::new();
    for second in 1..=60 {
        std::thread::sleep(Duration::from_secs(1));
        let Ok(cpu) = read_cpu(device) else {
            break;
        };
        if second % 5 == 0 || cpu < COOL_CELSIUS {
            let reads = try_read_each(device, GET_FAN, &FANS)
                .and_then(|target| Ok((target, try_read_each(device, GET_FAN_RPM, &FANS)?)));
            let Ok((target, real)) = reads else {
                break;
            };
            cooling.push((target, real, cpu));
        }
        if cpu < COOL_CELSIUS {
            break;
        }
    }
    cooling
}

/// The CPU temperature now, in °C.
fn read_cpu<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<u8, String> {
    use razer_proto::Celsius;

    let query = Celsius::query();
    let reply = device
        .send(query.id, &query.args)
        .map_err(|error| error.to_string())?;
    Ok(Celsius::cpu_from_reply(reply.args())
        .ok_or("the CPU temperature can't be read: stopped")?
        .0)
}

/// Busy threads that stop, and are waited for, when this is dropped, also
/// on a panic.
struct Load {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Load {
    /// `cores` threads, each spinning until stopped.
    fn start(cores: usize) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let threads = (0..cores)
            .map(|_| {
                let stop = std::sync::Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut x = 0_u64;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        x = std::hint::black_box(
                            x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
                        );
                    }
                })
            })
            .collect();
        Self { stop, threads }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[test]
#[ignore = "needs the Blade 18 on AC and BLADE_LOAD_TEST=1; loads half the CPU cores for about 5 min"]
fn the_sleep_and_wake_sends_under_load() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, PerfMode, Rpm};

    if !load_test_asked_on_ac() {
        return;
    }
    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    // A stand-in for laptop check 34 (a lid close mid-game): the fan guard's
    // full speed, the quit preset the app sends at a sleep notice, and the
    // full speed again at the wake with its fan targets once more (tickets
    // 169, 172, 173). The fans are never manual below full speed under load.
    // Every core at CPU boost 3 passed 95 °C in 10 s while the fans still
    // climbed (2026-11-12): so boost 1, half the cores, and the
    // fans at full speed before the load.
    let custom = |fan| Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan,
    };
    let full = custom(FanMode::Manual(
        Rpm::new(BLADE_18.custom.fan_rpm.max, &BLADE_18).unwrap(),
    ));
    let mut sleep = custom(FanMode::Auto).commands(&BLADE_18);
    sleep.extend(PerfMode::Balanced.commands(&BLADE_18).unwrap());

    // The fans at full speed first, with no load, with the fan targets once
    // more 2 s after the Custom (from auto they stall near 2900, ticket 173).
    // The load starts only once both fans run within 300 RPM of full.
    let mut warm_up = Vec::new();
    let top = BLADE_18.custom.fan_rpm.max;
    let warmed = device
        .run(&full.commands(&BLADE_18))
        .map_err(|error| error.to_string())
        .and_then(|()| watch_under_load(&mut device, 2, &mut warm_up))
        .and_then(|()| {
            device
                .run(&full.fan_target_commands(&BLADE_18))
                .map_err(|error| error.to_string())
        })
        .and_then(|()| watch_under_load(&mut device, 90, &mut warm_up))
        .and_then(|()| {
            if fans_near(&warm_up, top) {
                Ok(())
            } else {
                Err("the fans did not reach full speed: no load started".to_owned())
            }
        });

    // From here on every step keeps its result; the load stops and the
    // state is put back before any assert.
    let mut phases: Vec<(&str, Vec<LoadRead>)> = vec![("warm-up, no load", warm_up)];
    let result = warmed.and_then(|()| {
        // Heat: half the cores busy, stopped when `_load` is dropped.
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get) / 2;
        let _load = Load::start(cores.max(1));
        let mut run = |name,
                       commands: &[razer_proto::Command],
                       seconds,
                       device: &mut Device<HidTransport, SystemClock>| {
            let mut seen = Vec::new();
            let result = device
                .run(commands)
                .map_err(|error| error.to_string())
                .and_then(|()| watch_under_load(device, seconds, &mut seen));
            phases.push((name, seen));
            result
        };
        run(
            "full speed, loaded",
            &full.commands(&BLADE_18),
            60,
            &mut device,
        )?;
        run("sleep's quit preset", &sleep, 30, &mut device)?;
        // The wake's Custom, then its fan targets 2 s later, as the app.
        run(
            "wake's full speed",
            &full.commands(&BLADE_18),
            2,
            &mut device,
        )?;
        run(
            "fan targets again",
            &full.fan_target_commands(&BLADE_18),
            30,
            &mut device,
        )
    });

    phases.push(("cooling", cool_down(&mut device)));
    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (name, seen) in &phases {
        for (step, (target, real, cpu)) in seen.iter().enumerate() {
            eprintln!(
                "{name} {:>2} s: target {target:02X?}, real {real:02X?}, CPU {cpu} °C",
                (step + 1) * 5
            );
        }
    }
    eprintln!("result {result:?}\nput back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    result.unwrap();
}

/// A state [`custom_state`] could read: Balanced on auto in each zone,
/// boosts 1, fans 3500 RPM.
fn sane_state() -> Vec<Vec<u8>> {
    let mut state: Vec<Vec<u8>> = (1..=4).map(|zone| vec![1, zone, 0x00, 0x00]).collect();
    state.extend([vec![1, 1, 1], vec![1, 2, 1], vec![1, 1, 35], vec![1, 2, 35]]);
    state
}

/// `sane_state` with one reply's args replaced.
fn state_with(index: usize, args: &[u8]) -> Vec<Vec<u8>> {
    let mut state = sane_state();
    state[index] = args.to_vec();
    state
}

#[test]
fn a_sane_state_is_put_back() {
    refuse_unknown_state(&sane_state());
    refuse_unknown_state(&state_with(0, &[1, 1, 0x04, 0x01]));
    refuse_unknown_state(&state_with(3, &[1, 4, 0x05, 0x00]));
    refuse_unknown_state(&state_with(4, &[1, 1, 3]));
    refuse_unknown_state(&state_with(5, &[1, 2, 0]));
}

#[test]
#[should_panic(expected = "not run")]
fn a_short_state_is_refused() {
    refuse_unknown_state(&sane_state()[..7]);
}

#[test]
#[should_panic(expected = "zone 2 mode reply")]
fn a_mode_reply_for_another_zone_is_refused() {
    refuse_unknown_state(&state_with(1, &[1, 1, 0x00, 0x00]));
}

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn an_unknown_mode_byte_is_refused() {
    refuse_unknown_state(&state_with(0, &[1, 1, 0x07, 0x00]));
}

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn an_unknown_fan_byte_is_refused() {
    refuse_unknown_state(&state_with(0, &[1, 1, 0x00, 0x02]));
}

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn a_mode_reply_of_another_length_is_refused() {
    refuse_unknown_state(&state_with(0, &[1, 1, 0x00]));
}

#[test]
#[should_panic(expected = "cluster 1 boost reply")]
fn a_boost_reply_for_another_cluster_is_refused() {
    refuse_unknown_state(&state_with(4, &[1, 2, 1]));
}

#[test]
#[should_panic(expected = "cluster 1 boost reply")]
fn a_cpu_boost_above_the_model_is_refused() {
    refuse_unknown_state(&state_with(4, &[1, 1, 4]));
}

#[test]
#[should_panic(expected = "cluster 2 boost reply")]
fn a_gpu_boost_above_the_model_is_refused() {
    refuse_unknown_state(&state_with(5, &[1, 2, 4]));
}

#[test]
#[should_panic(expected = "cluster 2 boost reply")]
fn a_boost_reply_of_another_length_is_refused() {
    refuse_unknown_state(&state_with(5, &[1, 2, 1, 0]));
}

#[test]
#[should_panic(expected = "outside the model's range")]
fn a_fan_target_outside_the_range_is_still_refused() {
    refuse_unknown_state(&state_with(6, &[1, 1, 0]));
}
