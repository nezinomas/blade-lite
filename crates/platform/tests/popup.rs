//! The real work area, which the power mode popup sits in.

use platform::work_area;

#[test]
fn the_work_area_has_a_size() {
    let (_, _, width, height) = work_area().unwrap();

    assert!(width > 0 && height > 0, "{width} x {height}");
}
