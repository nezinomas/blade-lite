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
#[cfg(test)]
mod window_tests;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use blade_core::{
    APP_NAME, Config, Log, POLL_INTERVAL, Start, razer_software, razer_warning, start_instance,
    starts_in_tray,
};
use blade_lite::{
    App, Machine, accent_colour, config_or_defaults, icon, open_log, report, report_panics,
    try_times,
};
use platform::{
    WindowsDisplays, WindowsInstance, WindowsKeys, WindowsPrograms, WindowsRegistry, num_lock_on,
};
use razer_device::{Device, HidTransport, Reopening, SystemClock};
use razer_proto::{BLADE_18, Rgb};
use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::winit::window::{BadIcon, Icon};
use slint::{Timer, TimerMode};

use crate::background::{
    follow_the_power_key, follow_the_touchpad, on_battery_now, open_window_on_wake, power_events,
    watch_touchpad,
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
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error.to_string());
            show_start_error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// The window title of the start's message boxes.
const START_BOX_TITLE: &str = "Blade Lite";

/// Shows a failed start in a message box with the log's path: a release
/// build has no console, and the app ends with no window or tray icon.
fn show_start_error(error: &str) {
    let log = std::env::var_os("APPDATA").map_or_else(
        || "not written (no APPDATA)".to_owned(),
        |appdata| Log::path_in(&PathBuf::from(appdata)).display().to_string(),
    );
    platform::show_error(
        START_BOX_TITLE,
        &format!("Blade Lite stopped:\n\n{error}\n\nLog: {log}"),
    );
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
/// but the last is logged, so the log shows how late it was.
fn open_controller_at_start(in_tray: bool) -> Result<HidTransport, Box<dyn std::error::Error>> {
    let tries = if in_tray { TRAY_OPEN_TRIES } else { 1 };
    let mut count = 0;
    try_times(tries, TRAY_OPEN_PAUSE, || {
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
    })
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
fn the_window_icon_is_made_from_the_tray_picture() {
    assert!(window_icon(DEFAULT_ACCENT).is_ok());
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
    let controller = open_controller_at_start(in_tray)?;
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
    // Once, before the first send, so the log has it.
    app.check_firmware();
    let firmware_unchecked = app.firmware_warning().is_some();
    let started = app.start();
    // The rate follows even if the start fails.
    let rate = app.start_rate();
    if let (Err(_), Err(error)) = (&started, &rate) {
        // Only the start error is shown, so this one is logged.
        report(&format!("the rate put back at start failed too: {error}"));
    }
    let started = started.and(rate);
    // A failed start shows in the window, so a login start opens it.
    let start_failed = started.is_err();

    window.set_accent(bind::colour(accent));
    flyout.set_accent(bind::colour(accent));
    popup.set_accent(bind::colour(accent));
    let shell = Shell::new(window, flyout, popup, app);
    bind::callbacks(&shell);
    shell.watch_windows();
    let razer_found = shell.look_for_razer();
    shell.refresh(started.err().as_ref());
    SHELL.with(|cell| cell.replace(Some(Rc::clone(&shell))));

    // The start has sent settings: an error from here on quits as Quit
    // does, so no manual fan or Performance runs on with no app.
    let tray = Rc::new(shell.quit_on_error(Tray::new(accent, &autostart_shown()))?);
    tray.on_action(on_tray);
    TRAY.with(|cell| cell.replace(Some(Rc::clone(&tray))));
    open_window_on_wake(shell.quit_on_error(instance.listener())?);
    if let Some(watch) = touchpad {
        follow_the_touchpad(watch);
    }
    follow_the_power_key();

    shell.listen_for_power(power_events());
    let poll = Timer::default();
    let ticking = Rc::clone(&shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        ticking.tick();
    });

    // At login the app starts in the tray, with the settings re-applied,
    // unless Razer software runs or the firmware is not one checked: the
    // window then shows the warning.
    if !in_tray || razer_found || firmware_unchecked || start_failed {
        shell.show_window();
    }
    // Closing the window keeps the app in the tray. Quit is in its menu.
    let looped = slint::run_event_loop_until_quit();
    shell.quit();
    looped?;
    SHELL.with(RefCell::take);
    TRAY.with(RefCell::take);
    Ok(())
}
