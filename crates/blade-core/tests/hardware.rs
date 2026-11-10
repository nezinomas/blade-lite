//! Checks on the real laptop. Run with `cargo hw`.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use blade_core::{
    APP_NAME, BatteryRate, Config, NativeModeState, PowerKeyPresses, Query, RefreshRate,
    TOUCHPAD_KEY, charge_limit_on, logo_on, native_mode_state, refresh_rates, set_charge_limit,
    set_logo, set_native_mode, set_refresh_rate, set_touchpad, touchpad_on,
};
use platform::{
    Display, DisplayApi, InstanceLock, OutputTechnology, PowerEvent, PowerEvents, RegistryWatch,
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

/// Puts the touchpad back to `before`. `set_touchpad` presses the toggle key
/// only if the state differs, so after a switch that was not seen, first
/// wait out a late one: else the restore sees `before`, sends nothing, and
/// the late switch leaves the touchpad changed.
fn put_touchpad_back(before: bool, switch_seen: bool) -> Result<bool, String> {
    if !switch_seen {
        thread::sleep(Duration::from_secs(3));
    }
    switch_touchpad(before)
}

#[test]
#[ignore = "needs a laptop with a precision touchpad, and toggles it"]
fn the_touchpad_is_toggled_and_put_back() {
    let before = touchpad_on(&WindowsRegistry).unwrap();
    eprintln!("touchpad on before: {before}");

    let toggled = switch_touchpad(!before);
    // Put it back even if the toggle failed.
    let restored = put_touchpad_back(before, toggled == Ok(true));

    // The lasting state first, so its error is never hidden.
    let restored = restored.unwrap_or_else(|error| panic!("{error}; check the touchpad by hand"));
    assert!(restored, "the touchpad was not put back to on = {before}");
    assert!(toggled.unwrap(), "the state did not change");
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

/// The panel's current refresh rate, or the error as text.
fn read_panel_rate() -> Result<RefreshRate, String> {
    read_panel().map(|panel| RefreshRate(panel.current.hz))
}

/// The rate saved for the panel, which a restart starts at.
fn read_saved_rate() -> Result<RefreshRate, String> {
    let panel = read_panel()?;
    WindowsDisplays
        .saved_mode(&panel.name)
        .map(|mode| RefreshRate(mode.hz))
        .ok_or_else(|| "no saved mode for the panel".to_string())
}

fn read_panel() -> Result<Display, String> {
    let displays = WindowsDisplays.displays().map_err(text)?;
    displays
        .into_iter()
        .find(|display| display.technology == OutputTechnology::Internal)
        .ok_or_else(|| "no internal panel found".to_string())
}

/// The panel's current refresh rate.
fn panel_rate() -> RefreshRate {
    read_panel_rate().unwrap()
}

#[test]
#[ignore = "needs a laptop with its own panel turned on, and switches its rate"]
fn the_refresh_rate_is_switched_and_put_back() {
    let before = panel_rate();
    let rates = refresh_rates(&WindowsDisplays).unwrap();
    let other = *rates.iter().find(|&&rate| rate != before).unwrap();
    eprintln!("rate before: {before:?}, switching to {other:?}");

    let switched = set_refresh_rate(&mut WindowsDisplays, other)
        .map_err(text)
        .and_then(|()| read_panel_rate());
    // Put it back even if the switch or its read failed.
    let restored = put_rate_back(before);

    // The lasting state first, so its error is never hidden.
    assert_eq!(restored.unwrap(), before, "the old rate was not put back");
    assert_eq!(switched.unwrap(), other, "the rate did not change");
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
    set_native_mode(mode, device, &BLADE_18).map_err(text)?;
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

    // The lasting state first, so its error is never hidden.
    let put_back = format!("the next boot may start in {other:?}: choose {before:?} in the app");
    let restored = restored.unwrap_or_else(|error| panic!("{error}; {put_back}"));
    assert_eq!(restored, NativeModeState::Active(before), "{put_back}");
    assert_eq!(chosen.unwrap(), NativeModeState::AppliesAfterRestart(other));
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

    // The lasting state first, so its error is never hidden.
    let put_back = format!("the logo may be left on = {}: set it in the app", !before);
    let restored = restored.unwrap_or_else(|error| panic!("{error}; {put_back}"));
    assert_eq!(restored, Some(before), "{put_back}");
    assert_eq!(flipped.unwrap(), Some(!before));
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

    let flip = set_touchpad(!before, &WindowsRegistry, &mut WindowsKeys).map_err(text);
    // Always wait for the switch, even if the watch failed, so a late one
    // can't land after the restore.
    let seen = flip
        .clone()
        .and_then(|()| watch.wait(Duration::from_secs(3)).map_err(text));
    let flipped = flip.and_then(|()| wait_for_touchpad(!before));
    // Put it back even if the flip failed.
    let restored = put_touchpad_back(before, flipped == Ok(true));

    // The lasting state first, so its error is never hidden.
    let restored = restored.unwrap_or_else(|error| panic!("{error}; check the touchpad by hand"));
    assert!(restored, "the touchpad was not put back to on = {before}");
    assert!(flipped.unwrap(), "the touchpad did not switch");
    assert!(seen.unwrap(), "the watch did not see the switch");
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
    // The app would switch the rate too.
    hold_lock();
    let before = panel_rate();
    assert_ne!(before, RefreshRate(60), "set another rate first");
    let saved = read_saved_rate().unwrap();
    let config = Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    };
    eprintln!("rate before: {before:?}, saved: {saved:?}");

    let seen = rates_until_ac(&config);
    // Put the rate back even if a step failed.
    let restored = put_rate_back(before);

    // The lasting state first, so its error is never hidden.
    let put_back = format!("the panel may be left at 60 Hz: set {before:?} in Windows");
    let restored = restored.unwrap_or_else(|error| panic!("{error}; {put_back}"));
    assert_eq!(restored, before, "{put_back}");
    let (on_battery, after_ac) = seen.unwrap();
    assert_eq!(
        on_battery,
        Some((RefreshRate(60), saved)),
        "no 60 Hz on battery, or it was saved"
    );
    assert_eq!(after_ac, Some(before), "the rate was not put back on AC");
}

/// The panel's rate, and the rate saved for it.
type RateAndSaved = (RefreshRate, RefreshRate);

/// Feeds the power events to a `BatteryRate` until the charger goes back in
/// (or none comes), and returns the panel's rate and saved rate on battery,
/// and its rate after AC. Stops at the first error.
fn rates_until_ac(config: &Config) -> Result<(Option<RateAndSaved>, Option<RefreshRate>), String> {
    let mut battery_rate = BatteryRate::default();
    let events = power_events();
    let (mut on_battery, mut after_ac) = (None, None);
    loop {
        let event = match events.recv_timeout(PLUG_IN_WAIT) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => return Err("power events stopped".into()),
        };
        battery_rate
            .on_event(event, config, &mut WindowsDisplays)
            .map_err(text)?;
        let rate = read_panel_rate()?;
        eprintln!("{event:?}: panel at {rate:?}");
        match event {
            PowerEvent::OnBattery => on_battery = Some((rate, read_saved_rate()?)),
            PowerEvent::OnAc if on_battery.is_some() => {
                after_ac = Some(rate);
                break;
            }
            _ => {}
        }
    }
    Ok((on_battery, after_ac))
}

/// Sets the panel back to `before` unless it is there already (or can't be
/// read), and returns the rate it has then.
fn put_rate_back(before: RefreshRate) -> Result<RefreshRate, String> {
    if read_panel_rate() != Ok(before) {
        set_refresh_rate(&mut WindowsDisplays, before).map_err(text)?;
    }
    read_panel_rate()
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
