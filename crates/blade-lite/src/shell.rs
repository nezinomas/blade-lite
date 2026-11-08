//! The app and its windows: the settings window, the tray flyout and the
//! power mode popup.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use blade_core::{PollSchedule, RESUME_PAUSE, Readings, ReadingsSampler, View};
use blade_lite::{
    App, AppError, Rect, SaidLately, ShownErrors, flyout_position, popup_position, report,
    took_effect,
};
use platform::{
    PowerEvent, WindowsDisplays, WindowsKeys, WindowsRegistry, WindowsSensors, round_corners,
    set_mica, work_area,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use razer_device::{Device, HidTransport, SystemClock};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};
use ui::{FlyoutView, ModeWatch};
use winit::platform::windows::WindowExtWindows;

use crate::tray::px;
use crate::{FlyoutWindow, MainWindow, ModePopup, bind};

pub type RealApp =
    App<Device<HidTransport, SystemClock>, WindowsDisplays, WindowsRegistry, WindowsKeys>;

/// A tray click this soon after the flyout closed is the click that closed
/// it, so it must not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// How often to look for the handle of a window that was just shown, and
/// how many times.
const SHOW_DELAY: Duration = Duration::from_millis(16);
const SHOW_TRIES: u32 = 60;

/// A sensor or fan read error is logged once, then not again for this long,
/// since it can come and go on each read.
const READ_ERROR_QUIET: Duration = Duration::from_secs(10 * 60);

/// How long the power mode popup stays before it fades, and how long the
/// fade takes (a little longer than its animation in the markup).
const POPUP_HOLD: Duration = Duration::from_millis(1500);
const POPUP_FADE: Duration = Duration::from_millis(300);

pub struct Shell {
    window: MainWindow,
    flyout: FlyoutWindow,
    popup: ModePopup,
    app: RefCell<RealApp>,
    sampler: RefCell<(ReadingsSampler, WindowsSensors)>,
    readings: Cell<Readings>,
    schedule: RefCell<PollSchedule>,
    flyout_open: Cell<bool>,
    flyout_closed_at: Cell<Option<Instant>>,
    shown_errors: RefCell<ShownErrors>,
    read_errors: RefCell<SaidLately>,
    mode_watch: Cell<ModeWatch>,
    /// Counts the popups, so the fade of an older one leaves a newer alone.
    popups: Cell<u32>,
    /// Whether the popup has been made click-through.
    popup_ready: Cell<bool>,
}

impl Shell {
    pub fn new(
        window: MainWindow,
        flyout: FlyoutWindow,
        popup: ModePopup,
        app: RealApp,
    ) -> Rc<Self> {
        let mode_watch = Cell::new(ModeWatch::new(app.running_preset()));
        Rc::new(Self {
            window,
            flyout,
            popup,
            app: RefCell::new(app),
            sampler: RefCell::new((ReadingsSampler::default(), WindowsSensors::default())),
            readings: Cell::new(Readings::default()),
            schedule: RefCell::new(PollSchedule::default()),
            flyout_open: Cell::new(false),
            flyout_closed_at: Cell::new(None),
            shown_errors: RefCell::new(ShownErrors::default()),
            read_errors: RefCell::new(SaidLately::new(READ_ERROR_QUIET)),
            mode_watch,
            popups: Cell::new(0),
            popup_ready: Cell::new(false),
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
    /// A new power mode, from any change the laptop took (even one not
    /// saved), shows the popup.
    pub fn act(self: &Rc<Self>, change: impl FnOnce(&mut RealApp) -> Result<(), AppError>) {
        let result = change(&mut self.app.borrow_mut());
        let worked = took_effect(&result);
        self.refresh(result.err().as_ref());
        let mut watch = self.mode_watch.get();
        let changed = watch.changed(self.app.borrow().running_preset(), worked);
        self.mode_watch.set(watch);
        if let Some(mode) = changed {
            self.pop_up(mode);
        }
    }

    /// Shows the popup with `mode` above the taskbar clock, without taking
    /// the focus, then fades it out. A new one starts the time again.
    fn pop_up(self: &Rc<Self>, mode: &str) {
        self.popup.set_mode(mode.into());
        let count = self.popups.get().wrapping_add(1);
        self.popups.set(count);
        // Placed before the show. The first time the size is not known yet,
        // so setup places it again.
        self.place_popup();
        // Every window shows without taking the focus (see main), so this
        // show leaves it where the user is typing.
        if let Err(error) = self.popup.show() {
            report(&format!("cannot show the power mode popup: {error}"));
            return;
        }
        if self.popup_ready.get() {
            self.popup.set_shown(true);
        } else {
            self.once_shown(
                |shell| shell.popup.window(),
                |shell, _| shell.set_up_popup(),
            );
        }
        self.after(POPUP_HOLD, count, move |shell| {
            shell.popup.set_shown(false);
            shell.after(POPUP_FADE, count, |shell| {
                let _ = shell.popup.hide();
            });
        });
    }

    /// Once the popup has a handle: no clicks, no taskbar button, placed
    /// again at its real size, then faded in. If it would take clicks it is
    /// hidden, and done again on the next popup.
    fn set_up_popup(&self) {
        let hittest = self.popup.window().with_winit_window(|window| {
            window.set_skip_taskbar(true);
            window.set_cursor_hittest(false)
        });
        // A popup that takes clicks could take the focus, so it stays
        // hidden, and setup runs again on the next one.
        match hittest {
            Some(Ok(())) => self.popup_ready.set(true),
            Some(Err(error)) => {
                report(&format!("the popup would take clicks: {error}"));
                let _ = self.popup.hide();
                return;
            }
            None => {
                report("the popup has no window");
                let _ = self.popup.hide();
                return;
            }
        }
        self.place_popup();
        self.popup.set_shown(true);
    }

    /// Runs `then` after `wait`, unless another popup came since `count`.
    fn after(self: &Rc<Self>, wait: Duration, count: u32, then: impl FnOnce(&Rc<Self>) + 'static) {
        let shell = Rc::downgrade(self);
        Timer::single_shot(wait, move || {
            if let Some(shell) = shell.upgrade()
                && shell.popups.get() == count
            {
                then(&shell);
            }
        });
    }

    fn place_popup(&self) {
        let area = match work_area() {
            Ok((x, y, width, height)) => Rect {
                x,
                y,
                width,
                height,
            },
            Err(error) => {
                report(&format!("the popup can't find the taskbar: {error}"));
                return;
            }
        };
        let size = self.popup.window().size();
        let (x, y) = popup_position(area, (px(size.width), px(size.height)));
        self.popup
            .window()
            .set_position(PhysicalPosition::new(x, y));
    }

    /// Fills both windows again. `error` is shown above the groups, if any.
    pub fn refresh(&self, error: Option<&AppError>) {
        let mut app = self.app.borrow_mut();
        let mut errors: Vec<String> = error.iter().map(ToString::to_string).collect();
        let header = app.header(self.readings.get());
        self.say_read_errors(app.read_errors());
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
        self.window
            .set_vapor_light_on(app.vapor_light_on() == Some(true));
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
    /// since the touchpad key can change it at any time. The reads that
    /// failed are logged once in a while.
    fn tick_readings(&self) {
        let (readings, mut errors) = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            (sampler.sample(sensors), sampler.errors().to_vec())
        };
        self.readings.set(readings);
        let mut app = self.app.borrow_mut();
        let header = app.header(readings);
        errors.extend_from_slice(app.read_errors());
        self.say_read_errors(&errors);
        bind::header(&self.window, &header);
        if let Ok(view) = app.keyboard_view() {
            bind::keyboard(&self.window, &view);
        }
    }

    /// Logs the failed reads not logged in the quiet time.
    fn say_read_errors(&self, errors: &[String]) {
        for error in self.read_errors.borrow_mut().to_say(errors, Instant::now()) {
            report(&format!("read failed: {error}"));
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
