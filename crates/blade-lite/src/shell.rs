//! The app and its windows: the settings window, the tray flyout and the
//! power mode popup.

mod popup;
mod razer;
mod ticks;
mod windows;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_core::{Controller, PollSchedule, Query, Readings, ReadingsSampler};
use blade_lite::{App, AppError, SaidLately, ShownErrors, report, took_effect};
use platform::{
    DisplayApi, KeyInjector, Programs, Registry, WindowsDisplays, WindowsKeys, WindowsPrograms,
    WindowsRegistry, WindowsSensors,
};
use razer_device::{Device, HidTransport, SystemClock};
use ui::{FlyoutView, ModeWatch};

use crate::{FlyoutWindow, MainWindow, ModePopup, bind};

/// The parts of the laptop the shell's app drives: the real ones, or fakes
/// in the window tests.
pub trait Parts: 'static {
    type Controller: Controller + Query;
    type Displays: DisplayApi;
    type Registry: Registry;
    type Keys: KeyInjector;
    /// What runs, to look for Razer's own software. Read only.
    type Programs: Programs + Default;
    /// Whether the laptop runs on its battery now. Read only.
    fn on_battery() -> bool;
}

/// The real laptop.
pub struct Laptop;

impl Parts for Laptop {
    type Controller = Device<HidTransport, SystemClock>;
    type Displays = WindowsDisplays;
    type Registry = WindowsRegistry;
    type Keys = WindowsKeys;
    type Programs = WindowsPrograms;

    fn on_battery() -> bool {
        crate::background::on_battery_now()
    }
}

/// The app over the parts `P`.
pub type AppOn<P> = App<
    <P as Parts>::Controller,
    <P as Parts>::Displays,
    <P as Parts>::Registry,
    <P as Parts>::Keys,
>;

/// A sensor or fan read error is logged once, then not again for this long,
/// since it can come and go on each read.
const READ_ERROR_QUIET: Duration = Duration::from_secs(10 * 60);

pub struct Shell<P: Parts = Laptop> {
    window: MainWindow,
    flyout: FlyoutWindow,
    popup: ModePopup,
    app: RefCell<AppOn<P>>,
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
    /// The resumes still waiting out their pause. The touchpad key is not
    /// painted on a Num Lock or touchpad change then, as the controller is
    /// not ready; the resume paints it.
    resuming: Cell<u32>,
    programs: P::Programs,
    /// The warning for the Razer software last found, shown above the
    /// groups while it runs.
    razer_warning: RefCell<Option<String>>,
    /// Whether the window was opened for Razer software. Only once per run.
    razer_opened: Cell<bool>,
}

impl<P: Parts> Shell<P> {
    pub fn new(
        window: MainWindow,
        flyout: FlyoutWindow,
        popup: ModePopup,
        app: AppOn<P>,
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
            resuming: Cell::new(0),
            programs: P::Programs::default(),
            razer_warning: RefCell::new(None),
            razer_opened: Cell::new(false),
        })
    }

    pub fn window(&self) -> &MainWindow {
        &self.window
    }

    pub fn flyout(&self) -> &FlyoutWindow {
        &self.flyout
    }

    #[cfg(test)]
    pub fn popup(&self) -> &ModePopup {
        &self.popup
    }

    #[cfg(test)]
    pub fn app(&self) -> std::cell::RefMut<'_, AppOn<P>> {
        self.app.borrow_mut()
    }

    /// Puts back the rate from before battery as the app quits. A failure
    /// goes to the log, as no window is left to show it.
    pub fn quit(&self) {
        if let Err(error) = self.app.borrow_mut().on_quit() {
            report(&format!("the panel may stay at 60 Hz: {error}"));
        }
    }

    /// Reads the keyboard brightness while Fn is held or as it comes up,
    /// as Fn + F10 or F11 may have set it. The window is filled again only for a new level
    /// or a failed send or save, so an Fn press for anything else leaves a
    /// shown error. A failed read is only logged, at most once in a quiet
    /// spell, as Fn is pressed for many things. Nothing is read while a
    /// resume waits: the controller may still run another level, and the
    /// resume sends the saved one.
    pub fn follow_brightness_keys(&self) {
        if self.resuming.get() > 0 {
            return;
        }
        let result = self.app.borrow_mut().on_brightness_keys();
        match result {
            Ok(false) => {}
            Ok(true) => self.refresh(None),
            Err(AppError::Device(error)) => {
                self.say_read_errors(&[format!("keyboard brightness: {error}")]);
            }
            Err(error) => self.refresh(Some(&error)),
        }
    }

    /// Runs one change, then shows the new state, or the error if it failed.
    /// A new power mode, from any change the laptop took (even one not
    /// saved), shows the popup.
    pub fn act(self: &Rc<Self>, change: impl FnOnce(&mut AppOn<P>) -> Result<(), AppError>) {
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
        // The Razer warning is logged when it changes, not with the errors.
        let warning = self.razer_warning.borrow();
        let shown: Vec<&str> = warning
            .as_deref()
            .into_iter()
            .chain((!errors.is_empty()).then_some(errors.as_str()))
            .collect();
        self.window.set_error(shown.join("\n").into());
    }
}
