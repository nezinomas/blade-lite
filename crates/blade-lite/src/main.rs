//! Blade Lite: the settings window and tray flyout for the Blade 18.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Not at the crate root: the generated Slint code unwraps.
#[deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]
mod background;
#[deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]
mod bind;
#[deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]
mod shell;
#[deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]
mod tray;
#[deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]
mod tray_menu;

// The app tests' fakes, for the window tests. Some are used only there.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/app/fakes.rs"]
mod fakes;
// The rules of ticket 146 for what is sent, shared with the app tests.
// Some are used only there.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/app/rules.rs"]
mod rules;
#[cfg(test)]
mod window_tests;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_core::{
    APP_NAME, Config, Launch, Log, POLL_INTERVAL, Quit, QuitWait, RESUME_PAUSE, Start, Task,
    launch, quit_first, razer_software, razer_updater, razer_warning, start_instance,
};
use blade_lite::{
    App, Machine, PollTime, SlowPolls, accent_colour, config_or_defaults, end_on_loop, icon,
    open_log, poll_late, report, report_panics, report_usb_suspend, try_times, try_until_it_works,
};
use platform::{
    InstanceLock, SessionEndWatch, WindowsDisplays, WindowsInstance, WindowsKeys, WindowsPrograms,
    WindowsRegistry, num_lock_on, serve_sent_messages,
};
use razer_device::{Device, HidTransport, Reopening, SystemClock};
use razer_proto::{BLADE_18, Rgb};
use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::winit::window::{BadIcon, Icon};
use slint::{Timer, TimerMode};

use crate::background::{
    beat, follow_the_power_key, follow_the_touchpad, loop_ended, on_battery_now,
    open_window_on_wake, power_events, quit_when_asked, watch_the_loop, watch_touchpad,
};
use crate::shell::{CONTROLLER_HELD, Laptop, Parts, REOPEN_GAP, Shell, reopen_controller};
use crate::tray::Tray;
use crate::tray_menu::{autostart_shown, on_tray};

slint::include_modules!();

/// The Windows default, for when the accent can't be read.
const DEFAULT_ACCENT: Rgb = {
    let [red, green, blue] = icon::DEFAULT_ACCENT;
    Rgb::new(red, green, blue)
};

/// The window icon's size in pixels, big enough for the taskbar at 200%.
const WINDOW_ICON_SIZE: u16 = 64;

thread_local! {
    /// The shell, for the tray's handlers on the event-loop thread.
    static SHELL: RefCell<Option<Rc<Shell>>> = const { RefCell::new(None) };
    /// The tray, for its own handlers on the event-loop thread.
    static TRAY: RefCell<Option<Rc<Tray>>> = const { RefCell::new(None) };
}

fn main() -> ExitCode {
    match launch(std::env::args_os().skip(1)) {
        Ok(Launch::App { in_tray }) => {
            show_if_failed(limit_dll_search().and_then(|()| run_caught(move || run(in_tray))))
        }
        Ok(Launch::Task(task)) => do_task(task),
        // Nothing runs, and no box: an installer may have sent it, and a
        // box would hold it.
        Err(error) => {
            // Without a log the line goes to stderr only.
            let _ = open_the_log();
            report(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// Logs a failed start and shows it in the start box.
fn show_if_failed(ran: Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    let Err(error) = ran else {
        return ExitCode::SUCCESS;
    };
    report(&error.to_string());
    show_start_error(&error.to_string());
    ExitCode::FAILURE
}

/// The exit code of a `--quit` whose copy took the ask but has not ended:
/// it must not be ended, as it sends the quit preset as it ends.
const STILL_QUITTING: u8 = 2;

/// Runs a task for an installer (ticket 148). It shows no box, so an
/// installer that runs it is never held: the exit code says how it went
/// (0 it worked, [`STILL_QUITTING`], else 1), and the log why.
fn do_task(task: Task) -> ExitCode {
    let ran = open_the_log()
        .and_then(|_| limit_dll_search())
        .and_then(|()| run_caught(move || run_task(task)));
    ran.unwrap_or_else(|error| {
        report(&format!("{}: {error}", task.flag()));
        ExitCode::FAILURE
    })
}

/// How long `--quit` waits for the running copy. It takes the ask at once
/// unless it is busy, such as in its start sends; then its quit may wait
/// out the rest of a pause after a wake (2 s), send twice, with a retry,
/// each command waiting up to 3 s for the controller (`FEATURE_WAIT` in
/// `platform/src/hid.rs`), then put the panel's rate back. Once it took the
/// ask, a long wait: it must never be ended mid-quit, and only this start
/// can read whether its quit preset went.
const QUIT_WAIT: QuitWait = QuitWait {
    to_take: Duration::from_secs(20),
    to_end: Duration::from_secs(120),
    every: Duration::from_millis(100),
};

fn run_task(task: Task) -> Result<ExitCode, Box<dyn std::error::Error>> {
    report(&format!(
        "{} starts, version {}",
        task.flag(),
        env!("CARGO_PKG_VERSION")
    ));
    let mut instance = WindowsInstance::new(APP_NAME)?;
    match task {
        Task::Quit => quit_running_copy(&mut instance),
        Task::Reset => reset_alone(&mut instance).map(|()| ExitCode::SUCCESS),
    }
}

/// `--quit`: asks the running copy to quit, as the tray's Quit does, and
/// waits for it to end. Sends nothing itself.
fn quit_running_copy(
    instance: &mut WindowsInstance,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let take = QUIT_WAIT.to_take.as_secs();
    let all = (QUIT_WAIT.to_take + QUIT_WAIT.to_end).as_secs();
    let (line, code) = match quit_first(instance, QUIT_WAIT, std::thread::sleep)? {
        Quit::NoneRan => (
            "no copy of the app runs, so nothing was asked".to_owned(),
            ExitCode::SUCCESS,
        ),
        Quit::Ended => (
            "the running copy sent its quit preset and has quit".to_owned(),
            ExitCode::SUCCESS,
        ),
        Quit::EndedUnsent => (
            "the running copy has quit, but its quit preset was not sent, so a manual fan or \
             Performance may run on: run --reset"
                .to_owned(),
            ExitCode::FAILURE,
        ),
        Quit::Quitting => (
            format!(
                "the running copy is quitting but has not ended within {all} s (a menu or a \
                 file box of its own may be open). Do not end it: it sends the quit preset as \
                 it ends. Whether that works can't be known: once it has ended, run --reset"
            ),
            ExitCode::from(STILL_QUITTING),
        ),
        Quit::StillRuns => (
            format!(
                "the running copy did not take the ask within {take} s, so it was taken back. \
                 A copy that waits at login for the controller, or on its start box about the \
                 settings, has sent nothing yet and can't hear the ask: end it in Task \
                 Manager, then run --reset"
            ),
            ExitCode::FAILURE,
        ),
    };
    report(&format!("--quit: {line}"));
    Ok(code)
}

/// `--reset`: when no copy runs, ends a manual fan that a crash or a kill
/// may have left, then sends Balanced. Nothing is sent while another copy
/// runs (it drives the controller), or while Razer software runs (it may
/// hold or update the controller), or if the look for it fails.
fn reset_alone(instance: &mut WindowsInstance) -> Result<(), Box<dyn std::error::Error>> {
    if start_instance(instance, false)? == Start::Second {
        return Err("Blade Lite runs, so nothing was sent: use --quit".into());
    }
    let found = razer_software(&WindowsPrograms)
        .map_err(|error| format!("can't look for Razer software, so nothing was sent: {error}"))?;
    if !found.is_empty() {
        return Err(format!(
            "Razer software runs ({}), so nothing was sent",
            found.join(", ")
        )
        .into());
    }
    let mut controller = Device::new(open_controller()?, SystemClock);
    blade_core::reset(&BLADE_18, &mut controller)?;
    report("--reset: Custom with the fans on auto, then Balanced, was sent");
    Ok(())
}

/// Opens the log in `%APPDATA%`, so even a failed start leaves its reason
/// there, and logs each panic.
fn open_the_log() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // With no APPDATA there is no log: this error goes to stderr only.
    let appdata = PathBuf::from(std::env::var_os("APPDATA").ok_or("no APPDATA")?);
    let opened = open_log(Log::new(Log::path_in(&appdata)));
    debug_assert!(opened, "a report came before the log was opened");
    report_panics();
    Ok(appdata)
}

/// Limits the DLL search to System32 before any DLL is loaded by name, or
/// stops the start (ticket 223).
fn limit_dll_search() -> Result<(), Box<dyn std::error::Error>> {
    platform::limit_dll_search()
        .map_err(|error| format!("cannot limit the DLL search to System32: {error}").into())
}

/// Runs `run`; a panic (a bug) it lets out ends as the error
/// [`shell::PANICKED`], so the start box shows it (ticket 164). The panic
/// hook has logged it, unless it came before the log was opened.
fn run_caught<T>(
    run: impl FnOnce() -> Result<T, Box<dyn std::error::Error>> + std::panic::UnwindSafe,
) -> Result<T, Box<dyn std::error::Error>> {
    std::panic::catch_unwind(run).unwrap_or_else(|_| Err(shell::PANICKED.into()))
}

/// The window title of the start's message boxes.
const START_BOX_TITLE: &str = "Blade Lite";

/// Where to read what to do when the fans or the power mode are stuck.
const RECOVERY: &str =
    "Fans or power mode stuck? See docs/recovery.md at https://github.com/nezinomas/blade-lite";

/// Shows a failed start in a message box with the log's path: a release
/// build has no console, and the app ends with no window or tray icon.
fn show_start_error(error: &str) {
    platform::show_error(
        START_BOX_TITLE,
        &start_box_text("Blade Lite stopped:", error),
    );
}

/// The start box's text: `headline`, the error, the log's path and where
/// to read about stuck fans.
fn start_box_text(headline: &str, error: &str) -> String {
    let log = std::env::var_os("APPDATA").map_or_else(
        || "not written (no APPDATA)".to_owned(),
        |appdata| Log::path_in(&PathBuf::from(appdata)).display().to_string(),
    );
    format!("{headline}\n\n{error}\n\nLog: {log}\n\n{RECOVERY}")
}

/// How long apart a login start tries the controller again once its first
/// tries failed (ticket 153).
const TRAY_RETRY_PAUSE: Duration = Duration::from_secs(30);

/// A login start whose first tries failed: shows the error once, on a
/// thread of its own so the tries don't wait for the user, then tries every
/// [`TRAY_RETRY_PAUSE`] until the controller opens. What a crash or a kill
/// left running (a manual fan, Performance) then gets the app back. No try
/// while Razer software runs (or the look fails): it may hold the
/// controller, or update it. After an open it waits the wake's pause, as
/// the controller may just have come back. `None` if Windows is ending the
/// session by then: nothing is sent.
fn keep_trying_to_open(error: &dyn std::error::Error) -> Option<HidTransport> {
    let every = TRAY_RETRY_PAUSE.as_secs();
    report(&format!(
        "the controller can't be opened at login, so the app tries again every {every} s: {error}"
    ));
    let text = start_box_text(
        &format!(
            "Blade Lite can't open the controller yet. It tries again every {every} s in the \
             background, with no tray icon until then; starting it again does nothing \
             meanwhile. To stop it, end blade-lite.exe in Task Manager. This box is not \
             updated: close it."
        ),
        &error.to_string(),
    );
    std::thread::spawn(move || platform::show_error(START_BOX_TITLE, &text));
    let controller = try_until_it_works(TRAY_RETRY_PAUSE, || {
        match razer_software(&WindowsPrograms) {
            Ok(found) if found.is_empty() => open_controller(),
            Ok(found) => Err(format!("Razer software runs ({})", found.join(", ")).into()),
            Err(error) => Err(format!("can't look for Razer software: {error}").into()),
        }
    });
    report("the controller was opened at last");
    std::thread::sleep(RESUME_PAUSE);
    if platform::shutting_down() {
        report("Windows ends the session, so the late start sends nothing");
        return None;
    }
    Some(controller)
}

/// How many times a login start (`--tray`) tries to open the controller,
/// and how long apart: the controller may not be ready yet at login.
const TRAY_OPEN_TRIES: u32 = 5;
const TRAY_OPEN_PAUSE: Duration = Duration::from_secs(2);

/// Opens the controller. If it can't be opened, the error names any Razer
/// software that runs, such as a Razer driver that took it.
fn open_controller() -> Result<HidTransport, Box<dyn std::error::Error>> {
    HidTransport::open(&BLADE_18).map_err(|error| {
        let found = razer_software(&WindowsPrograms).unwrap_or_default();
        match razer_warning(&found) {
            Some(warning) => format!("{error}. {warning}").into(),
            None => error.into(),
        }
    })
}

/// Opens the controller; a login start (`--tray`) tries
/// [`TRAY_OPEN_TRIES`] times, as it may not be ready yet. Each failed try
/// but the last is logged, so the log shows how late it was; then it keeps
/// trying (`None` if Windows ends the session first).
fn open_controller_at_start(
    in_tray: bool,
) -> Result<Option<HidTransport>, Box<dyn std::error::Error>> {
    let tries = if in_tray { TRAY_OPEN_TRIES } else { 1 };
    let mut count = 0;
    let opened = try_times(tries, TRAY_OPEN_PAUSE, || {
        count += 1;
        let opened = open_controller();
        if let Err(error) = &opened
            && count < tries
        {
            report(&format!(
                "controller open try {count} of {tries} failed: {error}"
            ));
        }
        opened
    });
    match opened {
        Err(error) if in_tray => Ok(keep_trying_to_open(error.as_ref())),
        opened => opened.map(Some),
    }
}

/// The tray's icon in the accent colour, for the title bars and the taskbar.
/// Given to winit as each window is made: Slint's `icon` property drops a
/// picture made from pixels (it has no cache key, so it looks unchanged).
fn window_icon(accent: Rgb) -> Result<Icon, BadIcon> {
    let pixels = icon::pixels([accent.red, accent.green, accent.blue], WINDOW_ICON_SIZE);
    let size = WINDOW_ICON_SIZE.into();
    Icon::from_rgba(pixels, size, size)
}

#[cfg(test)]
#[test]
fn a_panic_in_the_run_ends_with_an_error() {
    let ran = run_caught::<()>(|| panic!("a bug"));

    assert_eq!(
        ran.map_err(|error| error.to_string()),
        Err(shell::PANICKED.to_owned())
    );
}

#[cfg(test)]
#[test]
fn the_window_icon_is_made_from_the_tray_picture() {
    assert!(window_icon(DEFAULT_ACCENT).is_ok());
}

/// Whether a Razer updater runs as the app starts. A look that fails
/// counts as one (fail safe): the shell's next look that works lets go.
fn updater_at_start() -> bool {
    razer_software(&WindowsPrograms).map_or_else(
        |error| {
            report(&format!(
                "can't look for a Razer updater, so nothing is sent until a look works: {error}"
            ));
            true
        },
        |found| razer_updater(&found),
    )
}

/// Starts the poll, each [`POLL_INTERVAL`], and the watchdog that logs it
/// if it stalls (ticket 143). A poll that holds the window, or begins
/// late, is logged (ticket 206).
fn poll(shell: &Rc<Shell>) -> Timer {
    let poll = Timer::default();
    let ticking = Rc::clone(shell);
    let mut slow = SlowPolls::default();
    let mut last: Option<(Instant, Instant)> = None;
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        beat();
        let between = ticking.take_steps();
        let began = Instant::now();
        ticking.tick();
        // The tick's own fills and the like are in its time, not between.
        ticking.take_steps();
        let now = Instant::now();
        let late = last.map_or(Duration::ZERO, |(last_began, last_ended)| {
            poll_late(last_began, last_ended, began, POLL_INTERVAL)
        });
        last = Some((began, now));
        let times = PollTime {
            took: now.saturating_duration_since(began),
            late,
            between,
            ..ticking.take_read_times()
        };
        if let Some(line) = slow.note(times, now) {
            report(&line);
        }
    });
    watch_the_loop();
    poll
}

/// How long the session end waits for the event loop to quit. Longer than
/// the quit's own wait for a pause after a wake ([`RESUME_PAUSE`]), with
/// room for a rate change to end first. Past about 5 s Windows may show
/// the app as blocking the shutdown, where the user can end it at once.
const SESSION_END_WAIT: Duration = Duration::from_secs(10);

/// Windows ends the session, heard on the watch's own thread: the quit
/// preset goes on the event loop before this returns (ticket 159).
fn on_session_end() {
    end_on_loop(
        slint::invoke_from_event_loop,
        end_session_on_loop,
        serve_sent_messages,
        SESSION_END_WAIT,
    );
}

/// The session end, on the event loop.
fn end_session_on_loop() {
    if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
        shell.end_session();
    }
}

/// Claims the app for this copy, or `None` if another copy has it (it is
/// told to open its window, unless `in_tray`).
fn claim_the_app(in_tray: bool) -> Result<Option<WindowsInstance>, Box<dyn std::error::Error>> {
    let mut instance = WindowsInstance::new(APP_NAME)?;
    if start_instance(&mut instance, !in_tray)? == Start::Second {
        report("another copy runs, so this one ends");
        return Ok(None);
    }
    // No ask to quit made before this copy held the lock is meant for it,
    // nor an older copy's word that its quit preset went (ticket 148). An
    // ask made between the claim and this look is dropped too: its --quit
    // then says the app is quitting (exit 2), and this line says why.
    match instance.take_back_quit() {
        Ok(true) => report("an old --quit ask was dropped"),
        Ok(false) => {}
        Err(error) => report(&format!("an old --quit ask may quit the app: {error}")),
    }
    if let Err(error) = instance.forget_quit_sent() {
        report(&format!(
            "a --quit start may be told an old quit preset was sent: {error}"
        ));
    }
    Ok(Some(instance))
}

/// Tells a `--quit` start that waits that the quit preset went (ticket 148).
fn tell_a_quit_start(shell: &Shell, instance: &WindowsInstance) {
    if shell.quit_preset_went()
        && let Err(error) = instance.note_quit_sent()
    {
        report(&format!(
            "a --quit start can't be told the quit preset was sent: {error}"
        ));
    }
}

fn run(in_tray: bool) -> Result<(), Box<dyn std::error::Error>> {
    // First, so even a failed start leaves its reason in the log.
    let appdata = open_the_log()?;
    report(concat!("starts, version ", env!("CARGO_PKG_VERSION")));
    // Before the controller is opened: a second copy must never drive it.
    let Some(instance) = claim_the_app(in_tray)? else {
        return Ok(());
    };
    // Reads the registry only, on its own thread: never waits, never sends.
    report_usb_suspend(BLADE_18.usb);
    // Before the windows are made: a long wait for the controller must not
    // leave windows nobody answers. Opening sends nothing.
    let Some(controller) = open_controller_at_start(in_tray)? else {
        return Ok(());
    };

    let accent = accent_colour(&WindowsRegistry).unwrap_or(DEFAULT_ACCENT);
    let window_icon = window_icon(accent)?;
    slint::BackendSelector::new()
        .backend_name("winit".into())
        // Drawn by the CPU: the default OpenGL renderer loads the graphics
        // driver, about 95 MB of private memory (ticket 226, `NOTES.md`).
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(move |attributes| {
            // Inactive: winit then shows each window without the focus,
            // each time, which the power mode popup needs. The window and
            // the flyout ask for the focus after each show.
            attributes
                .with_transparent(true)
                .with_active(false)
                .with_window_icon(Some(window_icon.clone()))
                .with_taskbar_icon(Some(window_icon.clone()))
        })
        .select()?;

    let config_path = Config::path_in(&appdata);
    let config = config_or_defaults(&config_path, |question| {
        platform::ask_yes_no(START_BOX_TITLE, question)
    })?;
    // Before the start sends settings: a failure here sends nothing.
    let window = MainWindow::new()?;
    let flyout = FlyoutWindow::new()?;
    let popup = ModePopup::new()?;
    let machine = Machine {
        controller: Device::new(
            Reopening::new(
                controller,
                reopen_controller as fn(bool) -> _,
                REOPEN_GAP,
                CONTROLLER_HELD.with(Rc::clone),
            ),
            SystemClock,
        ),
        displays: WindowsDisplays,
        registry: WindowsRegistry,
        keys: WindowsKeys,
    };
    let mut app = App::new(&BLADE_18, config, config_path, machine);
    app.set_num_lock(num_lock_on());
    app.set_mic_muted(Laptop::mic_muted());
    // Before the start sends the preset, so a start on battery sends the
    // battery one. The power events come only after the start.
    app.take_on_battery(on_battery_now());
    // Before the start reads the touchpad, so no switch is missed.
    let touchpad = watch_touchpad();
    // A Razer updater must never meet the start's sends (ticket 145).
    app.hold_controller(updater_at_start());
    // Once, before the first send, so the log has it.
    app.check_firmware();
    let firmware_unchecked = app.firmware_warning().is_some();

    window.set_accent(bind::colour(accent));
    flyout.set_accent(bind::colour(accent));
    popup.set_accent(bind::colour(accent));
    let shell = Shell::new(window, flyout, popup, app);

    // The start sends settings: however the rest ends (Quit, or an error
    // or a panic in the start or after it), the quit preset goes, so no
    // manual fan or Performance runs on with no app (ticket 164).
    let served = shell.serve(|| -> Result<(), Box<dyn std::error::Error>> {
        // First, inside `serve`: never move it out (ticket 164).
        let started = shell.start();
        // A failed start shows in the window, so a login start opens it.
        let start_failed = started.is_err();
        bind::callbacks(&shell);
        shell.watch_windows();
        SHELL.with(|cell| cell.replace(Some(Rc::clone(&shell))));

        // These read the controller, so a panic in them quits too.
        let razer_found = shell.look_for_razer();
        shell.refresh(started.err().as_ref());
        let tray = Rc::new(Tray::new(accent, &autostart_shown())?);
        tray.on_action(on_tray);
        TRAY.with(|cell| cell.replace(Some(Rc::clone(&tray))));
        open_window_on_wake(instance.listener()?);
        quit_when_asked(instance.quit_listener()?);
        // Held to the end of the run: dropping it stops the watch. Without
        // it the app runs on, as before it had one.
        let _session = SessionEndWatch::start(on_session_end)
            .inspect_err(|error| {
                report(&format!(
                    "a shutdown or sign-out will send nothing: {error}"
                ));
            })
            .ok();
        if let Some(watch) = touchpad {
            follow_the_touchpad(watch);
        }
        follow_the_power_key();

        shell.listen_for_power(power_events());
        // Held to the end of the run: dropping it stops the poll.
        let _poll = poll(&shell);

        // At login the app starts in the tray, with the settings re-applied,
        // unless Razer software runs or the firmware is not one checked: the
        // window then shows the warning.
        if !in_tray || razer_found || firmware_unchecked || start_failed {
            shell.show_window();
        }
        // Closing the window keeps the app in the tray. Quit is in its menu.
        let ran = slint::run_event_loop_until_quit();
        loop_ended();
        ran?;
        Ok(())
    });
    tell_a_quit_start(&shell, &instance);
    SHELL.with(RefCell::take);
    TRAY.with(RefCell::take);
    served
}
