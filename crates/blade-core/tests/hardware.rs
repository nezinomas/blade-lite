//! Checks on the real laptop. Run with `cargo hw`.

use std::thread;
use std::time::{Duration, Instant};

use blade_core::{RefreshRate, refresh_rates, set_refresh_rate, set_touchpad, touchpad_on};
use platform::{DisplayApi, OutputTechnology, WindowsDisplays, WindowsKeys, WindowsRegistry};

/// Waits until Windows reports the touchpad state, or gives up.
fn wait_for_touchpad(on: bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if touchpad_on(&WindowsRegistry).unwrap() == on {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
#[ignore = "needs a laptop with a precision touchpad, and toggles it"]
fn the_touchpad_is_toggled_and_put_back() {
    let before = touchpad_on(&WindowsRegistry).unwrap();
    eprintln!("touchpad on before: {before}");

    set_touchpad(!before, &WindowsRegistry, &mut WindowsKeys).unwrap();
    let toggled = wait_for_touchpad(!before);
    set_touchpad(before, &WindowsRegistry, &mut WindowsKeys).unwrap();
    let restored = wait_for_touchpad(before);

    assert!(toggled, "the state did not change");
    assert!(restored, "the old state was not put back");
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
