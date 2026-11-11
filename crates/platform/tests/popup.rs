//! The real work areas, which the power mode popup and the tray flyout sit in.

use platform::{work_area, work_area_at};

#[test]
fn the_work_area_has_a_size() {
    let (_, _, width, height) = work_area().unwrap();

    assert!(width > 0 && height > 0, "{width} x {height}");
}

#[test]
fn a_point_on_the_main_screen_gets_its_work_area() {
    let (x, y, width, height) = work_area().unwrap();

    let area = work_area_at((x + width / 2, y + height / 2)).unwrap();

    assert_eq!(area, (x, y, width, height));
}

#[test]
fn a_point_off_every_screen_gets_the_nearest_work_area() {
    let (_, _, width, height) = work_area_at((i32::MIN, i32::MAX)).unwrap();

    assert!(width > 0 && height > 0, "{width} x {height}");
}
