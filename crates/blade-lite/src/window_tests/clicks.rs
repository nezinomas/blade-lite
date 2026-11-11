//! Scripted clicks in the window and the flyout: what is sent, saved and
//! shown after each one.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset};
use blade_lite::Rect;
use i_slint_backend_testing::AccessibleRole::{Button, Slider, Switch, TextInput};
use i_slint_backend_testing::{ElementHandle, ElementRoot};
use platform::{Key, PowerEvent};
use razer_proto::{BLADE_18, Celsius, Charger, Command, PerfMode, Rgb, static_colour};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::winit_030::winit::event::WindowEvent as WinitEvent;
use slint::{ComponentHandle, LogicalPosition};

use super::{
    FAKE_MIC, FAKE_ON_BATTERY, Fixture, click, control, fixture, log_path, selected_preset,
};
use crate::fakes::{FakeCharger, brightness_commands, lighting_commands, logo};

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
    assert_eq!(selected_preset(&f), "Silent");
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
fn quitting_on_battery_puts_the_rate_back() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));

    f.shell.quit();

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
}

#[test]
fn a_failed_put_back_on_quit_is_logged() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    // 240 Hz is gone, so the put-back is refused.
    f.shell
        .app()
        .machine_mut()
        .displays
        .0
        .modes
        .retain(|mode| mode.hz != 240);

    f.shell.quit();

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    let log = std::fs::read_to_string(log_path()).unwrap();
    assert!(log.contains("the panel may stay at 60 Hz"), "{log}");
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

#[test]
fn both_brightness_sliders_run_from_0_to_100() {
    let f = fixture(Config::default());
    let sliders = [
        control(f.shell.window(), Slider, "Brightness"),
        control(f.shell.flyout(), Slider, "Keyboard light"),
    ];

    for slider in sliders {
        assert_eq!(slider.accessible_value_minimum(), Some(0.0));
        assert_eq!(slider.accessible_value_maximum(), Some(100.0));
    }
}

/// Drags the brightness slider `label` to its left end and lets it go
/// while every send fails: the send is tried and its error shown, and the
/// slider and its text go back to the brightness the laptop has.
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

    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
    assert_eq!(f.shell.app().config().lighting, None);
    assert_eq!(slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(root, component), "100%");
}

#[test]
fn the_window_brightness_goes_back_after_a_failed_send() {
    let f = fixture(Config::default());
    failed_send_shows_the_brightness_again(&f, f.shell.window(), "Brightness", "KeyboardGroup");
}

#[test]
fn the_flyout_brightness_goes_back_after_a_failed_send() {
    let f = fixture(Config::default());
    failed_send_shows_the_brightness_again(&f, f.shell.flyout(), "Keyboard light", "FlyoutWindow");
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
fn the_flyout_slider_follows_a_change_from_the_window_after_its_own_drag() {
    let f = fixture(Config::default());
    let flyout = control(f.shell.flyout(), Slider, "Keyboard light");
    let start = flyout.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + flyout.size().height / 2.0);
    flyout.mock_drag(left_end, PointerEventButton::Left);
    assert_eq!(percent_shown(f.shell.flyout(), "FlyoutWindow"), "Off");
    let window = control(f.shell.window(), Slider, "Brightness");
    let start = window.absolute_position();
    let right_end = LogicalPosition::new(
        start.x + window.size().width,
        start.y + window.size().height / 2.0,
    );

    window.mock_drag(right_end, PointerEventButton::Left);

    assert_eq!(flyout.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(f.shell.flyout(), "FlyoutWindow"), "100%");
}

/// Presses the brightness slider `label` in its middle and never lets it
/// go, as when the window hides mid-drag: the slider shows 50%, unsent.
fn press_without_release<R: ElementRoot + ComponentHandle>(root: &R, label: &str) {
    let slider = control(root, Slider, label);
    let start = slider.absolute_position();
    let size = slider.size();
    root.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(start.x + size.width / 2.0, start.y + size.height / 2.0),
        button: PointerEventButton::Left,
    });
}

/// A tray icon for opening the flyout.
const ICON: Rect = Rect {
    x: 1800,
    y: 1100,
    width: 24,
    height: 24,
};

/// After a show: the slider `label` and its text show the laptop's 100%,
/// and moving the mouse to the slider's left end and letting go (the old
/// drag's mouse-up) neither moves it nor sends anything since `before`.
/// That mouse-up calls `released` with 100, which sends nothing because
/// `move_brightness` skips the brightness the laptop already has.
fn shows_the_laptop_brightness<R: ElementRoot + ComponentHandle>(
    f: &Fixture,
    root: &R,
    label: &str,
    component: &str,
    before: usize,
) {
    let slider = control(root, Slider, label);
    assert_eq!(slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(root, component), "100%");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);

    root.window()
        .dispatch_event(WindowEvent::PointerMoved { position: left_end });
    root.window().dispatch_event(WindowEvent::PointerReleased {
        position: left_end,
        button: PointerEventButton::Left,
    });

    assert_eq!(slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(root, component), "100%");
    assert_eq!(sent_since(f, before), []);
}

#[test]
fn showing_the_window_puts_back_a_brightness_drag_cut_short() {
    let f = fixture(Config::default());
    press_without_release(f.shell.window(), "Brightness");
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "50%");
    let before = sent_count(&f);

    f.shell.show_window();

    shows_the_laptop_brightness(&f, f.shell.window(), "Brightness", "KeyboardGroup", before);
}

#[test]
fn showing_the_window_puts_back_a_brightness_set_by_narrator() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    control(f.shell.window(), Slider, "Brightness").set_accessible_value("30");
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "30%");

    f.shell.show_window();

    shows_the_laptop_brightness(&f, f.shell.window(), "Brightness", "KeyboardGroup", before);
}

#[test]
fn focusing_the_window_puts_back_a_brightness_drag_cut_short() {
    let f = fixture(Config::default());
    press_without_release(f.shell.window(), "Brightness");
    let before = sent_count(&f);

    f.shell.on_window_event(&WinitEvent::Focused(true));

    shows_the_laptop_brightness(&f, f.shell.window(), "Brightness", "KeyboardGroup", before);
}

#[test]
fn focusing_the_window_puts_back_a_brightness_set_by_narrator() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    control(f.shell.window(), Slider, "Brightness").set_accessible_value("30");

    f.shell.on_window_event(&WinitEvent::Focused(true));

    shows_the_laptop_brightness(&f, f.shell.window(), "Brightness", "KeyboardGroup", before);
}

#[test]
fn losing_the_focus_keeps_a_brightness_set_by_narrator() {
    let f = fixture(Config::default());
    control(f.shell.window(), Slider, "Brightness").set_accessible_value("30");

    f.shell.on_window_event(&WinitEvent::Focused(false));

    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "30%");
}

#[test]
fn opening_the_flyout_puts_back_a_brightness_drag_cut_short() {
    let f = fixture(Config::default());
    press_without_release(f.shell.flyout(), "Keyboard light");
    assert_eq!(percent_shown(f.shell.flyout(), "FlyoutWindow"), "50%");
    let before = sent_count(&f);

    f.shell.toggle_flyout(ICON);

    shows_the_laptop_brightness(
        &f,
        f.shell.flyout(),
        "Keyboard light",
        "FlyoutWindow",
        before,
    );
}

#[test]
fn opening_the_flyout_puts_back_a_brightness_set_by_narrator() {
    let f = fixture(Config::default());
    let before = sent_count(&f);
    control(f.shell.flyout(), Slider, "Keyboard light").set_accessible_value("30");
    assert_eq!(percent_shown(f.shell.flyout(), "FlyoutWindow"), "30%");

    f.shell.toggle_flyout(ICON);

    shows_the_laptop_brightness(
        &f,
        f.shell.flyout(),
        "Keyboard light",
        "FlyoutWindow",
        before,
    );
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
fn the_keyboard_group_works_when_the_touchpad_cant_be_read() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().registry.touchpad = None;
    let before = sent_count(&f);

    click(f.shell.window(), Switch, "Lid logo");
    let slider = control(f.shell.window(), Slider, "Brightness");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);
    slider.mock_drag(left_end, PointerEventButton::Left);

    let mut sent = logo().commands(false);
    sent.extend(lighting_commands(0, WHITE));
    assert_eq!(sent_since(&f, before), sent);
    assert!(!f.shell.window().get_logo_on());
    assert_eq!(f.shell.window().get_brightness(), 0);
    assert_eq!(f.shell.flyout().get_brightness(), 0);
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
fn the_flyout_shows_and_resets_the_preset_when_the_touchpad_cant_be_read() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().registry.touchpad = None;

    click(f.shell.flyout(), Button, "Silent");
    assert!(f.shell.flyout().get_reset_enabled());
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
fn the_flyout_shows_and_resets_the_preset_when_the_native_mode_cant_be_read() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.native_fails = true;

    click(f.shell.flyout(), Button, "Silent");
    assert!(f.shell.flyout().get_reset_enabled());
    let slider = control(f.shell.flyout(), Slider, "Keyboard light");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);
    slider.mock_drag(left_end, PointerEventButton::Left);
    assert_eq!(f.shell.flyout().get_brightness(), 0);
    let before = sent_count(&f);
    click(f.shell.flyout(), Button, "Reset to Balanced");

    assert_eq!(
        sent_since(&f, before),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
    assert!(!f.shell.flyout().get_reset_enabled());
}

#[test]
fn the_flyout_rates_lock_on_battery_with_sixty_hz_on() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    assert!(f.shell.flyout().get_rates_enabled());

    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));

    assert!(!f.shell.flyout().get_rates_enabled());
    assert!(f.shell.flyout().get_sixty_hz_on_battery());
}

#[test]
fn a_flyout_rate_click_sets_the_panel_and_sends_nothing() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.flyout(), Button, "60 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(sent_since(&f, before), []);
}

/// The Slint `enabled` lock and the view's own check both stop this click;
/// the view's check has its own test in `crates/ui/tests/display.rs`.
#[test]
fn a_locked_flyout_rate_does_nothing_on_a_click() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    FAKE_ON_BATTERY.set(true);
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert!(!f.shell.flyout().get_rates_enabled());
    let before = sent_count(&f);

    click(f.shell.flyout(), Button, "120 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(sent_since(&f, before), []);
}

/// On battery at 120 Hz (not the highest rate, so a put-back must find the
/// rate that ran), with 60 Hz on battery off.
fn on_battery_fixture() -> Fixture {
    let f = fixture(Config::default());
    click(f.shell.window(), Button, "120 Hz");
    FAKE_ON_BATTERY.set(true);
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 120);
    f
}

/// What both views show for the 60 Hz on battery switch and the rate lock.
/// The clicked switch flips itself in Slint, so only the other view, the
/// config and the refresh prove the click went through.
fn shows_sixty_hz_on_battery(f: &Fixture, on: bool, rates_enabled: bool) {
    assert_eq!(f.shell.app().config().sixty_hz_on_battery, on);
    assert_eq!(f.shell.window().get_sixty_hz_on_battery(), on);
    assert_eq!(f.shell.flyout().get_sixty_hz_on_battery(), on);
    assert_eq!(f.shell.window().get_rates_enabled(), rates_enabled);
    assert_eq!(f.shell.flyout().get_rates_enabled(), rates_enabled);
}

/// Clicks the 60 Hz on battery switch in `root` twice: on drops the panel
/// to 60 Hz and locks the rates, off puts the old rate back. Both views
/// follow, the switch is saved, and nothing goes to the controller.
fn sixty_hz_switch_drops_the_rate_and_puts_it_back(f: &Fixture, root: &impl ElementRoot) {
    let before = sent_count(f);

    click(root, Switch, "Switch to 60 Hz on battery");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    shows_sixty_hz_on_battery(f, true, false);

    click(root, Switch, "Switch to 60 Hz on battery");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 120);
    shows_sixty_hz_on_battery(f, false, true);
    assert_eq!(sent_since(f, before), []);
}

/// On battery with 60 Hz gone from the panel, a click on the switch in
/// `root` is refused: the error shows, and the switch goes back off.
fn sixty_hz_switch_refused_goes_back_off(f: &Fixture, root: &impl ElementRoot) {
    f.shell
        .app()
        .machine_mut()
        .displays
        .0
        .modes
        .retain(|mode| mode.hz != 60);
    let before = sent_count(f);

    click(root, Switch, "Switch to 60 Hz on battery");

    assert_ne!(f.shell.window().get_error(), "");
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 120);
    shows_sixty_hz_on_battery(f, false, true);
    assert_eq!(sent_since(f, before), []);
}

#[test]
fn the_window_sixty_hz_switch_goes_back_off_when_60_hz_is_refused() {
    let f = on_battery_fixture();
    sixty_hz_switch_refused_goes_back_off(&f, f.shell.window());
}

#[test]
fn the_flyout_sixty_hz_switch_goes_back_off_when_60_hz_is_refused() {
    let f = on_battery_fixture();
    sixty_hz_switch_refused_goes_back_off(&f, f.shell.flyout());
}

#[test]
fn the_sixty_hz_switch_on_ac_only_saves() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Switch, "Switch to 60 Hz on battery");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
    shows_sixty_hz_on_battery(&f, true, true);
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn the_window_sixty_hz_switch_drops_the_rate_on_battery_and_puts_it_back() {
    let f = on_battery_fixture();
    sixty_hz_switch_drops_the_rate_and_puts_it_back(&f, f.shell.window());
}

#[test]
fn the_flyout_sixty_hz_switch_drops_the_rate_on_battery_and_puts_it_back() {
    let f = on_battery_fixture();
    sixty_hz_switch_drops_the_rate_and_puts_it_back(&f, f.shell.flyout());
}

#[test]
fn the_flyout_touchpad_switch_presses_the_touchpad_shortcut_once() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.flyout(), Switch, "Touchpad");

    assert_eq!(
        f.shell.app().machine().keys.0,
        [Key::Win, Key::Ctrl, Key::F24]
    );
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn open_window_hides_the_flyout_and_shows_the_window() {
    let f = fixture(Config::default());
    f.shell.window().hide().unwrap();
    f.shell.flyout().hide().unwrap();
    f.shell.toggle_flyout(ICON);
    assert!(f.shell.flyout().window().is_visible());
    let before = sent_count(&f);

    click(f.shell.flyout(), Button, "Open window");

    assert!(f.shell.window().window().is_visible());
    assert!(!f.shell.flyout().window().is_visible());
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn the_touchpad_switches_show_off_and_greyed_when_the_touchpad_cant_be_read() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().registry.touchpad = None;
    f.shell.refresh(None);

    let error = f.shell.window().get_error();
    assert!(error.contains("touchpad"), "{error}");
    assert!(!f.shell.window().get_touchpad_on());
    assert!(!f.shell.window().get_touchpad_known());
    assert!(!f.shell.flyout().get_touchpad_on());
    assert!(!f.shell.flyout().get_touchpad_known());
    assert_eq!(
        control(f.shell.flyout(), Switch, "Touchpad").accessible_enabled(),
        Some(false)
    );
    assert_eq!(
        control(f.shell.window(), Switch, "Touchpad").accessible_enabled(),
        Some(false)
    );
    click(f.shell.flyout(), Switch, "Touchpad");
    click(f.shell.window(), Switch, "Touchpad");
    assert_eq!(f.shell.app().machine().keys.0, []);
    assert!(!f.shell.window().get_touchpad_on());
    assert!(!f.shell.flyout().get_touchpad_on());

    f.shell.app().machine_mut().registry.touchpad = Some(1);
    f.shell.refresh(None);

    assert!(f.shell.window().get_touchpad_on());
    assert!(f.shell.window().get_touchpad_known());
    assert!(f.shell.flyout().get_touchpad_on());
    assert!(f.shell.flyout().get_touchpad_known());
}

#[test]
fn a_failed_click_shows_the_error_and_logs_it() {
    let f = fixture(Config::default());
    // Other tests fail the native mode read too, so the shared log check
    // shows only that such a line is logged, not that this test wrote it.
    f.shell.app().machine_mut().controller.native_fails = true;

    click(f.shell.window(), Button, "60 Hz");

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
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

/// With the lighting saved, the touchpad on and the mic not muted.
fn mic_fixture() -> Fixture {
    let f = touchpad_off_fixture();
    f.shell.app().machine_mut().registry.touchpad = Some(1);
    f.shell.follow_mic(Some(false));
    f
}

/// The frame with the mic key red, or plain while the real Num Lock is on:
/// the poll and the resume read it.
fn mic_red_unless_num_lock() -> Vec<Command> {
    if platform::num_lock_on() {
        static_colour(WHITE, &BLADE_18)
    } else {
        mic_red()
    }
}

fn mic_red() -> Vec<Command> {
    let mic = razer_proto::RedKeys {
        touchpad: false,
        mic: true,
    };
    razer_proto::static_colour_red(WHITE, &BLADE_18, mic)
}

#[test]
fn a_mic_mute_paints_the_mic_key_once_per_change() {
    let f = mic_fixture();
    let before = sent_count(&f);

    f.shell.follow_mic(Some(true));
    f.shell.follow_mic(Some(true));

    assert_eq!(sent_since(&f, before), mic_red());
}

#[test]
fn fn_mic_mutes_the_mic_and_paints_the_key_then_unmutes_it() {
    let f = mic_fixture();
    FAKE_MIC.set(Some(false));
    let before = sent_count(&f);

    f.shell.toggle_mic();
    assert_eq!(FAKE_MIC.get(), Some(true));
    f.shell.toggle_mic();

    assert_eq!(FAKE_MIC.get(), Some(false));
    let mut expected = mic_red();
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent_since(&f, before), expected);
}

#[test]
fn the_poll_follows_a_mute_made_elsewhere() {
    let f = mic_fixture();
    FAKE_MIC.set(Some(true));
    let before = sent_count(&f);

    f.shell.tick();

    assert_eq!(f.shell.app().mic_muted(), Some(true));
    let sent = sent_since(&f, before);
    if platform::num_lock_on() {
        assert_eq!(sent, []);
    } else {
        assert_eq!(sent, mic_red());
    }
}

#[test]
fn a_failed_mic_read_keeps_the_key_as_it_is() {
    let f = mic_fixture();
    FAKE_MIC.set(Some(true));
    f.shell.tick();
    let before = sent_count(&f);

    FAKE_MIC.set(None);
    f.shell.tick();
    FAKE_MIC.set(Some(true));
    f.shell.tick();

    assert_eq!(f.shell.app().mic_muted(), Some(true));
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn fn_mic_with_a_wake_not_yet_followed_mutes_but_sends_nothing() {
    let f = mic_fixture();
    FAKE_MIC.set(Some(false));
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    let before = sent_count(&f);

    f.shell.toggle_mic();

    assert_eq!(FAKE_MIC.get(), Some(true));
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn fn_mic_during_the_resume_pause_mutes_but_sends_nothing() {
    let f = mic_fixture();
    FAKE_MIC.set(Some(false));
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
    let before = sent_count(&f);

    f.shell.toggle_mic();

    assert_eq!(FAKE_MIC.get(), Some(true));
    assert_eq!(f.shell.app().mic_muted(), Some(true));
    assert_eq!(sent_since(&f, before), []);
    // The resume paints it.
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    assert!(sent_since(&f, before).ends_with(&mic_red_unless_num_lock()));
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

/// The fixture with a resume waiting out its pause.
pub(super) fn resuming(f: &Fixture) {
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
}

#[test]
fn a_preset_click_during_the_resume_pause_sends_nothing() {
    let f = fixture(Config::default());
    resuming(&f);
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
}

#[test]
fn a_click_with_a_wake_not_yet_followed_sends_nothing() {
    let f = fixture(Config::default());
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_touchpad_change_with_a_wake_not_yet_followed_sends_nothing() {
    let f = touchpad_off_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    let before = sent_count(&f);

    f.shell.follow_touchpad();

    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_switch_clicked_in_the_resume_pause_shows_the_saved_value_after_it() {
    let f = fixture(Config::default());
    let saved = f.shell.window().get_charge_limit_on();
    resuming(&f);

    click(f.shell.window(), Switch, "Battery charge limit");
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );

    assert_eq!(f.shell.window().get_charge_limit_on(), saved);
}

#[test]
fn the_power_mode_key_during_the_resume_pause_sends_nothing() {
    let f = fixture(Config::default());
    resuming(&f);
    let before = sent_count(&f);

    f.shell.user_act(blade_lite::App::on_power_key);

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
}

#[test]
fn a_preset_click_after_the_resume_pause_sends_it() {
    let f = fixture(Config::default());
    resuming(&f);
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(
        sent_since(&f, before),
        PerfMode::Silent.commands(&BLADE_18).unwrap()
    );
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

#[test]
fn a_wake_on_battery_sends_balanced_for_a_saved_performance() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let before = sent_count(&f);
    FAKE_ON_BATTERY.set(true);
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();

    f.shell.follow_power(&receiver);
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );

    let sent = sent_since(&f, before);
    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    assert!(
        sent.windows(balanced.len()).any(|run| run == balanced),
        "{sent:?}"
    );
    assert!(
        !sent
            .windows(performance.len())
            .any(|run| run == performance),
        "{sent:?}"
    );
}

/// With a teal lighting at 35% saved.
fn lit_fixture() -> Fixture {
    fixture(Config {
        lighting: Some(blade_core::Lighting {
            brightness: 35,
            colour: Rgb::new(0x00, 0xB7, 0xC3),
            vapor_light: true,
        }),
        ..Config::default()
    })
}

#[test]
fn fn_f11_moves_both_brightness_sliders() {
    let f = lit_fixture();
    let before = sent_count(&f);
    f.shell.app().machine_mut().controller.brightness_level = Some(255);

    f.shell.follow_brightness_keys();
    // The sliders follow in their change handlers, run by the event loop.
    slint::platform::update_timers_and_animations();

    let vapor = BLADE_18.vapor_light.unwrap();
    let full = razer_proto::Brightness::new(100).unwrap();
    assert_eq!(sent_since(&f, before), [vapor.command(full, true)]);
    let window_slider = control(f.shell.window(), Slider, "Brightness");
    assert_eq!(window_slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "100%");
    let flyout_slider = control(f.shell.flyout(), Slider, "Keyboard light");
    assert_eq!(flyout_slider.accessible_value().unwrap(), "100");
    assert_eq!(percent_shown(f.shell.flyout(), "FlyoutWindow"), "100%");
}

#[test]
fn an_fn_press_that_changes_nothing_keeps_a_shown_error() {
    let f = lit_fixture();
    f.shell.app().machine_mut().controller.native_fails = true;
    click(f.shell.window(), Button, "60 Hz");
    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
    f.shell.app().machine_mut().controller.native_fails = false;
    let level = razer_proto::Brightness::new(35).unwrap().level();
    f.shell.app().machine_mut().controller.brightness_level = Some(level);

    f.shell.follow_brightness_keys();

    assert_eq!(f.shell.window().get_error(), error);
}

#[test]
fn fn_up_during_the_resume_pause_reads_and_sends_nothing() {
    // The controller may still run another level then; the resume sends
    // the saved one.
    let f = lit_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);
    let before = sent_count(&f);
    f.shell.app().machine_mut().controller.brightness_level = Some(255);

    f.shell.follow_brightness_keys();

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(f.shell.app().config().lighting.unwrap().brightness, 35);
}

#[test]
fn a_failed_brightness_read_after_fn_is_logged_not_shown() {
    // Fn is pressed for many things: a lasting fault must not fill the
    // window on each press.
    let f = lit_fixture();
    let before = sent_count(&f);

    f.shell.follow_brightness_keys();

    assert_eq!(f.shell.window().get_error(), "");
    assert_eq!(sent_since(&f, before), []);
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "35%");
    let log = std::fs::read_to_string(log_path()).unwrap();
    assert!(log.contains("keyboard brightness"), "{log}");
}

#[test]
fn a_failed_vapor_send_after_fn_shows_the_error_and_the_new_level() {
    let f = lit_fixture();
    f.shell.app().machine_mut().controller.brightness_level = Some(255);
    f.shell.app().machine_mut().controller.fails = true;

    f.shell.follow_brightness_keys();
    slint::platform::update_timers_and_animations();

    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
    assert_eq!(percent_shown(f.shell.window(), "KeyboardGroup"), "100%");
}

/// Gives the fake sensors a battery health of 99% and 86 cycles.
fn read_health(f: &Fixture) {
    f.shell.set_readings(blade_core::Readings {
        battery_health: platform::BatteryHealth {
            percent: Some(99),
            cycles: Some(86),
        },
        ..Default::default()
    });
}

/// Whether a text `label` shows in the window.
fn text_shown(f: &Fixture, label: &str) -> bool {
    ElementHandle::find_by_accessible_label(f.shell.window(), label)
        .next()
        .is_some()
}

#[test]
fn the_battery_health_ends_the_charge_limit_note() {
    let f = fixture(Config::default());
    assert_eq!(f.shell.window().get_health(), "", "nothing known yet");
    assert!(text_shown(&f, "Off. Turn on to stop at 80%"));
    read_health(&f);

    f.shell.refresh(None);

    assert!(text_shown(
        &f,
        "Off. Turn on to stop at 80% \u{b7} Health 99%, 86 cycles"
    ));
    assert!(!text_shown(&f, "Battery health"), "no row of its own");
    click(f.shell.window(), Switch, "Battery charge limit");
    assert!(text_shown(
        &f,
        "Stops charging at 80% \u{b7} Health 99%, 86 cycles"
    ));
}

#[test]
fn without_the_charge_limit_the_battery_health_has_its_own_row() {
    let f = fixture(Config::default());
    // A failed set, then a failed read, leaves the limit's state unknown.
    f.shell.app().machine_mut().controller.fails = true;
    f.shell.app().machine_mut().controller.charge = None;
    click(f.shell.window(), Switch, "Battery charge limit");
    f.shell.app().machine_mut().controller.fails = false;
    assert!(!text_shown(&f, "Battery health"), "nothing known yet");
    read_health(&f);

    f.shell.refresh(None);

    assert!(!text_shown(&f, "Battery charge limit"));
    assert!(text_shown(&f, "Battery health"));
    assert!(text_shown(&f, "99%, 86 cycles"));
}

#[test]
fn plugging_in_does_not_show_the_old_charging_flag() {
    let f = fixture(Config::default());
    FAKE_ON_BATTERY.set(true);
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    f.shell.set_readings(blade_core::Readings {
        charging: Some(false),
        ..Default::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    FAKE_ON_BATTERY.set(false);
    sender.send(PowerEvent::OnAc).unwrap();

    f.shell.follow_power(&receiver);

    assert_eq!(f.shell.window().get_power(), "Plugged in");
    assert_eq!(f.shell.flyout().get_power(), "Plugged in");
}

#[test]
fn a_touchpad_change_shows_the_touchpad_popup() {
    let f = fixture(Config::default());
    // The physical key, the window's switch or Windows turned it off.
    f.shell.app().machine_mut().registry.touchpad = Some(0);

    f.shell.follow_touchpad();

    assert_eq!(f.shell.popup().get_caption(), "Touchpad");
    assert_eq!(f.shell.popup().get_mode(), "Off");
}

#[test]
fn a_touchpad_watch_with_no_change_shows_no_popup() {
    let f = fixture(Config::default());

    f.shell.follow_touchpad();

    assert_eq!(f.shell.popup().get_mode(), "");
}

#[test]
fn a_new_mode_after_a_touchpad_popup_says_power_mode() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().registry.touchpad = Some(0);
    f.shell.follow_touchpad();

    click(f.shell.window(), Button, "Silent");

    assert_eq!(f.shell.popup().get_caption(), "Power mode");
    assert_eq!(f.shell.popup().get_mode(), "Silent");
}

#[test]
fn a_touchpad_change_with_a_wake_not_yet_followed_shows_the_popup_and_sends_nothing() {
    let f = fixture(Config::default());
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.app().machine_mut().registry.touchpad = Some(0);
    let before = sent_count(&f);

    f.shell.follow_touchpad();

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(f.shell.popup().get_caption(), "Touchpad");
    assert_eq!(f.shell.popup().get_mode(), "Off");
}

#[test]
fn a_hot_spell_shows_the_heat_popup_once() {
    let f = fixture(Config::default());
    let start = Instant::now();

    for second in 0..=ui::HEAT_HOLD.as_secs() * 2 {
        f.shell
            .watch_heat(Some(101), None, start + Duration::from_secs(second));
        if second == ui::HEAT_HOLD.as_secs() {
            assert_eq!(f.shell.popup().get_caption(), "Too hot");
            assert_eq!(f.shell.popup().get_mode(), "CPU 101°");
            f.shell.popup().set_mode("".into());
        }
    }

    assert_eq!(f.shell.popup().get_mode(), "", "once per spell");
}

/// The temperature queries the controller got since `before` queries.
fn temperature_reads_since(f: &Fixture, before: usize) -> usize {
    let app = f.shell.app();
    let asked = &app.machine().controller.asked[before..];
    asked.iter().filter(|&c| *c == Celsius::query()).count()
}

fn asked_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.asked.len()
}

#[test]
fn with_no_view_open_a_tick_reads_only_the_cpu_temperature_and_the_charger() {
    let f = fixture(Config::default());
    let (asked, sent) = (asked_count(&f), sent_count(&f));

    f.shell.tick();

    let app = f.shell.app();
    assert_eq!(
        app.machine().controller.asked[asked..],
        [Celsius::query(), Charger::query()]
    );
    drop(app);
    assert_eq!(sent_since(&f, sent), []);
    assert_eq!(f.shell.app().cpu_celsius(), None, "the fake can't read it");
}

#[test]
fn the_next_tick_within_ten_seconds_reads_nothing() {
    let f = fixture(Config::default());
    f.shell.tick();
    let asked = asked_count(&f);

    f.shell.tick();

    assert_eq!(asked_count(&f), asked);
}

#[test]
fn no_temperature_read_while_a_wake_waits() {
    let f = fixture(Config::default());
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    let asked = asked_count(&f);

    f.shell.tick();

    assert_eq!(temperature_reads_since(&f, asked), 0);
}

#[test]
fn with_the_window_open_a_tick_reads_no_controller_while_a_wake_waits() {
    let f = fixture(Config::default());
    f.shell.show_window();
    resuming(&f);
    let asked = asked_count(&f);

    f.shell
        .tick_at(Instant::now() + Duration::from_secs(60), SystemTime::now());

    assert_eq!(asked_count(&f), asked);
}

#[test]
fn with_the_window_open_a_tick_reads_the_controller_after_the_wake_pause() {
    let f = fixture(Config::default());
    f.shell.show_window();
    resuming(&f);
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    let asked = asked_count(&f);

    f.shell
        .tick_at(Instant::now() + Duration::from_secs(60), SystemTime::now());

    assert!(temperature_reads_since(&f, asked) > 0);
}

#[test]
fn a_poll_due_in_the_wake_pause_reads_on_the_first_tick_after_it() {
    let f = fixture(Config::default());
    f.shell.show_window();
    let later = Instant::now() + Duration::from_secs(60);
    resuming(&f);
    f.shell.tick_at(later, SystemTime::now());
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    let asked = asked_count(&f);

    f.shell.tick_at(later, SystemTime::now());

    assert!(temperature_reads_since(&f, asked) > 0);
}

/// Ticks every `step` from `start` for `duration`, as the 1 s timer would
/// at those times.
fn tick_for(f: &Fixture, start: Instant, step: Duration, duration: Duration) {
    let wall = SystemTime::now();
    let mut at = Duration::ZERO;
    while at <= duration {
        f.shell.tick_at(start + at, wall + at);
        at += step;
    }
}

#[test]
fn with_no_view_open_a_hot_cpu_shows_the_heat_popup() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.cpu_celsius = Some(101);

    tick_for(&f, Instant::now(), Duration::from_secs(1), ui::HEAT_HOLD);

    assert_eq!(f.shell.popup().get_caption(), "Too hot");
    assert_eq!(f.shell.popup().get_mode(), "CPU 101°");
}

#[test]
fn with_the_flyout_open_a_hot_cpu_shows_the_heat_popup() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.cpu_celsius = Some(101);
    f.shell.toggle_flyout(ICON);

    tick_for(&f, Instant::now(), Duration::from_secs(1), ui::HEAT_HOLD);

    assert_eq!(f.shell.popup().get_mode(), "CPU 101°");
}

#[test]
fn no_background_temperature_read_while_razer_software_runs() {
    let f = fixture(Config::default());
    f.shell.programs().set(&["Razer Synapse Service"]);
    f.shell.look_for_razer();
    let asked = asked_count(&f);

    f.shell.tick();

    assert_eq!(temperature_reads_since(&f, asked), 0);
}

#[test]
fn with_the_window_open_a_tick_reads_no_controller_while_razer_software_runs() {
    let f = fixture(Config::default());
    f.shell.programs().set(&["Razer Synapse Service"]);
    f.shell.look_for_razer();
    f.shell.app().machine_mut().controller.cpu_celsius = Some(50);
    f.shell.show_window();
    let asked = asked_count(&f);

    f.shell
        .tick_at(Instant::now() + Duration::from_secs(60), SystemTime::now());

    assert_eq!(asked_count(&f), asked);
    assert_eq!(f.shell.app().cpu_celsius(), None, "not read, so unknown");
}

#[test]
fn a_cpu_and_gpu_warning_together_show_both() {
    let f = fixture(Config::default());
    let start = Instant::now();

    for second in (0..=ui::HEAT_HOLD.as_secs()).step_by(10) {
        let now = start + Duration::from_secs(second);
        f.shell.watch_heat(Some(101), Some(97), now);
    }

    assert_eq!(f.shell.popup().get_mode(), "CPU 101° · GPU 97°");
}

#[test]
fn a_heat_warning_is_logged() {
    let f = fixture(Config::default());
    let start = Instant::now();

    for second in (0..=ui::HEAT_HOLD.as_secs()).step_by(10) {
        f.shell
            .watch_heat(None, Some(98), start + Duration::from_secs(second));
    }

    let log = std::fs::read_to_string(log_path()).unwrap();
    assert!(log.contains("too hot for 3 min: GPU 98°"), "{log}");
}

#[test]
fn a_wake_starts_the_hot_spell_again() {
    let f = fixture(Config::default());
    let start = Instant::now();
    for second in (0..ui::HEAT_HOLD.as_secs()).step_by(10) {
        f.shell
            .watch_heat(Some(101), None, start + Duration::from_secs(second));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.follow_power(&receiver);

    f.shell.watch_heat(Some(101), None, start + ui::HEAT_HOLD);

    assert_eq!(f.shell.popup().get_mode(), "");
}

#[test]
fn a_short_peak_shows_no_heat_popup_and_sends_nothing() {
    let f = fixture(Config::default());
    let start = Instant::now();
    let before = sent_count(&f);

    for second in 0..ui::HEAT_HOLD.as_secs() {
        f.shell
            .watch_heat(None, Some(99), start + Duration::from_secs(second));
    }

    assert_eq!(f.shell.popup().get_mode(), "");
    assert_eq!(sent_since(&f, before), []);
}

#[test]
fn a_wake_60_hz_refused_on_battery_is_set_on_a_later_tick_with_no_error() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    FAKE_ON_BATTERY.set(true);
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.tick();
    // Windows puts 240 Hz back in the wake, and refuses the first 60 Hz.
    f.shell.app().machine_mut().displays.0.current.hz = 240;
    f.shell.app().machine_mut().displays.1 = 1;
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.tick();
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);

    f.shell.tick();

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn a_wake_60_hz_refused_every_time_shows_only_the_last_error() {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    FAKE_ON_BATTERY.set(true);
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.tick();
    f.shell.app().machine_mut().displays.0.current.hz = 240;
    f.shell.app().machine_mut().displays.1 = usize::MAX;
    sender.send(PowerEvent::Resumed).unwrap();
    f.shell.tick();
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );

    for _ in 1..blade_core::RESUME_RATE_TRIES {
        f.shell.tick();
        assert_eq!(f.shell.window().get_error(), "");
    }
    f.shell.tick();
    assert!(f.shell.window().get_error().contains("refresh rate"));
    let left = f.shell.app().machine().displays.1;
    f.shell.tick();

    assert_eq!(f.shell.app().machine().displays.1, left);
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
}

/// A switch to battery whose 60 Hz Windows refuses `refusals` times,
/// passed on by one tick.
fn switched_with_60_hz_refused(refusals: usize) -> Fixture {
    let f = fixture(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    FAKE_ON_BATTERY.set(true);
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    f.shell.app().machine_mut().displays.1 = refusals;
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.tick();
    f
}

#[test]
fn a_switch_60_hz_refused_is_set_on_the_next_tick_with_no_error() {
    let f = switched_with_60_hz_refused(1);
    // Not tried again in the tick that was refused.
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
    assert_eq!(f.shell.window().get_error(), "");

    f.shell.tick();

    assert_eq!(f.shell.app().machine().displays.0.current.hz, 60);
    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn a_switch_60_hz_refused_every_time_shows_only_the_last_error() {
    let f = switched_with_60_hz_refused(usize::MAX);

    for _ in 1..blade_core::RESUME_RATE_TRIES {
        f.shell.tick();
        assert_eq!(f.shell.window().get_error(), "");
    }
    f.shell.tick();

    assert!(f.shell.window().get_error().contains("refresh rate"));
    assert_eq!(f.shell.app().machine().displays.0.current.hz, 240);
}

#[test]
fn a_typed_custom_colour_sends_and_saves_it_on_enter() {
    let f = fixture(Config::default());
    assert_eq!(f.shell.app().config().lighting, None);
    let before = sent_count(&f);
    let field = control(f.shell.window(), TextInput, "Custom colour");
    let start = field.absolute_position();
    let size = field.size();
    let middle = LogicalPosition::new(start.x + size.width / 2.0, start.y + size.height / 2.0);
    let window = f.shell.window().window();
    let button = PointerEventButton::Left;
    window.dispatch_event(WindowEvent::PointerPressed {
        position: middle,
        button,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: middle,
        button,
    });

    field.set_accessible_value("#00b7c3");
    assert_eq!(sent_since(&f, before), []);
    let enter: slint::SharedString = slint::platform::Key::Return.into();
    window.dispatch_event(WindowEvent::KeyPressed {
        text: enter.clone(),
    });
    window.dispatch_event(WindowEvent::KeyReleased { text: enter });

    let colour = Rgb::new(0x00, 0xB7, 0xC3);
    assert_eq!(sent_since(&f, before), lighting_commands(100, colour));
    assert_eq!(f.shell.app().config().lighting.unwrap().colour, colour);
    assert_eq!(field.accessible_value().unwrap(), "#00B7C3");
}

#[test]
fn a_custom_cpu_boost_click_one_step_up_sends_the_custom_commands() {
    let f = fixture(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    let before = sent_count(&f);
    let slider = control(f.shell.window(), Slider, "CPU boost");
    assert_eq!(slider.accessible_value().unwrap(), "1");
    let start = slider.absolute_position();
    let size = slider.size();
    // Level 2 of 0 to 3, on the track.
    let position = LogicalPosition::new(
        start.x + size.width * 2.0 / 3.0,
        start.y + size.height / 2.0,
    );
    let window = f.shell.window().window();
    let button = PointerEventButton::Left;
    window.dispatch_event(WindowEvent::PointerPressed { position, button });
    window.dispatch_event(WindowEvent::PointerReleased { position, button });

    let custom = razer_proto::Custom {
        cpu: razer_proto::CpuBoost::new(2, &BLADE_18).unwrap(),
        gpu: razer_proto::GpuBoost::new(1, &BLADE_18).unwrap(),
        fan: razer_proto::FanMode::Auto,
    };
    assert_eq!(sent_since(&f, before), custom.commands(&BLADE_18));
    assert_eq!(f.shell.app().config().custom.cpu_boost, 2);
    // The rows are made again after a change, so find the slider again.
    let slider = control(f.shell.window(), Slider, "CPU boost");
    assert_eq!(slider.accessible_value().unwrap(), "2");
}

#[test]
fn a_custom_slider_goes_back_after_a_failed_send() {
    let f = fixture(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    f.shell.app().machine_mut().controller.fails = true;
    let slider = control(f.shell.window(), Slider, "CPU boost");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);

    slider.mock_drag(left_end, PointerEventButton::Left);

    let error = f.shell.window().get_error();
    assert!(error.contains("busy"), "{error}");
    assert_eq!(f.shell.app().config().custom.cpu_boost, 1);
    // The rows are made again after a change, so find the slider again.
    let slider = control(f.shell.window(), Slider, "CPU boost");
    assert_eq!(slider.accessible_value().unwrap(), "1");
}

#[test]
fn focusing_the_window_puts_back_a_custom_slider_drag_cut_short() {
    custom_drag_cut_short_is_put_back(|f| f.shell.on_window_event(&WinitEvent::Focused(true)));
}

#[test]
fn showing_the_window_puts_back_a_custom_slider_drag_cut_short() {
    custom_drag_cut_short_is_put_back(|f| f.shell.show_window());
}

/// Presses the CPU boost slider at its left end and never lets it go, runs
/// `come_back` (a show or focus), then lets go there: nothing is sent and
/// the slider shows the saved 1 again.
fn custom_drag_cut_short_is_put_back(come_back: impl FnOnce(&Fixture)) {
    let f = fixture(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    let slider = control(f.shell.window(), Slider, "CPU boost");
    let start = slider.absolute_position();
    let left_end = LogicalPosition::new(start.x, start.y + slider.size().height / 2.0);
    f.shell
        .window()
        .window()
        .dispatch_event(WindowEvent::PointerPressed {
            position: left_end,
            button: PointerEventButton::Left,
        });
    assert_eq!(slider.accessible_value().unwrap(), "0");
    let before = sent_count(&f);

    come_back(&f);
    // Back again: a press elsewhere, let go over the slider.
    f.shell
        .window()
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: left_end,
            button: PointerEventButton::Left,
        });

    assert_eq!(sent_since(&f, before), []);
    assert_eq!(f.shell.app().config().custom.cpu_boost, 1);
    let slider = control(f.shell.window(), Slider, "CPU boost");
    assert_eq!(slider.accessible_value().unwrap(), "1");
}

fn charger_reads_since(f: &Fixture, before: usize) -> usize {
    let app = f.shell.app();
    let asked = &app.machine().controller.asked[before..];
    asked.iter().filter(|&c| *c == Charger::query()).count()
}

#[test]
fn a_weak_charge_seen_on_a_tick_runs_balanced_and_says_so() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    f.shell.app().machine_mut().controller.charger = FakeCharger::UsbC;
    let sent = sent_count(&f);

    f.shell.tick();

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), balanced);
    assert_eq!(
        f.shell.window().get_power(),
        "Plugged in \u{b7} Weak charger"
    );
    assert_eq!(f.shell.popup().get_mode(), "Balanced");
    assert_eq!(f.shell.app().config().preset, Preset::Performance);
}

#[test]
fn the_full_charger_back_on_a_tick_runs_the_saved_preset_again() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    f.shell.app().machine_mut().controller.charger = FakeCharger::UsbC;
    let start = Instant::now();
    tick_for(&f, start, Duration::from_secs(1), Duration::ZERO);
    f.shell.app().machine_mut().controller.charger = FakeCharger::Razer;
    let sent = sent_count(&f);

    // Two full power reads in a row bring it back.
    tick_for(
        &f,
        start + blade_core::HEAT_INTERVAL,
        blade_core::HEAT_INTERVAL,
        blade_core::HEAT_INTERVAL,
    );

    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), performance);
    assert_eq!(f.shell.window().get_power(), "Plugged in");
}

#[test]
fn no_charger_read_while_a_wake_waits() {
    let f = fixture(Config::default());
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::Resumed).unwrap();
    let asked = asked_count(&f);

    f.shell.tick();

    assert_eq!(charger_reads_since(&f, asked), 0);
}

#[test]
fn no_background_charger_read_while_razer_software_runs() {
    let f = fixture(Config::default());
    f.shell.programs().set(&["Razer Synapse Service"]);
    f.shell.look_for_razer();
    let asked = asked_count(&f);

    f.shell.tick();

    assert_eq!(charger_reads_since(&f, asked), 0);
}

#[test]
fn a_weak_charge_with_balanced_saved_greys_out_performance_and_says_so() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.charger = FakeCharger::UsbC;
    let sent = sent_count(&f);

    f.shell.tick();

    assert_eq!(sent_since(&f, sent), []);
    assert_eq!(
        f.shell.flyout().get_power(),
        "Plugged in \u{b7} Weak charger"
    );
    let view = f.shell.app().performance_view();
    let performance = view
        .presets
        .iter()
        .find(|o| o.preset == Preset::Performance);
    assert!(!performance.unwrap().enabled);
}

/// Performance saved, then a switch to battery that failed.
fn failed_switch_fixture() -> Fixture {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    FAKE_ON_BATTERY.set(true);
    f.shell.app().machine_mut().controller.fails = true;
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.follow_power(&receiver);
    f.shell.app().machine_mut().controller.fails = false;
    f
}

/// Two heat reads in a row, as with no view open: the first poll after a
/// failure waits, the second may send again.
fn two_heat_reads(f: &Fixture) {
    let (start, wall) = (Instant::now(), SystemTime::now());
    f.shell.tick_at(start, wall);
    let later = blade_core::HEAT_INTERVAL;
    f.shell.tick_at(start + later, wall + later);
}

#[test]
fn a_failed_switch_to_battery_is_sent_again_on_the_poll_after_the_next() {
    let f = failed_switch_fixture();
    let error = f.shell.window().get_error();
    let sent = sent_count(&f);

    f.shell.tick();
    assert_eq!(sent_since(&f, sent), []);
    two_heat_reads(&f);

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), balanced);
    // Quiet: the switch's error stays shown.
    assert_ne!(error, "");
    assert_eq!(f.shell.window().get_error(), error);
}

#[test]
fn a_switch_that_fails_again_in_a_tick_is_not_sent_again_in_that_tick() {
    let f = failed_switch_fixture();
    f.shell.tick();
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender.send(PowerEvent::OnAc).unwrap();
    FAKE_ON_BATTERY.set(false);
    // Each run takes one command, then fails.
    f.shell.app().machine_mut().controller.fails_after = Some(1);
    let sent = sent_count(&f);

    let later = blade_core::HEAT_INTERVAL;
    f.shell
        .tick_at(Instant::now() + later, SystemTime::now() + later);

    assert_eq!(sent_since(&f, sent).len(), 1);
}

#[test]
fn no_resend_while_a_wake_waits() {
    let f = failed_switch_fixture();
    resuming(&f);
    let sent = sent_count(&f);

    two_heat_reads(&f);

    assert_eq!(sent_since(&f, sent), []);
}

#[test]
fn a_power_event_in_the_wake_pause_waits_for_the_resume() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(PowerEvent::Resumed).unwrap();
    FAKE_ON_BATTERY.set(true);
    sender.send(PowerEvent::OnBattery).unwrap();
    let sent = sent_count(&f);

    f.shell.follow_power(&receiver);

    assert_eq!(sent_since(&f, sent), []);
    i_slint_backend_testing::mock_elapsed_time(
        blade_core::RESUME_PAUSE + std::time::Duration::from_millis(1),
    );
    let after = sent_since(&f, sent);
    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    assert!(after.windows(balanced.len()).any(|run| run == balanced));
    assert!(
        !after
            .windows(performance.len())
            .any(|run| run == performance)
    );
}

#[test]
fn with_no_power_events_a_tick_follows_the_power_source() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    // Windows refused the power events: the sender is gone.
    let (_, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    FAKE_ON_BATTERY.set(true);
    let sent = sent_count(&f);

    f.shell.tick();
    f.shell.tick();

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), balanced);
    assert!(f.shell.app().on_battery());
}

#[test]
fn with_power_events_a_tick_does_not_read_the_power_source() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (_sender, receiver) = std::sync::mpsc::channel::<PowerEvent>();
    f.shell.listen_for_power(receiver);
    FAKE_ON_BATTERY.set(true);
    let sent = sent_count(&f);

    f.shell.tick();

    assert_eq!(sent_since(&f, sent), []);
    assert!(!f.shell.app().on_battery());
}

#[test]
fn a_switch_that_fails_in_a_tick_is_not_sent_again_in_that_tick() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    FAKE_ON_BATTERY.set(true);
    sender.send(PowerEvent::OnBattery).unwrap();
    // Each run takes one command, then fails.
    f.shell.app().machine_mut().controller.fails_after = Some(1);
    let sent = sent_count(&f);

    f.shell.tick();

    assert_eq!(sent_since(&f, sent).len(), 1);
    assert!(f.shell.app().preset_unsure());
}

#[test]
fn no_resend_while_razer_software_runs() {
    let f = failed_switch_fixture();
    f.shell.programs().set(&["Razer Synapse Service"]);
    f.shell.look_for_razer();
    let sent = sent_count(&f);

    two_heat_reads(&f);

    assert_eq!(sent_since(&f, sent), []);
}

#[test]
fn with_no_power_events_an_unknown_source_changes_nothing() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (_, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    super::FAKE_SOURCE_UNKNOWN.set(true);
    let sent = sent_count(&f);

    f.shell.tick();

    assert_eq!(sent_since(&f, sent), []);
    assert!(!f.shell.app().on_battery());
}

/// A heat read `n` intervals after `start`, as with no view open.
fn heat_read(f: &Fixture, start: (Instant, SystemTime), n: u32) {
    let later = blade_core::HEAT_INTERVAL * n;
    f.shell.tick_at(start.0 + later, start.1 + later);
}

#[test]
fn a_switch_that_fails_in_a_tick_waits_out_the_next_tick_too() {
    let f = fixture(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    FAKE_ON_BATTERY.set(true);
    sender.send(PowerEvent::OnBattery).unwrap();
    f.shell.app().machine_mut().controller.fails = true;
    let start = (Instant::now(), SystemTime::now());
    heat_read(&f, start, 0);
    f.shell.app().machine_mut().controller.fails = false;
    let sent = sent_count(&f);

    heat_read(&f, start, 1);
    assert_eq!(sent_since(&f, sent), []);
    heat_read(&f, start, 2);

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), balanced);
}

#[test]
fn a_failed_resend_waits_out_the_next_poll() {
    let f = failed_switch_fixture();
    let start = (Instant::now(), SystemTime::now());
    heat_read(&f, start, 0);
    f.shell.app().machine_mut().controller.fails = true;
    heat_read(&f, start, 1);
    f.shell.app().machine_mut().controller.fails = false;
    let sent = sent_count(&f);

    heat_read(&f, start, 2);
    assert_eq!(sent_since(&f, sent), []);
    heat_read(&f, start, 3);

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert_eq!(sent_since(&f, sent), balanced);
}
