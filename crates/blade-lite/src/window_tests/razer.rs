//! Razer's own software found at start or after a resume: a warning, and
//! nothing sent for it.

use std::time::Duration;

use blade_core::{Config, RESUME_PAUSE};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::NativeMode;
use slint::Model;

use super::{FAKE_ON_BATTERY, Fixture, click, fixture, fixture_with_programs, log_path};
use crate::Choice;

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
