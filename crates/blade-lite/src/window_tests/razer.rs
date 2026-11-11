//! Razer's own software found at start or after a resume: a warning, and
//! nothing sent for it.

use std::time::Duration;

use blade_core::{Config, Preset, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::{BLADE_18, NativeMode, PerfMode};
use slint::{ComponentHandle, Model};

use super::{
    FAKE_ON_BATTERY, FAKE_RELEASED, FAKE_REOPENED, Fixture, click, fixture, fixture_with_programs,
    log_path,
};
use crate::Choice;
use crate::shell::{Reopened, note_open};

const SYNAPSE: &str = "Razer Synapse Service";

/// Looks, then fills the windows, as the app does at start.
fn look(f: &Fixture) -> bool {
    let first = f.shell.look_for_razer();
    f.shell.refresh(None);
    first
}

#[test]
fn razer_software_shows_a_warning_and_sends_nothing() {
    let f = fixture(Config::default());
    f.shell.programs().set(&["explorer.exe", SYNAPSE]);
    let before = f.shell.app().machine().controller.sent.len();

    let first = look(&f);

    assert!(first, "the first find opens the window");
    assert_eq!(f.shell.app().machine().controller.sent.len(), before);
    let error = f.shell.window().get_error();
    assert!(error.contains(SYNAPSE), "{error}");
    assert!(!error.contains("explorer"), "{error}");
    let log = std::fs::read_to_string(log_path()).unwrap();
    assert!(log.contains(SYNAPSE), "{log}");
}

#[test]
fn the_window_opens_for_razer_software_only_once() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    assert!(look(&f));

    assert!(!look(&f));
    assert!(f.shell.window().get_error().contains(SYNAPSE));
}

#[test]
fn the_warning_goes_once_razer_software_is_gone() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    look(&f);

    f.shell.programs().set(&[]);
    assert!(!look(&f));

    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn razer_software_is_looked_for_after_a_resume() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();

    f.shell.follow_power(&receiver);
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));

    assert!(f.shell.window().get_error().contains(SYNAPSE));
}

/// Opens the controller again, as a failed send would, and ticks once.
/// Returns what was sent then.
fn reopen(f: &Fixture, asked: Reopened) -> usize {
    f.shell.window().hide().unwrap();
    FAKE_RELEASED.set(false);
    let before = f.shell.app().machine().controller.sent.len();
    FAKE_REOPENED.set(asked);
    f.shell.tick();
    f.shell.app().machine().controller.sent.len() - before
}

fn pause_ends() {
    i_slint_backend_testing::mock_elapsed_time(RESUME_PAUSE + Duration::from_millis(1));
}

#[test]
fn after_the_controller_is_opened_again_razer_software_shows_and_the_window_opens() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    f.shell.tick();
    assert_eq!(
        f.shell.window().get_error(),
        "",
        "no look without a new open"
    );

    assert_eq!(
        reopen(&f, Reopened::Opened),
        0,
        "nothing is sent in the pause"
    );
    assert!(!FAKE_RELEASED.get(), "held in the pause");
    pause_ends();

    assert!(FAKE_RELEASED.get());
    assert!(f.shell.window().get_error().contains(SYNAPSE));
    assert!(f.shell.window().window().is_visible());
    assert_eq!(FAKE_REOPENED.get(), Reopened::No, "the flag is taken");
}

#[test]
fn after_the_controller_is_opened_again_the_settings_are_sent_again() {
    let f = fixture(Config::default());
    let before = f.shell.app().machine().controller.sent.len();

    assert_eq!(reopen(&f, Reopened::Opened), 0);
    pause_ends();

    assert!(f.shell.app().machine().controller.sent.len() > before);
    assert_eq!(f.shell.window().get_error(), "");
    assert!(!f.shell.window().window().is_visible());
}

#[test]
fn a_click_before_the_tick_after_an_open_again_sends_nothing() {
    let f = fixture(Config::default());
    let before = f.shell.app().machine().controller.sent.len();
    FAKE_REOPENED.set(Reopened::Opened);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(f.shell.app().machine().controller.sent.len(), before);
    pause_ends();
}

#[test]
fn a_charger_pulled_in_the_pause_after_an_open_again_sends_no_ac_preset() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    let before = f.shell.app().machine().controller.sent.len();

    reopen(&f, Reopened::Opened);
    FAKE_ON_BATTERY.set(true);
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.tick();
    pause_ends();

    let app = f.shell.app();
    let sent = &app.machine().controller.sent[before..];
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    assert!(
        !sent.iter().any(|command| performance.contains(command)),
        "{sent:?}"
    );
    assert!(app.on_battery());
}

#[test]
fn a_failed_open_again_only_looks_for_razer_software() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);

    assert_eq!(reopen(&f, Reopened::Failed), 0);

    assert!(f.shell.window().window().is_visible());
    assert!(!FAKE_RELEASED.get(), "no handle to release");
    // No pause: a click goes on at once.
    let before = f.shell.app().machine().controller.sent.len();
    click(f.shell.window(), Button, "Silent");
    assert!(f.shell.app().machine().controller.sent.len() > before);
}

#[test]
fn opens_again_are_logged_once_per_run_and_each_new_failure_once() {
    // Texts no other test uses, as the log is shared.
    let gone = std::io::Error::other("ReopenLogTest gone");
    let taken = std::io::Error::other("ReopenLogTest taken");
    let lines = |text: &str| {
        let log = std::fs::read_to_string(log_path()).unwrap();
        log.lines().filter(|line| line.contains(text)).count()
    };
    let mut said = blade_lite::ShownErrors::default();
    let opened_before = lines("the controller was opened again");

    assert_eq!(note_open(&mut said, true, Some(&gone)), Reopened::Failed);
    assert_eq!(note_open(&mut said, true, Some(&gone)), Reopened::No);
    assert_eq!(note_open(&mut said, true, Some(&taken)), Reopened::Failed);
    assert_eq!(note_open(&mut said, true, None), Reopened::Opened);
    // That handle died before any reply was read: the same run, not logged.
    assert_eq!(note_open(&mut said, false, None), Reopened::Opened);
    assert_eq!(note_open(&mut said, false, Some(&gone)), Reopened::Failed);

    assert_eq!(lines("ReopenLogTest gone"), 2);
    assert_eq!(lines("ReopenLogTest taken"), 1);
    // Other tests may log it too, at the same time.
    assert!(lines("the controller was opened again") > opened_before);
}
#[test]
fn an_unlisted_firmware_shows_a_warning_beside_the_razer_one() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    f.shell.app().machine_mut().controller.firmware = Some(vec![1, 9]);
    f.shell.app().check_firmware();

    look(&f);

    let error = f.shell.window().get_error();
    assert!(error.contains(SYNAPSE), "{error}");
    assert!(error.contains("firmware is 1.9"), "{error}");
}

/// The label chosen in `choices`.
fn chosen(choices: &slint::ModelRc<Choice>) -> String {
    let chosen = choices.iter().find(|choice| choice.selected);
    chosen.expect("one is chosen").label.into()
}

fn native_reads_since(f: &Fixture, before: usize) -> usize {
    let app = f.shell.app();
    let asked = &app.machine().controller.asked[before..];
    asked.iter().filter(|&c| *c == NativeMode::query()).count()
}

#[test]
fn a_window_fill_reads_no_native_mode_while_razer_software_runs() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    let before = f.shell.app().machine().controller.asked.len();

    look(&f);

    assert_eq!(native_reads_since(&f, before), 0);
}

#[test]
fn while_razer_software_runs_a_fill_shows_the_rate_and_the_native_mode_last_read() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    look(&f);
    f.shell.app().machine_mut().displays.0.current.hz = 120;

    f.shell.refresh(None);

    assert_eq!(chosen(&f.shell.window().get_rates()), "120 Hz");
    assert_eq!(chosen(&f.shell.flyout().get_rates()), "120 Hz");
    let natives = f.shell.window().get_native_modes();
    assert_eq!(chosen(&natives), "3840 × 2400, up to 240 Hz");
}

#[test]
fn with_razer_software_at_start_the_rates_show_and_no_native_mode_is_chosen() {
    let f = fixture_with_programs(Config::default(), &[SYNAPSE]);

    assert_eq!(native_reads_since(&f, 0), 0);
    assert_eq!(chosen(&f.shell.window().get_rates()), "240 Hz");
    assert_eq!(chosen(&f.shell.flyout().get_rates()), "240 Hz");
    let natives = f.shell.window().get_native_modes();
    assert!(natives.row_count() > 0);
    assert!(natives.iter().all(|choice| !choice.selected));
    assert_eq!(f.shell.window().get_native_note(), "Unknown");
}

#[test]
fn while_razer_software_runs_a_rate_click_sets_the_rate_and_reads_no_native_mode() {
    let f = fixture_with_programs(Config::default(), &[SYNAPSE]);

    click(f.shell.window(), Button, "120 Hz");

    assert_eq!(native_reads_since(&f, 0), 0);
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 120);
    assert_eq!(chosen(&f.shell.window().get_rates()), "120 Hz");
}

#[test]
fn while_razer_software_runs_a_refused_60_hz_tried_again_shows_with_no_native_mode_read() {
    let f = fixture_with_programs(
        Config {
            sixty_hz_on_battery: true,
            ..Config::default()
        },
        &[SYNAPSE],
    );
    FAKE_ON_BATTERY.set(true);
    f.shell.app().machine_mut().displays.1 = 1;
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::OnBattery).unwrap();

    for _ in 0..3 {
        f.shell.tick();
    }

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(chosen(&f.shell.window().get_rates()), "60 Hz");
    assert_eq!(native_reads_since(&f, 0), 0);
}

#[test]
fn while_razer_software_runs_a_native_mode_click_shows_the_new_mode() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    look(&f);

    click(f.shell.window(), Button, "1920 × 1200, up to 440 Hz");

    let natives = f.shell.window().get_native_modes();
    assert_eq!(chosen(&natives), "1920 × 1200, up to 440 Hz");
}

#[test]
fn a_failed_look_shows_no_warning() {
    let f = fixture(Config::default());
    f.shell.programs().fail();

    assert!(!look(&f));

    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn a_failed_look_keeps_the_warning() {
    let f = fixture(Config::default());
    f.shell.programs().set(&[SYNAPSE]);
    look(&f);

    f.shell.programs().fail();
    assert!(!look(&f));

    assert!(f.shell.window().get_error().contains(SYNAPSE));
}

#[test]
fn the_warning_is_logged_when_it_comes_and_goes() {
    // A name no other test uses, as the log is shared.
    const ONLY_HERE: &str = "RazerLogTest.exe";
    let lines = |text: &str| {
        let log = std::fs::read_to_string(log_path()).unwrap();
        log.lines().filter(|line| line.contains(text)).count()
    };
    let f = fixture(Config::default());
    f.shell.programs().set(&[ONLY_HERE]);
    let gone_before = lines("Razer software no longer runs");

    look(&f);
    look(&f);
    f.shell.programs().set(&[]);
    look(&f);

    assert_eq!(lines(ONLY_HERE), 1);
    // Other tests may log it too, at the same time.
    assert!(lines("Razer software no longer runs") > gone_before);
}
