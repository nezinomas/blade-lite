//! The Keyboard and touchpad group's view model.

use razer_proto::{Brightness, Rgb};
use ui::{KeyboardCommand, KeyboardState, KeyboardView};

const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);
const TEAL: Rgb = Rgb::new(0x00, 0xB7, 0xC3);
const PINK: Rgb = Rgb::new(0xFF, 0x69, 0xB4);

fn brightness(percent: u8) -> Brightness {
    Brightness::new(percent).expect("0 to 100")
}

/// Lit at 70% in teal, with the touchpad on.
fn state() -> KeyboardState {
    KeyboardState {
        brightness: brightness(70),
        colour: TEAL,
        touchpad_on: true,
    }
}

fn view() -> KeyboardView {
    KeyboardView::new(&state())
}

fn dark() -> KeyboardView {
    KeyboardView::new(&KeyboardState {
        brightness: brightness(0),
        ..state()
    })
}

#[test]
fn the_brightness_slider_runs_from_0_to_100_and_shows_the_percent() {
    let view = view();

    assert_eq!(view.brightness_max, 100);
    assert_eq!(view.brightness, 70);
    assert_eq!(view.brightness_text, "70%");
}

#[test]
fn moving_the_brightness_slider_sets_that_brightness() {
    let view = view();

    assert_eq!(
        view.move_brightness(0),
        Some(KeyboardCommand::SetBrightness(brightness(0)))
    );
    assert_eq!(
        view.move_brightness(100),
        Some(KeyboardCommand::SetBrightness(brightness(100)))
    );
}

#[test]
fn a_brightness_move_that_changes_nothing_or_goes_off_the_end_does_nothing() {
    let view = view();

    assert_eq!(view.move_brightness(70), None);
    assert_eq!(view.move_brightness(101), None);
}

#[test]
fn at_0_the_lights_are_off_and_the_colour_choice_is_disabled() {
    let view = dark();

    assert_eq!(view.brightness_text, "Off");
    assert!(!view.colours_enabled);
    assert_eq!(view.pick_swatch(0), None);
    assert_eq!(view.pick_custom(PINK), None);
}

#[test]
fn above_0_the_colour_choice_is_enabled() {
    assert!(view().colours_enabled);
}

#[test]
fn the_swatch_with_the_current_colour_is_selected() {
    let view = view();

    let selected: Vec<Rgb> = view
        .swatches
        .iter()
        .filter(|swatch| swatch.selected)
        .map(|swatch| swatch.colour)
        .collect();
    assert_eq!(selected, [TEAL]);
    assert!(!view.custom_selected);
}

#[test]
fn picking_another_swatch_sets_its_colour() {
    let view = view();
    let white = view
        .swatches
        .iter()
        .position(|swatch| swatch.colour == WHITE)
        .expect("a white swatch");
    let teal = view
        .swatches
        .iter()
        .position(|swatch| swatch.selected)
        .expect("a selected swatch");

    assert_eq!(
        view.pick_swatch(white),
        Some(KeyboardCommand::SetColour(WHITE))
    );
    assert_eq!(view.pick_swatch(teal), None);
    assert_eq!(view.pick_swatch(view.swatches.len()), None);
}

#[test]
fn a_colour_off_the_swatches_shows_as_the_custom_colour() {
    let view = KeyboardView::new(&KeyboardState {
        colour: PINK,
        ..state()
    });

    assert!(view.swatches.iter().all(|swatch| !swatch.selected));
    assert!(view.custom_selected);
    assert_eq!(view.custom_colour, PINK);
}

#[test]
fn picking_a_custom_colour_sets_it_unless_it_is_the_current_one() {
    let view = view();

    assert_eq!(
        view.pick_custom(PINK),
        Some(KeyboardCommand::SetColour(PINK))
    );
    assert_eq!(view.pick_custom(TEAL), None);
}

#[test]
fn the_touchpad_switch_shows_the_current_state() {
    let off = KeyboardView::new(&KeyboardState {
        touchpad_on: false,
        ..state()
    });

    assert!(view().touchpad_on);
    assert!(!off.touchpad_on);
}

#[test]
fn flipping_the_touchpad_switch_asks_for_the_other_state() {
    let off = KeyboardView::new(&KeyboardState {
        touchpad_on: false,
        ..state()
    });

    assert_eq!(
        view().toggle_touchpad(),
        KeyboardCommand::SetTouchpad(false)
    );
    assert_eq!(off.toggle_touchpad(), KeyboardCommand::SetTouchpad(true));
}
