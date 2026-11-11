//! Showing, placing and hiding the settings window and the tray flyout.

use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_core::View;
use blade_lite::{Rect, flyout_position, report};
use platform::{hide_caption_buttons, round_corners, set_mica};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::platform::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};

use super::{Parts, Shell};
use crate::tray::px;

/// A tray click this soon after the flyout closed is the click that closed
/// it, so it must not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// How often to look for the handle of a window that was just shown, and
/// how many times.
const SHOW_DELAY: Duration = Duration::from_millis(16);
const SHOW_TRIES: u32 = 60;

impl<P: Parts> Shell<P> {
    /// Closing the window hides it, focusing it puts the brightness slider
    /// back, and the flyout hides when it loses focus.
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
            if matches!(event, winit::event::WindowEvent::Focused(false))
                && let Some(shell) = shell.upgrade()
            {
                shell.hide_flyout();
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
            self.put_back_brightness();
        }
    }

    /// The window shown or focused shows the laptop's brightness, not a
    /// Narrator set-value or a drag it lost while away.
    fn put_back_brightness(&self) {
        drop_lost_drag(self.window.window());
        self.window.invoke_show_brightness();
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
        self.put_back_brightness();
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
                shell.place_flyout(icon);
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

    fn place_flyout(&self, icon: Rect) {
        let size = self.flyout.window().size();
        let flyout = (px(size.width), px(size.height));
        let screen = self
            .flyout
            .window()
            .with_winit_window(|window| {
                let monitor = window.current_monitor()?;
                let (position, size) = (monitor.position(), monitor.size());
                Some(Rect {
                    x: position.x,
                    y: position.y,
                    width: px(size.width),
                    height: px(size.height),
                })
            })
            .flatten();
        let Some(screen) = screen else {
            return;
        };
        let (x, y) = flyout_position(icon, flyout, screen);
        self.flyout
            .window()
            .set_position(PhysicalPosition::new(x, y));
    }
}

/// Ends a drag the window lost (hidden, minimised or left by Alt+Tab before
/// the mouse-up), so the next mouse move can't drag a slider of the shown
/// window on. The Slider still
/// thinks its handle is pressed, so a later mouse-up over it calls
/// `released` with the value `show-brightness` set: the laptop's own, which
/// `KeyboardView::move_brightness` skips, so nothing is sent.
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
