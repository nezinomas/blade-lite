//! Razer's own software found at start or after a resume: a warning, and
//! nothing sent for it.

use std::time::Duration;

use blade_core::{Config, RESUME_PAUSE};
use platform::PowerEvent;

use super::{Fixture, fixture, log_path};

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

#[test]
fn a_failed_look_shows_no_warning() {
    let f = fixture(Config::default());
    f.shell.programs().fail();

    assert!(!look(&f));

    assert_eq!(f.shell.window().get_error(), "");
}
