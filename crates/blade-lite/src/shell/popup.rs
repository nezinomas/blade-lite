//! The power mode popup above the taskbar clock.

use std::rc::Rc;
use std::time::Duration;

use blade_lite::{Rect, popup_position, report};
use platform::work_area;
use slint::winit_030::{WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};
use winit::platform::windows::WindowExtWindows;

use super::{Parts, Shell};
use crate::tray::px;

/// How long the power mode popup stays before it fades, and how long the
/// fade takes (a little longer than its animation in the markup).
const POPUP_HOLD: Duration = Duration::from_millis(1500);
const POPUP_FADE: Duration = Duration::from_millis(300);

impl<P: Parts> Shell<P> {
    /// Shows the popup with `mode` above the taskbar clock, without taking
    /// the focus, then fades it out. A new one starts the time again.
    pub(super) fn pop_up(self: &Rc<Self>, mode: &str) {
        self.popup.set_mode(mode.into());
        let count = self.popups.get().wrapping_add(1);
        self.popups.set(count);
        // Placed before the show. The first time the size is not known yet,
        // so setup places it again.
        self.place_popup();
        // Every window shows without taking the focus (see main), so this
        // show leaves it where the user is typing.
        if let Err(error) = self.popup.show() {
            report(&format!("cannot show the power mode popup: {error}"));
            return;
        }
        if self.popup_ready.get() {
            self.popup.set_shown(true);
        } else {
            self.once_shown(
                |shell| shell.popup.window(),
                |shell, _| shell.set_up_popup(),
            );
        }
        self.after(POPUP_HOLD, count, move |shell| {
            shell.popup.set_shown(false);
            shell.after(POPUP_FADE, count, |shell| {
                let _ = shell.popup.hide();
            });
        });
    }

    /// Once the popup has a handle: no clicks, no taskbar button, placed
    /// again at its real size, then faded in. If it would take clicks it is
    /// hidden, and done again on the next popup.
    fn set_up_popup(&self) {
        let hittest = self.popup.window().with_winit_window(|window| {
            window.set_skip_taskbar(true);
            window.set_cursor_hittest(false)
        });
        // A popup that takes clicks could take the focus, so it stays
        // hidden, and setup runs again on the next one.
        match hittest {
            Some(Ok(())) => self.popup_ready.set(true),
            Some(Err(error)) => {
                report(&format!("the popup would take clicks: {error}"));
                let _ = self.popup.hide();
                return;
            }
            None => {
                report("the popup has no window");
                let _ = self.popup.hide();
                return;
            }
        }
        self.place_popup();
        self.popup.set_shown(true);
    }

    /// Runs `then` after `wait`, unless another popup came since `count`.
    fn after(self: &Rc<Self>, wait: Duration, count: u32, then: impl FnOnce(&Rc<Self>) + 'static) {
        let shell = Rc::downgrade(self);
        Timer::single_shot(wait, move || {
            if let Some(shell) = shell.upgrade()
                && shell.popups.get() == count
            {
                then(&shell);
            }
        });
    }

    fn place_popup(&self) {
        let area = match work_area() {
            Ok((x, y, width, height)) => Rect {
                x,
                y,
                width,
                height,
            },
            Err(error) => {
                report(&format!("the popup can't find the taskbar: {error}"));
                return;
            }
        };
        let size = self.popup.window().size();
        let (x, y) = popup_position(area, (px(size.width), px(size.height)));
        self.popup
            .window()
            .set_position(PhysicalPosition::new(x, y));
    }
}
