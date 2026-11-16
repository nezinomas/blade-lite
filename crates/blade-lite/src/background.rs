//! Threads that wait for the world outside the app: a second start, a
//! `--quit` start, the touchpad, the power-mode key and the power source.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use blade_core::{PowerKeyPresses, TOUCHPAD_KEY};
use blade_lite::{
    HeardReports, KEY_REOPEN_PAUSE, KeyEvent, LOOK_GAP, ReaderEnd, STALL, ShownErrors,
    UNHEARD_SPELLS_BEFORE_ALL, UnheardSpells, Watchdog, WindowsFnKeys, reads_on, report,
    step_on_presses,
};
use platform::{
    LidAndScreen, PowerEvent, PowerEvents, RegistryWatch, SuspendHandled, WakeListener,
    WindowsPowerEvents,
};
use razer_device::{DroppedReport, HidKeyReports};
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

/// Whether a stall box is open: a new stall shows none until it is closed.
static STALL_BOX_OPEN: AtomicBool = AtomicBool::new(false);

/// Whether the event loop has ended: the quit's own wait is no stall to show.
static LOOP_ENDED: AtomicBool = AtomicBool::new(false);

/// The event loop has ended: a stall from now on is only logged.
pub fn loop_ended() {
    LOOP_ENDED.store(true, Ordering::Release);
}

/// Logs a stalled event loop (no beat for 30 s), and its end, from a
/// thread that looks every few seconds. A stall is also shown in a message
/// box, once per stall (ticket 208). It sends nothing.
pub fn watch_the_loop() {
    spawn_part("watchdog", || {
        let mut dog = Watchdog::new(Instant::now());
        loop {
            std::thread::sleep(LOOK_GAP);
            let beat = BEAT.lock().ok().and_then(|beat| *beat);
            if let Some(text) = dog.look(beat, Instant::now()) {
                report(&text);
                if dog.stalled() {
                    show_stall();
                }
            }
        }
    });
}

/// Shows a stall in a message box on a thread of its own, as the event
/// loop can't show it and the box waits until it is closed.
fn show_stall() {
    if LOOP_ENDED.load(Ordering::Acquire) || STALL_BOX_OPEN.swap(true, Ordering::AcqRel) {
        return;
    }
    let text = stall_text();
    let shown = std::thread::Builder::new().spawn(move || {
        platform::show_warning(crate::START_BOX_TITLE, &text);
        STALL_BOX_OPEN.store(false, Ordering::Release);
    });
    if let Err(error) = shown {
        STALL_BOX_OPEN.store(false, Ordering::Release);
        report(&format!("can't show the stall in a message box: {error}"));
    }
}

/// The stall box's text.
fn stall_text() -> String {
    format!(
        "Blade Lite is stuck: it has not run its checks for {} s. Until it runs again, \
         there is no fan guard, heat warning or battery preset, and the fans may stay as \
         they are. The log has the details. This box is not updated: close it.\n\n{}",
        STALL.as_secs(),
        crate::RECOVERY
    )
}

/// Opens the window each time a second copy asks, from a thread that waits.
pub fn open_window_on_wake(listener: WakeListener) {
    on_each_ask(listener, "second start", "open the window", || {
        if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
            shell.show_window();
        }
    });
}

/// Quits as the tray's Quit does (so the quit preset is sent) when a
/// `--quit` start asks, from a thread that waits (ticket 148).
pub fn quit_when_asked(listener: WakeListener) {
    on_each_ask(listener, "quit ask", "quit", || {
        report("a --quit start asks the app to quit");
        // An error: the loop has already ended.
        let _ = slint::quit_event_loop();
    });
}

/// Runs `act` on the event loop each time a second copy asks through
/// `listener`, from the `part`'s own thread. Once the loop has ended it
/// still takes each ask, and drops it, until the app ends: a `--quit` then
/// sees its ask taken (the app is quitting), never one nobody heard.
fn on_each_ask(listener: WakeListener, part: &'static str, what: &'static str, act: fn()) {
    spawn_part(part, move || {
        loop {
            match listener.wait(Duration::MAX) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    report(&format!("a second start can't {what} now: {error}"));
                    return;
                }
            }
            // An error: the event loop has ended, so the app is quitting.
            let _ = slint::invoke_from_event_loop(act);
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
        // The key state is kept over a reopen after a quiet spell (for the
        // read-all spell, ticket 215), so a key stuck down does not step
        // again; new after a failure, as the gap is unknown. A first
        // report listing the key counts (ticket 174).
        let mut presses = PowerKeyPresses::new(key);
        // The collection that sent last: the next open reads it alone, so
        // its reads can wait long (ticket 188). Forgotten as the key state
        // is, as the gap is unknown.
        let mut sender: Option<String> = None;
        // The last sender logged, so a new one is logged once: a dead key
        // can then be traced to the collection read.
        let mut logged: Option<String> = None;
        // The dropped reports logged: a reopen that drops one again does
        // not log it again, so the log does not fill (ticket 210).
        let mut logged_drops: Vec<DroppedReport> = Vec::new();
        // The quiet spells with no key report heard: after about an hour all
        // key collections are read for one, in case the profile's one never
        // sends (ticket 215), and the reports are opened again, in case a
        // handle went dead with no error (ticket 230). Said once a run, so
        // the log does not fill.
        let mut unheard = UnheardSpells::default();
        let mut said_read_all = false;
        loop {
            let read_all = unheard.take_read_all();
            let opened = if read_all {
                if !std::mem::replace(&mut said_read_all, true) {
                    report(&format!(
                        "no key report in {UNHEARD_SPELLS_BEFORE_ALL} quiet spells: \
                         all key collections are read for one spell"
                    ));
                }
                HidKeyReports::open_all(&BLADE_18)
            } else {
                HidKeyReports::open_preferring(&BLADE_18, sender.as_deref())
            };
            let (problem, pause) = match opened {
                Ok(reports) => {
                    let mut reports = HeardReports::new(reports);
                    // At the open when one collection is opened, so a dead
                    // key can be traced before any press (ticket 209).
                    log_read_path(reports.reports().read_path(), &mut logged);
                    let (end, ran) = loop {
                        let started = Instant::now();
                        let end =
                            step_on_presses(&mut reports, &mut WindowsFnKeys, &mut presses, on_key);
                        let ran = started.elapsed();
                        let heard = reports.take_heard();
                        let open = reports.reports();
                        log_read_path(open.read_path(), &mut logged);
                        log_drops(open.take_dropped(), &mut logged_drops);
                        // After a spell the Fn, Enter and mic states start
                        // unknown, a release lost in a sleep with it (the
                        // safe side: a Fn still held then gives no Fn up).
                        presses = end.next_presses(ran, &presses);
                        sender = if end.keeps_key_state(ran) {
                            open.sender().map(str::to_owned).or(sender)
                        } else {
                            None
                        };
                        // No reopen after a quiet spell (ticket 230).
                        if !reads_on(&end, ran, read_all, heard, &mut unheard) {
                            break (end, ran);
                        }
                    };
                    let pause = end.pause_before_open(ran);
                    let problem = match end {
                        ReaderEnd::Quit => return,
                        ReaderEnd::QuietSpell => String::new(),
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

/// Logs the one collection the power-mode key is read on, once each time
/// it changes.
fn log_read_path(path: Option<&str>, logged: &mut Option<String>) {
    if let Some(path) = path
        && logged.as_deref() != Some(path)
    {
        report(&format!("the power-mode key is read on {path}"));
        *logged = Some(path.to_owned());
    }
}

/// The most dropped reports logged in one run, so reopens can't fill the
/// log even if the drops change each time.
const MOST_DROPS_LOGGED: usize = 8;

/// Logs each report the key reports dropped that was not logged before in
/// this run, up to [`MOST_DROPS_LOGGED`]: a key that stays dead because its
/// reports no longer match the profile can then be traced (ticket 210).
/// Logged as the open ends, so up to one quiet spell late.
fn log_drops(dropped: Vec<DroppedReport>, logged: &mut Vec<DroppedReport>) {
    for line in drop_lines(dropped, logged) {
        report(&line);
    }
}

/// A line for each of `dropped` not in `logged`, added to it while it has
/// room, and one more line as it fills up.
fn drop_lines(dropped: Vec<DroppedReport>, logged: &mut Vec<DroppedReport>) -> Vec<String> {
    let mut lines = Vec::new();
    for each in dropped {
        if logged.len() < MOST_DROPS_LOGGED && !logged.contains(&each) {
            lines.push(format!(
                "a report on a key collection was not a key report: {each}"
            ));
            logged.push(each);
            if logged.len() == MOST_DROPS_LOGGED {
                lines.push("more such reports are not logged in this run".to_owned());
            }
        }
    }
    lines
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

    use razer_device::DroppedReport;

    use super::{MOST_DROPS_LOGGED, drop_lines, newly_unknown, stall_text};

    #[test]
    fn the_stall_text_says_the_fans_may_stay_in_one_clean_paragraph() {
        let text = stall_text();

        assert!(text.contains("for 30 s"), "{text}");
        assert!(text.contains("the fans may stay as they are"), "{text}");
        assert!(!text.contains("  "), "{text}");
        assert_eq!(text.matches('\n').count(), 2, "{text}");
    }

    fn dropped(id: u8) -> DroppedReport {
        DroppedReport {
            id,
            len: 16,
            path: "col04".to_owned(),
        }
    }

    #[test]
    fn a_drop_is_logged_once_in_a_run() {
        let mut logged = Vec::new();

        assert_eq!(
            drop_lines(vec![dropped(5), dropped(6)], &mut logged),
            [
                "a report on a key collection was not a key report: report ID 05, 16 bytes, on col04",
                "a report on a key collection was not a key report: report ID 06, 16 bytes, on col04",
            ]
        );
        assert_eq!(
            drop_lines(vec![dropped(6), dropped(5)], &mut logged),
            Vec::<String>::new()
        );
        assert_eq!(drop_lines(Vec::new(), &mut logged), Vec::<String>::new());
    }

    #[test]
    fn drops_that_take_turns_are_not_logged_again() {
        let mut logged = Vec::new();

        let said: usize = [5, 6, 5, 6, 5]
            .into_iter()
            .map(|id| drop_lines(vec![dropped(id)], &mut logged).len())
            .sum();

        assert_eq!(said, 2);
    }

    #[test]
    fn no_more_drops_are_logged_once_the_most_are_and_that_is_said() {
        let mut logged = Vec::new();
        let many = (0..=u8::MAX).map(dropped).collect();

        let lines = drop_lines(many, &mut logged);

        assert_eq!(lines.len(), MOST_DROPS_LOGGED + 1);
        assert_eq!(
            lines.last().map(String::as_str),
            Some("more such reports are not logged in this run")
        );
        assert_eq!(
            drop_lines(vec![dropped(u8::MAX)], &mut logged),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unknown_source_is_said_once_until_a_read_tells_again() {
        let unknown = AtomicBool::new(false);
        let reads = [None, None, Some(true), None, Some(false)];

        let said: Vec<bool> = reads.map(|read| newly_unknown(&unknown, read)).to_vec();

        assert_eq!(said, [true, false, false, true, false]);
    }
}
