//! Blade Lite: the settings window for the Blade 18.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bind;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use blade_core::{Config, POLL_INTERVAL, RESUME_PAUSE, Readings, ReadingsSampler};
use blade_lite::{App, AppError, Machine, accent_colour};
use platform::{
    PowerEvent, PowerEvents, WindowsDisplays, WindowsKeys, WindowsPowerEvents, WindowsRegistry,
    WindowsSensors, set_mica,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use razer_device::{Device, HidTransport, SystemClock};
use razer_proto::BLADE_18;
use slint::{ComponentHandle, Timer, TimerMode};

slint::include_modules!();

type RealApp =
    App<Device<HidTransport, SystemClock>, WindowsDisplays, WindowsRegistry, WindowsKeys>;

/// How long to wait for the window's handle before setting Mica.
const MICA_DELAY: Duration = Duration::from_millis(100);

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

    let window = MainWindow::new()?;
    if let Some(accent) = accent_colour(&WindowsRegistry) {
        window.set_accent(bind::colour(accent));
    }
    let shell = Rc::new(Shell {
        window: window.as_weak(),
        app: RefCell::new(app),
        sampler: RefCell::new((ReadingsSampler::default(), WindowsSensors::default())),
        readings: RefCell::new(Readings::default()),
    });
    bind::callbacks(&window, &shell);
    shell.refresh(started.err().as_ref());

    let power = power_events();
    let poll = Timer::default();
    let ticking = Rc::clone(&shell);
    poll.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        ticking.tick(&power);
    });
    shell.tick_readings();

    let weak = window.as_weak();
    Timer::single_shot(MICA_DELAY, move || {
        if let Some(window) = weak.upgrade() {
            put_mica(&window);
        }
    });

    window.run()?;
    Ok(())
}

/// The app and the window it fills.
pub struct Shell {
    window: slint::Weak<MainWindow>,
    app: RefCell<RealApp>,
    sampler: RefCell<(ReadingsSampler, WindowsSensors)>,
    readings: RefCell<Readings>,
}

impl Shell {
    /// Runs one change, then shows the new state, or the error if it failed.
    pub fn act(&self, change: impl FnOnce(&mut RealApp) -> Result<(), AppError>) {
        let result = change(&mut self.app.borrow_mut());
        self.refresh(result.err().as_ref());
    }

    /// Fills every group again. `error` is shown above them, if any.
    pub fn refresh(&self, error: Option<&AppError>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let mut app = self.app.borrow_mut();
        let mut errors: Vec<String> = error.iter().map(ToString::to_string).collect();
        bind::header(&window, &app.header(*self.readings.borrow()));
        bind::performance(&window, &app.performance_view());
        match app.display_view() {
            Ok(view) => bind::display(&window, &view),
            Err(error) => errors.push(error.to_string()),
        }
        match app.keyboard_view() {
            Ok(view) => bind::keyboard(&window, &view),
            Err(error) => errors.push(error.to_string()),
        }
        window.set_error(errors.join("\n").into());
    }

    /// Shows an error that did not come from the app, such as bad input.
    pub fn show_error(&self, error: &str) {
        if let Some(window) = self.window.upgrade() {
            window.set_error(error.into());
        }
    }

    /// Passes on power events, then reads the sensors.
    fn tick(self: &Rc<Self>, power: &Receiver<PowerEvent>) {
        for event in power.try_iter() {
            self.power_event(event);
        }
        self.tick_readings();
    }

    fn power_event(self: &Rc<Self>, event: PowerEvent) {
        if event == PowerEvent::Resumed {
            // The controller needs a moment after the wake.
            let shell = Rc::clone(self);
            Timer::single_shot(RESUME_PAUSE, move || {
                shell.act(|app| app.on_power(PowerEvent::Resumed));
            });
        } else {
            self.act(|app| app.on_power(event));
        }
    }

    /// Reads the sensors, then shows the header and the keyboard group,
    /// since the touchpad key can change it at any time.
    fn tick_readings(&self) {
        let readings = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            sampler.sample(sensors)
        };
        *self.readings.borrow_mut() = readings;
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let mut app = self.app.borrow_mut();
        bind::header(&window, &app.header(readings));
        if let Ok(view) = app.keyboard_view() {
            bind::keyboard(&window, &view);
        }
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

/// Puts Mica behind the window, light as the mockup is.
fn put_mica(window: &MainWindow) {
    let handle = window.window().window_handle();
    let result = match handle.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(win32)) => {
            set_mica(win32.hwnd.get(), false).map_err(|e| e.to_string())
        }
        Ok(_) => Err("not a Win32 window".to_owned()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(error) = result {
        eprintln!("blade-lite: no Mica: {error}");
    }
}
