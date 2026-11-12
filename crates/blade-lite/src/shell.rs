//! The app and its windows: the settings window, the tray flyout and the
//! popup.

mod popup;
mod razer;
mod settings;
mod ticks;
mod windows;

use std::cell::{Cell, RefCell};
use std::io;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use blade_core::{
    ApplyError, Controller, PollSchedule, Query, RESUME_PAUSE, Readings, ReadingsSampler,
};
use blade_lite::{App, AppError, Rect, SaidLately, ShownErrors, report, took_effect};
use platform::{
    DisplayApi, FileDialog, KeyInjector, PowerEvent, Programs, Registry, WindowsDisplays,
    WindowsKeys, WindowsPrograms, WindowsRegistry, WindowsSensors,
};
use razer_device::{Device, DeviceError, HidTransport, Reopening, SystemClock};
use razer_proto::BLADE_18;
use ui::{FanGuard, HeatWatch, ModeWatch, TouchpadWatch};

use popup::{POPUP_HOLD, POWER_MODE};

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
    /// Whether the laptop runs on its battery now; `None` if Windows can't
    /// tell. Read only.
    fn on_battery() -> Option<bool>;
    /// Asks the user for a file in the `dialog` over the `owner` window,
    /// then calls `done` on the app's thread with the file, `None` if the
    /// user cancels, or the dialog's error. The app keeps running while
    /// the dialog is open.
    fn pick_file(owner: Option<isize>, dialog: FileDialog, done: PickedFile);
    /// Whether the default mic is muted now. `None` if it can't be read.
    fn mic_muted() -> Option<bool>;
    /// Mutes the default mic if it is not muted, or the other way round.
    /// Whether it is muted after; `None` if that can't be told.
    fn toggle_mic_mute() -> Option<bool>;
    /// What the opens of the controller again asked since the last call.
    fn controller_reopened() -> Reopened;
    /// Lets the controller send on its new handle.
    fn release_controller();
    /// Whether Windows still waits for the app to act on its last sleep
    /// notice. If not, the app may have been frozen and thawed since, so a
    /// send for it could reach a controller that just woke (ticket 169).
    fn sleep_notice_waits() -> bool;
}

/// What [`Parts::pick_file`] calls with the user's answer.
pub type PickedFile = Box<dyn FnOnce(io::Result<Option<PathBuf>>)>;

/// The controller, opened again after its handle dies.
pub type LaptopTransport = Reopening<HidTransport, fn(bool) -> io::Result<HidTransport>>;

/// How long an open again waits after the last one: each open lists every
/// HID device, on the event-loop thread.
pub const REOPEN_GAP: Duration = Duration::from_secs(5);

/// What the last opens again asked of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reopened {
    /// Nothing new.
    #[default]
    No,
    /// A new handle waits, held: pause, look for Razer software, release
    /// it and send the settings again.
    Opened,
    /// An open failed for a new reason: only look for Razer software (a
    /// Razer driver that took the controller makes every open fail).
    Failed,
}

thread_local! {
    /// What [`reopen_controller`] asked since the last look. Both run on the
    /// event-loop thread.
    static REOPENED: Cell<Reopened> = const { Cell::new(Reopened::No) };
    /// Set by the controller while a new handle waits for the shell.
    pub static CONTROLLER_HELD: Rc<Cell<bool>> = Rc::default();
    /// The last failed open again, so each one is logged once.
    static REOPEN_ERRORS: RefCell<ShownErrors> = RefCell::new(ShownErrors::default());
}

/// Opens the controller again after its handle died (a driver install, the
/// USB device going and coming back). See [`note_open`].
pub fn reopen_controller(worked: bool) -> io::Result<HidTransport> {
    let opened = HidTransport::open(&BLADE_18);
    let asked =
        REOPEN_ERRORS.with_borrow_mut(|said| note_open(said, worked, opened.as_ref().err()));
    if asked != Reopened::No {
        REOPENED.set(asked);
    }
    opened
}

/// Logs an open again (`error` if it failed; `worked`: a reply was read on the last
/// handle): the first that works in a run of failures, and each new
/// reason one fails once. An open that works always asks the shell to
/// release the handle; a failure asks for a look only when it is new.
pub fn note_open(said: &mut ShownErrors, worked: bool, error: Option<&io::Error>) -> Reopened {
    let problem = error.map_or_else(String::new, |error| {
        format!("can't open the controller again: {error}")
    });
    let new = said.new_text(&problem).is_some();
    if new {
        report(&problem);
    }
    match error {
        None => {
            if worked {
                report("the controller was opened again");
            }
            Reopened::Opened
        }
        Some(_) if new => Reopened::Failed,
        Some(_) => Reopened::No,
    }
}

/// The real laptop.
pub struct Laptop;

impl Parts for Laptop {
    type Controller = Device<LaptopTransport, SystemClock>;
    type Displays = WindowsDisplays;
    type Registry = WindowsRegistry;
    type Keys = WindowsKeys;
    type Programs = WindowsPrograms;

    fn on_battery() -> Option<bool> {
        crate::background::on_battery_now()
    }

    /// A read that fails is said once, until a read works again.
    fn mic_muted() -> Option<bool> {
        let read = platform::mic_muted();
        say_mic_error(read.as_ref().err());
        read.ok()
    }

    fn controller_reopened() -> Reopened {
        REOPENED.take()
    }

    fn release_controller() {
        CONTROLLER_HELD.with(|held| held.set(false));
    }

    fn sleep_notice_waits() -> bool {
        crate::background::sleep_notice_waits()
    }

    fn toggle_mic_mute() -> Option<bool> {
        let toggled = platform::toggle_mic_mute();
        if let Err(error) = &toggled {
            report(&format!("Fn + mic did nothing: {error}"));
        }
        toggled.ok().or_else(Self::mic_muted)
    }

    /// The dialog runs on a thread of its own, so the polls, power events
    /// and popups go on while it is open. Without the window's handle no
    /// dialog opens, as it would then not block the window.
    fn pick_file(owner: Option<isize>, dialog: FileDialog, done: PickedFile) {
        let Some(owner) = owner else {
            done(Err(io::Error::other("the window has no handle yet")));
            return;
        };
        settings::PICKED.set(Some(done));
        let spawned = std::thread::Builder::new().spawn(move || {
            let picked = std::panic::catch_unwind(|| platform::pick_file(Some(owner), dialog))
                .unwrap_or_else(|_| Err(io::Error::other("the dialog's thread panicked")));
            let posted = slint::invoke_from_event_loop(move || {
                if let Some(done) = settings::PICKED.take() {
                    done(picked);
                }
            });
            if let Err(error) = posted {
                report(&format!("the picked file was lost: {error}"));
            }
        });
        if let Err(error) = spawned
            && let Some(done) = settings::PICKED.take()
        {
            done(Err(error));
        }
    }
}

/// Whether a failed mic read has been said since the last one that worked.
static MIC_ERROR_SAID: AtomicBool = AtomicBool::new(false);

/// Whether a send failed before reaching the controller: its handle is
/// held after an open again (ticket 132), or was just opened.
fn nothing_sent(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Apply(ApplyError::Device(DeviceError::Io(io)))
            if io.kind() == io::ErrorKind::NotConnected
    )
}

/// Says a failed mic read once, until a read works again: the mute is read
/// on each poll.
fn say_mic_error(error: Option<&io::Error>) {
    let said = MIC_ERROR_SAID.swap(error.is_some(), Ordering::Relaxed);
    if let (Some(error), false) = (error, said) {
        report(&format!("the mic key stays plain: {error}"));
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
const READ_ERROR_QUIET: Duration = Duration::from_mins(10);

/// The error a run ends with when a panic (a bug) stopped it.
pub const PANICKED: &str = "a bug stopped the app (a panic, in the log)";

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
    /// The tray icon the open flyout was first placed at, kept for one more
    /// placing after a DPI scale change (taken by it, cleared on hide).
    flyout_icon: Cell<Option<Rect>>,
    shown_errors: RefCell<ShownErrors>,
    read_errors: RefCell<SaidLately>,
    mode_watch: Cell<ModeWatch>,
    /// The touchpad state last seen, for the touchpad popup.
    touchpad_watch: Cell<TouchpadWatch>,
    /// The CPU and GPU hot spells, for the heat popup.
    heat_watch: Cell<HeatWatch>,
    /// The fan guard (ticket 129), fed by the same readings.
    fan_guard: Cell<FanGuard>,
    /// Counts the popups, so the fade of an older one leaves a newer alone.
    popups: Cell<u32>,
    /// Whether the popup has been made click-through.
    popup_ready: Cell<bool>,
    /// The resumes still waiting out their pause. The controller may not be
    /// ready then: clicks and the power-mode key do nothing, and the touchpad
    /// key is not painted on a Num Lock or touchpad change (the resume
    /// paints it). A change of power source is dropped: the resume reads
    /// the source when the pause ends (ticket 88).
    resuming: Cell<u32>,
    /// When the last of those pauses began, so a quit can wait out the
    /// rest of it.
    pause_began: Cell<Option<Instant>>,
    /// The app's count of failed preset sends at the start of the last tick.
    /// A poll sends the preset again only while it is unchanged, so never
    /// in the tick of a failure or the one after it.
    preset_failures_seen: Cell<u32>,
    /// The power events, read on each poll and before each click or key.
    power: RefCell<Option<Rc<Receiver<PowerEvent>>>>,
    programs: P::Programs,
    /// The warning for the Razer software last found, shown above the
    /// groups while it runs.
    razer_warning: RefCell<Option<String>>,
    /// Whether a Razer updater ended since the last tick, which then sends
    /// the settings again.
    updater_ended: Cell<bool>,
    /// When Razer software was last looked for by the poll.
    razer_looked: Cell<Option<Instant>>,
    /// Whether the window was opened for Razer software. Only once per run.
    razer_opened: Cell<bool>,
    /// Whether a file dialog is open, so a second one is not opened.
    dialog_open: Cell<bool>,
    /// The background parts a panic stopped, shown above the groups.
    stopped_parts: RefCell<Vec<&'static str>>,
    /// Whether a read failure of a closed or held controller was logged
    /// since it was last opened again.
    controller_gone: Cell<bool>,
    /// Whether the quit preset went, so it goes once.
    quit_sent: Cell<bool>,
    /// Whether Windows told of a sleep and no wake came since (ticket 169).
    /// It counts as one more pause in `resuming`, so nothing is sent until
    /// the wake, which ends it.
    asleep: Cell<bool>,
    /// Whether the pause of an open again or an updater ended in a sleep,
    /// so the wake releases the handle and reads the firmware again too.
    redo_at_wake: Cell<bool>,
}

impl<P: Parts> Shell<P> {
    pub fn new(
        window: MainWindow,
        flyout: FlyoutWindow,
        popup: ModePopup,
        app: AppOn<P>,
    ) -> Rc<Self> {
        let mode_watch = Cell::new(ModeWatch::new(app.running_preset()));
        let touchpad_watch = Cell::new(TouchpadWatch::new(app.touchpad_on()));
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
            flyout_icon: Cell::new(None),
            shown_errors: RefCell::new(ShownErrors::default()),
            read_errors: RefCell::new(SaidLately::new(READ_ERROR_QUIET)),
            mode_watch,
            touchpad_watch,
            heat_watch: Cell::new(HeatWatch::default()),
            fan_guard: Cell::new(FanGuard::default()),
            popups: Cell::new(0),
            popup_ready: Cell::new(false),
            resuming: Cell::new(0),
            pause_began: Cell::new(None),
            preset_failures_seen: Cell::new(0),
            power: RefCell::new(None),
            programs: P::Programs::default(),
            razer_warning: RefCell::new(None),
            razer_looked: Cell::new(None),
            updater_ended: Cell::new(false),
            razer_opened: Cell::new(false),
            dialog_open: Cell::new(false),
            stopped_parts: RefCell::new(Vec::new()),
            quit_sent: Cell::new(false),
            asleep: Cell::new(false),
            redo_at_wake: Cell::new(false),
            controller_gone: Cell::new(false),
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
    pub fn set_readings(&self, readings: Readings) {
        self.readings.set(readings);
    }

    #[cfg(test)]
    pub fn app(&self) -> std::cell::RefMut<'_, AppOn<P>> {
        self.app.borrow_mut()
    }

    /// Sends Balanced (Silent stays Silent; see `App::quit_preset`), then
    /// puts back the rate from before battery, as the app quits. A failure
    /// goes to the log, as no window is left to show it.
    pub fn quit(&self) {
        // Once: a session end may have sent it already.
        if self.quit_sent.get() {
            return;
        }
        // A session end can come inside a Windows call made while the app
        // is borrowed: the quit after the loop then sends it.
        let Ok(mut app) = self.app.try_borrow_mut() else {
            report(
                "the app was busy, so the quit preset was not sent now: it goes at the end of the loop, if Windows lets the app run that long",
            );
            return;
        };
        self.quit_sent.set(true);
        self.note_pending_wake();
        self.wait_out_the_pause();
        // Closed however the sends end, a panic too.
        let sent = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // First: the fans matter more than the rate.
            let mut sent = app.quit_preset();
            if sent.as_ref().is_err_and(nothing_sent) {
                // The controller holds a new handle (or just opened one), so
                // nothing reached it: one more try is safe. It skips the
                // Razer look a reopen gets, and the 2 s pause unless one was
                // already running, as the app ends; a controller not ready
                // fails the reply check, and is logged.
                P::release_controller();
                sent = app.quit_preset();
            }
            if let Err(error) = sent {
                report(&format!(
                    "Balanced was not sent at quit, so a manual fan or Performance may keep running: {error}"
                ));
            }
        }));
        // Nothing more may reach the controller: an event still queued, or
        // the call this one came inside, would undo the quit preset.
        app.close_controller();
        if let Err(panic) = sent {
            std::panic::resume_unwind(panic);
        }
        // After the fans: only the rate, a Windows call that may be slow.
        if let Err(error) = app.on_quit() {
            report(&format!("the panel may stay at 60 Hz: {error}"));
        }
    }

    /// Starts a pause after a wake, an open again or an updater: the
    /// controller may not be ready, so nothing is sent until it ends.
    pub(super) fn begin_pause(&self) {
        self.resuming.set(self.resuming.get().saturating_add(1));
        self.pause_began.set(Some(Instant::now()));
    }

    /// A wake that Windows told of but no poll has read yet starts its
    /// pause now, so the quit waits it out too. The events read here are
    /// dropped: the app ends.
    fn note_pending_wake(&self) {
        let power = self.power.borrow().clone();
        if power.is_some_and(|power| power.try_iter().any(|event| event == PowerEvent::Resumed)) {
            self.begin_pause();
        }
    }

    /// Waits out the rest of a pause that has not ended (ticket 137): a
    /// quit sends at once otherwise, and the event loop that would end the
    /// pause has stopped. At most [`RESUME_PAUSE`] (see `end_session` for
    /// the time Windows gives). The newest pause ends last, as each lasts
    /// [`RESUME_PAUSE`], so its start is the one kept. A sleep's count is
    /// no pause: the quit preset is safe to send then.
    fn wait_out_the_pause(&self) {
        if self.resuming.get() == u32::from(self.asleep.get()) {
            return;
        }
        let left = self.pause_began.get().map_or(RESUME_PAUSE, |began| {
            RESUME_PAUSE.saturating_sub(began.elapsed())
        });
        if !left.is_zero() {
            report(&format!(
                "the quit waits {} ms for the controller to be ready",
                left.as_millis()
            ));
            std::thread::sleep(left);
        }
    }

    /// Runs `serve`, the app's run after the start, then quits as Quit
    /// does, however it ended: an error or a panic (a bug) too ends the app
    /// with the start's settings sent and nothing to watch them. A panic
    /// ends as the error [`PANICKED`]; the panic hook has logged it. Only
    /// after the start.
    pub fn serve<T, E: From<&'static str>>(
        &self,
        serve: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        // Each borrow taken in `serve` is dropped as the panic unwinds, so
        // the quit finds the app free.
        let served = std::panic::catch_unwind(AssertUnwindSafe(serve));
        if served.is_err() {
            self.app.borrow_mut().note_panic();
        }
        // A quit that panics too still ends with the start box.
        if std::panic::catch_unwind(AssertUnwindSafe(|| self.quit())).is_err() {
            report("the quit panicked too, so Balanced may not have been sent");
            return Err(PANICKED.into());
        }
        served.unwrap_or_else(|_| Err(PANICKED.into()))
    }

    /// Windows ends the session (shut down, restart, sign out) or an
    /// installer closes the app: quits as Quit does, then ends the event
    /// loop. Windows may end the process when this returns. Like Quit it
    /// first waits out the rest of a pause after a wake (at most 2 s),
    /// taken to fit in the about 5 s Windows gives before it shows the
    /// apps that block it: not checked on the laptop (laptop check 30).
    pub fn end_session(&self) {
        report("Windows ends the session, so the app quits");
        if std::panic::catch_unwind(AssertUnwindSafe(|| self.quit())).is_err() {
            report("the quit at the session end panicked");
        }
        // An error: the loop has already ended.
        let _ = slint::quit_event_loop();
    }

    /// Notes a background part (a thread) that a panic stopped: the app
    /// goes on without it, so the window opens and says so until the app
    /// starts again. Nothing is sent.
    pub fn part_stopped(self: &Rc<Self>, part: &'static str) {
        report(&format!("a bug stopped the {part} part"));
        self.stopped_parts.borrow_mut().push(part);
        // It fills the window too.
        self.show_window();
    }

    /// Reads the keyboard brightness while Fn is held or as it comes up,
    /// as Fn + F10 or F11 may have set it. The window is filled again only for a new level
    /// or a failed send or save, so an Fn press for anything else leaves a
    /// shown error. A failed read is only logged, at most once in a quiet
    /// spell, as Fn is pressed for many things. Nothing is read while a
    /// resume waits: the controller may still run another level, and the
    /// resume sends the saved one.
    pub fn follow_brightness_keys(self: &Rc<Self>) {
        self.follow_own_power();
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
            self.pop_up(POWER_MODE, mode, POPUP_HOLD);
        }
    }

    /// Runs a change the user asked for: a click or the power-mode key.
    /// The power events that came first are followed first, so a wake or a
    /// new power source not yet polled is seen. While a resume waits it does
    /// nothing: the controller may not be awake yet, and the charger may have
    /// been pulled in the sleep. The resume fills the windows again, which
    /// puts back a switch the click flipped. After a sleep notice with no
    /// wake since, it says so, as no wake may ever come.
    pub fn user_act(self: &Rc<Self>, change: impl FnOnce(&mut AppOn<P>) -> Result<(), AppError>) {
        // A Razer updater may have started since the last look.
        if self.look_for_razer() {
            self.show_window();
        }
        self.follow_own_power();
        if self.asleep.get() {
            self.refresh(Some(&AppError::Asleep));
        } else if self.resuming.get() == 0 {
            self.act(change);
        }
    }

    /// Sets both touchpad switches. `None`, a state that can't be read,
    /// shows off and greyed.
    fn show_touchpad(&self, on: Option<bool>) {
        self.window.set_touchpad_on(on == Some(true));
        self.window.set_touchpad_known(on.is_some());
        self.flyout.set_touchpad_on(on == Some(true));
        self.flyout.set_touchpad_known(on.is_some());
    }

    /// Fills both windows again. `error` is shown above the groups, if any.
    pub fn refresh(&self, error: Option<&AppError>) {
        let mut app = self.app.borrow_mut();
        let mut errors: Vec<String> = error.iter().map(ToString::to_string).collect();
        let header = self.header(&mut app, self.readings.get());
        self.say_read_errors(app.read_errors());
        let performance = app.performance_view();
        bind::header(&self.window, &header);
        bind::performance(&self.window, &performance);
        bind::flyout_performance(&self.flyout, &performance, &header);
        // A failed view leaves its parts as last shown and shows the error.
        if let Some(view) = self.display_view(&mut app, &mut errors) {
            bind::display(&self.window, &view);
            bind::flyout_display(&self.flyout, &view);
        }
        match app.keyboard_view() {
            Ok(view) => {
                bind::keyboard(&self.window, &view);
                bind::flyout_keyboard(&self.flyout, &view);
            }
            Err(error) => errors.push(error.to_string()),
        }
        // A switch flips itself on a click. Set it from the config, the
        // touchpad state read now, or the logo state last read or set, even
        // when a view failed, so it never shows a value that was not saved
        // or sent. A touchpad state that can't be read shows off, greyed.
        let sixty_hz = app.config().sixty_hz_on_battery;
        self.window.set_sixty_hz_on_battery(sixty_hz);
        self.flyout.set_sixty_hz_on_battery(sixty_hz);
        let touchpad = app.read_touchpad();
        if let Err(error) = &touchpad {
            errors.push(error.to_string());
        }
        self.show_touchpad(touchpad.ok());
        self.window
            .set_vapor_light_on(app.vapor_light_on() == Some(true));
        self.window.set_logo_shown(app.logo_on().is_some());
        self.window.set_logo_on(app.logo_on() == Some(true));
        // Equal lines next to each other show once, such as a click's error
        // that the fill finds again.
        errors.dedup();
        let errors = errors.join("\n");
        if let Some(new) = self.shown_errors.borrow_mut().new_text(&errors) {
            report(new);
        }
        // The Razer and firmware warnings are logged when they come, not
        // with the errors.
        let warning = self.razer_warning.borrow();
        let mismatch = app.preset_mismatch().map(|differs| {
            format!("The controller does not run the preset sent ({differs}). It is not sent again by itself.")
        });
        let stopped = self.stopped_parts.borrow();
        let stopped = stopped
            .iter()
            .map(|part| format!("A bug stopped the {part} part. Quit Blade Lite from the tray and start it again."))
            .collect::<Vec<_>>();
        let shown: Vec<&str> = warning
            .as_deref()
            .into_iter()
            .chain(stopped.iter().map(String::as_str))
            .chain(app.firmware_warning())
            .chain(mismatch.as_deref())
            .chain((!errors.is_empty()).then_some(errors.as_str()))
            .collect();
        self.window.set_error(shown.join("\n").into());
    }
}
