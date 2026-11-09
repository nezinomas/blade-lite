//! Checks on the real laptop. Run with `cargo hw`.

use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use blade_core::{
    APP_NAME, BatteryRate, Config, NativeModeState, PowerKeyPresses, Query, RefreshRate,
    TOUCHPAD_KEY, charge_limit_on, logo_on, native_mode_state, refresh_rates, set_charge_limit,
    set_logo, set_native_mode, set_refresh_rate, set_touchpad, touchpad_on,
};
use platform::{
    DisplayApi, InstanceLock, OutputTechnology, PowerEvent, PowerEvents, RegistryWatch,
    WindowsDisplays, WindowsInstance, WindowsKeys, WindowsPowerEvents, WindowsRegistry,
};
use razer_device::{Device, HidKeyReports, HidTransport, SystemClock};
use razer_proto::{BLADE_18, Command, NativeMode, PerfMode};

/// Takes the app's one-copy lock for this test run, so the tests can't drive
/// the controller while Blade Lite runs. It is held until the run ends.
fn hold_lock() {
    static LOCK: Mutex<Option<WindowsInstance>> = Mutex::new(None);
    let mut held = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    if held.is_none() {
        let mut instance = WindowsInstance::new(APP_NAME).unwrap();
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
    HidTransport::open(&BLADE_18).unwrap()
}

/// An error as text, so a test can keep the `Result` of each step and put
/// the old state back before it asserts.
fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Waits until Windows reports the touchpad state, or gives up.
fn wait_for_touchpad(on: bool) -> Result<bool, String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if touchpad_on(&WindowsRegistry).map_err(text)? == on {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(false)
}

/// Switches the touchpad to `on` and waits until Windows reports it.
fn switch_touchpad(on: bool) -> Result<bool, String> {
    set_touchpad(on, &WindowsRegistry, &mut WindowsKeys).map_err(text)?;
    wait_for_touchpad(on)
}

#[test]
#[ignore = "needs a laptop with a precision touchpad, and toggles it"]
fn the_touchpad_is_toggled_and_put_back() {
    let before = touchpad_on(&WindowsRegistry).unwrap();
    eprintln!("touchpad on before: {before}");

    let toggled = switch_touchpad(!before);
    // Put it back even if the toggle failed.
    let restored = switch_touchpad(before);

    assert!(toggled.unwrap(), "the state did not change");
    assert!(restored.unwrap(), "the old state was not put back");
}

#[test]
#[ignore = "needs a laptop with its own panel turned on"]
fn the_panel_offers_its_current_rate() {
    let rates = refresh_rates(&WindowsDisplays).unwrap();
    eprintln!("rates on offer: {rates:?}");

    let displays = WindowsDisplays.displays().unwrap();
    let panel = displays
        .iter()
        .find(|display| display.technology == OutputTechnology::Internal)
        .unwrap();
    assert!(rates.contains(&RefreshRate(panel.current.hz)));
}

/// The panel's current refresh rate.
fn panel_rate() -> RefreshRate {
    let displays = WindowsDisplays.displays().unwrap();
    let panel = displays
        .iter()
        .find(|display| display.technology == OutputTechnology::Internal)
        .unwrap();
    RefreshRate(panel.current.hz)
}

#[test]
#[ignore = "needs a laptop with its own panel turned on, and switches its rate"]
fn the_refresh_rate_is_switched_and_put_back() {
    let before = panel_rate();
    let rates = refresh_rates(&WindowsDisplays).unwrap();
    let other = *rates.iter().find(|&&rate| rate != before).unwrap();
    eprintln!("rate before: {before:?}, switching to {other:?}");

    let switched = set_refresh_rate(&mut WindowsDisplays, other).map(|()| panel_rate());
    let restored = set_refresh_rate(&mut WindowsDisplays, before).map(|()| panel_rate());

    assert_eq!(switched.unwrap(), other, "the rate did not change");
    assert_eq!(restored.unwrap(), before, "the old rate was not put back");
}

/// The native mode the controller has chosen, and its state.
fn native_state(device: &mut Device<HidTransport, SystemClock>) -> Result<NativeModeState, String> {
    let query = NativeMode::query();
    let reply = device.send(query.id, &query.args).map_err(text)?;
    eprintln!("0D8E reply: {:02X?}", reply.args());
    let chosen = NativeMode::from_reply(reply.args(), &BLADE_18)
        .ok_or("the 0D8E reply names no known mode")?;
    native_mode_state(chosen, &BLADE_18, &WindowsDisplays).map_err(text)
}

/// Chooses `mode` for the next boot, then reads the state back.
fn choose_native_mode(
    mode: NativeMode,
    device: &mut Device<HidTransport, SystemClock>,
) -> Result<NativeModeState, String> {
    set_native_mode(mode, device).map_err(text)?;
    native_state(device)
}

#[test]
#[ignore = "needs the Blade 18; chooses the other native mode, then puts it back"]
fn the_native_mode_is_chosen_for_the_next_boot_and_put_back() {
    let transport = open();
    let mut device = Device::new(transport, SystemClock);
    let NativeModeState::Active(before) = native_state(&mut device).unwrap() else {
        panic!("a mode change already waits for a restart");
    };
    let other = *BLADE_18
        .native_modes
        .iter()
        .find(|&&mode| mode != before)
        .unwrap();
    eprintln!("booted in {before:?}, choosing {other:?}");

    let chosen = choose_native_mode(other, &mut device);
    // Put it back even if the choice failed, so the next boot is as before.
    let restored = choose_native_mode(before, &mut device);

    assert_eq!(chosen.unwrap(), NativeModeState::AppliesAfterRestart(other));
    assert_eq!(restored.unwrap(), NativeModeState::Active(before));
}

#[test]
#[ignore = "needs the Blade 18; flips the lid logo, then puts it back (as static)"]
fn the_lid_logo_is_flipped_and_put_back() {
    let transport = open();
    let mut device = Device::new(transport, SystemClock);
    let before = logo_on(&mut device, &BLADE_18)
        .unwrap()
        .expect("the logo reads on or off");
    eprintln!("logo on before: {before}");

    let flipped = switch_logo(!before, &mut device);
    // Put it back even if the flip failed.
    let restored = switch_logo(before, &mut device);

    assert_eq!(flipped.unwrap(), Some(!before));
    assert_eq!(restored.unwrap(), Some(before));
}

/// Turns the lid logo on or off, then reads it back.
fn switch_logo(
    on: bool,
    device: &mut Device<HidTransport, SystemClock>,
) -> Result<Option<bool>, String> {
    set_logo(on, device, &BLADE_18).map_err(text)?;
    logo_on(device, &BLADE_18).map_err(text)
}

#[test]
#[ignore = "needs the Blade 18; flips the touchpad, then puts it back"]
fn the_watch_sees_the_touchpad_switch() {
    hold_lock();
    let before = touchpad_on(&WindowsRegistry).unwrap();
    let watch = RegistryWatch::open(TOUCHPAD_KEY).unwrap();

    let flip = (|| {
        set_touchpad(!before, &WindowsRegistry, &mut WindowsKeys).map_err(text)?;
        let seen = watch.wait(Duration::from_secs(3)).map_err(text)?;
        Ok::<_, String>((seen, wait_for_touchpad(!before)?))
    })();
    // Put it back even if the flip failed.
    let restored = switch_touchpad(before);

    let (seen, flipped) = flip.unwrap();
    assert!(flipped, "the touchpad did not switch");
    assert!(seen, "the watch did not see the switch");
    assert!(restored.unwrap(), "the touchpad was not put back");
}

#[test]
#[ignore = "needs the Blade 18; flips the battery charge limit, then puts it back"]
fn the_charge_limit_is_flipped_and_put_back() {
    let transport = open();
    let mut device = Device::new(transport, SystemClock);
    let before = charge_limit_on(&mut device, &BLADE_18)
        .unwrap()
        .expect("the limit reads on or off at 80%");
    let flags = device.query(&power_flags()).unwrap();
    eprintln!("charge limit on before: {before}, power flags {flags:02x?}");

    // The set reads the limit back, so Ok means the controller took it.
    let flipped = set_charge_limit(!before, &mut device, &BLADE_18);
    // Put it back even if the flip failed.
    let restored = set_charge_limit(before, &mut device, &BLADE_18);

    flipped.unwrap();
    restored.unwrap();
    assert_eq!(
        charge_limit_on(&mut device, &BLADE_18).unwrap(),
        Some(before)
    );
    // Synapse writes these flags back after a change. The app does not, so
    // check that the limit leaves them as they were.
    assert_eq!(device.query(&power_flags()).unwrap(), flags);
}

/// Reads the power flags (`078F [00]`), as Synapse does after a limit change.
fn power_flags() -> Command {
    Command {
        id: 0x078F,
        args: vec![0x00],
    }
}

/// How long the unplug check waits for the charger to go back in.
const PLUG_IN_WAIT: Duration = Duration::from_secs(300);

/// The power events from Windows, on their own thread.
fn power_events() -> std::sync::mpsc::Receiver<PowerEvent> {
    let (sender, events) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut windows = WindowsPowerEvents::register().unwrap();
        while let Some(event) = windows.next_event() {
            if sender.send(event).is_err() {
                break;
            }
        }
    });
    events
}

#[test]
#[ignore = "start on battery, then plug the charger in; switches the panel's rate"]
fn battery_drops_to_60_hz_and_ac_puts_the_rate_back() {
    let before = panel_rate();
    let config = Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    };
    let mut battery_rate = BatteryRate::default();
    let events = power_events();
    let (mut on_battery, mut after_ac) = (None, None);
    eprintln!("rate before: {before:?}");

    while let Ok(event) = events.recv_timeout(PLUG_IN_WAIT) {
        battery_rate
            .on_event(event, &config, &mut WindowsDisplays)
            .unwrap();
        eprintln!("{event:?}: panel at {:?}", panel_rate());
        match event {
            PowerEvent::OnBattery => on_battery = Some(panel_rate()),
            PowerEvent::OnAc if on_battery.is_some() => {
                after_ac = Some(panel_rate());
                break;
            }
            _ => {}
        }
    }
    if panel_rate() != before {
        set_refresh_rate(&mut WindowsDisplays, before).unwrap();
    }

    assert_eq!(on_battery, Some(RefreshRate(60)), "no 60 Hz on battery");
    assert_eq!(after_ac, Some(before), "the rate was not put back on AC");
}

/// How long the power-mode key test waits for the press.
const KEY_PRESS_WAIT: Duration = Duration::from_secs(30);

/// The raw preset replies, one per zone.
fn preset_replies(device: &mut Device<HidTransport, SystemClock>) -> Vec<Vec<u8>> {
    PerfMode::queries(&BLADE_18)
        .iter()
        .map(|query| device.query(query).unwrap())
        .collect()
}

/// Waits for one power-mode key press, or gives up.
fn wait_for_power_key() -> bool {
    let key = BLADE_18.power_key.unwrap();
    let mut reports = HidKeyReports::open(&BLADE_18).unwrap();
    let mut presses = PowerKeyPresses::new(key);
    let end = Instant::now() + KEY_PRESS_WAIT;
    while Instant::now() < end {
        if let Some(report) = reports.read(Duration::from_millis(100)).unwrap() {
            eprintln!("key report: {report:02X?}");
            if presses.on_report(&report, Instant::now()) {
                return true;
            }
        }
    }
    false
}

#[test]
#[ignore = "press Fn + numpad Enter once within 30 s; checks the keyboard leaves the preset alone"]
fn the_power_key_leaves_the_preset_to_the_app() {
    let mut device = Device::new(open(), SystemClock);
    let before = preset_replies(&mut device);
    eprintln!("press Fn + numpad Enter once now");

    let pressed = wait_for_power_key();
    thread::sleep(Duration::from_secs(1));
    let after = preset_replies(&mut device);

    assert!(pressed, "no power-mode key press came");
    assert_eq!(after, before, "the keyboard changed the preset by itself");
}
