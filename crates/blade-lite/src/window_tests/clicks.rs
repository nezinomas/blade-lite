//! Scripted clicks in the window and the flyout: what is sent, saved and
//! shown after each one.

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::{Button, Slider, Switch};
use i_slint_backend_testing::{ElementHandle, ElementRoot};
use platform::{Key, PowerEvent};
use razer_proto::{BLADE_18, Command, PerfMode, Rgb, static_colour};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};

use super::{Fixture, click, control, fixture, log_path};
use crate::fakes::{brightness_commands, lighting_commands, logo};

const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);

/// What the controller got since `before` commands.
fn sent_since(f: &Fixture, before: usize) -> Vec<Command> {
    f.shell.app().machine().controller.sent[before..].to_vec()
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.sent.len()
}

#[test]
fn a_preset_click_sends_saves_and_shows_it() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    assert_eq!(
        sent_since(&f, before),
        PerfMode::Silent.commands(&BLADE_18).unwrap()
    );
    assert_eq!(f.shell.window().get_preset(), "Silent");
    assert_eq!(f.shell.popup().get_mode(), "Silent");
}

#[test]
fn a_click_on_the_chosen_preset_sends_nothing() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Balanced");

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_rate_click_sets_the_panel_and_sends_nothing() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "60 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_locked_rate_does_nothing_on_a_click() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    let before = sent_count(&f);

    click(f.shell.window(), Button, "120 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_native_mode_click_sends_that_mode() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "1920 × 1200, up to 440 Hz");

    assert_eq!(sent_since(&f, before), [BLADE_18.native_modes[1].command()]);
}

#[test]
fn the_touchpad_switch_presses_the_touchpad_shortcut_once() {
    let f = fixture(Config::default());

    click(f.shell.window(), Switch, "Touchpad");

    assert_eq!(
        f.shell.app().machine().keys.0,
        [Key::Win, Key::Ctrl, Key::F24]
    );
}

#[test]
fn the_brightness_slider_sends_where_it_is_let_go() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    let slider = control(f.shell.window(), Slider, "Brightness");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);

    slider.mock_drag(left_end, PointerEventButton::Left);

    assert_eq!(sent_since(&f, before), lighting_commands(0, WHITE));
    assert_eq!(f.shell.app().config().lighting.unwrap().brightness, 0);
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "Off");
}

/// The text beside the brightness slider of `component` in `root`.
fn percent_shown(root: &impl ElementRoot, component: &str) -> String {
    let id = format!("{component}::brightness-percent");
    let text = ElementHandle::find_by_element_id(root, &id)
        .next()
        .unwrap_or_else(|| panic!("no {id}"));
    text.accessible_label().unwrap().into()
}

/// Presses the brightness slider `label` in its middle and drags it to its
/// left end. The text follows the slider before it is let go, and nothing
/// is sent until then.
fn drag_shows_the_percent<R: ElementRoot + ComponentHandle>(
    f: &Fixture,
    root: &R,
    label: &str,
    component: &str,
) {
    let before = sent_count(f);
    let slider = control(root, Slider, label);
    let start = slider.absolute_position();
    let size = slider.size();
    let middle = LogicalPosition::new(start.x + size.width / 2.0, start.y + size.height / 2.0);
    let left_end = LogicalPosition::new(start.x, middle.y);
    let window = root.window();
    let button = PointerEventButton::Left;

    window.dispatch_event(WindowEvent::PointerPressed {
        position: middle,
        button,
    });
    assert_eq!(percent_shown(root, component), "50%");
    window.dispatch_event(WindowEvent::PointerMoved { position: left_end });
    assert_eq!(percent_shown(root, component), "Off");
    assert_eq!(sent_since(f, before), []);

    window.dispatch_event(WindowEvent::PointerReleased {
        position: left_end,
        button,
    });
    assert_eq!(sent_since(f, before), lighting_commands(0, WHITE));
    assert_eq!(percent_shown(root, component), "Off");
}

#[test]
fn the_window_brightness_text_follows_the_slider_while_it_is_dragged() {
    let f = fixture(Config::default());
    drag_shows_the_percent(&f, f.shell.window(), "Brightness", "KeyboardGroup");
}

#[test]
fn the_flyout_brightness_text_follows_the_slider_while_it_is_dragged() {
    let f = fixture(Config::default());
    drag_shows_the_percent(&f, f.shell.flyout(), "Keyboard light", "FlyoutWindow");
}

/// Drags the brightness slider `label` to its left end and lets it go
/// while every send fails: nothing is sent, and the slider and its text
/// go back to the brightness the laptop has.
fn failed_send_shows_the_brightness_again<R: ElementRoot + ComponentHandle>(
    f: &Fixture,
    root: &R,
    label: &str,
    component: &str,
) {
    f.shell.app().machine_mut().controller.fails = true;
    let slider = control(root, Slider, label);
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);

    slider.mock_drag(left_end, PointerEventButton::Left);

    assert_eq!(slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(root, component), "100%");
}

#[test]
fn the_window_brightness_goes_back_after_a_failed_send() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    failed_send_shows_the_brightness_again(&f, f.shell.window(), "Brightness", "KeyboardGroup");
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn the_flyout_brightness_goes_back_after_a_failed_send() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    failed_send_shows_the_brightness_again(&f, f.shell.flyout(), "Keyboard light", "FlyoutWindow");
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_drag_cut_short_gives_way_to_a_change_from_the_flyout() {
    let f = fixture(Config::default());
    let window = f.shell.window();
    let slider = control(window, Slider, "Brightness");
    let start = slider.absolute_position();
    let middle = LogicalPosition::new(
        start.x + slider.size().width / 2.0,
        start.y + slider.size().height / 2.0,
    );
    // Pressed, but never let go: the window lost the mouse.
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: middle,
        button: PointerEventButton::Left,
    });
    assert_eq!(percent_shown(window, "KeyboardGroup"), "50%");
    let flyout = control(f.shell.flyout(), Slider, "Keyboard light");
    let flyout_start = flyout.absolute_position();
    let flyout_left_end =
        LogicalPosition::new(flyout_start.x, flyout_start.y + flyout.size().height / 2.0);

    flyout.mock_drag(flyout_left_end, PointerEventButton::Left);

    assert_eq!(slider.accessible_value().unwrap(), "0");
    assert_eq!(percent_shown(window, "KeyboardGroup"), "Off");
}

#[test]
fn a_swatch_click_sends_and_saves_that_colour() {
    let f = fixture(Config::default());
    let red = f.shell.app().keyboard_view().unwrap().swatches[1].colour;
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Colour 2");

    assert_eq!(sent_since(&f, before), lighting_commands(100, red));
    assert_eq!(f.shell.app().config().lighting.unwrap().colour, red);
}

#[test]
fn the_vapor_light_switch_turns_it_off() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Switch, "Vapor chamber light");

    let mut expected = brightness_commands(100, false);
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent_since(&f, before), expected);
    assert!(!f.shell.window().get_vapor_light_on());
}

#[test]
fn the_lid_logo_switch_turns_it_off() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Switch, "Lid logo");

    assert_eq!(sent_since(&f, before), logo().commands(false));
    assert!(!f.shell.window().get_logo_on());
}

#[test]
fn the_charge_limit_switch_turns_it_on() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Switch, "Battery charge limit");

    let limit = BLADE_18.charge_limit.unwrap();
    assert_eq!(sent_since(&f, before), [limit.command(true).unwrap()]);
    assert!(f.shell.window().get_charge_limit_on());
}

#[test]
fn the_flyout_sets_a_preset_and_resets_to_balanced() {
    let f = fixture(Config::default());

    click(f.shell.flyout(), Button, "Silent");
    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    let before = sent_count(&f);
    click(f.shell.flyout(), Button, "Reset to Balanced");

    assert_eq!(
        sent_since(&f, before),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert!(!f.shell.flyout().get_reset_enabled());
}

#[test]
fn a_failed_click_shows_the_error_and_logs_it() {
    let f = fixture(Config::default());
    // No other test fails this way, so the line in the shared log is this
    // test's own.
    f.shell.app().machine_mut().registry.touchpad = None;

    click(f.shell.window(), Switch, "Touchpad");

    assert_eq!(f.shell.app().machine().keys.0, []);
    let error = f.shell.window().get_error();
    assert!(error.contains("touchpad"), "{error}");
    let log = std::fs::read_to_string(log_path()).unwrap();
    // The log keeps each report on one line.
    let first = error.lines().next().unwrap();
    assert!(log.contains(first), "{log}");
}

/// With the lighting saved and the touchpad off, as the touchpad key shows
/// it.
fn touchpad_off_fixture() -> Fixture {
    let f = fixture(Config {
        lighting: Some(blade_core::Lighting {
            brightness: 100,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    });
    f.shell.app().machine_mut().registry.touchpad = Some(0);
    f
}

#[test]
fn num_lock_paints_the_touchpad_key_once_per_change() {
    let f = touchpad_off_fixture();
    f.shell.follow_num_lock(true);
    let before = sent_count(&f);

    f.shell.follow_num_lock(false);
    f.shell.follow_num_lock(false);

    assert_eq!(
        sent_since(&f, before),
        razer_proto::static_colour_touchpad_off(WHITE, &BLADE_18)
    );
}

#[test]
fn num_lock_during_the_resume_pause_sends_nothing() {
    let f = touchpad_off_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
    let before = sent_count(&f);

    f.shell.follow_num_lock(true);

    assert_eq!(sent_since(&f, before), []);
    assert!(f.shell.app().num_lock_on());
}

#[test]
fn a_touchpad_change_during_the_resume_pause_sends_nothing() {
    let f = touchpad_off_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
    let before = sent_count(&f);

    f.shell.follow_touchpad();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_touchpad_change_paints_the_key_outside_a_resume() {
    let f = touchpad_off_fixture();
    let before = sent_count(&f);

    f.shell.follow_touchpad();

    assert_eq!(
        sent_since(&f, before),
        razer_proto::static_colour_touchpad_off(WHITE, &BLADE_18)
    );
}
