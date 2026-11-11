//! The mic key, red while the default mic is muted and Num Lock is off.

use blade_core::{Config, Lighting};
use razer_proto::{BLADE_18, Command, PerfMode, RedKeys, Rgb, static_colour, static_colour_red};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::fakes::brightness_commands;
use crate::{TestApp, app, app_with, sent};

const GREEN: Rgb = Rgb::new(0x44, 0xD6, 0x2C);
const MIC: RedKeys = RedKeys {
    touchpad: false,
    mic: true,
};

/// An app with the lighting set, the touchpad on, Num Lock off and the
/// mic not muted.
fn lit_app() -> (TestApp, TempDir, PathBuf) {
    let (mut app, dir, path) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 36,
            colour: GREEN,
            vapor_light: true,
        }),
        ..Config::default()
    });
    app.machine_mut().registry.touchpad = Some(1);
    app.set_mic_muted(Some(false));
    (app, dir, path)
}

fn started(app: &mut TestApp) -> usize {
    app.start().unwrap();
    sent(app).len()
}

fn red(keys: RedKeys) -> Vec<Command> {
    static_colour_red(GREEN, &BLADE_18, keys)
}

fn plain() -> Vec<Command> {
    static_colour(GREEN, &BLADE_18)
}

#[test]
fn start_paints_the_mic_key_red_if_the_mic_is_muted() {
    let (mut app, _dir, _) = lit_app();
    app.set_mic_muted(Some(true));

    app.start().unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(brightness_commands(36, true));
    expected.extend(red(MIC));
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_mute_paints_the_key_red_and_an_unmute_paints_it_back() {
    let (mut app, _dir, _) = lit_app();
    let before = started(&mut app);

    assert!(app.on_mic_muted(Some(true)).unwrap());
    assert!(app.on_mic_muted(Some(false)).unwrap());

    let mut expected = red(MIC);
    expected.extend(plain());
    assert_eq!(&sent(&app)[before..], expected);
}

#[test]
fn the_same_mute_state_sends_nothing() {
    let (mut app, _dir, _) = lit_app();
    let before = started(&mut app);

    assert!(!app.on_mic_muted(Some(false)).unwrap());

    assert_eq!(&sent(&app)[before..], []);
}

#[test]
fn a_mute_while_num_lock_is_on_keeps_the_key_plain() {
    let (mut app, _dir, _) = lit_app();
    app.set_num_lock(true);
    let before = started(&mut app);

    assert!(!app.on_mic_muted(Some(true)).unwrap());

    assert_eq!(&sent(&app)[before..], []);
}

#[test]
fn num_lock_off_while_muted_paints_the_key_red() {
    let (mut app, _dir, _) = lit_app();
    app.set_num_lock(true);
    app.set_mic_muted(Some(true));
    let before = started(&mut app);

    app.on_num_lock(false).unwrap();

    assert_eq!(&sent(&app)[before..], red(MIC));
}

#[test]
fn an_unknown_mute_keeps_the_key_plain() {
    let (mut app, _dir, _) = lit_app();
    app.set_mic_muted(Some(true));
    let before = started(&mut app);

    assert!(app.on_mic_muted(None).unwrap());

    assert_eq!(&sent(&app)[before..], plain());
    assert_eq!(app.mic_muted(), None);
}

#[test]
fn both_keys_are_red_with_the_touchpad_off_and_the_mic_muted() {
    let (mut app, _dir, _) = lit_app();
    app.machine_mut().registry.touchpad = Some(0);
    let before = started(&mut app);

    app.on_mic_muted(Some(true)).unwrap();

    let both = RedKeys {
        touchpad: true,
        mic: true,
    };
    assert_eq!(&sent(&app)[before..], red(both));
}

#[test]
fn a_mute_without_saved_lighting_is_kept_and_sends_nothing() {
    let (mut app, _dir, _) = app();
    let before = started(&mut app);

    assert!(!app.on_mic_muted(Some(true)).unwrap());

    assert_eq!(app.mic_muted(), Some(true));
    assert_eq!(&sent(&app)[before..], []);
}

#[test]
fn a_failed_paint_is_painted_on_the_next_change() {
    let (mut app, _dir, _) = lit_app();
    let before = started(&mut app);
    app.machine_mut().controller.fails = true;
    assert!(app.on_mic_muted(Some(true)).is_err());
    app.machine_mut().controller.fails = false;

    app.on_mic_muted(Some(false)).unwrap();

    assert_eq!(&sent(&app)[before..], plain());
}
