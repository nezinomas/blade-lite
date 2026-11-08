//! Brightness, colour, the touchpad switch and the vapor light.

use blade_core::{Config, Lighting};
use platform::{Key, PowerEvent};
use razer_proto::{BLADE_18, Brightness, ModelProfile, PerfMode, Rgb, static_colour};
use std::path::PathBuf;
use tempfile::TempDir;
use ui::KeyboardCommand;

use crate::{TestApp, WHITE, app, app_on, app_with, brightness_commands, lighting_commands, sent};

#[test]
fn brightness_sends_and_saves_the_lighting() {
    let (mut app, _dir, path) = app();
    let half = Brightness::new(50).unwrap();

    app.on_keyboard(KeyboardCommand::SetBrightness(half))
        .unwrap();

    assert_eq!(sent(&app), lighting_commands(50, WHITE));
    let saved = Config::load(&path).unwrap().lighting;
    assert_eq!(
        saved,
        Some(Lighting {
            brightness: 50,
            colour: WHITE,
            vapor_light: true,
        })
    );
    assert_eq!(app.keyboard_view().unwrap().brightness, 50);
}

#[test]
fn a_colour_keeps_the_saved_brightness() {
    let red = Rgb::new(0xE8, 0x11, 0x23);
    let (mut app, _dir, _) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    });

    app.on_keyboard(KeyboardCommand::SetColour(red)).unwrap();

    assert_eq!(sent(&app), lighting_commands(30, red));
}

#[test]
fn the_touchpad_switch_sends_the_shortcut() {
    let (mut app, _dir, _) = app();

    app.on_keyboard(KeyboardCommand::SetTouchpad(false))
        .unwrap();

    assert_eq!(app.machine().keys.0, [Key::Win, Key::Ctrl, Key::F24]);
}

static NO_VAPOR_LIGHT: ModelProfile = ModelProfile {
    vapor_light: None,
    ..BLADE_18
};

/// An app lit at 30% in white, with the vapor light as given.
fn vapor_app(vapor_light: bool) -> (TestApp, TempDir, PathBuf) {
    app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light,
        }),
        ..Config::default()
    })
}

#[test]
fn the_vapor_light_switch_shows_the_saved_state() {
    assert_eq!(
        vapor_app(true).0.keyboard_view().unwrap().vapor_light_on,
        Some(true)
    );
    assert_eq!(
        vapor_app(false).0.keyboard_view().unwrap().vapor_light_on,
        Some(false)
    );
}

#[test]
fn the_vapor_light_switch_sends_and_saves_it() {
    let (mut app, _dir, path) = vapor_app(true);

    app.on_keyboard(KeyboardCommand::SetVaporLight(false))
        .unwrap();

    let mut expected = brightness_commands(30, false);
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
    let saved = Config::load(&path).unwrap().lighting.unwrap();
    assert!(!saved.vapor_light);
    assert_eq!(app.keyboard_view().unwrap().vapor_light_on, Some(false));
}

#[test]
fn a_brightness_change_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.on_keyboard(KeyboardCommand::SetBrightness(Brightness::new(80).unwrap()))
        .unwrap();

    let mut expected = brightness_commands(80, false);
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_failed_vapor_light_send_saves_nothing() {
    let (mut app, _dir, path) = vapor_app(true);
    app.machine_mut().controller.fails = true;

    assert!(
        app.on_keyboard(KeyboardCommand::SetVaporLight(false))
            .is_err()
    );

    assert!(app.config().lighting.unwrap().vapor_light);
    assert!(!path.exists());
}

#[test]
fn a_model_without_a_vapor_light_hides_the_switch_and_ignores_it() {
    let (mut app, _dir, path) = app_on(&NO_VAPOR_LIGHT, Config::default());

    assert_eq!(app.keyboard_view().unwrap().vapor_light_on, None);
    app.on_keyboard(KeyboardCommand::SetVaporLight(false))
        .unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_resume_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.on_power(PowerEvent::Resumed).unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(brightness_commands(30, false));
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
}

#[test]
fn start_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.start().unwrap();

    let vapor = BLADE_18
        .vapor_light
        .unwrap()
        .command(Brightness::new(30).unwrap(), false);
    assert!(sent(&app).contains(&vapor), "{:?}", sent(&app));
    assert_eq!(vapor.args, [0x01, 0x26, 0]);
}
