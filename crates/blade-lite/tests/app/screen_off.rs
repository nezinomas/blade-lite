//! The keyboard goes dark while Windows has the screen off, and gets its
//! level back when the screen comes on or the app quits (ticket 241).

use blade_core::{Config, Lighting};
use razer_proto::{BLADE_18, Brightness, Command, Rgb};
use ui::KeyboardCommand;

use crate::{TestApp, app, app_with, sent};

const TEAL: Rgb = Rgb::new(0x00, 0xB7, 0xC3);

/// The keyboard's level commands only: the vapor light is not touched.
fn level(percent: u8) -> Vec<Command> {
    Brightness::new(percent).unwrap().commands(&BLADE_18)
}

fn lit_at(app: &mut TestApp, level: u8) {
    app.machine_mut().controller.inner.brightness_level = Some(level);
}

fn forget_sent(app: &mut TestApp) {
    app.machine_mut().controller.inner.sent.clear();
}

#[test]
fn the_screen_off_sends_the_keyboard_level_0() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);

    app.follow_screen(true).unwrap();

    assert_eq!(sent(&app), level(0));
}

#[test]
fn the_screen_on_puts_the_level_read_before_back() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn a_second_screen_off_sends_nothing() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);
    // The keyboard now reads 0: a second read must not take it as the level.
    lit_at(&mut app, 0);

    app.follow_screen(true).unwrap();
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn the_screen_on_with_no_dark_keys_sends_nothing() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_read_sends_nothing_then_or_at_the_screen_on() {
    let (mut app, _dir, _path) = app();

    assert!(app.follow_screen(true).is_err());
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_keyboard_already_dark_is_left_alone() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 0);

    app.follow_screen(true).unwrap();
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_dark_send_still_puts_the_level_back_at_the_screen_on() {
    // The send may have gone in part: the level is sent back to be sure.
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.machine_mut().controller.inner.fails = true;
    assert!(app.follow_screen(true).is_err());
    app.machine_mut().controller.inner.fails = false;

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn a_failed_put_back_is_tried_again_at_the_next_look() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);
    app.machine_mut().controller.inner.fails = true;
    assert!(app.follow_screen(false).is_err());
    app.machine_mut().controller.inner.fails = false;

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn the_quit_puts_the_level_back() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    app.keys_back_on().unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn the_quit_with_lit_keys_sends_nothing() {
    let (mut app, _dir, _path) = app();

    app.keys_back_on().unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn the_brightness_keys_take_no_level_while_the_keys_are_dark() {
    // A Fn press read before the screen-on notice would save 0.
    let lighting = Lighting {
        brightness: 100,
        colour: TEAL,
        vapor_light: true,
    };
    let (mut app, _dir, path) = app_with(Config {
        lighting: Some(lighting),
        ..Config::default()
    });
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 0);

    assert!(!app.on_brightness_keys().unwrap());

    assert!(!path.exists());
    assert_eq!(app.config().lighting, Some(lighting));
}

#[test]
fn a_start_with_saved_lighting_lights_the_keys_so_nothing_is_put_back() {
    let (mut app, _dir, _path) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 40,
            colour: TEAL,
            vapor_light: true,
        }),
        ..Config::default()
    });
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    app.start().unwrap();
    forget_sent(&mut app);

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_start_without_saved_lighting_leaves_the_level_to_put_back() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    app.start().unwrap();
    forget_sent(&mut app);

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn a_brightness_click_while_dark_is_the_new_level() {
    let (mut app, _dir, _path) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    app.on_keyboard(KeyboardCommand::SetBrightness(Brightness::new(30).unwrap()))
        .unwrap();
    forget_sent(&mut app);

    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}
