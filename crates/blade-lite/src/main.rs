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
    APP_NAME, Config, POLL_INTERVAL, Start, TOUCHPAD_KEY, autostart_on, set_autostart,
    start_instance, starts_in_tray,
};
use blade_lite::{App, Machine, accent_colour};
use platform::{
    PowerEvent, PowerEvents, RegistryWatch, WakeListener, WindowsDisplays, WindowsInstance,
    WindowsKeys, WindowsPowerEvents, WindowsRegistry,
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
    let in_tray = starts_in_tray(std::env::args_os().skip(1));
    let mut instance = WindowsInstance::new(APP_NAME)?;
    if start_instance(&mut instance, !in_tray)? == Start::Second {
        eprintln!("blade-lite: another copy runs, so this one ends");
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
    // Before the start reads the touchpad, so no switch is missed.
    let touchpad = watch_touchpad();
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
    if let Some(watch) = touchpad {
        follow_the_touchpad(watch);
    }

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
        loop {
            match listener.wait(Duration::MAX) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    eprintln!("blade-lite: a second start can't open the window now: {error}");
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
fn watch_touchpad() -> Option<RegistryWatch> {
    RegistryWatch::open(TOUCHPAD_KEY)
        .map_err(|error| {
            eprintln!("blade-lite: the touchpad key can't follow the touchpad: {error}");
        })
        .ok()
}

/// Paints the touchpad key each time the touchpad state changes, however
/// it was switched, from a thread that waits.
fn follow_the_touchpad(watch: RegistryWatch) {
    std::thread::spawn(move || {
        loop {
            match wait_for_quiet_change(&watch) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    eprintln!("blade-lite: the touchpad key stops following the touchpad: {error}");
                    return;
                }
            }
            let followed = slint::invoke_from_event_loop(|| {
                if let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) {
                    shell.act(blade_lite::App::on_touchpad_changed);
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
