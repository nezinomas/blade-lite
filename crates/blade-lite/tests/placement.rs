//! Where the tray flyout opens: above the tray icon, inside the screen.

use blade_lite::{Rect, flyout_position, popup_position};

/// A 2560 × 1600 screen with the taskbar at the bottom.
const SCREEN: Rect = Rect {
    x: 0,
    y: 0,
    width: 2560,
    height: 1600,
};

const FLYOUT: (i32, i32) = (400, 500);

fn icon(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        width: 40,
        height: 48,
    }
}

#[test]
fn it_opens_above_the_icon_centred_on_it() {
    let (x, y) = flyout_position(icon(1000, 1552), FLYOUT, SCREEN);

    assert_eq!(x, 1020 - 200);
    assert_eq!(y, 1552 - 500 - 12);
}

#[test]
fn it_stays_inside_the_right_edge() {
    let (x, _) = flyout_position(icon(2480, 1552), FLYOUT, SCREEN);

    assert_eq!(x, 2560 - 400 - 12);
}

#[test]
fn it_stays_inside_the_left_edge() {
    let (x, _) = flyout_position(icon(10, 1552), FLYOUT, SCREEN);

    assert_eq!(x, 12);
}

#[test]
fn on_a_screen_narrower_than_the_flyout_it_keeps_the_left_edge() {
    let narrow = Rect {
        width: 300,
        ..SCREEN
    };

    let (x, _) = flyout_position(icon(100, 1552), FLYOUT, narrow);

    assert_eq!(x, 12);
}

#[test]
fn on_a_screen_of_no_width_it_keeps_the_left_edge() {
    let none = Rect { width: 0, ..SCREEN };

    let (x, _) = flyout_position(icon(0, 1552), FLYOUT, none);

    assert_eq!(x, 12);
}

#[test]
fn on_a_narrow_screen_left_of_the_main_one_it_keeps_that_left_edge() {
    let narrow = Rect {
        x: -1920,
        width: 300,
        ..SCREEN
    };

    let (x, _) = flyout_position(icon(-1820, 1552), FLYOUT, narrow);

    assert_eq!(x, -1920 + 12);
}

#[test]
fn a_broken_huge_screen_size_does_not_overflow() {
    let huge = Rect {
        x: i32::MAX,
        y: i32::MAX,
        width: i32::MAX,
        height: i32::MAX,
    };

    let _ = flyout_position(icon(i32::MAX, i32::MAX), (i32::MAX, i32::MAX), huge);
    let _ = popup_position(huge, (i32::MIN, i32::MIN));
}

#[test]
fn with_the_taskbar_at_the_top_it_opens_below_the_icon() {
    let (_, y) = flyout_position(icon(1000, 0), FLYOUT, SCREEN);

    assert_eq!(y, 48 + 12);
}

#[test]
fn below_the_icon_on_a_short_screen_it_stays_inside_the_bottom_edge() {
    let short = Rect {
        height: 560,
        ..SCREEN
    };

    let (_, y) = flyout_position(icon(1000, 0), FLYOUT, short);

    assert_eq!(y, 560 - 500 - 12);
}

#[test]
fn above_a_bottom_taskbar_on_a_screen_shorter_than_the_flyout_it_keeps_the_top_edge() {
    let short = Rect {
        height: 300,
        ..SCREEN
    };

    let (_, y) = flyout_position(icon(1000, 252), FLYOUT, short);

    assert_eq!(y, 12);
}

#[test]
fn on_a_screen_shorter_than_the_flyout_it_keeps_the_top_edge() {
    let short = Rect {
        y: -900,
        height: 300,
        ..SCREEN
    };

    let (_, y) = flyout_position(icon(1000, -900), FLYOUT, short);

    assert_eq!(y, -900 + 12);
}

/// The screen above the taskbar: 2560 × 1600 with a 48 px taskbar.
const WORK_AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 2560,
    height: 1552,
};

#[test]
fn the_popup_sits_above_the_taskbar_clock() {
    let (x, y) = popup_position(WORK_AREA, (300, 90));

    assert_eq!(x, 2560 - 300 - 12);
    assert_eq!(y, 1552 - 90 - 12);
}

#[test]
fn the_popup_follows_a_work_area_that_does_not_start_at_zero() {
    let work_area = Rect {
        x: -1920,
        y: 100,
        ..WORK_AREA
    };

    let (x, y) = popup_position(work_area, (300, 90));

    assert_eq!(x, -1920 + 2560 - 300 - 12);
    assert_eq!(y, 100 + 1552 - 90 - 12);
}

#[test]
/// Documents why the flyout gets the work area, not the whole screen.
fn it_stays_off_a_taskbar_on_the_left() {
    let work_area = Rect {
        x: 96,
        width: 2560 - 96,
        ..SCREEN
    };

    let (x, _) = flyout_position(icon(28, 800), FLYOUT, work_area);

    assert_eq!(x, 96 + 12);
}
