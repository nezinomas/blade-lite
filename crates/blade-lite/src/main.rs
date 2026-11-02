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

use blade_core::{Config, POLL_INTERVAL};
use blade_lite::{App, Machine, accent_colour};
use platform::{
    PowerEvent, PowerEvents, WindowsDisplays, WindowsKeys, WindowsPowerEvents, WindowsRegistry,
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

    let tray = Tray::new(accent)?;
    tray.on_action(on_tray);

    let power = power_events();
    let poll = Timer::default();
    let ticking = Rc::clone(&shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        ticking.tick(&power);
    });

    shell.show_window();
    // Closing the window keeps the app in the tray. Quit is in its menu.
    slint::run_event_loop_until_quit()?;
    SHELL.with(RefCell::take);
    Ok(())
}

fn on_tray(action: TrayAction) {
    if let TrayAction::Quit = action {
        let _ = slint::quit_event_loop();
        return;
    }
    let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
        return;
    };
    match action {
        TrayAction::Clicked(icon) => shell.toggle_flyout(icon),
        TrayAction::OpenWindow => shell.show_window(),
        TrayAction::Quit => {}
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
