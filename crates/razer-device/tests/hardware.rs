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
    refuse_unknown_effect(&before);

    let painted = device.run(&razer_proto::static_colour(
        razer_proto::Rgb::new(0xFF, 0, 0),
        &razer_proto::BLADE_18,
    ));
    // From here on every step keeps its result, and the effect is put back
    // before any unwrap.
    let during = device
        .send(GET_EFFECT, &query)
        .map(|reply| reply.args().to_vec());
    std::thread::sleep(Duration::from_secs(5));

    let restored = device.send(SET_EFFECT, &before).map(|_| ());
    let after = device
        .send(GET_EFFECT, &query)
        .map(|reply| reply.args().to_vec());
    eprintln!(
        "effect before {before:02X?}, painted {painted:?}, during {during:02X?}, restored {restored:?}, after {after:02X?}"
    );
    restored.unwrap();
    assert_eq!(after.unwrap(), before);
    painted.unwrap();
    during.unwrap();
}

/// Refuses to run when the effect reply read before the colour test is not
/// one the put-back can safely send as `0F02`: it must echo storage 1 and
/// the keyboard LED, with as many args as the query. The effect bytes after
/// them are the controller's own, sent back to the same LED unchecked.
fn refuse_unknown_effect(args: &[u8]) {
    assert!(
        args.len() == EFFECT_ARGS && args.starts_with(&[1, 5]),
        "effect reply {args:02X?} not understood: not run"
    );
}

/// Reads each zone's preset args, as the controller sends them back.
fn preset_args<C: Clock>(device: &mut Device<HidTransport, C>) -> Vec<Vec<u8>> {
    try_preset_args(device).unwrap()
}

/// What [`preset_args`] reads, keeping the first error.
fn try_preset_args<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<Vec<Vec<u8>>, String> {
    razer_proto::PerfMode::queries(&razer_proto::BLADE_18)
        .iter()
        .map(|query| {
            device
                .send(query.id, &query.args)
                .map(|reply| reply.args().to_vec())
                .map_err(|error| error.to_string())
        })
        .collect()
}

#[test]
#[ignore = "needs the Blade 18"]
fn each_preset_is_set_and_read_back_and_the_old_one_restored() {
    use razer_proto::PerfMode;

    let mut device = Device::new(open(), SystemClock);
    let before = preset_args(&mut device);
    // The put-back writes the replies as read.
    refuse_unknown_modes(&before);

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

/// One line of the GPU temperature probe: the GPU's power state (a read
/// that does not wake it), the raw `0D85` reply args and NVML's reading.
fn probe_gpu_line<C: Clock>(
    device: &mut Device<HidTransport, C>,
    sensors: &mut platform::WindowsSensors,
    started: std::time::Instant,
    with_nvml: bool,
) -> String {
    use platform::Sensors;
    use razer_proto::Celsius;

    let awake = sensors.gpu_awake();
    let query = Celsius::query();
    let reply = device
        .send(query.id, &query.args)
        .map(|reply| reply.args().iter().take(4).copied().collect::<Vec<_>>());
    let nvml = with_nvml.then(|| sensors.gpu());
    format!(
        "{:>5.1} s  awake {awake:?}  0D85 {reply:02X?}  nvml {nvml:?}",
        started.elapsed().as_secs_f32()
    )
}

/// Whether the GPU probes were asked for (`BLADE_GPU_PROBE=1`; they wake the
/// GPU and take minutes, so not in every `cargo hw`).
fn gpu_probe_asked() -> bool {
    let asked = std::env::var_os("BLADE_GPU_PROBE").is_some_and(|value| value == "1");
    if !asked {
        eprintln!("skipped: set BLADE_GPU_PROBE=1 to run it");
    }
    asked
}

/// Ticket 193: does an NVML read on an open handle wake a sleeping GPU?
/// Starts NVML (a wake), waits up to 30 s for the GPU to sleep, then for
/// 60 s reads it through NVML each second and its power state (no wake)
/// ten times a second. Asserts no awake power state after the first sleep.
/// The controller is not used, but the lock keeps the app from reading too.
#[test]
#[ignore = "needs the Blade 18 and BLADE_GPU_PROBE=1; wakes the GPU about 2 s; about 95 s"]
fn an_nvml_read_does_not_wake_a_sleeping_gpu() {
    use platform::Sensors;

    if !gpu_probe_asked() {
        return;
    }

    hold_lock();
    let mut sensors = platform::WindowsSensors::default();
    eprintln!("start: {:?}", sensors.gpu());
    let started = std::time::Instant::now();
    while sensors.gpu_awake().unwrap() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the GPU stays awake"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("asleep after {:?}", started.elapsed());
    let mut wakes = Vec::new();
    for second in 0..60 {
        let read = sensors.gpu();
        for tenth in 0..10 {
            let awake = sensors.gpu_awake();
            if !matches!(awake, Ok(false)) {
                wakes.push((second, tenth, awake.map_err(|error| error.to_string())));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("{second:>2} s  nvml {read:?}");
    }
    assert!(wakes.is_empty(), "not asleep at (s, tenth): {wakes:?}");
}

/// Ticket 193: does the second field of the `0D85` reply follow the NVIDIA
/// GPU? Only reads: no command here changes the controller. 20 s with the
/// GPU left alone (no NVML), 20 s with NVML open and read each second (its
/// start wakes the GPU), then NVML closed and 40 s more with no NVML, while
/// the GPU falls asleep again.
#[test]
#[ignore = "needs the Blade 18 and BLADE_GPU_PROBE=1; wakes the GPU for about 25 s; about 80 s"]
fn the_second_temperature_field_follows_the_gpu() {
    if !gpu_probe_asked() {
        return;
    }
    let mut device = Device::new(open(), SystemClock);
    let started = std::time::Instant::now();
    let phases = [("left alone", 20, false), ("NVML open", 20, true)];
    let mut sensors = platform::WindowsSensors::default();
    for (name, seconds, with_nvml) in phases {
        eprintln!("-- {name}");
        for _ in 0..seconds {
            let line = probe_gpu_line(&mut device, &mut sensors, started, with_nvml);
            eprintln!("{line}");
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    drop(sensors);
    eprintln!("-- NVML closed");
    let mut sensors = platform::WindowsSensors::default();
    for _ in 0..40 {
        let line = probe_gpu_line(&mut device, &mut sensors, started, false);
        eprintln!("{line}");
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "needs the Blade 18"]
fn the_key_reports_open_and_wait_quietly() {
    // Only read, so the one-copy lock is not needed.
    let mut reports = HidKeyReports::open(&razer_proto::BLADE_18).unwrap();
    // Only the profile's collection is opened, so the reads can wait long
    // (ticket 209).
    let path = reports.read_path().unwrap_or_default().to_ascii_uppercase();
    assert!(path.contains("&MI_01&COL04#"), "{path}");

    // No key is pressed, so nothing comes, or only a key-up report.
    let report = reports.read(Duration::from_millis(100)).unwrap();

    if let Some(report) = report {
        let key = razer_proto::BLADE_18.power_key.unwrap();
        assert_ne!(key.held(&report), Some(true), "{report:02X?}");
    }
}

/// Ticket 202: which key collection sends which key? Opens each of the
/// model's interface 1 collections and prints each report with its
/// collection for 60 s, while the user presses keys. It reads them raw, not
/// through [`HidKeyReports`], so it also shows reports that are not key
/// reports, such as Col05's `05` at each USB wake (ticket 203). Only reads
/// the keyboard: nothing is sent, so the lock is not needed
/// (`BLADE_KEY_PROBE=1`, with `--nocapture`).
#[test]
#[ignore = "needs the Blade 18, BLADE_KEY_PROBE=1 and a person pressing keys; 60 s"]
fn each_key_collection_prints_its_reports() {
    if std::env::var_os("BLADE_KEY_PROBE").is_none_or(|value| value != "1") {
        eprintln!("skipped: set BLADE_KEY_PROBE=1 to run it");
        return;
    }
    let readers: Vec<(String, platform::HidDevice)> = platform::hid_paths()
        .unwrap()
        .into_iter()
        .filter(|path| path.to_uppercase().contains("VID_1532&PID_02C7&MI_01&COL"))
        .filter_map(|path| {
            let device = platform::HidDevice::open(&path).ok()?;
            let tag = path.to_uppercase().split('#').nth(1)?.to_owned();
            Some((tag, device))
        })
        .collect();
    assert!(readers.len() > 1, "fewer than two key collections opened");
    for (tag, _) in &readers {
        eprintln!("reading {tag}");
    }
    eprintln!("press keys for 60 s");
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        for (tag, device) in &readers {
            let mut buffer = [0; 64];
            let read = device.read(&mut buffer, Duration::from_millis(10)).unwrap();
            if read > 0 {
                let at = started.elapsed().as_millis();
                eprintln!("{at:>6} ms  {tag}  {:02X?}", &buffer[..read]);
            }
        }
    }
}

/// What [`custom_state`] reads, keeping the first error.
fn try_custom_state<C: Clock>(
    device: &mut Device<HidTransport, C>,
) -> Result<Vec<Vec<u8>>, String> {
    let mut state = try_preset_args(device)?;
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

/// Refuses to run when the mode replies read before a test are not ones
/// the put-back can safely write: one per zone, each echoing its zone, with
/// a preset the model offers on auto fans, or Custom on auto or manual fans
/// (a manual fan byte only with Custom, as the app and the open-source
/// tools send it).
fn refuse_unknown_modes(modes: &[Vec<u8>]) {
    use razer_proto::{BLADE_18, PerfMode};

    let zones = BLADE_18.perf_zones;
    assert!(
        modes.len() == zones.len(),
        "mode replies {modes:02X?} not understood: not run"
    );
    let custom = BLADE_18.custom.mode;
    for (args, &zone) in modes.iter().zip(zones) {
        let preset = PerfMode::from_reply(&BLADE_18, zone, args).is_some();
        assert!(
            matches!(args[..], [1, echoed, mode, fan] if echoed == zone
                && ((preset && fan == FAN_AUTO)
                    || (mode == custom && matches!(fan, FAN_AUTO | FAN_MANUAL)))),
            "zone {zone} mode reply {args:02X?} not understood: not run"
        );
    }
}

/// The mode writes that put back what the battery probe read before it
/// sent Balanced (ticket 211): the replies as read when each zone holds one
/// of the model's battery presets with fans auto, else Balanced with fans
/// auto, as the app sends it on battery. So an unknown mode such as `06`,
/// a preset the app never runs on battery, and manual fans (whose targets
/// the probe does not read) are never written back.
fn modes_to_put_back(before: &[Vec<u8>]) -> Vec<Vec<u8>> {
    use razer_proto::{BLADE_18, PerfMode};

    let zones = BLADE_18.perf_zones;
    let battery_preset = |(args, &zone): (&Vec<u8>, &u8)| {
        args.get(3) == Some(&FAN_AUTO)
            && PerfMode::from_reply(&BLADE_18, zone, args)
                .is_some_and(|preset| BLADE_18.battery_presets.contains(&preset))
    };
    if before.len() == zones.len() && before.iter().zip(zones).all(battery_preset) {
        return before.to_vec();
    }
    PerfMode::Balanced
        .commands(&BLADE_18)
        .unwrap()
        .into_iter()
        .map(|command| command.args)
        .collect()
}

/// Refuses to run when the state read before the test is not one the
/// put-back can safely write: the modes as [`refuse_unknown_modes`] takes
/// them, each boost reply must echo its cluster and hold a level the model
/// takes, and each fan target must be in the model's range (0 would stop
/// the fans: the put-back writes it while the fans are still manual).
fn refuse_unknown_state(before: &[Vec<u8>]) {
    use razer_proto::{BLADE_18, CpuBoost, GpuBoost};

    let zones = BLADE_18.perf_zones;
    // The callers and the put-back slice the state as 4 modes, 2 boosts
    // and 2 fan targets.
    assert!(
        zones.len() == 4 && before.len() == zones.len() + CLUSTERS.len() + FANS.len(),
        "state {before:02X?} not understood: not run"
    );
    refuse_unknown_modes(&before[..4]);
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
        let [1, echoed, hundreds] = args[..] else {
            panic!("fan {fan} target reply {args:02X?} not understood: not run");
        };
        assert!(
            echoed == fan,
            "fan {fan} target reply {args:02X?} not understood: not run"
        );
        let rpm = u16::from(hundreds) * 100;
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

/// The mode replies, fan targets, real speeds and the CPU temperature, one
/// read.
type ModeRead = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>, u8);

/// Reads the modes, fan targets, real speeds and the CPU temperature each
/// 5 s for `seconds`, kept in `seen`. Stops with an error on a failed read,
/// a fan reply for another fan, or the CPU at [`LOAD_STOP_CELSIUS`].
fn watch_modes_and_fans<C: Clock>(
    device: &mut Device<HidTransport, C>,
    seconds: u64,
    seen: &mut Vec<ModeRead>,
) -> Result<(), String> {
    for _ in 0..seconds / 5 {
        std::thread::sleep(Duration::from_secs(5));
        let modes = try_preset_args(device)?;
        let target = try_read_each(device, GET_FAN, &FANS)?;
        let real = try_read_each(device, GET_FAN_RPM, &FANS)?;
        let cpu = read_cpu(device)?;
        seen.push((modes, target.clone(), real.clone(), cpu));
        let echoed = |args: &[Vec<u8>]| {
            args.iter()
                .zip(FANS)
                .all(|(args, fan)| args.starts_with(&[1, fan]))
        };
        if !echoed(&target) || !echoed(&real) {
            return Err("a fan reply for another fan: stopped".to_owned());
        }
        if cpu >= LOAD_STOP_CELSIUS {
            return Err(format!("CPU at {cpu} °C: stopped"));
        }
    }
    Ok(())
}

#[test]
#[ignore = "needs the Blade 18 on AC and idle, and runs the fans on manual for about 30 s; about 3 min"]
fn the_sleeps_quit_preset_leaves_the_fans_on_auto() {
    use razer_proto::{BLADE_18, CpuBoost, Custom, FanMode, GpuBoost, PerfMode, Rpm};

    // Ticket 218: in laptop check 33 (2026-11-15) the fans ran at about
    // 1600 RPM in a Start-menu sleep after Custom 3600 on AC. This sends
    // what the app sends around such a sleep, with no app and no real
    // sleep, and reads whether the fan byte is auto after it.
    assert_eq!(
        platform::on_battery(),
        Some(false),
        "not on AC (or unknown): not run"
    );
    let mut device = Device::new(open(), SystemClock);
    let before = custom_state(&mut device);
    refuse_unknown_state(&before);
    let custom = |fan| Custom {
        cpu: CpuBoost::new(1, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(1, &BLADE_18).unwrap(),
        fan,
    };
    let manual = custom(FanMode::Manual(Rpm::new(3600, &BLADE_18).unwrap()));
    // The quit preset on AC with a manual Custom: Custom on auto (boosts
    // kept), then Balanced (`quit_preset` in the app).
    let mut sleep = custom(FanMode::Auto).commands(&BLADE_18);
    sleep.extend(PerfMode::Balanced.commands(&BLADE_18).unwrap());

    // From here on every step keeps its result, and the state is put back
    // before any assert.
    let mut manual_seen = Vec::new();
    let mut sleep_seen = Vec::new();
    // The Custom click, with its fan targets again 2 s later (ticket 173).
    let result = device
        .run(&manual.commands(&BLADE_18))
        .and_then(|()| {
            std::thread::sleep(Duration::from_secs(2));
            device.run(&manual.fan_target_commands(&BLADE_18))
        })
        .map_err(|error| error.to_string())
        .and_then(|()| watch_modes_and_fans(&mut device, 30, &mut manual_seen))
        .and_then(|()| device.run(&sleep).map_err(|error| error.to_string()))
        .and_then(|()| watch_modes_and_fans(&mut device, 120, &mut sleep_seen));

    let put_back = put_back_custom(&mut device, &before);
    let after = custom_state(&mut device);

    eprintln!("before {before:02X?}");
    for (name, seen) in [
        ("manual 3600", &manual_seen),
        ("after the sleep's quit preset", &sleep_seen),
    ] {
        for (step, (modes, target, real, cpu)) in seen.iter().enumerate() {
            eprintln!(
                "{name} {:>3} s: modes {modes:02X?}, target {target:02X?}, real {real:02X?}, CPU {cpu} °C",
                (step + 1) * 5
            );
        }
    }
    eprintln!("result {result:?}\nput back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    assert_eq!(after, before);
    result.unwrap();
    // The manual phase took: every zone Custom with the fan byte manual,
    // both targets 3600 RPM. Else the reads after prove nothing.
    for (modes, target, ..) in &manual_seen {
        for (args, &zone) in modes.iter().zip(BLADE_18.perf_zones) {
            assert_eq!(
                args[..],
                [1, zone, BLADE_18.custom.mode, FAN_MANUAL],
                "{args:02X?}"
            );
        }
        for (args, fan) in target.iter().zip(FANS) {
            assert_eq!(args[..], [1, fan, 36], "{args:02X?}");
        }
    }
    assert_eq!(sleep_seen.len(), 24);
    // Each read after the quit preset: every zone Balanced, fan byte auto.
    for (modes, ..) in &sleep_seen {
        for (args, &zone) in modes.iter().zip(BLADE_18.perf_zones) {
            assert_eq!(
                PerfMode::from_reply(&BLADE_18, zone, args),
                Some(PerfMode::Balanced),
                "{args:02X?}"
            );
            assert_eq!(args.get(3), Some(&FAN_AUTO), "{args:02X?}");
        }
    }
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

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn a_manual_fan_byte_outside_custom_is_refused() {
    refuse_unknown_state(&state_with(0, &[1, 1, 0x00, 0x01]));
}

#[test]
#[should_panic(expected = "fan 1 target reply")]
fn a_fan_target_reply_of_another_length_is_refused() {
    refuse_unknown_state(&state_with(6, &[1, 1, 35, 0]));
}

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn a_mode_reply_of_another_storage_is_refused() {
    refuse_unknown_state(&state_with(0, &[0, 1, 0x00, 0x00]));
}

#[test]
#[should_panic(expected = "zone 4 mode reply")]
fn a_later_zone_that_does_not_echo_is_refused() {
    refuse_unknown_state(&state_with(3, &[1, 3, 0x00, 0x00]));
}

#[test]
#[should_panic(expected = "cluster 2 boost reply")]
fn a_boost_reply_of_another_storage_is_refused() {
    refuse_unknown_state(&state_with(5, &[0, 2, 1]));
}

#[test]
#[should_panic(expected = "fan 1 target reply")]
fn a_fan_target_reply_for_another_fan_is_refused() {
    refuse_unknown_state(&state_with(6, &[1, 2, 35]));
}

#[test]
fn the_preset_test_puts_back_only_known_modes() {
    refuse_unknown_modes(&sane_state()[..4]);
}

#[test]
#[should_panic(expected = "zone 1 mode reply")]
fn the_preset_test_refuses_an_unknown_mode() {
    let mut modes = sane_state()[..4].to_vec();
    modes[0] = vec![1, 1, 0x07, 0x00];
    refuse_unknown_modes(&modes);
}

#[test]
#[should_panic(expected = "not run")]
fn the_preset_test_refuses_a_missing_zone() {
    refuse_unknown_modes(&sane_state()[..3]);
}

/// The mode write of Balanced with fans auto in each zone, as the app sends it.
fn balanced_writes() -> Vec<Vec<u8>> {
    (1..=4).map(|zone| vec![1, zone, 0x00, 0x00]).collect()
}

#[test]
fn the_battery_probe_puts_back_battery_presets_as_read() {
    let mut modes = balanced_writes();
    modes[0] = vec![1, 1, 0x05, 0x00];

    assert_eq!(modes_to_put_back(&modes), modes);
}

/// The app never runs Performance or Custom on battery.
#[test]
fn the_battery_probe_puts_back_balanced_over_presets_not_run_on_battery() {
    for args in [[1, 4, 0x02, 0x00], [1, 4, 0x04, 0x00]] {
        let mut modes = balanced_writes();
        modes[3] = args.to_vec();

        assert_eq!(modes_to_put_back(&modes), balanced_writes(), "{args:02X?}");
    }
}

#[test]
fn the_battery_probe_puts_back_balanced_over_mode_06() {
    let mut modes = balanced_writes();
    modes[2] = vec![1, 3, 0x06, 0x00];

    assert_eq!(modes_to_put_back(&modes), balanced_writes());
}

/// The probe reads no fan targets, and its Balanced write may reset them,
/// so manual fans are never written back.
#[test]
fn the_battery_probe_puts_back_balanced_over_a_manual_fan() {
    let mut modes = balanced_writes();
    modes[3] = vec![1, 4, 0x04, 0x01];

    assert_eq!(modes_to_put_back(&modes), balanced_writes());
}

#[test]
fn the_battery_probe_puts_back_balanced_over_a_missing_zone() {
    assert_eq!(
        modes_to_put_back(&balanced_writes()[..3]),
        balanced_writes()
    );
}

/// An effect reply as the colour test could read it: storage 1, keyboard
/// LED 5, then the effect bytes.
fn effect_with(start: &[u8]) -> Vec<u8> {
    let mut args = vec![0; EFFECT_ARGS];
    args[..2].copy_from_slice(&[1, 5]);
    args[..start.len()].copy_from_slice(start);
    args
}

#[test]
fn a_keyboard_effect_reply_is_not_refused() {
    refuse_unknown_effect(&effect_with(&[]));
}

#[test]
#[should_panic(expected = "effect reply")]
fn an_effect_reply_of_another_storage_is_refused() {
    refuse_unknown_effect(&effect_with(&[0, 5]));
}

#[test]
#[should_panic(expected = "effect reply")]
fn an_effect_reply_for_another_led_is_refused() {
    refuse_unknown_effect(&effect_with(&[1, 4]));
}

#[test]
#[should_panic(expected = "effect reply")]
fn a_short_effect_reply_is_refused() {
    refuse_unknown_effect(&effect_with(&[])[..EFFECT_ARGS - 1]);
}

#[test]
#[should_panic(expected = "effect reply")]
fn a_long_effect_reply_is_refused() {
    let mut args = effect_with(&[]);
    args.push(0);
    refuse_unknown_effect(&args);
}

#[test]
#[should_panic(expected = "effect reply")]
fn an_empty_effect_reply_is_refused() {
    refuse_unknown_effect(&[]);
}

/// The UTC time of day, as the app log and `tools/Watch-DevicePower.ps1` print it.
fn utc_now() -> String {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let day = since.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        day / 3600,
        day / 60 % 60,
        day % 60,
        since.subsec_millis()
    )
}

/// What [`the_state_holds_across_a_usb_sleep`] reads, all read-only: each
/// zone's mode, both boosts, both fan targets, the keyboard and vapor light
/// levels, and the keyboard effect (storage 1).
fn sleep_probe_state<C: Clock>(
    device: &mut Device<HidTransport, C>,
) -> Result<Vec<Vec<u8>>, String> {
    let mut state = try_custom_state(device)?;
    state.push(levels(device)?);
    let mut query = [0; EFFECT_ARGS];
    query[..2].copy_from_slice(&[1, 5]);
    let effect = device
        .send(GET_EFFECT, &query)
        .map_err(|error| error.to_string())?;
    state.push(effect.args().to_vec());
    Ok(state)
}

/// Ticket 204: does the controller keep its mode, fan mode, fan targets,
/// boosts and lights when the Razer USB device sleeps and wakes? Reads them
/// (read-only, nothing is set), then 6 times waits 12 s, more than 2 s after
/// the last read (the device sleeps 2 to 5 s after its last use, ticket 202),
/// reads them at once (the read wakes it) and 2 s later. Rounds are 14 s apart. Run `tools/Watch-DevicePower.ps1` beside it to
/// see the sleeps. Fails on any read that differs from the first.
#[test]
#[ignore = "needs the Blade 18 and BLADE_SLEEP_PROBE=1; read-only; about 90 s"]
fn the_state_holds_across_a_usb_sleep() {
    if !opted_in("BLADE_SLEEP_PROBE") {
        return;
    }
    let mut device = Device::new(open(), SystemClock);
    let first = sleep_probe_state(&mut device).unwrap();
    // A reply for another zone, cluster, fan or LED would pass later as
    // unchanged. Mode bytes are not checked: an unknown one is a finding.
    let echoes = razer_proto::BLADE_18
        .perf_zones
        .iter()
        .chain(&CLUSTERS)
        .chain(&FANS);
    for (args, &index) in first.iter().zip(echoes) {
        assert!(
            args.starts_with(&[1, index]),
            "reply {args:02X?} not for {index}: not run"
        );
    }
    refuse_unknown_effect(&first[9]);
    eprintln!("{}  first        {first:02X?}", utc_now());
    let mut changed = Vec::new();
    for round in 1..=6 {
        std::thread::sleep(Duration::from_secs(12));
        for label in ["at the wake ", "2 s later   "] {
            let at = utc_now();
            let read = sleep_probe_state(&mut device);
            eprintln!("{at}  {round} {label} {read:02X?}");
            if read.as_ref() != Ok(&first) {
                changed.push((at, read));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    assert!(
        changed.is_empty(),
        "first {first:02X?}, changed {changed:02X?}"
    );
}

/// Whether a probe that is not part of every `cargo hw` run was asked for
/// with `<name>=1`; says how to run it when not.
fn opted_in(name: &str) -> bool {
    let on = std::env::var_os(name).is_some_and(|value| value == "1");
    if !on {
        eprintln!("skipped: set {name}=1 to run it");
    }
    on
}

/// Ticket 211: when does the controller put mode `06` in the zones, and
/// does it go back when the charger is plugged in again? Read-only: reads
/// each zone's mode reply and Windows' power source each 2 s for 3 min,
/// and prints the first read and each change. Pull and plug the charger
/// during it. Fails on a failed read, or when it saw no battery and no AC.
#[test]
#[ignore = "needs the Blade 18 and BLADE_MODE_WATCH=1; read-only; 3 min"]
fn the_modes_follow_the_power_source() {
    if !opted_in("BLADE_MODE_WATCH") {
        return;
    }
    let mut device = Device::new(open(), SystemClock);
    let mut last = None;
    let mut failed = Vec::new();
    let mut sources = Vec::new();
    for _ in 0..90 {
        let at = utc_now();
        let read = (platform::on_battery(), try_preset_args(&mut device));
        if !sources.contains(&read.0) {
            sources.push(read.0);
        }
        if last.as_ref() != Some(&read) {
            eprintln!("{at}  on battery {:?}  modes {:02X?}", read.0, read.1);
        }
        if let Err(error) = &read.1 {
            failed.push((at, error.clone()));
        }
        last = Some(read);
        std::thread::sleep(Duration::from_secs(2));
    }
    assert!(failed.is_empty(), "failed reads {failed:?}");
    assert!(
        sources.contains(&Some(true)) && sources.contains(&Some(false)),
        "the charger was not pulled and plugged in: saw only {sources:?}"
    );
}

/// Ticket 211: does Balanced, sent on battery as the app sends it, hold, or
/// does the controller put mode `06` over it? Sends Balanced with fans auto
/// in each zone, reads the zones each 2 s for 60 s, then puts back the
/// modes read before only if [`unknown_modes`] takes them, else Balanced
/// (never an unknown mode such as `06`). Refuses to run off battery. Fails
/// on any read after the send that is not Balanced on auto.
#[test]
#[ignore = "needs the Blade 18 on battery and BLADE_BALANCED_PROBE=1; sends Balanced; about 70 s"]
fn balanced_holds_on_battery() {
    use razer_proto::{BLADE_18, PerfMode};

    if !opted_in("BLADE_BALANCED_PROBE") {
        return;
    }
    assert_eq!(
        platform::on_battery(),
        Some(true),
        "not on battery: not run"
    );
    let mut device = Device::new(open(), SystemClock);
    // A state it can't read is not sent to; a clean 06 is what it is for.
    let before = try_preset_args(&mut device)
        .unwrap_or_else(|error| panic!("modes not read ({error}): not run"));
    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();

    // From here on every step keeps its result, and the modes are put back
    // before any assert.
    let set = device.run(&balanced).map_err(|error| error.to_string());
    let mut seen = Vec::new();
    if set.is_ok() {
        for _ in 0..30 {
            std::thread::sleep(Duration::from_secs(2));
            seen.push((
                utc_now(),
                platform::on_battery(),
                try_preset_args(&mut device),
            ));
        }
    }
    let put_back: Vec<_> = modes_to_put_back(&before)
        .iter()
        .map(|args| {
            device
                .send(0x0D02, args)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .collect();
    let after = try_preset_args(&mut device);

    eprintln!("before {before:02X?}\nset {set:?}");
    for (at, on_battery, read) in &seen {
        eprintln!("{at}  on battery {on_battery:?}  modes {read:02X?}");
    }
    eprintln!("put back {put_back:?}\nafter {after:02X?}");
    // The lasting state first, so its error is never hidden.
    assert!(put_back.iter().all(Result::is_ok), "check the fans by hand");
    set.unwrap();
    let written: Vec<Vec<u8>> = balanced.into_iter().map(|command| command.args).collect();
    let other: Vec<_> = seen
        .iter()
        .filter(|(_, _, read)| read.as_ref() != Ok(&written))
        .collect();
    assert!(
        other.is_empty(),
        "not Balanced after the send: {other:02X?}"
    );
}

/// Prints the Razer USB device's power state and each of its HID
/// interfaces' `SelectiveSuspendEnabled` value, as each probe does first
/// (ticket 219's rule). Reads Windows only: the device is not opened.
fn print_usb_power_and_suspend(usb: razer_proto::UsbId) {
    match platform::usb_device_ids(usb.vendor_id, usb.product_id) {
        Ok(ids) => {
            for id in ids {
                eprintln!("power state of {id}: {:?}", platform::device_power(&id));
            }
        }
        Err(error) => eprintln!("the USB device's instances: {error}"),
    }
    let reads = platform::usb_interfaces(
        usb.vendor_id,
        usb.product_id,
        usb.hid_interfaces,
        "SelectiveSuspendEnabled",
    );
    for (number, read) in reads.iter().enumerate() {
        match read {
            Ok(instances) => {
                for instance in instances {
                    eprintln!(
                        "MI_{number:02X} SelectiveSuspendEnabled {:?} (driver {:?})",
                        instance.value, instance.service
                    );
                }
            }
            Err(error) => eprintln!("MI_{number:02X}: {error}"),
        }
    }
}

/// The logo as the app sets it (storage 0), and the other stored value
/// (storage 1), read-only.
fn logo_reads<C: Clock>(device: &mut Device<HidTransport, C>) -> Result<[Option<bool>; 2], String> {
    let set = razer_proto::BLADE_18
        .logo
        .expect("the Blade 18 has a lid logo");
    let stored = razer_proto::Logo { storage: 1, ..set };
    let mut read = |logo: razer_proto::Logo| {
        let query = logo.query();
        device
            .send(query.id, &query.args)
            .map(|reply| logo.from_reply(reply.args()))
            .map_err(|error| error.to_string())
    };
    Ok([read(set)?, read(stored)?])
}

/// The longest time awake with no sleep notice before a sleep probe ends.
const PROBE_AWAKE: Duration = Duration::from_mins(5);

/// The longest tick counted as awake, so the time asleep does not count.
const PROBE_TICK: Duration = Duration::from_secs(2);

/// The time a sleep probe has been awake since its start or the last sleep
/// notice, counted in ticks of at most [`PROBE_TICK`].
struct AwakeTime {
    awake: Duration,
    last_tick: std::time::Instant,
}

impl AwakeTime {
    fn new(now: std::time::Instant) -> Self {
        Self {
            awake: Duration::ZERO,
            last_tick: now,
        }
    }

    /// Counts the time since the last tick.
    fn tick(&mut self, now: std::time::Instant) {
        self.awake += now.duration_since(self.last_tick).min(PROBE_TICK);
        self.last_tick = now;
    }

    fn time(&self) -> Duration {
        self.awake
    }

    /// Starts counting again, at a sleep notice.
    fn restart(&mut self) {
        self.awake = Duration::ZERO;
    }
}

/// Registers for Windows' power events and passes each one on, with the
/// time it came (so a probe busy reading does not move it). Each sleep
/// notice is answered at once, so a probe never holds a sleep back.
fn answered_power_notices() -> std::sync::mpsc::Receiver<(std::time::Instant, platform::PowerEvent)>
{
    use platform::{PowerEvent, PowerEvents, WindowsPowerEvents};

    let mut events = WindowsPowerEvents::register().unwrap();
    let handled = events.suspend_handled();
    let (sender, notices) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Some(event) = events.next_event() {
            if event == PowerEvent::Suspending {
                handled.done(handled.number());
            }
            if sender.send((std::time::Instant::now(), event)).is_err() {
                return;
            }
        }
    });
    notices
}

/// When the logo is read after each wake notice.
const LOGO_READS_AFTER_WAKE: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_secs(5),
    Duration::from_secs(30),
];

/// Ticket 222 (b): does the lid logo, set off with storage 0 as the app
/// sets it, come on again by itself at a wake, and does it follow the other
/// stored value (storage 1)? Read-only: it never sends a set command.
///
/// Set the logo off in the app, quit the app from the tray, then start it
/// by its name only, never with `cargo hw` (that runs every hardware test,
/// also those that send, while you sleep the laptop):
///
/// `$env:BLADE_LOGO_PROBE=1; cargo test -p razer-device --test hardware the_lid_logo_across_a_sleep -- --ignored --nocapture --test-threads=1; Remove-Item Env:BLADE_LOGO_PROBE`
///
/// Never leave `BLADE_LOGO_PROBE` set. It prints the USB device's power
/// state and suspend values and Windows' power source, refuses to go on
/// unless the logo reads off, reads both values once, then answers each
/// sleep notice at once and reads both at once, 5 s and 30 s after each
/// wake notice. After a failed read it opens the controller again (a
/// hibernate may kill the handle). It ends after 5 min awake with no sleep
/// notice (counted in ticks of at most 2 s, so the time asleep does not
/// count). Its first read after a wake does not wait the app's 2 s pause
/// (`RESUME_PAUSE`); it only reads. Fails if no wake was seen or any read
/// gave the logo on.
#[test]
#[ignore = "needs the Blade 18, BLADE_LOGO_PROBE=1 and a person sleeping the laptop; read-only"]
fn the_lid_logo_across_a_sleep() {
    use platform::PowerEvent;
    use std::sync::mpsc;
    use std::time::Instant;

    if !opted_in("BLADE_LOGO_PROBE") {
        return;
    }
    print_usb_power_and_suspend(razer_proto::BLADE_18.usb);
    eprintln!("on battery: {:?}", platform::on_battery());
    let mut device = Device::new(open(), SystemClock);
    let first = logo_reads(&mut device).unwrap();
    eprintln!("{}  before the sleep  [set, stored] {first:?}", utc_now());
    assert_eq!(
        first[0],
        Some(false),
        "the logo does not read off: set it off in the app, quit it, and start again (this run is void)"
    );

    let notices = answered_power_notices();

    let mut wakes = 0;
    let mut turned_on = Vec::new();
    let mut woke: Option<Instant> = None;
    let mut next_read = LOGO_READS_AFTER_WAKE.len();
    // Whether this slot's read was made again on a new handle: once only.
    let mut read_again = false;
    let mut awake = AwakeTime::new(Instant::now());
    while awake.time() < PROBE_AWAKE {
        let notice = notices.recv_timeout(Duration::from_secs(1));
        let now = Instant::now();
        awake.tick(now);
        match notice {
            Ok((_, PowerEvent::Suspending)) => {
                eprintln!("{}  sleep notice (answered)", utc_now());
                awake.restart();
                woke = None;
            }
            Ok((_, PowerEvent::Resumed)) => {
                eprintln!("{}  wake notice", utc_now());
                wakes += 1;
                woke = Some(now);
                next_read = 0;
                read_again = false;
            }
            Ok((_, source)) => eprintln!("{}  {source:?}", utc_now()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the power events stopped"),
        }
        let Some(since) = woke.map(|at| now.duration_since(at)) else {
            continue;
        };
        if next_read >= LOGO_READS_AFTER_WAKE.len() || since < LOGO_READS_AFTER_WAKE[next_read] {
            continue;
        }
        let read = logo_reads(&mut device);
        eprintln!(
            "{}  {:.1} s after the wake  [set, stored] {read:?}",
            utc_now(),
            since.as_secs_f32()
        );
        match read {
            Ok(values) if values[0] == Some(true) => turned_on.push((utc_now(), values)),
            Ok(_) => {}
            Err(_) => match HidTransport::open(&razer_proto::BLADE_18) {
                Ok(transport) => {
                    device = Device::new(transport, SystemClock);
                    if !read_again {
                        eprintln!("opened the controller again; this read is made again on it");
                        read_again = true;
                        continue;
                    }
                    eprintln!("opened the controller again; the next reads use it");
                }
                Err(error) => eprintln!("could not open the controller again: {error}"),
            },
        }
        next_read += 1;
        read_again = false;
    }
    eprintln!("{}  5 min awake with no sleep notice: the end", utc_now());
    assert!(wakes > 0, "no wake was seen");
    assert!(turned_on.is_empty(), "the logo came on: {turned_on:?}");
}

/// How often the wake probes run their step after a wake notice.
const WAKE_READ_STEP: Duration = Duration::from_millis(100);

/// How long after a wake notice the wake probes run their step. Ends after
/// the app's pause (`RESUME_PAUSE`, 2 s), with time to spare.
const WAKE_READS_FOR: Duration = Duration::from_secs(3);

/// How long after a wake notice the wake probes go on when no step has
/// worked yet, so a late first answer is still timed.
const WAKE_READS_AT_MOST: Duration = Duration::from_secs(10);

/// The time after a wake notice of the next step after `since`. Steps are
/// [`WAKE_READ_STEP`] apart by the clock from the notice, so a slow step
/// skips the times it ran over.
fn next_wake_read(since: Duration) -> Duration {
    let steps = since.as_nanos() / WAKE_READ_STEP.as_nanos() + 1;
    WAKE_READ_STEP * u32::try_from(steps).unwrap_or(u32::MAX)
}

/// One step a wake probe runs on the controller after a wake notice, given
/// the wake's number (from 1): whether it worked, as the probe asks, and
/// what it saw; or its error.
type WakeStep<'a> =
    dyn FnMut(&mut Device<HidTransport, SystemClock>, u32) -> Result<(bool, String), String> + 'a;

/// Runs `step` on `device`, opening the controller again first if the last
/// step failed (`device` is `None`), and prints the step with its start
/// since the wake notice and how long it took. After a failed step
/// `device` is `None`: that handle is not used again. Returns whether the
/// step worked.
fn wake_step(
    device: &mut Option<Device<HidTransport, SystemClock>>,
    woke: std::time::Instant,
    number: u32,
    step: &mut WakeStep<'_>,
) -> bool {
    use std::time::Instant;

    if device.is_none() {
        match HidTransport::open(&razer_proto::BLADE_18) {
            Ok(transport) => {
                eprintln!(
                    "{}  {:>4} ms after the notice: opened the controller again",
                    utc_now(),
                    woke.elapsed().as_millis()
                );
                *device = Some(Device::new(transport, SystemClock));
            }
            Err(error) => {
                eprintln!(
                    "{}  {:>4} ms after the notice: could not open the controller again: {error}",
                    utc_now(),
                    woke.elapsed().as_millis()
                );
                return false;
            }
        }
    }
    let Some(open) = device.as_mut() else {
        return false;
    };
    let start = Instant::now();
    let done = step(open, number);
    let line = match &done {
        Ok((_, seen)) => seen.clone(),
        Err(error) => format!("error: {error}"),
    };
    eprintln!(
        "{}  start {:>4} ms after the notice, took {:>4} ms: {line}",
        utc_now(),
        start.duration_since(woke).as_millis(),
        start.elapsed().as_millis()
    );
    if done.is_err() {
        *device = None;
    }
    matches!(done, Ok((true, _)))
}

/// Ticket 219: when does the controller answer after a wake? The app waits
/// 2 s (`RESUME_PAUSE`) before it sends; that time was never measured.
/// Read-only: it reads only the firmware version and never sends a set
/// command.
///
/// Quit the app from the tray, then start the probe by its name only, never
/// with `cargo hw` (that runs every hardware test, also those that send,
/// while you sleep the laptop):
///
/// `$env:BLADE_WAKE_PROBE=1; cargo test -p razer-device --test hardware the_controller_answers_after_a_wake -- --ignored --nocapture --test-threads=1; Remove-Item Env:BLADE_WAKE_PROBE`
///
/// Never leave `BLADE_WAKE_PROBE` set. It prints the USB device's power
/// state and suspend values and Windows' power source, opens the controller
/// and keeps that handle over the sleep (as the app does), and reads the
/// firmware version once. Then it answers each sleep notice at once, and
/// after each wake notice reads at once and then each 100 ms by the clock
/// until 3 s after the notice, or until 10 s when no read has worked by 3 s
/// (each HID call of a read may take up to 3 s,
/// so one read can take 6 s or more: the printed time taken is the measure),
/// printing
/// each read's start, time taken and reply or error. After a failed read it
/// opens the controller again and reads on the new handle. It ends after
/// 5 min awake with no sleep notice (counted in ticks of at most 2 s, so the
/// time asleep does not count). Fails if no wake was seen.
#[test]
#[ignore = "needs the Blade 18, BLADE_WAKE_PROBE=1 and a person sleeping the laptop; read-only"]
fn the_controller_answers_after_a_wake() {
    if !opted_in("BLADE_WAKE_PROBE") {
        return;
    }
    print_usb_power_and_suspend(razer_proto::BLADE_18.usb);
    eprintln!("on battery: {:?}", platform::on_battery());
    let mut device = Device::new(open(), SystemClock);
    let first = device.firmware_version().unwrap();
    eprintln!("{}  before the sleep: {first:?}", utc_now());

    let wakes = run_wake_steps(device, &mut |device, _| {
        device
            .firmware_version()
            .map(|version| (true, format!("{version:?}")))
            .map_err(|error| format!("{error:?}"))
    });
    assert!(wakes > 0, "no wake was seen");
}

/// The loop of the wake probes. It answers each sleep notice at once. After
/// each wake notice it runs `step` at once and then each 100 ms by the
/// clock until 3 s after the notice, or until 10 s when no step has worked
/// by 3 s, on the handle kept over the sleep (a new one after a failed
/// step). It ends after 5 min awake with no sleep notice (counted in ticks
/// of at most 2 s, so the time asleep does not count). Returns how many
/// wakes it saw.
fn run_wake_steps(device: Device<HidTransport, SystemClock>, step: &mut WakeStep<'_>) -> u32 {
    use platform::PowerEvent;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Instant;

    let mut device = Some(device);

    let notices = answered_power_notices();
    let mut wakes = 0;
    let mut woke: Option<Instant> = None;
    let mut next_read = Duration::ZERO;
    let mut answered = false;
    let mut awake = AwakeTime::new(Instant::now());
    while awake.time() < PROBE_AWAKE {
        let wait = woke.map_or(Duration::from_secs(1), |at| {
            (at + next_read).saturating_duration_since(Instant::now())
        });
        let notice = notices.recv_timeout(wait);
        awake.tick(Instant::now());
        match notice {
            Ok((_, PowerEvent::Suspending)) => {
                eprintln!("{}  sleep notice (answered)", utc_now());
                awake.restart();
                woke = None;
            }
            Ok((at, PowerEvent::Resumed)) => {
                eprintln!("{}  wake notice", utc_now());
                wakes += 1;
                woke = Some(at);
                next_read = Duration::ZERO;
                answered = false;
            }
            Ok((_, source)) => eprintln!("{}  {source:?}", utc_now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("the power events stopped"),
        }
        let Some(at) = woke else {
            continue;
        };
        if at.elapsed() < next_read {
            continue;
        }
        if next_read > WAKE_READS_FOR && (answered || next_read > WAKE_READS_AT_MOST) {
            eprintln!(
                "{}  steps after the wake done; a step worked: {answered}",
                utc_now()
            );
            woke = None;
            continue;
        }
        answered |= wake_step(&mut device, at, wakes, step);
        next_read = next_wake_read(at.elapsed());
    }
    eprintln!("{}  5 min awake with no sleep notice: the end", utc_now());
    wakes
}

/// Where the wake set probe is: send the set, read once after a failed
/// step before it sends again, or only read after a set was taken.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum SetPhase {
    #[default]
    Send,
    ReadAfterFailure,
    Taken,
}

/// What the wake set probe sends and how it reads each step's result. A
/// set is taken when it got a Success reply and the level read back is the
/// one sent. Since the probe sends the level that is already there, a taken
/// set shows the controller accepted it, not that it changed anything.
/// Sets are few: none after one is taken in a wake, and none again before
/// a read works after a failed step (`Device::send` never repeats a
/// command whose result is not known).
#[derive(Default)]
struct WakeSet {
    wake: u32,
    phase: SetPhase,
    /// Steps that read back a level other than the one sent.
    odd: u32,
}

impl WakeSet {
    /// Starts each new wake with a set, unless a failed step still waits
    /// for its read.
    fn begin(&mut self, wake: u32) {
        if wake != self.wake {
            self.wake = wake;
            if self.phase == SetPhase::Taken {
                self.phase = SetPhase::Send;
            }
        }
    }

    /// Whether this step sends the set before it reads.
    fn sends(&self) -> bool {
        self.phase == SetPhase::Send
    }

    /// A set or read failed: if a set may have been sent, the next step
    /// only reads.
    fn failed(&mut self) {
        if self.phase == SetPhase::Send {
            self.phase = SetPhase::ReadAfterFailure;
        }
    }

    /// The level read in this step: whether the step counts as worked, and
    /// its line.
    fn read(&mut self, sent: u8, read: u8) -> (bool, String) {
        match self.phase {
            SetPhase::Send if read == sent => {
                self.phase = SetPhase::Taken;
                (true, format!("set {sent}, read back {read}"))
            }
            SetPhase::Send => {
                self.odd += 1;
                (false, format!("NOT TAKEN: set {sent}, read back {read}"))
            }
            SetPhase::ReadAfterFailure => {
                self.phase = SetPhase::Send;
                (false, format!("after a failed step: read {read}"))
            }
            SetPhase::Taken if read == sent => (true, format!("read {read}")),
            SetPhase::Taken => {
                self.odd += 1;
                (true, format!("LEVEL CHANGED: read {read}, set {sent}"))
            }
        }
    }
}

/// Reads the level of the keyboard's brightness LED.
fn keyboard_level(device: &mut Device<HidTransport, SystemClock>, led: u8) -> Result<u8, String> {
    let query = razer_proto::Brightness::level_query(led);
    let reply = device
        .send(query.id, &query.args)
        .map_err(|error| format!("{error:?}"))?;
    razer_proto::Brightness::level_from_reply(led, reply.args())
        .ok_or_else(|| format!("reply for LED {led:02X}: {:02X?}", reply.args()))
}

/// Ticket 229: does a set work right after a wake? The app waits 2 s
/// (`RESUME_PAUSE`) before it sends; ticket 219 showed reads answer at
/// once, but no set was tried.
///
/// It sends only one kind of set command: the keyboard brightness
/// (`0303 [01, 05, level]`, the command and storage the app uses at each
/// wake), always with the level it read before the sleep, so the level
/// never changes. The vapor light and every other setting are left alone.
///
/// Quit the app from the tray, then start the probe by its name only, never
/// with `cargo hw` (that runs every hardware test, also those that send,
/// while you sleep the laptop). Do not press the brightness keys
/// (Fn + F10 / F11) during the run:
///
/// `$env:BLADE_WAKE_SET_PROBE=1; cargo test -p razer-device --test hardware a_set_works_after_a_wake -- --ignored --nocapture --test-threads=1; Remove-Item Env:BLADE_WAKE_SET_PROBE`
///
/// Never leave `BLADE_WAKE_SET_PROBE` set. It prints the USB device's power
/// state and suspend values and Windows' power source, opens the controller
/// and keeps that handle over the sleep (as the app does), and reads the
/// keyboard level; it refuses to go on if that read fails. After each wake
/// notice the first command is the set, then a read-back, as [`WakeSet`]
/// says: once a set is taken, the steps only read; after a failed step the
/// next one only reads, and the set goes once more after a read works. So
/// a wake that works at once sends one set; one that fails sends at most
/// one set for each two steps. Steps run at once and each 100 ms until 3 s
/// after the notice, or until 10 s when no set was taken by 3 s (as
/// [`run_wake_steps`] says), and each prints its start, time taken and
/// result. At the end it reads the level once more. Fails if no wake was
/// seen, if any step read back another level, or if the level at the end
/// is not the one before the sleep.
#[test]
#[ignore = "needs the Blade 18, BLADE_WAKE_SET_PROBE=1 and a person sleeping the laptop; sends the keyboard level it read"]
fn a_set_works_after_a_wake() {
    if !opted_in("BLADE_WAKE_SET_PROBE") {
        return;
    }
    print_usb_power_and_suspend(razer_proto::BLADE_18.usb);
    eprintln!("on battery: {:?}", platform::on_battery());
    let led = *razer_proto::BLADE_18
        .brightness_leds
        .first()
        .expect("the model has a keyboard brightness LED");
    let mut device = Device::new(open(), SystemClock);
    let level = keyboard_level(&mut device, led)
        .unwrap_or_else(|error| panic!("refused: the keyboard level read failed: {error}"));
    eprintln!("{}  before the sleep: keyboard level {level}", utc_now());

    let mut steps = WakeSet::default();
    let wakes = run_wake_steps(device, &mut |device, wake| {
        steps.begin(wake);
        if steps.sends()
            && let Err(error) = device.send(SET_BRIGHTNESS, &[1, led, level])
        {
            steps.failed();
            return Err(format!("set: {error:?}"));
        }
        keyboard_level(device, led)
            .map(|read| steps.read(level, read))
            .map_err(|error| {
                steps.failed();
                format!("read: {error}")
            })
    });
    let end = HidTransport::open(&razer_proto::BLADE_18)
        .map_err(|error| format!("open: {error}"))
        .and_then(|transport| keyboard_level(&mut Device::new(transport, SystemClock), led));
    eprintln!("{}  at the end: keyboard level {end:?}", utc_now());
    assert!(wakes > 0, "no wake was seen");
    assert_eq!(steps.odd, 0, "a step read back another level");
    assert_eq!(end, Ok(level), "the level at the end is not the one before");
}

/// How long one read on the kept key handle waits, as the app's one read
/// waits its whole quiet spell (`KEY_QUIET_SPELL`, 120 s), so a read is
/// waiting when the laptop sleeps.
const KEPT_READ_WAIT: Duration = Duration::from_secs(120);

/// How long the kept key handle waits after a failed read before it opens
/// again, as the app does after a read error.
const KEPT_OPEN_PAUSE: Duration = Duration::from_secs(5);

/// How long one read on a fresh key handle waits, so it sees its stop soon.
const FRESH_READ_WAIT: Duration = Duration::from_millis(100);

/// A read that ended long before its wait with no report and no error:
/// ticket 131's broken handle. It counts as a failure, not as a silent
/// handle, and the thread pauses rather than spins. The time the read
/// took is printed with it: whether a read's wait counts time asleep is not
/// checked, so a read over a sleep could end early and look quick.
const QUICK: &str = "a read returned at once with no report and no error (ticket 131's case)";

/// What one key handle's thread saw.
enum KeyRead {
    Opened,
    Report(Vec<u8>),
    Failed(String),
}

/// One thing a key handle's thread saw, with when and on which handle.
struct KeyNote {
    at: std::time::Instant,
    /// `kept` or `fresh`, and the collection, such as `fresh COL04`.
    handle: String,
    kept: bool,
    profiles: bool,
    read: KeyRead,
}

/// What one kind of handle on the profile's key collection (kept or fresh)
/// did from one sleep notice to the next.
#[derive(Default)]
struct HandleTally {
    /// It opened: for the kept handle, so it was not kept over this sleep.
    opened: bool,
    /// It heard a key report after the wake notice.
    heard: bool,
    /// Its first error.
    error: Option<String>,
}

/// What the key handles did from one sleep notice to the next.
#[derive(Default)]
struct WakeTally {
    woke: bool,
    kept: HandleTally,
    fresh: HandleTally,
}

impl WakeTally {
    fn verdict(&self) -> String {
        if let Some(error) = &self.kept.error {
            return format!("the kept handle failed ({error}): no silent stale handle");
        }
        if self.kept.opened {
            return "void: the kept handle was opened after this sleep".into();
        }
        match (self.kept.heard, self.fresh.heard) {
            (true, true) => "both heard: no silent stale handle".into(),
            (true, false) => {
                "the kept handle heard, the fresh one not: no silent stale handle".into()
            }
            (false, true) => "STALE: the kept handle stayed silent, the fresh one heard".into(),
            (false, false) => match &self.fresh.error {
                Some(error) => format!("void: the fresh handle failed ({error})"),
                None if !self.fresh.opened => {
                    "void: no fresh handle on the profile's collection opened".into()
                }
                None => "void: no key report heard (a missed press?)".into(),
            },
        }
    }

    /// Counts one note. A key report counts only when it came after the
    /// wake notice (`woke`), so a press before the sleep or the press that
    /// woke the laptop does not pass for one heard after the wake. A kept
    /// handle's error counts from the sleep notice on.
    fn note(
        &mut self,
        note: &KeyNote,
        key: &razer_proto::PowerKey,
        woke: Option<std::time::Instant>,
    ) {
        let after_wake = woke.is_some_and(|at| note.at >= at);
        let handle = if note.kept {
            &mut self.kept
        } else if note.profiles && after_wake {
            // A fresh handle of the wake before, stopped at the sleep
            // notice, may still send a note: it is not this wake's.
            &mut self.fresh
        } else {
            return;
        };
        match &note.read {
            KeyRead::Opened => handle.opened = true,
            KeyRead::Report(report) => {
                handle.heard |= after_wake && key.is_key_report(report);
            }
            KeyRead::Failed(error) => {
                handle.error.get_or_insert_with(|| error.clone());
            }
        }
    }
}

/// The paths of all collections on the model's key interface (`MI_01`),
/// each with its tag (such as `COL04`) and whether it is the one the
/// profile names. All of them on purpose, by path alone, as the ticket 202
/// probe reads them: more than the app's key collections (which also match
/// the usage), so some, such as those Windows holds, print a failed read.
fn key_collection_paths(model: &razer_proto::ModelProfile) -> Vec<(String, String, bool)> {
    let key = model.power_key.expect("the model has a power key");
    let interface = format!(
        "VID_{:04X}&PID_{:04X}&MI_{:02X}&",
        model.usb.vendor_id, model.usb.product_id, key.interface_number
    );
    let named = key.collection.map(|number| format!("COL{number:02X}"));
    let paths = platform::hid_paths().unwrap_or_else(|error| {
        eprintln!("{}  the HID list: {error}", utc_now());
        Vec::new()
    });
    paths
        .into_iter()
        .filter_map(|path| {
            let device = path.to_ascii_uppercase().split('#').nth(1)?.to_owned();
            let tag = device.strip_prefix(&interface)?.to_owned();
            let profiles = named.as_deref() == Some(tag.as_str());
            Some((path, tag, profiles))
        })
        .collect()
}

/// Reads the profile's key collection on one handle kept over each sleep,
/// as the app does, but never opened again while it works (no 2 min
/// reopen). After a failed read it waits [`KEPT_OPEN_PAUSE`] and opens a
/// new handle, as the app does after a read error.
fn spawn_kept_key_reader(path: String, tag: &str, notes: std::sync::mpsc::Sender<KeyNote>) {
    let handle = format!("kept {tag}");
    let send = move |read| {
        notes.send(KeyNote {
            at: std::time::Instant::now(),
            handle: handle.clone(),
            kept: true,
            profiles: true,
            read,
        })
    };
    std::thread::spawn(move || {
        let mut open_failed = false;
        loop {
            let device = match platform::HidDevice::open(&path) {
                Ok(device) => device,
                Err(error) => {
                    // Only the first failed open in a row is printed.
                    if !open_failed && send(KeyRead::Failed(format!("open: {error}"))).is_err() {
                        return;
                    }
                    open_failed = true;
                    std::thread::sleep(KEPT_OPEN_PAUSE);
                    continue;
                }
            };
            open_failed = false;
            if send(KeyRead::Opened).is_err() {
                return;
            }
            loop {
                let mut buffer = [0; 64];
                let started = std::time::Instant::now();
                let read = match device.read(&mut buffer, KEPT_READ_WAIT) {
                    Ok(0) if started.elapsed() < KEPT_READ_WAIT / 2 => KeyRead::Failed(format!(
                        "{QUICK}, after {} ms",
                        started.elapsed().as_millis()
                    )),
                    Ok(0) => continue,
                    Ok(read) => KeyRead::Report(buffer[..read].to_vec()),
                    Err(error) => KeyRead::Failed(error.to_string()),
                };
                let failed = matches!(read, KeyRead::Failed(_));
                if send(read).is_err() {
                    return;
                }
                if failed {
                    break;
                }
            }
            std::thread::sleep(KEPT_OPEN_PAUSE);
        }
    });
}

/// Opens a fresh handle on each key collection and reads it until `stop`.
fn spawn_fresh_key_readers(
    model: &razer_proto::ModelProfile,
    notes: &std::sync::mpsc::Sender<KeyNote>,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;

    for (path, tag, profiles) in key_collection_paths(model) {
        let notes = notes.clone();
        let stop = std::sync::Arc::clone(stop);
        let handle = format!("fresh {tag}");
        std::thread::spawn(move || {
            let send = |read| {
                notes.send(KeyNote {
                    at: std::time::Instant::now(),
                    handle: handle.clone(),
                    kept: false,
                    profiles,
                    read,
                })
            };
            let device = match platform::HidDevice::open(&path) {
                Ok(device) => device,
                Err(error) => {
                    let _ = send(KeyRead::Failed(format!("open: {error}")));
                    return;
                }
            };
            if send(KeyRead::Opened).is_err() {
                return;
            }
            while !stop.load(Ordering::Relaxed) {
                let mut buffer = [0; 64];
                let started = std::time::Instant::now();
                let read = match device.read(&mut buffer, FRESH_READ_WAIT) {
                    Ok(0) if started.elapsed() < FRESH_READ_WAIT / 2 => KeyRead::Failed(format!(
                        "{QUICK}, after {} ms",
                        started.elapsed().as_millis()
                    )),
                    Ok(0) => continue,
                    Ok(read) => KeyRead::Report(buffer[..read].to_vec()),
                    Err(error) => KeyRead::Failed(error.to_string()),
                };
                let failed = matches!(read, KeyRead::Failed(_));
                if send(read).is_err() || failed {
                    return;
                }
            }
        });
    }
}

/// Prints one key note, with its time since the last wake notice.
fn print_key_note(note: &KeyNote, woke: Option<std::time::Instant>) {
    let since = woke.map_or_else(
        || "before a wake".to_owned(),
        |at| {
            if note.at < at {
                format!("{:>6} ms before the wake", (at - note.at).as_millis())
            } else {
                format!("{:>6} ms after the wake", (note.at - at).as_millis())
            }
        },
    );
    let what = match &note.read {
        KeyRead::Opened => "opened".to_owned(),
        KeyRead::Report(report) => format!("{report:02X?}"),
        KeyRead::Failed(error) => format!("FAILED: {error}"),
    };
    eprintln!("{}  {since}  {:<11} {what}", utc_now(), note.handle);
}

/// Ticket 221: can a key handle kept over a sleep go stale with no error
/// (the guess behind the app's 2 min reopen, `KEY_QUIET_SPELL`)? Read-only:
/// it only reads the key collections and sends nothing to any of them.
///
/// Quit the app from the tray, then start the probe by its name only, never
/// with `cargo hw` (that runs every hardware test, also those that send,
/// while you sleep the laptop):
///
/// `$env:BLADE_KEY_SLEEP_PROBE=1; cargo test -p razer-device --test hardware a_kept_key_handle_across_a_sleep -- --ignored --nocapture --test-threads=1; Remove-Item Env:BLADE_KEY_SLEEP_PROBE`
///
/// Never leave `BLADE_KEY_SLEEP_PROBE` set. It prints the USB device's power
/// state and suspend values, opens the profile's key collection and keeps
/// that handle over each sleep, with a read waiting (as the app does), and
/// answers each sleep notice at once. At each wake notice it opens a fresh
/// handle on each key collection; press Fn once then. It prints each report
/// and error with its time and handle, and at the next sleep notice (or the
/// end) a verdict for that wake: kept silent and fresh heard is a stale
/// handle; a failed kept read or both heard is not; none heard is void, and
/// so is a wake with the kept handle opened again after the sleep, or with
/// the fresh handle failed or not opened. It
/// ends after 5 min awake with no sleep notice. Fails if no wake was seen.
/// One run covers a Start-menu sleep and a hibernate (lid close). Check 61.
#[test]
#[ignore = "needs the Blade 18, BLADE_KEY_SLEEP_PROBE=1 and a person sleeping the laptop; read-only"]
fn a_kept_key_handle_across_a_sleep() {
    use platform::PowerEvent;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc::RecvTimeoutError};
    use std::time::Instant;

    if !opted_in("BLADE_KEY_SLEEP_PROBE") {
        return;
    }
    hold_lock();
    let model = &razer_proto::BLADE_18;
    let key = model.power_key.expect("the model has a power key");
    print_usb_power_and_suspend(model.usb);
    eprintln!("on battery: {:?}", platform::on_battery());
    let collections = key_collection_paths(model);
    for (_, tag, profiles) in &collections {
        eprintln!(
            "key collection {tag}{}",
            if *profiles { " (the profile's)" } else { "" }
        );
    }
    let (path, tag, _) = collections
        .iter()
        .find(|(_, _, profiles)| *profiles)
        .expect("the profile's key collection is listed")
        .clone();

    let (notes, reads) = std::sync::mpsc::channel();
    spawn_kept_key_reader(path, &tag, notes.clone());
    let notices = answered_power_notices();
    let mut wakes = 0;
    let mut woke: Option<Instant> = None;
    let mut tally: Option<WakeTally> = None;
    let mut fresh_stop = Arc::new(AtomicBool::new(false));
    let mut awake = AwakeTime::new(Instant::now());
    let end_tally = |tally: &mut Option<WakeTally>| {
        if let Some(done) = tally.take().filter(|done| done.woke) {
            eprintln!("{}  VERDICT: {}", utc_now(), done.verdict());
        }
    };
    while awake.time() < PROBE_AWAKE {
        let notice = notices.recv_timeout(Duration::from_millis(50));
        awake.tick(Instant::now());
        match notice {
            Ok((_, PowerEvent::Suspending)) => {
                eprintln!("{}  sleep notice (answered)", utc_now());
                // A second sleep notice with no wake between keeps the
                // tally, so a kept error from the first is not lost.
                if tally.as_ref().is_none_or(|open| open.woke) {
                    end_tally(&mut tally);
                    tally = Some(WakeTally::default());
                }
                fresh_stop.store(true, Ordering::Relaxed);
                woke = None;
                awake.restart();
            }
            Ok((at, PowerEvent::Resumed)) => {
                eprintln!("{}  wake notice: press Fn once now", utc_now());
                wakes += 1;
                woke = Some(at);
                tally.get_or_insert_with(WakeTally::default).woke = true;
                fresh_stop.store(true, Ordering::Relaxed);
                fresh_stop = Arc::new(AtomicBool::new(false));
                spawn_fresh_key_readers(model, &notes, &fresh_stop);
            }
            Ok((_, source)) => eprintln!("{}  {source:?}", utc_now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("the power events stopped"),
        }
        for note in reads.try_iter() {
            print_key_note(&note, woke);
            if let Some(tally) = tally.as_mut() {
                tally.note(&note, &key, woke);
            }
        }
    }
    end_tally(&mut tally);
    eprintln!("{}  5 min awake with no sleep notice: the end", utc_now());
    assert!(wakes > 0, "no wake was seen");
}

#[test]
fn a_wake_set_sends_at_the_first_step() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    assert!(steps.sends());
}

#[test]
fn a_set_read_back_the_same_is_taken_and_not_sent_again() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    assert_eq!(
        steps.read(128, 128),
        (true, "set 128, read back 128".into())
    );
    assert!(!steps.sends());
    assert_eq!(steps.read(128, 128), (true, "read 128".into()));
    assert_eq!(steps.odd, 0);
}

#[test]
fn a_set_read_back_other_is_not_taken_and_does_not_count_as_worked() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    assert_eq!(
        steps.read(128, 0),
        (false, "NOT TAKEN: set 128, read back 0".into())
    );
    assert!(steps.sends());
    assert_eq!(steps.odd, 1);
}

#[test]
fn after_a_failed_set_one_read_comes_before_the_next_set() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    steps.failed();
    assert!(!steps.sends());
    assert_eq!(
        steps.read(128, 128),
        (false, "after a failed step: read 128".into())
    );
    assert!(steps.sends());
}

#[test]
fn a_new_wake_sends_once_more_after_a_taken_set() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    steps.read(128, 128);
    steps.begin(2);
    assert!(steps.sends());
}

#[test]
fn a_new_wake_still_reads_first_after_a_failed_set() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    steps.failed();
    steps.begin(2);
    assert!(!steps.sends());
}

#[test]
fn a_failed_read_after_a_taken_set_sends_nothing_more() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    steps.read(128, 128);
    steps.failed();
    assert!(!steps.sends());
}

#[test]
fn a_level_that_moves_after_a_taken_set_is_marked() {
    let mut steps = WakeSet::default();
    steps.begin(1);
    steps.read(128, 128);
    assert_eq!(
        steps.read(128, 64),
        (true, "LEVEL CHANGED: read 64, set 128".into())
    );
    assert_eq!(steps.odd, 1);
}

#[test]
fn the_first_wake_read_slot_is_one_step_after_the_notice() {
    assert_eq!(next_wake_read(Duration::ZERO), Duration::from_millis(100));
    assert_eq!(
        next_wake_read(Duration::from_millis(99)),
        Duration::from_millis(100)
    );
}

#[test]
fn a_wake_read_slot_that_has_come_moves_on() {
    assert_eq!(
        next_wake_read(Duration::from_millis(100)),
        Duration::from_millis(200)
    );
}

#[test]
fn a_slow_wake_read_skips_the_slots_it_ran_over() {
    assert_eq!(
        next_wake_read(Duration::from_millis(2_950)),
        Duration::from_secs(3)
    );
}

#[test]
fn awake_time_counts_each_tick() {
    let start = std::time::Instant::now();
    let mut awake = AwakeTime::new(start);
    awake.tick(start + Duration::from_secs(1));
    awake.tick(start + Duration::from_millis(1_500));
    assert_eq!(awake.time(), Duration::from_millis(1_500));
}

#[test]
fn awake_time_counts_a_long_tick_as_one_short_one() {
    let start = std::time::Instant::now();
    let mut awake = AwakeTime::new(start);
    awake.tick(start + Duration::from_mins(10));
    assert_eq!(awake.time(), PROBE_TICK);
}

#[test]
fn awake_time_starts_again_at_a_sleep_notice() {
    let start = std::time::Instant::now();
    let mut awake = AwakeTime::new(start);
    awake.tick(start + Duration::from_secs(1));
    awake.restart();
    awake.tick(start + Duration::from_secs(2));
    assert_eq!(awake.time(), Duration::from_secs(1));
}

#[test]
fn a_kept_key_handle_that_failed_is_no_silent_stale_handle() {
    let tally = WakeTally {
        woke: true,
        kept: HandleTally {
            error: Some("the device is gone".into()),
            ..HandleTally::default()
        },
        ..WakeTally::default()
    };
    assert!(tally.verdict().starts_with("the kept handle failed"));
}

#[test]
fn a_kept_key_handle_that_heard_is_not_stale() {
    let tally = WakeTally {
        woke: true,
        kept: HandleTally {
            heard: true,
            ..HandleTally::default()
        },
        fresh: HandleTally {
            heard: true,
            ..HandleTally::default()
        },
    };
    assert!(tally.verdict().starts_with("both heard"));
}

#[test]
fn a_silent_kept_key_handle_beside_a_fresh_one_that_heard_is_stale() {
    let tally = WakeTally {
        woke: true,
        fresh: HandleTally {
            heard: true,
            ..HandleTally::default()
        },
        ..WakeTally::default()
    };
    assert!(tally.verdict().starts_with("STALE"));
}

#[test]
fn a_wake_with_no_key_heard_is_void() {
    let tally = WakeTally {
        woke: true,
        fresh: HandleTally {
            opened: true,
            ..HandleTally::default()
        },
        ..WakeTally::default()
    };
    assert!(tally.verdict().starts_with("void"));
}

fn key_note(at: std::time::Instant, kept: bool, profiles: bool, read: KeyRead) -> KeyNote {
    KeyNote {
        at,
        handle: String::new(),
        kept,
        profiles,
        read,
    }
}

fn fn_report() -> KeyRead {
    let mut report = vec![0; 16];
    report[0] = 0x04;
    report[1] = 0x0A;
    KeyRead::Report(report)
}

fn tally_of(notes: &[KeyNote], woke: std::time::Instant) -> WakeTally {
    let key = razer_proto::BLADE_18.power_key.unwrap();
    let mut tally = WakeTally::default();
    for note in notes {
        tally.note(note, &key, Some(woke));
    }
    tally
}

#[test]
fn a_kept_key_report_before_the_wake_is_not_heard() {
    let before = std::time::Instant::now();
    let woke = before + Duration::from_millis(1);
    let tally = tally_of(&[key_note(before, true, true, fn_report())], woke);
    assert!(!tally.kept.heard);
}

#[test]
fn key_reports_after_the_wake_are_heard_on_the_profiles_collection_only() {
    let woke = std::time::Instant::now();
    let tally = tally_of(
        &[
            key_note(woke, true, true, fn_report()),
            key_note(woke, false, false, fn_report()),
        ],
        woke,
    );
    assert!(tally.kept.heard);
    assert!(!tally.fresh.heard);
    let fresh = tally_of(&[key_note(woke, false, true, fn_report())], woke);
    assert!(fresh.fresh.heard);
}

#[test]
fn a_report_that_is_not_a_key_report_is_not_heard() {
    let woke = std::time::Instant::now();
    let wake_report = KeyRead::Report(vec![0x05, 0x01]);
    let tally = tally_of(&[key_note(woke, true, true, wake_report)], woke);
    assert!(!tally.kept.heard);
}

#[test]
fn the_first_kept_failure_is_kept_even_before_the_wake() {
    let before = std::time::Instant::now();
    let woke = before + Duration::from_millis(1);
    let tally = tally_of(
        &[
            key_note(before, true, true, KeyRead::Failed("first".into())),
            key_note(woke, true, true, KeyRead::Failed("second".into())),
        ],
        woke,
    );
    assert_eq!(tally.kept.error.as_deref(), Some("first"));
}

#[test]
fn a_failed_fresh_handle_is_named_instead_of_a_missed_press() {
    let woke = std::time::Instant::now();
    let tally = WakeTally {
        woke: true,
        ..tally_of(
            &[key_note(woke, false, true, KeyRead::Failed("gone".into()))],
            woke,
        )
    };
    assert!(tally.verdict().starts_with("void: the fresh handle failed"));
}

#[test]
fn a_kept_key_handle_that_heard_alone_is_not_stale() {
    let tally = WakeTally {
        woke: true,
        kept: HandleTally {
            heard: true,
            ..HandleTally::default()
        },
        ..WakeTally::default()
    };
    assert!(tally.verdict().starts_with("the kept handle heard"));
}

#[test]
fn a_kept_key_handle_opened_after_the_sleep_is_void() {
    let woke = std::time::Instant::now();
    let tally = tally_of(
        &[
            key_note(woke, true, true, KeyRead::Opened),
            key_note(woke, true, true, fn_report()),
        ],
        woke,
    );
    assert!(
        tally
            .verdict()
            .starts_with("void: the kept handle was opened")
    );
}

#[test]
fn a_wake_with_no_fresh_handle_opened_says_so() {
    let tally = WakeTally {
        woke: true,
        ..WakeTally::default()
    };
    assert!(tally.verdict().starts_with("void: no fresh handle"));
}

#[test]
fn a_fresh_failure_is_not_a_kept_one() {
    let woke = std::time::Instant::now();
    let tally = tally_of(
        &[key_note(woke, false, true, KeyRead::Failed("gone".into()))],
        woke,
    );
    assert_eq!(tally.kept.error, None);
}

#[test]
fn a_fresh_failure_before_the_wake_is_not_this_wakes() {
    let before = std::time::Instant::now();
    let woke = before + Duration::from_millis(1);
    let tally = tally_of(
        &[key_note(before, false, true, KeyRead::Failed("old".into()))],
        woke,
    );
    assert_eq!(tally.fresh.error, None);
}
