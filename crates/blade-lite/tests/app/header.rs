//! The header's readings.

use blade_core::Readings;

use crate::app;

#[test]
fn readings_the_controller_cannot_give_show_dashes() {
    let (mut app, _dir, _) = app();

    let header = app.header(Readings::default());

    let fan = header.stats.iter().find(|s| s.label == "Fan 1").unwrap();
    assert_eq!(fan.value, ui::MISSING);
}

#[test]
fn the_header_keeps_the_controller_read_errors() {
    let (mut app, _dir, _) = app();

    app.header(Readings::default());

    assert!(
        app.read_errors().iter().any(|e| e.starts_with("fan 1: ")),
        "{:?}",
        app.read_errors()
    );
}
