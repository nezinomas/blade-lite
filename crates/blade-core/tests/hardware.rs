//! Checks on the real laptop. Run with `cargo hw`.

use std::thread;
use std::time::{Duration, Instant};

use blade_core::{set_touchpad, touchpad_on};
use platform::{WindowsKeys, WindowsRegistry};

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
