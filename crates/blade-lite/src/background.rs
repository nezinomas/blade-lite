//! Threads that wait for the world outside the app: a second start, the
//! touchpad, the power-mode key and the power source.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use blade_core::{PowerKeyPresses, TOUCHPAD_KEY};
use blade_lite::{ReaderEnd, ShownErrors, WindowsEnter, report, step_on_presses};
use platform::{PowerEvent, PowerEvents, RegistryWatch, WakeListener, WindowsPowerEvents};
use razer_device::HidKeyReports;
use razer_proto::BLADE_18;

use crate::{POWER, SHELL};

/// Opens the window each time a second copy asks, from a thread that waits.
pub fn open_window_on_wake(listener: WakeListener) {
    std::thread::spawn(move || {
        loop {
            match listener.wait(Duration::MAX) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    report(&format!(
                        "a second start can't open the window now: {error}"
                    ));
                    return;
                }
            }
            let shown = slint::invoke_from_event_loop(|| {
                if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                    shell.show_window();
                }
            });
            // The event loop has ended, so the app is quitting.
            if shown.is_err() {
                return;
            }
        }
    });
}

/// How long the touchpad state must stay quiet before the key follows it,
/// so a burst of registry writes paints the key once.
const TOUCHPAD_QUIET: Duration = Duration::from_millis(100);

/// A watch on the touchpad state. `None` if Windows can't watch it: the key
/// then follows the touchpad only at start and resume.
pub fn watch_touchpad() -> Option<RegistryWatch> {
    RegistryWatch::open(TOUCHPAD_KEY)
        .map_err(|error| {
            report(&format!(
                "the touchpad key can't follow the touchpad: {error}"
            ));
        })
        .ok()
}

/// Paints the touchpad key each time the touchpad state changes, however
/// it was switched, from a thread that waits.
pub fn follow_the_touchpad(watch: RegistryWatch) {
    std::thread::spawn(move || {
        loop {
            match wait_for_quiet_change(&watch) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    report(&format!(
                        "the touchpad key stops following the touchpad: {error}"
                    ));
                    return;
                }
            }
            let followed = slint::invoke_from_event_loop(|| {
                if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                    shell.follow_touchpad();
                }
            });
            // The event loop has ended, so the app is quitting.
            if followed.is_err() {
                return;
            }
        }
    });
}

/// Waits for a change, then until the state has been quiet for
/// [`TOUCHPAD_QUIET`].
fn wait_for_quiet_change(watch: &RegistryWatch) -> std::io::Result<bool> {
    if !watch.wait(Duration::MAX)? {
        return Ok(false);
    }
    while watch.wait(TOUCHPAD_QUIET)? {}
    Ok(true)
}

/// How long to wait before opening the key reports again, after they
/// fail or stay quiet too long.
const KEY_REOPEN_PAUSE: Duration = Duration::from_secs(5);

/// Steps the preset each time the power-mode key is pressed, from a thread
/// that reads the keyboard's key reports. Nothing if the model has no key.
pub fn follow_the_power_key() {
    let Some(key) = BLADE_18.power_key else {
        return;
    };
    std::thread::spawn(move || {
        // Each problem is said once, not each time it is tried again.
        let mut said = ShownErrors::default();
        loop {
            let problem = match HidKeyReports::open(&BLADE_18) {
                Ok(mut reports) => {
                    let presses = PowerKeyPresses::new(key);
                    match step_on_presses(&mut reports, &mut WindowsEnter, presses, step_the_preset)
                    {
                        ReaderEnd::Quit => return,
                        ReaderEnd::Reopen => String::new(),
                        ReaderEnd::Failed(error) => format!("the key reports stopped: {error}"),
                    }
                }
                Err(error) => format!("no power-mode key for now: {error}"),
            };
            if let Some(text) = said.new_text(&problem) {
                report(text);
            }
            std::thread::sleep(KEY_REOPEN_PAUSE);
        }
    });
}

/// Steps the preset on the event-loop thread. False once the app is
/// quitting.
fn step_the_preset() -> bool {
    slint::invoke_from_event_loop(|| {
        let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
            return;
        };
        // A power source change not yet seen would let a preset run that
        // the new source does not offer.
        if let Some(power) = POWER.with(|cell| cell.borrow().clone()) {
            shell.follow_power(&power);
        }
        shell.act(blade_lite::App::on_power_key);
    })
    .is_ok()
}

/// Whether the laptop runs on its battery now. If Windows can't tell, the
/// battery preset runs (fail safe) until the next power event.
pub fn on_battery_now() -> bool {
    platform::on_battery().unwrap_or_else(|| {
        report("power source unknown: the battery preset runs");
        true
    })
}

/// Power events, read on their own thread. No events come if Windows
/// refuses to send them.
pub fn power_events() -> Receiver<PowerEvent> {
    let (sender, receiver) = mpsc::channel();
    match WindowsPowerEvents::register() {
        Ok(mut events) => {
            std::thread::spawn(move || {
                while let Some(event) = events.next_event() {
                    if sender.send(event).is_err() {
                        break;
                    }
                }
            });
        }
        Err(error) => report(&format!("no power events: {error}")),
    }
    receiver
}
