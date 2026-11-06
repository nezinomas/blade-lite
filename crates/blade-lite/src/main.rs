//! Blade Lite: the settings window and tray flyout for the Blade 18.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bind;
mod shell;
mod tray;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use blade_core::{
    APP_NAME, Config, POLL_INTERVAL, Start, TRAY_FLAG, autostart_on, set_autostart,
    start_instance,
};
use blade_lite::{App, Machine, accent_colour};
use platform::{
    PowerEvent, PowerEvents, WakeListener, WindowsDisplays, WindowsInstance, WindowsKeys,
    WindowsPowerEvents, WindowsRegistry,
};
use razer_device::{Device, HidTransport, SystemClock};
use razer_proto::{BLADE_18, Rgb};
use slint::{Timer, TimerMode};

use crate::shell::Shell;
use crate::tray::{Tray, TrayAction};

slint::include_modules!();

/// The Windows default, for when the accent can't be read.
const DEFAULT_ACCENT: Rgb = Rgb::new(0x00, 0x78, 0xD4);

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
            eprintln!("blade-lite: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Before the controller is opened: a second copy must never drive it.
    let in_tray = std::env::args().skip(1).any(|arg| arg == TRAY_FLAG);
    let mut instance = WindowsInstance::new(APP_NAME)?;
    if start_instance(&mut instance, !in_tray)? == Start::Second {
        return Ok(());
    }

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| attributes.with_transparent(true))
        .select()?;

    let config_path = Config::path_in(&PathBuf::from(
        std::env::var_os("APPDATA").ok_or("no APPDATA")?,
    ));
    let config = Config::load(&config_path)?;
    let machine = Machine {
        controller: Device::new(HidTransport::open(&BLADE_18)?, SystemClock),
        displays: WindowsDisplays,
        registry: WindowsRegistry,
        keys: WindowsKeys,
    };
    let mut app = App::new(&BLADE_18, config, config_path, machine);
    let started = app.start();

    let accent = accent_colour(&WindowsRegistry).unwrap_or(DEFAULT_ACCENT);
    let window = MainWindow::new()?;
    let flyout = FlyoutWindow::new()?;
    window.set_accent(bind::colour(accent));
    flyout.set_accent(bind::colour(accent));
    let shell = Shell::new(window, flyout, app);
    bind::callbacks(&shell);
    shell.watch_windows();
    shell.refresh(started.err().as_ref());
    SHELL.with(|cell| cell.replace(Some(Rc::clone(&shell))));

    let tray = Rc::new(Tray::new(accent, autostart_state())?);
    tray.on_action(on_tray);
    TRAY.with(|cell| cell.replace(Some(Rc::clone(&tray))));
    open_window_on_wake(instance.listener()?);

    let power = power_events();
    let poll = Timer::default();
    let ticking = Rc::clone(&shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        ticking.tick(&power);
    });

    // At login the app starts in the tray, with the settings re-applied.
    if !in_tray {
        shell.show_window();
    }
    // Closing the window keeps the app in the tray. Quit is in its menu.
    slint::run_event_loop_until_quit()?;
    SHELL.with(RefCell::take);
    TRAY.with(RefCell::take);
    Ok(())
}

/// Opens the window each time a second copy asks, from a thread that waits.
fn open_window_on_wake(listener: WakeListener) {
    std::thread::spawn(move || {
        while let Ok(true) = listener.wait(Duration::MAX) {
            let _ = slint::invoke_from_event_loop(|| {
                if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                    shell.show_window();
                }
            });
        }
    });
}

/// Whether autostart is on for this exe. Off if it can't be read.
fn autostart_state() -> bool {
    std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| autostart_on(&WindowsRegistry, &exe).map_err(|error| error.to_string()))
        .unwrap_or_else(|error| {
            eprintln!("blade-lite: autostart unknown: {error}");
            false
        })
}

/// Turns autostart on or off as the tray tick now says. If that fails, the
/// tick goes back to what the registry holds.
fn sync_autostart(tray: &Tray) {
    let on = tray.autostart_ticked();
    let set = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| {
            set_autostart(on, &mut WindowsRegistry, &exe).map_err(|error| error.to_string())
        });
    if let Err(error) = set {
        eprintln!("blade-lite: could not set autostart: {error}");
        tray.tick_autostart(autostart_state());
    }
}

fn on_tray(action: TrayAction) {
    match action {
        TrayAction::Quit => {
            let _ = slint::quit_event_loop();
            return;
        }
        TrayAction::Autostart => {
            if let Some(tray) = TRAY.with(|cell| cell.borrow().clone()) {
                sync_autostart(&tray);
            }
            return;
        }
        _ => {}
    }
    let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
        return;
    };
    match action {
        TrayAction::Clicked(icon) => shell.toggle_flyout(icon),
        TrayAction::OpenWindow => shell.show_window(),
        TrayAction::Quit | TrayAction::Autostart => {}
    }
}

/// Power events, read on their own thread. No events come if Windows
/// refuses to send them.
fn power_events() -> Receiver<PowerEvent> {
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
        Err(error) => eprintln!("blade-lite: no power events: {error}"),
    }
    receiver
}
