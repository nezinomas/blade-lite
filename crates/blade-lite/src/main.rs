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
#[cfg(test)]
#[path = "../tests/app/rules.rs"]
mod rules;
#[cfg(test)]
mod window_tests;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use blade_core::{
    APP_NAME, Config, Log, POLL_INTERVAL, RESUME_PAUSE, Start, razer_software, razer_updater,
    razer_warning, start_instance, starts_in_tray,
};
use blade_lite::{
    App, Machine, accent_colour, config_or_defaults, end_on_loop, icon, open_log, report,
    report_panics, try_times, try_until_it_works,
};
use platform::{
    SessionEndWatch, WindowsDisplays, WindowsInstance, WindowsKeys, WindowsPrograms,
    WindowsRegistry, num_lock_on, serve_sent_messages,
};
use razer_device::{Device, HidTransport, Reopening, SystemClock};
use razer_proto::{BLADE_18, Rgb};
use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::winit::window::{BadIcon, Icon};
use slint::{Timer, TimerMode};

use crate::background::{
    beat, follow_the_power_key, follow_the_touchpad, on_battery_now, open_window_on_wake,
    power_events, watch_the_loop, watch_touchpad,
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
    match run_caught(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error.to_string());
            show_start_error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// Runs `run`; a panic (a bug) it lets out ends as the error
/// [`shell::PANICKED`], so the start box shows it (ticket 164). The panic
/// hook has logged it, unless it came before the log was opened.
fn run_caught(
    run: impl FnOnce() -> Result<(), Box<dyn std::error::Error>> + std::panic::UnwindSafe,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let ran = run_caught(|| panic!("a bug"));

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
/// if it stalls (ticket 143).
fn poll(shell: &Rc<Shell>) -> Timer {
    let poll = Timer::default();
    let ticking = Rc::clone(shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        beat();
        ticking.tick();
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

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // With no APPDATA there is no log: this error goes to stderr only.
    let appdata = PathBuf::from(std::env::var_os("APPDATA").ok_or("no APPDATA")?);
    // First, so even a failed start leaves its reason in the log.
    let opened = open_log(Log::new(Log::path_in(&appdata)));
    debug_assert!(opened, "a report came before the log was opened");
    report_panics();
    report(concat!("starts, version ", env!("CARGO_PKG_VERSION")));
    // Before the controller is opened: a second copy must never drive it.
    let in_tray = starts_in_tray(std::env::args_os().skip(1));
    let mut instance = WindowsInstance::new(APP_NAME)?;
    if start_instance(&mut instance, !in_tray)? == Start::Second {
        report("another copy runs, so this one ends");
        return Ok(());
    }
    // Before the windows are made: a long wait for the controller must not
    // leave windows nobody answers. Opening sends nothing.
    let Some(controller) = open_controller_at_start(in_tray)? else {
        return Ok(());
    };

    let accent = accent_colour(&WindowsRegistry).unwrap_or(DEFAULT_ACCENT);
    let window_icon = window_icon(accent)?;
    slint::BackendSelector::new()
        .backend_name("winit".into())
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
        slint::run_event_loop_until_quit()?;
        Ok(())
    });
    SHELL.with(RefCell::take);
    TRAY.with(RefCell::take);
    served
}
