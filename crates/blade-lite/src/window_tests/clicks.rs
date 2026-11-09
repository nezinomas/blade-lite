//! Scripted clicks in the window and the flyout: what is sent, saved and
//! shown after each one.

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::{Button, Slider, Switch};
use razer_proto::{BLADE_18, PerfMode};
use slint::LogicalPosition;
use slint::platform::PointerEventButton;

use super::{click, control, fixture, log_path};

#[test]
fn a_preset_click_sends_saves_and_shows_it() {
    let f = fixture(Config::default());
    let before = f.shell.app().machine().controller.sent.len();

    click(f.shell.window(), Button, "Silent");

    let app = f.shell.app();
    assert_eq!(app.config().preset, Preset::Silent);
    assert_eq!(
        app.machine().controller.sent[before..],
        PerfMode::Silent.commands(&BLADE_18).unwrap()
    );
    assert_eq!(f.shell.window().get_preset(), "Silent");
    assert_eq!(f.shell.popup().get_mode(), "Silent");
}

#[test]
fn a_click_on_the_chosen_preset_sends_nothing() {
    let f = fixture(Config::default());
    let before = f.shell.app().machine().controller.sent.len();

    click(f.shell.window(), Button, "Balanced");

    assert_eq!(f.shell.app().machine().controller.sent.len(), before);
}

#[test]
fn a_rate_click_sets_the_panel() {
    let f = fixture(Config::default());

    click(f.shell.window(), Button, "60 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
}

#[test]
fn the_touchpad_switch_presses_the_touchpad_key() {
    let f = fixture(Config::default());

    click(f.shell.window(), Switch, "Touchpad");

    assert_ne!(f.shell.app().machine().keys.0.len(), 0);
}

#[test]
fn the_brightness_slider_sends_where_it_is_let_go() {
    let f = fixture(Config::default());
    let slider = control(f.shell.window(), Slider, "Brightness");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);

    slider.mock_drag(left_end, PointerEventButton::Left);

    let lighting = f.shell.app().config().lighting.unwrap();
    assert_eq!(lighting.brightness, 0);
    assert_eq!(f.shell.window().get_brightness_text(), "Off");
}

#[test]
fn a_swatch_click_sets_that_colour() {
    let f = fixture(Config::default());
    let red = f.shell.app().keyboard_view().unwrap().swatches[1].colour;

    click(f.shell.window(), Button, "Colour 2");

    let lighting = f.shell.app().config().lighting.unwrap();
    assert_eq!(lighting.colour, red);
}

#[test]
fn the_flyout_sets_a_preset_and_resets_to_balanced() {
    let f = fixture(Config::default());

    click(f.shell.flyout(), Button, "Silent");
    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    click(f.shell.flyout(), Button, "Reset to Balanced");

    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert!(!f.shell.flyout().get_reset_enabled());
}

#[test]
fn a_failed_click_shows_the_error_and_keeps_the_preset() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.fails = true;

    click(f.shell.window(), Button, "Silent");

    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
    let log = std::fs::read_to_string(log_path()).unwrap();
    assert!(log.contains(error.as_str()), "{log}");
}
