//! When the power mode and touchpad popups show, and what they say.

use blade_core::Preset;
use ui::{ModeWatch, TouchpadWatch};

#[test]
fn the_mode_at_start_shows_no_popup() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Balanced, true), None);
}

#[test]
fn a_new_mode_shows_its_name_once() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Silent, true), Some("Silent"));
    assert_eq!(watch.changed(Preset::Silent, true), None);
}

#[test]
fn each_change_shows_the_mode_it_changed_to() {
    let mut watch = ModeWatch::new(Preset::Silent);

    assert_eq!(
        watch.changed(Preset::Performance, true),
        Some("Performance")
    );
    assert_eq!(watch.changed(Preset::Custom, true), Some("Custom"));
    assert_eq!(watch.changed(Preset::Balanced, true), Some("Balanced"));
}

#[test]
fn a_change_that_failed_shows_no_popup_now_or_later() {
    let mut watch = ModeWatch::new(Preset::Performance);

    assert_eq!(watch.changed(Preset::Balanced, false), None);
    assert_eq!(watch.changed(Preset::Balanced, true), None);
}

#[test]
fn the_touchpad_state_at_start_shows_no_popup() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(Some(true)), None);
}

#[test]
fn a_touchpad_change_shows_on_or_off_once() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(Some(false)), Some("Off"));
    assert_eq!(watch.changed(Some(false)), None);
    assert_eq!(watch.changed(Some(true)), Some("On"));
}

#[test]
fn a_touchpad_state_that_cant_be_read_shows_no_popup_now_or_after() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(None), None);
    assert_eq!(watch.changed(Some(false)), None);
    assert_eq!(watch.changed(Some(true)), Some("On"));
}
