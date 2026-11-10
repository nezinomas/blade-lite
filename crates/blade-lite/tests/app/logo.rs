//! The lid logo switch.

use blade_core::Config;
use platform::PowerEvent;
use ui::KeyboardCommand;

use crate::{app, app_with, logo, sent};

#[test]
fn start_reads_the_logo_state_for_the_keyboard_view() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 0]);

    app.start().unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn the_keyboard_view_does_not_ask_the_controller_for_the_logo() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    // A later read would now fail, and the view would hide the switch.
    app.machine_mut().controller.logo_reply = None;

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn a_failed_logo_read_hides_the_switch_and_keeps_the_rest() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();
    let view = app.keyboard_view().unwrap();

    assert_eq!(view.logo_on, None);
    assert!(view.touchpad_on);
}

#[test]
fn a_resume_reads_the_logo_state_again() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 0]);
    app.start().unwrap();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn the_logo_switch_shows_the_new_state() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();

    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_failed_logo_switch_keeps_the_old_state() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn the_logo_switch_sends_the_logo_commands_and_saves_the_state() {
    let (mut app, _dir, path) = app();

    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();

    assert_eq!(sent(&app), logo().commands(false));
    let saved = Config {
        lid_logo: Some(false),
        ..Config::default()
    };
    assert_eq!(Config::load(&path).unwrap(), saved);
}

#[test]
fn a_logo_switch_that_cannot_be_saved_is_still_sent_at_resume() {
    let (mut app, _dir, path) = app_with(Config {
        lid_logo: Some(true),
        ..Config::default()
    });
    // A folder in the file's place makes the save fail.
    std::fs::create_dir(&path).unwrap();
    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
}

#[test]
fn a_failed_logo_switch_saves_nothing() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.fails = true;

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());

    assert!(!path.exists());
}

#[test]
fn a_resume_turns_a_saved_off_logo_off_again() {
    let (mut app, _dir, _) = app();
    app.start().unwrap();
    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();
    // The laptop turns the logo on again while asleep.
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_failed_start_shows_the_logo_state_the_controller_reads() {
    let (mut app, _dir, _) = app_with(Config {
        lid_logo: Some(false),
        ..Config::default()
    });
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.machine_mut().controller.fails = true;

    assert!(app.start().is_err());

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn start_sends_the_saved_logo_state() {
    let (mut app, _dir, _) = app_with(Config {
        lid_logo: Some(false),
        ..Config::default()
    });
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);

    app.start().unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_logo_switch_that_fails_part_way_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    // The switch command runs, but its reply is Busy.
    app.machine_mut().controller.fails_after = Some(2);

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_failed_logo_switch_hides_the_switch_if_the_read_back_fails() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;
    app.machine_mut().controller.logo_reply = None;

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    assert_eq!(app.keyboard_view().unwrap().logo_on, None);
}

#[test]
fn a_logo_switch_that_fails_part_way_but_took_is_sent_at_resume() {
    let (mut app, _dir, path) = app_with(Config {
        lid_logo: Some(true),
        ..Config::default()
    });
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    app.machine_mut().controller.fails_after = Some(2);
    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    app.machine_mut().controller.fails_after = None;
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
    assert_eq!(Config::load(&path).unwrap().lid_logo, Some(false));
}

#[test]
fn a_logo_switch_that_fails_before_the_switch_shows_the_old_state() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    app.machine_mut().controller.fails_after = Some(1);

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());

    assert_eq!(sent(&app), &logo().commands(false)[..1]);
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
    assert!(!path.exists());
}
