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
/// screen narrower or shorter than the flyout, its left or top edge stays on
/// the screen. The sums saturate, so a broken monitor size can't overflow
/// them.
#[must_use]
pub fn flyout_position(icon: Rect, (width, height): (i32, i32), screen: Rect) -> (i32, i32) {
    let centred = icon
        .x
        .saturating_add(icon.width / 2)
        .saturating_sub(width / 2);
    let above = icon.y.saturating_sub(height).saturating_sub(GAP);
    let y = if above >= screen.y {
        above
    } else {
        icon.y.saturating_add(icon.height).saturating_add(GAP)
    };
    (
        inside(centred, screen.x, screen.width, width),
        inside(y, screen.y, screen.height, height),
    )
}

/// Moves `at` so a window of `size` there ends a gap before the far edge of
/// the span, but starts no less than a gap after its start: the start wins
/// when the span is too short for both.
fn inside(at: i32, start: i32, span: i32, size: i32) -> i32 {
    // Not `clamp`: it panics when the span is shorter than the window.
    at.min(far_edge(start, span, size))
        .max(start.saturating_add(GAP))
}

/// The popup's top-left corner: in the bottom-right corner of the work area
/// (the screen less the taskbar), so above the taskbar clock.
#[must_use]
pub fn popup_position(work_area: Rect, (width, height): (i32, i32)) -> (i32, i32) {
    (
        far_edge(work_area.x, work_area.width, width),
        far_edge(work_area.y, work_area.height, height),
    )
}

/// Where a window of `size` starts so it ends a gap before the far edge of
/// the span from `start`, `span` long.
fn far_edge(start: i32, span: i32, size: i32) -> i32 {
    start
        .saturating_add(span)
        .saturating_sub(size)
        .saturating_sub(GAP)
}
