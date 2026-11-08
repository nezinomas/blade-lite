//! When the power mode popup shows, and what it says.

use blade_core::Preset;
use ui::ModeWatch;

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

    assert_eq!(watch.changed(Preset::Performance, true), Some("Performance"));
    assert_eq!(watch.changed(Preset::Custom, true), Some("Custom"));
    assert_eq!(watch.changed(Preset::Balanced, true), Some("Balanced"));
}

#[test]
fn a_change_that_failed_shows_no_popup_now_or_later() {
    let mut watch = ModeWatch::new(Preset::Performance);

    assert_eq!(watch.changed(Preset::Balanced, false), None);
    assert_eq!(watch.changed(Preset::Balanced, true), None);
}
