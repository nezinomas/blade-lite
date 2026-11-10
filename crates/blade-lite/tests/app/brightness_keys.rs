//! Fn + F10 / F11: the laptop sets the keyboard brightness itself, and the
//! app reads it back when Fn comes up.

use blade_core::{Config, Lighting};
use razer_proto::{BLADE_18, Brightness, Rgb};

use crate::{TestApp, WHITE, app, app_with, break_save, sent};
use std::path::PathBuf;
use tempfile::TempDir;

const TEAL: Rgb = Rgb::new(0x00, 0xB7, 0xC3);

fn lighting(percent: u8, vapor_light: bool) -> Lighting {
    Lighting {
        brightness: percent,
        colour: TEAL,
        vapor_light,
    }
}

fn app_lit(percent: u8, vapor_light: bool) -> (TestApp, TempDir, PathBuf) {
    app_with(Config {
        lighting: Some(lighting(percent, vapor_light)),
        ..Config::default()
    })
}

fn vapor_command(percent: u8, on: bool) -> razer_proto::Command {
    BLADE_18
        .vapor_light
        .unwrap()
        .command(Brightness::new(percent).unwrap(), on)
}

#[test]
fn a_new_level_is_shown_saved_and_followed_by_the_vapor_light() {
    let (mut app, _dir, path) = app_lit(35, true);
    app.machine_mut().controller.brightness_level = Some(255);

    assert!(app.on_fn_up().unwrap());

    assert_eq!(app.keyboard_view().unwrap().brightness, 100);
    assert_eq!(
        Config::load(&path).unwrap().lighting,
        Some(lighting(100, true))
    );
    assert_eq!(sent(&app), [vapor_command(100, true)]);
}

#[test]
fn a_vapor_light_switched_off_stays_off() {
    let (mut app, _dir, path) = app_lit(35, false);
    app.machine_mut().controller.brightness_level = Some(0);

    app.on_fn_up().unwrap();

    assert_eq!(app.keyboard_view().unwrap().brightness, 0);
    assert_eq!(
        Config::load(&path).unwrap().lighting,
        Some(lighting(0, false))
    );
    assert_eq!(sent(&app), [vapor_command(0, false)]);
}

#[test]
fn the_same_level_sends_and_saves_nothing() {
    let (mut app, _dir, path) = app_lit(35, true);
    let level = Brightness::new(35).unwrap().level();
    app.machine_mut().controller.brightness_level = Some(level);

    assert!(!app.on_fn_up().unwrap());

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_failed_read_changes_nothing() {
    let (mut app, _dir, path) = app_lit(35, true);

    assert!(app.on_fn_up().is_err());

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
    assert_eq!(app.keyboard_view().unwrap().brightness, 35);
}

#[test]
fn without_saved_lighting_nothing_is_taken() {
    // The keyboard colour is not known, so a saved lighting would send
    // white at the next start.
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.brightness_level = Some(90);

    assert!(!app.on_fn_up().unwrap());

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
    assert_eq!(app.config().lighting, None);
}

#[test]
fn a_failed_vapor_send_still_keeps_the_new_level() {
    // The keyboard already runs the new level: the laptop set it.
    let (mut app, _dir, path) = app_lit(35, true);
    app.machine_mut().controller.brightness_level = Some(255);
    app.machine_mut().controller.fails = true;

    assert!(app.on_fn_up().is_err());

    assert_eq!(app.keyboard_view().unwrap().brightness, 100);
    assert_eq!(
        Config::load(&path).unwrap().lighting,
        Some(lighting(100, true))
    );
}

#[test]
fn a_failed_save_keeps_the_new_level_in_memory() {
    let (mut app, _dir, path) = app_lit(35, true);
    app.machine_mut().controller.brightness_level = Some(255);
    break_save(&path);

    let error = app.on_fn_up().unwrap_err();

    assert!(error.was_sent());
    assert_eq!(app.keyboard_view().unwrap().brightness, 100);
}

#[test]
fn the_colour_is_kept() {
    let (mut app, _dir, _) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 35,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    });
    app.machine_mut().controller.brightness_level = Some(255);

    app.on_fn_up().unwrap();

    assert_eq!(app.config().lighting.unwrap().colour, WHITE);
}
