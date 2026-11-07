//! The app and its two windows: the settings window and the tray flyout.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use blade_core::{PollSchedule, RESUME_PAUSE, Readings, ReadingsSampler, View};
use blade_lite::{App, AppError, Rect, ShownErrors, flyout_position, report};
use platform::{
    PowerEvent, WindowsDisplays, WindowsKeys, WindowsRegistry, WindowsSensors, round_corners,
    set_mica,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use razer_device::{Device, HidTransport, SystemClock};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};
use ui::FlyoutView;

use crate::tray::px;
use crate::{FlyoutWindow, MainWindow, bind};

pub type RealApp =
    App<Device<HidTransport, SystemClock>, WindowsDisplays, WindowsRegistry, WindowsKeys>;

/// A tray click this soon after the flyout closed is the click that closed
/// it, so it must not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// How often to look for the handle of a window that was just shown, and
/// how many times.
const SHOW_DELAY: Duration = Duration::from_millis(16);
const SHOW_TRIES: u32 = 60;

pub struct Shell {
    window: MainWindow,
    flyout: FlyoutWindow,
    app: RefCell<RealApp>,
    sampler: RefCell<(ReadingsSampler, WindowsSensors)>,
    readings: Cell<Readings>,
    schedule: RefCell<PollSchedule>,
    flyout_open: Cell<bool>,
    flyout_closed_at: Cell<Option<Instant>>,
    shown_errors: RefCell<ShownErrors>,
}

impl Shell {
    pub fn new(window: MainWindow, flyout: FlyoutWindow, app: RealApp) -> Rc<Self> {
        Rc::new(Self {
            window,
            flyout,
            app: RefCell::new(app),
            sampler: RefCell::new((ReadingsSampler::default(), WindowsSensors::default())),
            readings: Cell::new(Readings::default()),
            schedule: RefCell::new(PollSchedule::default()),
            flyout_open: Cell::new(false),
            flyout_closed_at: Cell::new(None),
            shown_errors: RefCell::new(ShownErrors::default()),
        })
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    pub fn flyout(&self) -> &FlyoutWindow {
        &self.flyout
    }

    /// Closing the window hides it, and the flyout hides when it loses focus.
    pub fn watch_windows(self: &Rc<Self>) {
        let shell = Rc::downgrade(self);
        self.window.window().on_close_requested(move || {
            if let Some(shell) = shell.upgrade() {
                shell.schedule.borrow_mut().hide(View::Window);
            }
            slint::CloseRequestResponse::HideWindow
        });
        let shell = Rc::downgrade(self);
        self.flyout.window().on_winit_window_event(move |_, event| {
            if matches!(event, winit::event::WindowEvent::Focused(false))
                && let Some(shell) = shell.upgrade()
            {
                shell.hide_flyout();
            }
            EventResult::Propagate
        });
    }

    pub fn show_window(self: &Rc<Self>) {
        self.hide_flyout();
        if let Err(error) = self.window.show() {
            report(&format!("cannot show the window: {error}"));
            return;
        }
        self.once_shown(
            |shell| shell.window.window(),
            |shell, hwnd| {
                put_backdrop(hwnd, false);
                // A window that is open but minimised or behind others comes
                // to the front, so a second start shows it.
                shell.window.window().with_winit_window(|window| {
                    window.set_minimized(false);
                    window.focus_window();
                });
            },
        );
        self.schedule
            .borrow_mut()
            .show(View::Window, Instant::now());
        self.refresh(None);
    }

    /// Opens the flyout above the tray icon, or closes it if it is open.
    pub fn toggle_flyout(self: &Rc<Self>, icon: Rect) {
        if self.flyout_open.get() {
            self.hide_flyout();
            return;
        }
        let just_closed = self
            .flyout_closed_at
            .get()
            .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
        if just_closed {
            return;
        }
        self.refresh(None);
        if let Err(error) = self.flyout.show() {
            report(&format!("cannot show the flyout: {error}"));
            return;
        }
        self.flyout_open.set(true);
        self.once_shown(
            |shell| shell.flyout.window(),
            move |shell, hwnd| {
                shell.place_flyout(icon);
                put_backdrop(hwnd, true);
                shell
                    .flyout
                    .window()
                    .with_winit_window(winit::window::Window::focus_window);
            },
        );
        self.schedule
            .borrow_mut()
            .show(View::Flyout, Instant::now());
    }

    /// Runs `act` with the window's handle once Windows has made the window.
    /// A window that was just shown has none until the event loop turns.
    fn once_shown(
        self: &Rc<Self>,
        window: fn(&Self) -> &slint::Window,
        act: impl FnOnce(&Self, isize) + 'static,
    ) {
        self.wait_for_handle(window, act, SHOW_TRIES);
    }

    fn wait_for_handle(
        self: &Rc<Self>,
        window: fn(&Self) -> &slint::Window,
        act: impl FnOnce(&Self, isize) + 'static,
        tries: u32,
    ) {
        if let Some(hwnd) = hwnd(window(self)) {
            act(self, hwnd);
            return;
        }
        if tries == 0 {
            report("the window got no handle");
            return;
        }
        let shell = Rc::downgrade(self);
        Timer::single_shot(SHOW_DELAY, move || {
            if let Some(shell) = shell.upgrade() {
                shell.wait_for_handle(window, act, tries - 1);
            }
        });
    }

    fn hide_flyout(&self) {
        if !self.flyout_open.replace(false) {
            return;
        }
        let _ = self.flyout.hide();
        self.flyout_closed_at.set(Some(Instant::now()));
        self.schedule.borrow_mut().hide(View::Flyout);
    }

    fn place_flyout(&self, icon: Rect) {
        let size = self.flyout.window().size();
        let flyout = (px(size.width), px(size.height));
        let screen = self
            .flyout
            .window()
            .with_winit_window(|window| {
                let monitor = window.current_monitor()?;
                let (position, size) = (monitor.position(), monitor.size());
                Some(Rect {
                    x: position.x,
                    y: position.y,
                    width: px(size.width),
                    height: px(size.height),
                })
            })
            .flatten();
        let Some(screen) = screen else {
            return;
        };
        let (x, y) = flyout_position(icon, flyout, screen);
        self.flyout
            .window()
            .set_position(PhysicalPosition::new(x, y));
    }

    /// Runs one change, then shows the new state, or the error if it failed.
    pub fn act(&self, change: impl FnOnce(&mut RealApp) -> Result<(), AppError>) {
        let result = change(&mut self.app.borrow_mut());
        self.refresh(result.err().as_ref());
    }

    /// Fills both windows again. `error` is shown above the groups, if any.
    pub fn refresh(&self, error: Option<&AppError>) {
        let mut app = self.app.borrow_mut();
        let mut errors: Vec<String> = error.iter().map(ToString::to_string).collect();
        let header = app.header(self.readings.get());
        let performance = app.performance_view();
        bind::header(&self.window, &header);
        bind::performance(&self.window, &performance);
        let display = app.display_view();
        let keyboard = app.keyboard_view();
        match &display {
            Ok(view) => bind::display(&self.window, view),
            Err(error) => errors.push(error.to_string()),
        }
        match &keyboard {
            Ok(view) => bind::keyboard(&self.window, view),
            Err(error) => errors.push(error.to_string()),
        }
        if let (Ok(display), Ok(keyboard)) = (&display, &keyboard) {
            let flyout = FlyoutView::new(&performance, display, keyboard);
            bind::flyout(&self.flyout, &flyout, &header);
        }
        // A switch flips itself on a click. Set it from the config even when
        // a view failed, so it never shows a value that isn't saved.
        let sixty_hz = app.config().sixty_hz_on_battery;
        self.window.set_sixty_hz_on_battery(sixty_hz);
        self.flyout.set_sixty_hz_on_battery(sixty_hz);
        let errors = errors.join("\n");
        if let Some(new) = self.shown_errors.borrow_mut().new_text(&errors) {
            report(new);
        }
        self.window.set_error(errors.into());
    }

    /// Passes on power events, then reads the sensors if a view shows them.
    pub fn tick(self: &Rc<Self>, power: &Receiver<PowerEvent>) {
        self.follow_power(power);
        if self.schedule.borrow_mut().poll_due(Instant::now()) {
            self.tick_readings();
        }
    }

    /// Acts on the power events that came since the last look.
    pub fn follow_power(self: &Rc<Self>, power: &Receiver<PowerEvent>) {
        for event in power.try_iter() {
            self.power_event(event);
        }
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
        self.readings.set(readings);
        let mut app = self.app.borrow_mut();
        let header = app.header(readings);
        bind::header(&self.window, &header);
        if let Ok(view) = app.keyboard_view() {
            bind::keyboard(&self.window, &view);
        }
    }
}

/// The window's Win32 handle, once Windows has made it.
fn hwnd(window: &slint::Window) -> Option<isize> {
    let handle = window.window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

/// Puts Mica behind a window, light as the mockup is. A frameless one also
/// gets round corners.
fn put_backdrop(hwnd: isize, frameless: bool) {
    let result = set_mica(hwnd, false).and_then(|()| {
        if frameless {
            round_corners(hwnd)
        } else {
            Ok(())
        }
    });
    if let Err(error) = result {
        report(&format!("no Mica: {error}"));
    }
}
