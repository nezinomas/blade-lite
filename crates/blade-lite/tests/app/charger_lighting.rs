//! The controller keeps one keyboard level per power source and swaps them
//! itself at a charger change (ticket 243), so the app sends the saved
//! lighting again then (ticket 244).

use blade_core::{Config, Lighting};
use platform::PowerEvent;
use razer_proto::{BLADE_18, Brightness, Command, static_colour};

use crate::{TestApp, WHITE, app, app_with, lighting_commands, sent};

fn lit_app() -> (TestApp, tempfile::TempDir, std::path::PathBuf) {
    app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    })
}

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
fn a_charger_pull_sends_the_saved_lighting() {
    let (mut app, _dir, _) = lit_app();

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn the_charger_back_sends_it_again() {
    let (mut app, _dir, _) = lit_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    forget_sent(&mut app);

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn an_event_for_the_same_source_sends_nothing() {
    let (mut app, _dir, _) = lit_app();

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_source_read_after_an_unknown_one_sends_nothing() {
    // The keys took the lighting sent after the unknown read, on the slot
    // the controller already ran.
    let (mut app, _dir, _) = lit_app();
    app.take_on_battery(None);

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn no_saved_lighting_sends_nothing() {
    let (mut app, _dir, _) = app();

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn dark_keys_stay_dark_with_the_lighting_set_and_the_saved_level_comes_back() {
    let (mut app, _dir, _) = lit_app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    app.on_power(PowerEvent::OnBattery).unwrap();

    let mut expected = level(0);
    expected.push(
        BLADE_18
            .vapor_light
            .unwrap()
            .command(Brightness::new(30).unwrap(), true),
    );
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(30));
}

#[test]
fn dark_keys_with_no_saved_lighting_get_only_the_dark_level() {
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), level(0));
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(100));
}
