//! The controller firmware is read once at start and logged; a version the
//! model profile does not list gets a warning, and the app still sends.

use blade_core::Config;
use razer_proto::FirmwareVersion;

use crate::fakes::TestApp;

fn checked(reply: Option<Vec<u8>>) -> TestApp {
    let (mut app, _dir, _) = crate::app_with(Config::default());
    app.machine_mut().controller.inner.firmware = reply;
    app.check_firmware();
    app
}

#[test]
fn a_listed_firmware_gives_no_warning() {
    for minor in [6, 7] {
        let app = checked(Some(vec![1, minor]));
        assert_eq!(app.firmware_warning(), None, "1.{minor}");
    }
}

#[test]
fn another_firmware_gives_a_warning_and_sends_still_go() {
    let mut app = checked(Some(vec![1, 9]));

    let warning = app.firmware_warning().unwrap().to_owned();
    assert!(warning.contains("1.9"), "{warning}");
    assert!(warning.contains("1.6 and 1.7"), "{warning}");
    app.start().unwrap();
    assert_ne!(app.machine().controller.inner.sent.len(), 0);
}

#[test]
fn a_failed_or_short_read_gives_a_warning() {
    for reply in [None, Some(vec![1])] {
        let app = checked(reply);
        let warning = app.firmware_warning().unwrap();
        assert!(warning.contains("could not be read"), "{warning}");
    }
}

#[test]
fn the_firmware_is_read_with_its_own_query() {
    let app = checked(None);
    assert_eq!(
        app.machine().controller.inner.asked,
        [FirmwareVersion::query()]
    );
    // A read only: nothing is sent.
    assert_eq!(app.machine().controller.inner.sent, []);
}

#[test]
fn each_read_sets_the_warning_anew_and_a_failing_read_is_logged_once() {
    let (mut app, _dir, _) = crate::app_with(Config::default());
    let mut warnings = Vec::new();
    for reply in [None, None, Some(vec![1, 9]), Some(vec![1, 6])] {
        app.machine_mut().controller.inner.firmware = reply;
        app.check_firmware();
        warnings.push(
            app.firmware_warning()
                .map(|w| w.contains("could not be read")),
        );
    }

    assert_eq!(warnings, [Some(true), Some(true), Some(false), None]);
    assert!(!app.firmware_unread());
}

#[test]
fn a_held_controller_keeps_the_last_firmware_result() {
    let (mut app, _dir, _) = crate::app_with(Config::default());
    app.machine_mut().controller.inner.firmware = Some(vec![1, 6]);
    app.check_firmware();
    app.hold_controller(true);

    app.check_firmware();

    assert_eq!(app.firmware_warning(), None);
}
