//! When the power mode popup shows, and what it says.

use blade_core::Preset;
use ui::ModeWatch;

#[test]
fn the_mode_at_start_shows_no_popup() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Balanced), None);
}

#[test]
fn a_new_mode_shows_its_name_once() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Silent), Some("Silent"));
    assert_eq!(watch.changed(Preset::Silent), None);
}

#[test]
fn each_change_shows_the_mode_it_changed_to() {
    let mut watch = ModeWatch::new(Preset::Silent);

    assert_eq!(watch.changed(Preset::Performance), Some("Performance"));
    assert_eq!(watch.changed(Preset::Custom), Some("Custom"));
    assert_eq!(watch.changed(Preset::Balanced), Some("Balanced"));
}
