//! Where the tray flyout and the power mode popup open, in physical pixels.

/// The gap between the flyout and the taskbar or the screen's edge.
const GAP: i32 = 12;

/// A rectangle on screen, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The flyout's top-left corner: centred over the tray icon and above it,
/// or below it when the taskbar is at the top, and inside the screen. On a
/// screen narrower than the flyout, its left edge stays on the screen.
#[must_use]
pub fn flyout_position(icon: Rect, (width, height): (i32, i32), screen: Rect) -> (i32, i32) {
    let centred = icon.x + icon.width / 2 - width / 2;
    // Not `clamp`: it panics when the screen is narrower than the flyout.
    let x = centred
        .min(screen.x + screen.width - width - GAP)
        .max(screen.x + GAP);
    let above = icon.y - height - GAP;
    let y = if above >= screen.y {
        above
    } else {
        icon.y + icon.height + GAP
    };
    (x, y)
}

/// The popup's top-left corner: in the bottom-right corner of the work area
/// (the screen less the taskbar), so above the taskbar clock.
#[must_use]
pub fn popup_position(work_area: Rect, (width, height): (i32, i32)) -> (i32, i32) {
    (
        work_area.x + work_area.width - width - GAP,
        work_area.y + work_area.height - height - GAP,
    )
}
