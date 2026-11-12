//! The window, flyout and popup, drawn off screen over fakes: scripted
//! clicks (`clicks`) and pictures compared with saved ones (`screens`).
//! Nothing here opens a real window or touches the laptop. The only real
//! Windows call is a read of the work area, when a click shows the popup.

mod clicks;
mod razer;
mod screens;
mod settings;
mod sleep;

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use blade_core::{Config, Log};
use blade_lite::open_quiet_log;
use i_slint_backend_testing::{
    AccessibleRole, ElementHandle, ElementRoot, TestingBackend, TestingBackendOptions,
};
use platform::{FileDialog, Programs};
use razer_proto::{BLADE_18, Rgb};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, LogicalSize};
use tempfile::TempDir;

use crate::fakes::{FakeController, FakeDisplays, FakeKeys, FakeRegistry, app_on};
use crate::shell::{Parts, PickedFile, Reopened, Shell};
use crate::{FlyoutWindow, MainWindow, ModePopup, bind};

/// The fakes in place of the laptop.
pub struct Fakes;

impl Parts for Fakes {
    type Controller = FakeController;
    type Displays = FakeDisplays;
    type Registry = FakeRegistry;
    type Keys = FakeKeys;
    type Programs = FakePrograms;

    fn on_battery() -> Option<bool> {
        (!FAKE_SOURCE_UNKNOWN.get()).then(|| FAKE_ON_BATTERY.get())
    }

    /// Answers at once, unless the test holds the answer back.
    fn pick_file(_owner: Option<isize>, dialog: FileDialog, done: PickedFile) {
        FAKE_DIALOGS.with_borrow_mut(|dialogs| dialogs.push(dialog));
        let picked = FAKE_PICKED.with_borrow(|picked| match picked {
            FakePick::File(path) => Ok(Some(path.clone())),
            FakePick::Cancel => Ok(None),
            FakePick::Fail => Err(io::ErrorKind::Other.into()),
        });
        if FAKE_HOLD.get() {
            FAKE_HELD.set(Some(Box::new(move || done(picked))));
        } else {
            done(picked);
        }
    }

    fn mic_muted() -> Option<bool> {
        FAKE_MIC.get()
    }

    fn toggle_mic_mute() -> Option<bool> {
        FAKE_MIC.set(FAKE_MIC.get().map(|muted| !muted));
        FAKE_MIC.get()
    }

    fn controller_reopened() -> Reopened {
        FAKE_REOPENED.take()
    }

    fn release_controller() {
        FAKE_RELEASED.set(true);
    }

    fn sleep_notice_waits() -> bool {
        !FAKE_SLEEP_NOTICE_LATE.get()
    }
}

thread_local! {
    /// The power source the fakes report: AC unless a test sets it. Each
    /// test runs on its own thread.
    pub static FAKE_ON_BATTERY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether the power source can't be told: known unless a test sets it.
    pub static FAKE_SOURCE_UNKNOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What the user does in a file dialog: cancel unless a test sets it.
    pub static FAKE_PICKED: RefCell<FakePick> = const { RefCell::new(FakePick::Cancel) };
    /// Whether the dialog stays open until the test answers it.
    pub static FAKE_HOLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The answer of a dialog held open.
    pub static FAKE_HELD: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    /// The default mic's mute: unknown unless a test sets it.
    pub static FAKE_MIC: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// What opens of the controller again asked: nothing unless a test sets it.
    pub static FAKE_REOPENED: std::cell::Cell<Reopened> = const { std::cell::Cell::new(Reopened::No) };
    /// Whether the shell let the controller send on a new handle.
    pub static FAKE_RELEASED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether Windows stopped waiting for the last sleep notice before the
    /// app read it: it waits unless a test sets it.
    pub static FAKE_SLEEP_NOTICE_LATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The file dialogs shown.
    pub static FAKE_DIALOGS: RefCell<Vec<FileDialog>> = const { RefCell::new(Vec::new()) };
}

/// What the user does in a file dialog.
pub enum FakePick {
    File(PathBuf),
    Cancel,
    /// The dialog itself fails.
    Fail,
}

/// What runs: nothing at first. `None` fails the read.
pub struct FakePrograms(RefCell<Option<Vec<String>>>);

impl Default for FakePrograms {
    fn default() -> Self {
        Self(RefCell::new(Some(Vec::new())))
    }
}

impl FakePrograms {
    pub fn set(&self, names: &[&str]) {
        let names = names.iter().map(|&name| name.to_owned()).collect();
        self.0.replace(Some(names));
    }

    pub fn fail(&self) {
        self.0.replace(None);
    }
}

impl Programs for FakePrograms {
    fn running(&self) -> io::Result<Vec<String>> {
        self.0
            .borrow()
            .clone()
            .ok_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

/// The Windows default accent, as the app uses when it can't read one.
const ACCENT: Rgb = Rgb::new(0x00, 0x78, 0xD4);

/// Tall enough that the window shows every group without scrolling.
const WINDOW_SIZE: LogicalSize = LogicalSize::new(760.0, 1380.0);

/// The shell over fakes, its windows shown off screen, and the folder that
/// holds its config.
pub struct Fixture {
    pub shell: Rc<Shell<Fakes>>,
    _dir: TempDir,
}

/// Starts the shell over fakes with `config`, as the app starts, with the
/// lid logo on and the charge limit off, so every row shows.
pub fn fixture(config: Config) -> Fixture {
    fixture_with_programs(config, &[])
}

/// [`fixture`], with `programs` running at start: the shell looks for
/// Razer software before the first fill, as the app does.
pub fn fixture_with_programs(config: Config, programs: &[&str]) -> Fixture {
    // Each test runs on its own thread, and Slint keeps one platform per
    // thread. The software renderer lets a test take a picture.
    slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
        mock_time: true,
        threading: false,
        renderer_name: Some("software".into()),
    })))
    .expect("the first Slint call on this thread");
    log_path();
    FAKE_ON_BATTERY.set(false);
    FAKE_SOURCE_UNKNOWN.set(false);
    FAKE_MIC.set(None);
    FAKE_HOLD.set(false);
    FAKE_HELD.take();
    FAKE_SLEEP_NOTICE_LATE.set(false);
    let (mut app, dir, _) = app_on(&BLADE_18, config);
    let controller = &mut app.machine_mut().controller.inner;
    controller.logo_reply = Some(vec![0, 4, 1]);
    controller.charge = Some(0x50);
    // A checked firmware, as the laptop has: a reopen reads it again.
    controller.firmware = Some(vec![1, 6]);
    let started = app.start();
    let window = MainWindow::new().expect("the window");
    let flyout = FlyoutWindow::new().expect("the flyout");
    let popup = ModePopup::new().expect("the popup");
    window.set_accent(bind::colour(ACCENT));
    flyout.set_accent(bind::colour(ACCENT));
    popup.set_accent(bind::colour(ACCENT));
    let shell: Rc<Shell<Fakes>> = Shell::new(window, flyout, popup, app);
    bind::callbacks(&shell);
    shell.programs().set(programs);
    shell.look_for_razer();
    shell.refresh(started.err().as_ref());
    shell.window().window().set_size(WINDOW_SIZE);
    shell.window().show().expect("the window shows");
    shell.flyout().show().expect("the flyout shows");
    Fixture { shell, _dir: dir }
}

/// The test run's log, opened by the first fixture: the reports of every
/// test go there, not to stderr. Each run has its own, so two runs at once
/// don't mix.
pub fn log_path() -> &'static Path {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/window-tests");
        std::fs::create_dir_all(&dir).expect("the log folder");
        let path = dir.join(format!("blade-lite-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(
            open_quiet_log(Log::new(path.clone())),
            "a report came before the first fixture, so the log could not be opened"
        );
        path
    })
}

/// The one control in `root` with `role` and `label`.
pub fn control(root: &impl ElementRoot, role: AccessibleRole, label: &str) -> ElementHandle {
    let mut found: Vec<_> = ElementHandle::find_by_accessible_label(root, label)
        .filter(|element| element.accessible_role() == Some(role))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one {role:?} labelled {label:?}. None at all? The tests need a debug build:          build.rs adds the debug info that finds controls only there"
    );
    found.remove(0)
}

/// Clicks the middle of the control, as a mouse would.
pub fn click(root: &impl ElementRoot, role: AccessibleRole, label: &str) {
    control(root, role, label).mock_single_click(PointerEventButton::Left);
}

/// The label of the preset chosen in the window's Performance mode row.
pub fn selected_preset(f: &Fixture) -> String {
    use slint::Model;
    let presets = f.shell.window().get_presets();
    let chosen = presets.iter().find(|choice| choice.selected);
    chosen.expect("one preset is chosen").label.into()
}
