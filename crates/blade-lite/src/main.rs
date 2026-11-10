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
use std::sync::mpsc::Receiver;

use blade_core::{
    APP_NAME, Config, Log, POLL_INTERVAL, Start, razer_software, razer_warning, start_instance,
    starts_in_tray,
};
use blade_lite::{App, Machine, accent_colour, open_log, report, report_panics};
use platform::{
    PowerEvent, WindowsDisplays, WindowsInstance, WindowsKeys, WindowsPrograms, WindowsRegistry,
    num_lock_on,
};
use razer_device::{Device, HidTransport, SystemClock};
use razer_proto::{BLADE_18, Rgb};
use slint::{Timer, TimerMode};

use crate::background::{
    follow_the_power_key, follow_the_touchpad, open_window_on_wake, power_events, watch_touchpad,
};
use crate::shell::Shell;
use crate::tray::Tray;
use crate::tray_menu::{autostart_shown, on_tray};

slint::include_modules!();

/// The Windows default, for when the accent can't be read.
const DEFAULT_ACCENT: Rgb = Rgb::new(0x00, 0x78, 0xD4);

thread_local! {
    /// The shell, for the tray's handlers on the event-loop thread.
    static SHELL: RefCell<Option<Rc<Shell>>> = const { RefCell::new(None) };
    /// The tray, for its own handlers on the event-loop thread.
    static TRAY: RefCell<Option<Rc<Tray>>> = const { RefCell::new(None) };
    /// The power events, so a key press can see them before it steps.
    static POWER: RefCell<Option<Rc<Receiver<PowerEvent>>>> = const { RefCell::new(None) };
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

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

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| {
            // Inactive: winit then shows each window without the focus,
            // each time, which the power mode popup needs. The window and
            // the flyout ask for the focus after each show.
            attributes.with_transparent(true).with_active(false)
        })
        .select()?;

    let config_path = Config::path_in(&appdata);
    let config = Config::load(&config_path)?;
    let machine = Machine {
        controller: Device::new(open_controller()?, SystemClock),
        displays: WindowsDisplays,
        registry: WindowsRegistry,
        keys: WindowsKeys,
    };
    let mut app = App::new(&BLADE_18, config, config_path, machine);
    app.set_num_lock(num_lock_on());
    // Before the start sends the preset, so a start on battery sends the
    // battery one. The power events come only after the start.
    app.take_on_battery(platform::on_battery());
    // Before the start reads the touchpad, so no switch is missed.
    let touchpad = watch_touchpad();
    let started = app.start();

    let accent = accent_colour(&WindowsRegistry).unwrap_or(DEFAULT_ACCENT);
    let window = MainWindow::new()?;
    let flyout = FlyoutWindow::new()?;
    let popup = ModePopup::new()?;
    window.set_accent(bind::colour(accent));
    flyout.set_accent(bind::colour(accent));
    popup.set_accent(bind::colour(accent));
    let shell = Shell::new(window, flyout, popup, app);
    bind::callbacks(&shell);
    shell.watch_windows();
    let razer_found = shell.look_for_razer();
    shell.refresh(started.err().as_ref());
    SHELL.with(|cell| cell.replace(Some(Rc::clone(&shell))));

    let tray = Rc::new(Tray::new(accent, &autostart_shown())?);
    tray.on_action(on_tray);
    TRAY.with(|cell| cell.replace(Some(Rc::clone(&tray))));
    open_window_on_wake(instance.listener()?);
    if let Some(watch) = touchpad {
        follow_the_touchpad(watch);
    }
    follow_the_power_key();

    let power = Rc::new(power_events());
    POWER.with(|cell| cell.replace(Some(Rc::clone(&power))));
    let poll = Timer::default();
    let ticking = Rc::clone(&shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        ticking.tick(&power);
    });

    // At login the app starts in the tray, with the settings re-applied,
    // unless Razer software runs: the window then shows the warning.
    if !in_tray || razer_found {
        shell.show_window();
    }
    // Closing the window keeps the app in the tray. Quit is in its menu.
    let looped = slint::run_event_loop_until_quit();
    shell.quit();
    looped?;
    SHELL.with(RefCell::take);
    TRAY.with(RefCell::take);
    POWER.with(RefCell::take);
    Ok(())
}
