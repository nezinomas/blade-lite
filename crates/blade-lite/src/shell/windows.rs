//! Showing, placing and hiding the settings window and the tray flyout.

use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_core::View;
use blade_lite::{Rect, flyout_position, report};
use platform::{hide_caption_buttons, round_corners, set_mica, work_area, work_area_at};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::platform::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};

use super::{Parts, Shell};
use crate::bind;
use crate::tray::px;

/// A tray click this soon after the flyout closed is the click that closed
/// it, so it must not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// How often to look for the handle of a window that was just shown, and
/// how many times.
const SHOW_DELAY: Duration = Duration::from_millis(16);
const SHOW_TRIES: u32 = 60;

impl<P: Parts> Shell<P> {
    /// Closing the window hides it, focusing it puts the sliders back, and the
    /// flyout hides when it loses focus.
    pub fn watch_windows(self: &Rc<Self>) {
        let shell = Rc::downgrade(self);
        self.window.window().on_close_requested(move || {
            if let Some(shell) = shell.upgrade() {
                shell.schedule.borrow_mut().hide(View::Window);
            }
            slint::CloseRequestResponse::HideWindow
        });
        let shell = Rc::downgrade(self);
        self.flyout.window().on_winit_window_event(move |_, event| {
            if let Some(shell) = shell.upgrade() {
                shell.on_flyout_event(event);
            }
            EventResult::Propagate
        });
        let shell = Rc::downgrade(self);
        self.window.window().on_winit_window_event(move |_, event| {
            if let Some(shell) = shell.upgrade() {
                shell.on_window_event(event);
            }
            EventResult::Propagate
        });
    }

    /// The window brought back (restored, Alt+Tab) gets the focus.
    pub fn on_window_event(&self, event: &winit::event::WindowEvent) {
        if matches!(event, winit::event::WindowEvent::Focused(true)) {
            self.put_back_sliders();
        }
    }

    /// The flyout hides when it loses focus, and is placed again when its
    /// DPI scale changes.
    fn on_flyout_event(self: &Rc<Self>, event: &winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::Focused(false) => self.hide_flyout(),
            winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                self.flyout_rescaled();
            }
            _ => {}
        }
    }

    /// Placing the flyout on the tray icon's monitor can give it that
    /// monitor's DPI scale, and Windows then resizes it, so it can end up
    /// over the taskbar or off centre. Placed once more, with the new size,
    /// after winit has put it at the rect Windows suggests (which can come
    /// before or after this event). Once per open, so a flyout too big for that monitor can't
    /// keep moving between two of them. True when it will be placed.
    pub fn flyout_rescaled(self: &Rc<Self>) -> bool {
        if !self.flyout_open.get() {
            return false;
        }
        let Some(icon) = self.flyout_icon.take() else {
            return false;
        };
        let shell = Rc::downgrade(self);
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(shell) = shell.upgrade()
                && shell.flyout_open.get()
            {
                shell.place_flyout(icon);
            }
        });
        true
    }

    /// The window shown or focused shows the laptop's brightness and Custom
    /// values, not a drag it lost while away.
    fn put_back_sliders(&self) {
        drop_lost_drag(self.window.window());
        self.window.invoke_show_brightness();
        bind::remake_sliders(&self.window, &self.app.borrow().performance_view());
    }

    pub fn show_window(self: &Rc<Self>) {
        self.hide_flyout();
        if let Err(error) = self.window.show() {
            report(&format!("cannot show the window: {error}"));
            return;
        }
        self.once_shown(
            |shell| shell.window.window(),
            |shell, hwnd| {
                put_backdrop(hwnd, false);
                // A window that is open but minimised or behind others comes
                // to the front, so a second start shows it.
                shell.window.window().with_winit_window(|window| {
                    window.set_minimized(false);
                    window.focus_window();
                });
            },
        );
        self.schedule
            .borrow_mut()
            .show(View::Window, Instant::now());
        self.refresh(None);
        self.put_back_sliders();
    }

    /// Opens the flyout above the tray icon, or closes it if it is open.
    pub fn toggle_flyout(self: &Rc<Self>, icon: Rect) {
        if self.flyout_open.get() {
            self.hide_flyout();
            return;
        }
        let just_closed = self
            .flyout_closed_at
            .get()
            .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
        if just_closed {
            return;
        }
        self.refresh(None);
        drop_lost_drag(self.flyout.window());
        self.flyout.invoke_show_brightness();
        if let Err(error) = self.flyout.show() {
            report(&format!("cannot show the flyout: {error}"));
            return;
        }
        self.flyout_open.set(true);
        self.once_shown(
            |shell| shell.flyout.window(),
            move |shell, hwnd| {
                // Hidden before it got its handle (a tray double click
                // shows the window): leave it, or it takes the focus.
                if !shell.flyout_open.get() {
                    return;
                }
                shell.flyout_placed_first(icon);
                put_backdrop(hwnd, true);
                shell
                    .flyout
                    .window()
                    .with_winit_window(winit::window::Window::focus_window);
            },
        );
        self.schedule
            .borrow_mut()
            .show(View::Flyout, Instant::now());
    }

    /// Runs `act` with the window's handle once Windows has made the window.
    /// A window that was just shown has none until the event loop turns.
    pub(super) fn once_shown(
        self: &Rc<Self>,
        window: fn(&Self) -> &slint::Window,
        act: impl FnOnce(&Self, isize) + 'static,
    ) {
        self.wait_for_handle(window, act, SHOW_TRIES);
    }

    fn wait_for_handle(
        self: &Rc<Self>,
        window: fn(&Self) -> &slint::Window,
        act: impl FnOnce(&Self, isize) + 'static,
        tries: u32,
    ) {
        if let Some(hwnd) = hwnd(window(self)) {
            act(self, hwnd);
            return;
        }
        if tries == 0 {
            report("the window got no handle");
            return;
        }
        let shell = Rc::downgrade(self);
        Timer::single_shot(SHOW_DELAY, move || {
            if let Some(shell) = shell.upgrade() {
                shell.wait_for_handle(window, act, tries - 1);
            }
        });
    }

    fn hide_flyout(&self) {
        if !self.flyout_open.replace(false) {
            return;
        }
        let _ = self.flyout.hide();
        self.flyout_closed_at.set(Some(Instant::now()));
        self.schedule.borrow_mut().hide(View::Flyout);
    }

    /// The first placing of an open flyout. Only a scale change after it
    /// (most likely caused by it) places the flyout again.
    pub fn flyout_placed_first(&self, icon: Rect) {
        // Set first: the scale change can come inside the move.
        self.flyout_icon.set(Some(icon));
        self.place_flyout(icon);
    }

    fn place_flyout(&self, icon: Rect) {
        if let Some((x, y)) = self.flyout_place(icon) {
            self.flyout
                .window()
                .set_position(PhysicalPosition::new(x, y));
        }
    }

    /// Where the flyout goes for the tray icon, at its present size.
    fn flyout_place(&self, icon: Rect) -> Option<(i32, i32)> {
        let size = self.flyout.window().size();
        let flyout = (px(size.width), px(size.height));
        // The icon's own monitor, less its taskbars: the flyout's last
        // monitor can be another one, or gone. Else the main screen's.
        let centre = (
            icon.x.saturating_add(icon.width / 2),
            icon.y.saturating_add(icon.height / 2),
        );
        let screen = match work_area_at(centre).or_else(|_| work_area()) {
            Ok((x, y, width, height)) => Rect {
                x,
                y,
                width,
                height,
            },
            Err(error) => {
                report(&format!("the flyout can't find a screen: {error}"));
                return None;
            }
        };
        Some(flyout_position(icon, flyout, screen))
    }
}

/// Ends a drag the window lost (hidden, minimised or left by Alt+Tab before
/// the mouse-up), so the next mouse move can't drag a slider of the shown
/// window on. The Slider still
/// thinks its handle is pressed, so a later mouse-up over it calls
/// `released` with the value `show-brightness` set: the laptop's own, which
/// `KeyboardView::move_brightness` skips, so nothing is sent. The Custom
/// sliders have no such value to set, so `bind::remake_sliders` makes them again.
fn drop_lost_drag(window: &slint::Window) {
    window.dispatch_event(WindowEvent::PointerExited);
}

/// The window's Win32 handle, once Windows has made it.
pub(super) fn hwnd(window: &slint::Window) -> Option<isize> {
    let handle = window.window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

/// Puts Mica behind a window, light as the mockup is. A frameless one also
/// gets round corners, and loses the min, max and close buttons that Mica
/// would show. If they can't be hidden, Mica still goes on: buttons on top
/// are better than a see-through flyout.
fn put_backdrop(hwnd: isize, frameless: bool) {
    if frameless && let Err(error) = hide_caption_buttons(hwnd) {
        report(&format!("caption buttons not hidden: {error}"));
    }
    let result = set_mica(hwnd, false).and_then(|()| {
        if frameless {
            round_corners(hwnd)
        } else {
            Ok(())
        }
    });
    if let Err(error) = result {
        report(&format!("no Mica: {error}"));
    }
}
