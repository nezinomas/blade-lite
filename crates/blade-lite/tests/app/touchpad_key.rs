//! The touchpad key, red while the touchpad is off.

use blade_core::{Config, Lighting};
use platform::PowerEvent;
use razer_proto::{BLADE_18, Command, PerfMode, Rgb, static_colour, static_colour_touchpad_off};
use std::path::PathBuf;
use tempfile::TempDir;
use ui::KeyboardCommand;

use crate::{TestApp, WHITE, app, app_with, lighting_commands, red_lighting_commands, sent};

const GREEN: Rgb = Rgb::new(0x44, 0xD6, 0x2C);

/// An app with the lighting set and the touchpad off.
fn touchpad_off_app() -> (TestApp, TempDir, PathBuf) {
    let (mut app, dir, path) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 36,
            colour: GREEN,
            vapor_light: true,
        }),
        ..Config::default()
    });
    app.machine_mut().registry.touchpad = Some(0);
    (app, dir, path)
}

fn red_key() -> Vec<Command> {
    static_colour_touchpad_off(GREEN, &BLADE_18)
}

fn plain_key() -> Vec<Command> {
    static_colour(GREEN, &BLADE_18)
}

/// The commands sent after the first `count`.
fn sent_after(app: &TestApp, count: usize) -> &[Command] {
    &sent(app)[count..]
}

#[test]
fn start_paints_the_touchpad_key_red_if_the_touchpad_is_off() {
    let (mut app, _dir, _) = touchpad_off_app();

    app.start().unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(red_lighting_commands(36, GREEN));
    assert_eq!(sent(&app), expected);
}

#[test]
fn without_saved_lighting_the_keyboard_effect_is_left_alone() {
    let (mut app, _dir, _) = app();
    app.machine_mut().registry.touchpad = Some(0);

    app.start().unwrap();
    app.on_touchpad_changed().unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_touchpad_change_paints_the_key_red_or_back() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    let started = sent(&app).len();

    app.machine_mut().registry.touchpad = Some(0);
    app.on_touchpad_changed().unwrap();
    app.machine_mut().registry.touchpad = Some(1);
    app.on_touchpad_changed().unwrap();

    let mut expected = red_key();
    expected.extend(plain_key());
    assert_eq!(sent_after(&app, started), expected);
}

#[test]
fn a_change_that_keeps_the_touchpad_state_sends_nothing() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_touchpad_changed().unwrap();
    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn a_new_colour_keeps_the_touchpad_key_red() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_keyboard(KeyboardCommand::SetColour(WHITE)).unwrap();

    assert_eq!(sent_after(&app, started), red_lighting_commands(36, WHITE));
}

#[test]
fn a_resume_paints_the_touchpad_key_red_again() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent_after(&app, started).ends_with(&red_key()));
    assert_eq!(sent(&app).len(), 2 * started);
}

#[test]
fn an_unreadable_touchpad_state_leaves_the_key_and_the_start_alone() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = None;

    app.start().unwrap();
    let started = sent(&app).len();
    assert!(app.on_touchpad_changed().is_err());

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(36, GREEN));
    assert_eq!(sent(&app), expected);
    assert_eq!(sent(&app).len(), started);
}

#[test]
fn after_a_failed_lighting_send_the_next_change_paints_the_key() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    app.machine_mut().controller.fails = true;
    assert!(app.on_keyboard(KeyboardCommand::SetColour(WHITE)).is_err());
    app.machine_mut().controller.fails = false;
    let before = sent(&app).len();

    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, before), red_key());
}

#[test]
fn a_failed_key_paint_is_tried_again_on_the_next_change() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    app.machine_mut().registry.touchpad = Some(0);
    app.machine_mut().controller.fails = true;
    assert!(app.on_touchpad_changed().is_err());
    app.machine_mut().controller.fails = false;
    let before = sent(&app).len();

    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, before), red_key());
}

#[test]
fn the_keyboard_view_shows_the_touchpad_state() {
    let (app, _dir, _) = app();

    assert!(app.keyboard_view().unwrap().touchpad_on);
}

/// As [`touchpad_off_app`], with Num Lock on: the numpad shows digits.
fn num_lock_app() -> (TestApp, TempDir, PathBuf) {
    let (mut app, dir, path) = touchpad_off_app();
    app.set_num_lock(true);
    (app, dir, path)
}

#[test]
fn start_keeps_the_key_plain_while_num_lock_is_on() {
    let (mut app, _dir, _) = num_lock_app();

    app.start().unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(36, GREEN));
    assert_eq!(sent(&app), expected);
}

#[test]
fn num_lock_off_paints_the_key_red_and_on_paints_it_back() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_num_lock(false).unwrap();
    app.on_num_lock(true).unwrap();

    let mut expected = red_key();
    expected.extend(plain_key());
    assert_eq!(sent_after(&app, started), expected);
}

#[test]
fn the_same_num_lock_state_sends_nothing() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_num_lock(true).unwrap();

    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn num_lock_with_the_touchpad_on_sends_nothing() {
    let (mut app, _dir, _) = num_lock_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_num_lock(false).unwrap();
    app.on_num_lock(true).unwrap();

    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn the_touchpad_off_while_num_lock_is_on_keeps_the_key_plain() {
    let (mut app, _dir, _) = num_lock_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    let started = sent(&app).len();

    app.machine_mut().registry.touchpad = Some(0);
    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn a_new_colour_while_num_lock_is_on_keeps_the_key_plain() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_keyboard(KeyboardCommand::SetColour(WHITE)).unwrap();

    assert_eq!(sent_after(&app, started), lighting_commands(36, WHITE));
}

#[test]
fn the_app_knows_the_num_lock_state() {
    let (mut app, _dir, _) = app();
    assert!(!app.num_lock_on());

    app.on_num_lock(true).unwrap();

    assert!(app.num_lock_on());
}

#[test]
fn num_lock_without_saved_lighting_is_kept_and_sends_nothing() {
    let (mut app, _dir, _) = app();
    app.machine_mut().registry.touchpad = Some(0);
    app.start().unwrap();
    let started = sent(&app).len();

    assert!(!app.on_num_lock(true).unwrap());

    assert!(app.num_lock_on());
    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn num_lock_with_an_unreadable_touchpad_keeps_the_key_plain() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    let started = sent(&app).len();
    app.machine_mut().registry.touchpad = None;

    assert!(!app.on_num_lock(false).unwrap());

    assert!(!app.num_lock_on());
    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn a_failed_paint_on_num_lock_is_painted_on_the_next_change() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    app.machine_mut().controller.fails = true;
    assert!(app.on_num_lock(false).is_err());
    app.machine_mut().controller.fails = false;
    assert!(!app.num_lock_on());
    let before = sent(&app).len();

    app.on_num_lock(true).unwrap();

    assert_eq!(sent_after(&app, before), plain_key());
}

#[test]
fn a_resume_with_num_lock_on_keeps_the_key_plain() {
    let (mut app, _dir, _) = num_lock_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent_after(&app, started).ends_with(&plain_key()));
}
