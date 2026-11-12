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

    // Boosts and fan targets first, as razer-laptop-control says a mode
    // write after them resets them (not seen on 1.6, ticket 144).
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
    // The fans go to the target already set: refuse one the model does
    // not take, such as 0, which would stop them.
    let range = BLADE_18.custom.fan_rpm;
    for args in &before[6..8] {
        let rpm = u16::from(args[2]) * 100;
        assert!(
            (range.min..=range.max).contains(&rpm),
            "fan target {rpm} RPM is outside the model's range: not run"
        );
    }

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

/// Refuses to run when a fan target read before the test is outside the
/// model's range, such as 0, which would stop the fans: the put-back
/// writes it while the fans are still manual.
fn refuse_targets_outside_the_range(before: &[Vec<u8>]) {
    let range = razer_proto::BLADE_18.custom.fan_rpm;
    for args in &before[6..8] {
        let rpm = u16::from(args[2]) * 100;
        assert!(
            (range.min..=range.max).contains(&rpm),
            "fan target {rpm} RPM is outside the model's range: not run"
        );
    }
}

/// Puts back what [`custom_state`] read, as the Custom test does: boosts and
/// fan targets, then modes. Every send is tried, so a failed one does not
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
    refuse_targets_outside_the_range(&before);
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
    refuse_targets_outside_the_range(&before);
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
    refuse_targets_outside_the_range(&before);
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
    refuse_targets_outside_the_range(&before);
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
