//! Threads that wait for the world outside the app: a second start, the
//! touchpad, the power-mode key and the power source.

use std::ffi::{CStr, CString};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use blade_core::{PowerKeyPresses, TOUCHPAD_KEY};
use blade_lite::{
    KEY_REOPEN_PAUSE, KeyEvent, LOOK_GAP, ReaderEnd, ShownErrors, Watchdog, WindowsFnKeys, report,
    step_on_presses,
};
use platform::{
    LidAndScreen, PowerEvent, PowerEvents, RegistryWatch, SuspendHandled, WakeListener,
    WindowsPowerEvents,
};
use razer_device::HidKeyReports;
use razer_proto::BLADE_18;

use crate::SHELL;

/// Runs a background part on its own thread. A panic in it (a bug) ends
/// only that part: the panic hook logs it, and the window says so.
fn spawn_part(part: &'static str, body: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        if std::panic::catch_unwind(AssertUnwindSafe(body)).is_ok() {
            return;
        }
        // If this fails, the event loop has ended, so the app is quitting.
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                shell.part_stopped(part);
            }
        });
    });
}

/// The event loop's last beat, stamped by its poll.
static BEAT: Mutex<Option<Instant>> = Mutex::new(None);

/// Stamps a beat: the event loop runs its checks.
pub fn beat() {
    if let Ok(mut beat) = BEAT.lock() {
        *beat = Some(Instant::now());
    }
}

/// Logs a stalled event loop (no beat for 30 s), and its end, from a
/// thread that looks every few seconds. It only logs: it sends nothing.
pub fn watch_the_loop() {
    spawn_part("watchdog", || {
        let mut dog = Watchdog::new(Instant::now());
        loop {
            std::thread::sleep(LOOK_GAP);
            let beat = BEAT.lock().ok().and_then(|beat| *beat);
            if let Some(text) = dog.look(beat, Instant::now()) {
                report(&text);
            }
        }
    });
}

/// Opens the window each time a second copy asks, from a thread that waits.
pub fn open_window_on_wake(listener: WakeListener) {
    spawn_part("second start", move || {
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
    spawn_part("touchpad key", move || {
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

/// Steps the preset each time the power-mode key is pressed, and reads the
/// keyboard brightness while Fn is held and as it comes up, from a thread that reads the
/// keyboard's key reports (no keyboard hook). Nothing if the model has no
/// key.
pub fn follow_the_power_key() {
    let Some(key) = BLADE_18.power_key else {
        return;
    };
    spawn_part("power-mode key", move || {
        // Each problem is said once, not each time it is tried again.
        let mut said = ShownErrors::default();
        // The key state is kept over a quiet-spell reopen, so a key stuck
        // down does not step again; new after a failure, as the gap is
        // unknown. A first report listing the key counts (ticket 174).
        let mut presses = PowerKeyPresses::new(key);
        // The collection that sent last: the next open reads it alone, so
        // its reads can wait long (ticket 188). Forgotten after a failure.
        let mut sender: Option<CString> = None;
        loop {
            let (problem, pause) =
                match HidKeyReports::open_preferring(&BLADE_18, sender.as_deref()) {
                    Ok(mut reports) => {
                        let started = Instant::now();
                        let end =
                            step_on_presses(&mut reports, &mut WindowsFnKeys, &mut presses, on_key);
                        let ran = started.elapsed();
                        presses = end.next_presses(ran, &presses);
                        sender = match end {
                            ReaderEnd::Reopen => reports.sender().map(CStr::to_owned).or(sender),
                            _ => None,
                        };
                        let pause = end.pause_before_open(ran);
                        let problem = match end {
                            ReaderEnd::Quit => return,
                            ReaderEnd::Reopen => String::new(),
                            ReaderEnd::Failed(error) => format!("the key reports stopped: {error}"),
                        };
                        (problem, pause)
                    }
                    Err(error) => {
                        presses = presses.fresh();
                        sender = None;
                        (
                            format!("no power-mode key for now: {error}"),
                            KEY_REOPEN_PAUSE,
                        )
                    }
                };
            if let Some(text) = said.new_text(&problem) {
                report(text);
            }
            std::thread::sleep(pause);
        }
    });
}

/// Acts on a key event on the event-loop thread. False once the app is
/// quitting.
fn on_key(event: KeyEvent) -> bool {
    match event {
        KeyEvent::PowerKey => step_the_preset(),
        KeyEvent::MicKey => toggle_the_mic(),
        KeyEvent::FnHeld => follow_the_brightness_keys(false),
        KeyEvent::FnUp => follow_the_brightness_keys(true),
    }
}

/// Whether a brightness read waits on the event loop.
static BRIGHTNESS_READ_WAITS: AtomicBool = AtomicBool::new(false);

/// Reads the keyboard brightness that Fn + F10 or F11 may have set. A read
/// while Fn is held is skipped while the last one still waits, so a slow
/// controller (Busy) can't pile reads up on the event loop. The read as Fn
/// comes up always goes, so the last level is never missed.
fn follow_the_brightness_keys(fn_up: bool) -> bool {
    if BRIGHTNESS_READ_WAITS.swap(true, Ordering::AcqRel) && !fn_up {
        return true;
    }
    slint::invoke_from_event_loop(|| {
        BRIGHTNESS_READ_WAITS.store(false, Ordering::Release);
        let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
            return;
        };
        shell.follow_brightness_keys();
    })
    .is_ok()
}

/// Steps the preset on the event-loop thread. False once the app is
/// quitting.
fn step_the_preset() -> bool {
    slint::invoke_from_event_loop(|| {
        let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
            return;
        };
        // `user_act` follows the power events first: a power source change
        // not yet seen would let a preset run that the new source does not
        // offer.
        shell.user_act(blade_lite::App::on_power_key);
    })
    .is_ok()
}

/// Mutes or unmutes the mic on the event-loop thread. False once the app
/// is quitting.
fn toggle_the_mic() -> bool {
    // Only a model with a mic key reads its keys as one.
    if BLADE_18.mic_key.is_none() {
        return true;
    }
    slint::invoke_from_event_loop(|| {
        if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
            shell.toggle_mic();
        }
    })
    .is_ok()
}

/// Whether the laptop runs on its battery now; `None` if Windows can't
/// tell. An unknown source is logged once, until a read tells again.
pub fn on_battery_now() -> Option<bool> {
    let read = platform::on_battery();
    if newly_unknown(&SOURCE_UNKNOWN, read) {
        report("power source unknown");
    }
    read
}

/// Whether `read` is unknown when the last one, kept in `unknown`, was not.
fn newly_unknown(unknown: &AtomicBool, read: Option<bool>) -> bool {
    let now = read.is_none();
    let was = unknown.swap(now, Ordering::Relaxed);
    now && !was
}

/// Whether the last power source read was unknown.
static SOURCE_UNKNOWN: AtomicBool = AtomicBool::new(false);

/// Power events, read on their own thread. No events come if Windows
/// refuses to send them. A sleep is acted on at once, not at the next
/// poll, as Windows freezes the app a few seconds after it tells of it;
/// Windows is told when the send is done (ticket 169).
pub fn power_events() -> Receiver<PowerEvent> {
    let (sender, receiver) = mpsc::channel();
    match WindowsPowerEvents::register() {
        Ok(mut events) => {
            let handled = events.suspend_handled();
            // Only one registration per run: a second keeps the first.
            let _ = SLEEP_NOTICE.set(handled.clone());
            let _ = LID_AND_SCREEN.set(events.lid_and_screen());
            for (which, error) in events.lid_and_screen_refused() {
                let lost = if which == "lid" {
                    "a shut lid is not seen, so input with the screen on can end a sleep with no wake notice"
                } else {
                    "input does not end a sleep with no wake notice"
                };
                report(&format!("no {which} notices, so {lost}: {error}"));
            }
            spawn_part("power events", move || {
                while let Some(event) = events.next_event() {
                    if sender.send(event).is_err() {
                        // Nobody reads them: Windows need not wait.
                        handled.done(handled.number());
                        break;
                    }
                    if event == PowerEvent::Suspending {
                        // The newest notice: a newer one than this event only
                        // if this one's wait has already ended, so its answer
                        // may end the newer wait early (not seen; the send
                        // still goes, as the newer one waits).
                        follow_the_sleep_now(&handled, handled.number());
                    }
                }
            });
        }
        Err(error) => report(&format!("no power events: {error}")),
    }
    receiver
}

/// The sleep notices' answer, for [`sleep_notice_waits`].
static SLEEP_NOTICE: OnceLock<SuspendHandled> = OnceLock::new();

/// Whether Windows still waits for the app to act on its last sleep
/// notice; true if no notices can come (nothing to act on then).
pub fn sleep_notice_waits() -> bool {
    SLEEP_NOTICE.get().is_none_or(SuspendHandled::waiting)
}

/// What Windows last said of the lid and the screen, for [`lid_open`] and
/// [`screen_on`].
static LID_AND_SCREEN: OnceLock<LidAndScreen> = OnceLock::new();

/// Whether the lid is open; `None` if Windows has not said, or no power
/// events come.
pub fn lid_open() -> Option<bool> {
    LID_AND_SCREEN.get().and_then(LidAndScreen::lid_open)
}

/// Whether the screen is on or dimmed; `None` if Windows has not said, or
/// no power events come.
pub fn screen_on() -> Option<bool> {
    LID_AND_SCREEN.get().and_then(LidAndScreen::screen_on)
}

/// Has the event loop follow the power events now, then tells Windows the
/// sleep notice `number` was acted on, even if that panicked (a bug) or the
/// loop has ended. Windows waits at most 1 s for it in any case
/// (`SUSPEND_WAIT`).
fn follow_the_sleep_now(handled: &SuspendHandled, number: u64) {
    let answer = handled.clone();
    let posted = slint::invoke_from_event_loop(move || {
        let followed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                shell.follow_own_power();
            }
        }));
        answer.done(number);
        if let Err(panic) = followed {
            std::panic::resume_unwind(panic);
        }
    });
    if posted.is_err() {
        handled.done(number);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::newly_unknown;

    #[test]
    fn an_unknown_source_is_said_once_until_a_read_tells_again() {
        let unknown = AtomicBool::new(false);
        let reads = [None, None, Some(true), None, Some(false)];

        let said: Vec<bool> = reads.map(|read| newly_unknown(&unknown, read)).to_vec();

        assert_eq!(said, [true, false, false, true, false]);
    }
}
