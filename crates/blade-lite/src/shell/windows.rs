//! Showing, placing and hiding the settings window and the tray flyout.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use blade_core::View;
use blade_lite::{Rect, centre_inside, flyout_position, report};
use platform::{hide_caption_buttons, round_corners, set_mica, work_area, work_area_at};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::platform::WindowEvent;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};

use super::{Parts, Shell};
use crate::tray::px;
use crate::{MainWindow, bind};

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
        self.flyout.window().on_winit_window_event(move |_, event| {
            if let Some(shell) = shell.upgrade() {
                shell.on_flyout_event(event);
            }
            EventResult::Propagate
        });
        self.watch_window();
    }

    /// The settings window's close and focus. Run again for each window
    /// made again.
    fn watch_window(self: &Rc<Self>) {
        let window = self.window();
        let shell = Rc::downgrade(self);
        window.window().on_close_requested(move || {
            if let Some(shell) = shell.upgrade() {
                shell.window_closed();
            }
            slint::CloseRequestResponse::HideWindow
        });
        let shell = Rc::downgrade(self);
        window.window().on_winit_window_event(move |_, event| {
            if let Some(shell) = shell.upgrade() {
                shell.on_window_event(event);
            }
            EventResult::Propagate
        });
    }

    /// The user closed the settings window. Slint hides it once this
    /// returns; on the next turn of the event loop a new window takes its
    /// place, and the old one goes with its picture (ticket 303: on Windows
    /// a hidden window keeps it, about 12 MB at 200 %). Not inside the
    /// close: Slint still uses the old window there.
    fn window_closed(self: &Rc<Self>) {
        self.schedule.borrow_mut().hide(View::Window);
        let shell = Rc::downgrade(self);
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(shell) = shell.upgrade() {
                shell.make_window_again();
            }
        });
    }

    /// Puts a new settings window, not shown, in place of the hidden one.
    /// Not while a file dialog it owns is open: the dialog's end makes it
    /// again. Nothing is sent or read: the open fills it. If it can't be
    /// made, the old one stays, with its picture (logged once).
    pub(super) fn make_window_again(self: &Rc<Self>) {
        self.steps
            .time("window made again", || self.make_window_again_now());
    }

    fn make_window_again_now(self: &Rc<Self>) {
        static FAILURE_SAID: AtomicBool = AtomicBool::new(false);
        let old = self.window();
        if old.window().is_visible() || self.dialog_open.get() {
            return;
        }
        let new = match MainWindow::new() {
            Ok(new) => new,
            Err(error) => {
                if !FAILURE_SAID.swap(true, Ordering::Relaxed) {
                    report(&format!(
                        "the closed window stays in memory, as a new one can't be made (said once): {error}"
                    ));
                }
                return;
            }
        };
        new.set_accent(old.get_accent());
        new.set_version(old.get_version());
        place_like(new.window(), old.window());
        // The display groups are filled only on a change (ticket 287), and a
        // failed read at the open leaves them as last shown.
        if let Some(view) = self.display_shown.borrow().as_ref() {
            bind::display(&new, view);
        }
        drop(old);
        self.window.replace(new);
        self.watch_window();
        bind::window_callbacks(self);
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
        let window = self.window();
        drop_lost_drag(window.window());
        window.invoke_show_brightness();
        bind::put_back_sliders(&window, &self.app.borrow().performance_view());
    }

    pub fn show_window(self: &Rc<Self>) {
        self.steps.time("window open", || self.show_window_now());
    }

    fn show_window_now(self: &Rc<Self>) {
        self.hide_flyout();
        let window = self.window();
        // A window that opens shows the Settings and About groups closed; one
        // already open (a second start) keeps the groups the user opened.
        if !window.window().is_visible() {
            window.invoke_close_groups();
        }
        // Filled before it shows: a window made again (ticket 303) has
        // nothing in it yet.
        self.refresh(None);
        if let Err(error) = self.steps.time("window show", || window.show()) {
            report(&format!("cannot show the window: {error}"));
            return;
        }
        self.once_shown(
            |shell| hwnd(shell.window().window()),
            |shell, hwnd| {
                put_backdrop(hwnd, false);
                // A window that is open but minimised or behind others comes
                // to the front, so a second start shows it.
                shell.window().window().with_winit_window(|window| {
                    window.set_minimized(false);
                    window.focus_window();
                });
            },
        );
        self.schedule
            .borrow_mut()
            .show(View::Window, Instant::now());
        self.put_back_sliders();
    }

    /// Opens the flyout above the tray icon, or closes it if it is open.
    pub fn toggle_flyout(self: &Rc<Self>, icon: Rect) {
        self.steps
            .time("flyout open", || self.toggle_flyout_now(icon));
    }

    fn toggle_flyout_now(self: &Rc<Self>, icon: Rect) {
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
            |shell| hwnd(shell.flyout.window()),
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
        handle: fn(&Self) -> Option<isize>,
        act: impl FnOnce(&Self, isize) + 'static,
    ) {
        self.wait_for_handle(handle, act, SHOW_TRIES);
    }

    fn wait_for_handle(
        self: &Rc<Self>,
        handle: fn(&Self) -> Option<isize>,
        act: impl FnOnce(&Self, isize) + 'static,
        tries: u32,
    ) {
        if let Some(hwnd) = handle(self) {
            self.steps.time("window backdrop", || act(self, hwnd));
            return;
        }
        if tries == 0 {
            report("the window got no handle");
            return;
        }
        let shell = Rc::downgrade(self);
        Timer::single_shot(SHOW_DELAY, move || {
            if let Some(shell) = shell.upgrade() {
                shell.wait_for_handle(handle, act, tries - 1);
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

/// Gives the `new` window the `old` one's place, as the old one kept it when
/// hidden: its spot, if its middle is still on a screen, and its size, kept
/// in logical pixels so a scale change since gives the same look. A
/// minimised window's place is off screen, so it gives none; a maximised one
/// opens maximised on the same screen (its size before that is lost).
fn place_like(new: &slint::Window, old: &slint::Window) {
    if old.is_minimized() {
        return;
    }
    let position = old.position();
    let size = old.size();
    let rect = Rect {
        x: position.x,
        y: position.y,
        width: px(size.width),
        height: px(size.height),
    };
    let centre = (
        rect.x.saturating_add(rect.width / 2),
        rect.y.saturating_add(rect.height / 2),
    );
    let on_screen = work_area_at(centre).is_ok_and(|(x, y, width, height)| {
        centre_inside(
            rect,
            Rect {
                x,
                y,
                width,
                height,
            },
        )
    });
    if on_screen {
        new.set_position(position);
    }
    if old.is_maximized() {
        new.set_maximized(true);
    } else {
        new.set_size(size.to_logical(old.scale_factor()));
    }
}

/// Ends a drag the window lost (hidden, minimised or left by Alt+Tab before
/// the mouse-up), so the next mouse move can't drag a slider of the shown
/// window on. The Slider still
/// thinks its handle is pressed, so a later mouse-up over it calls
/// `released` with the value `show-brightness` set: the laptop's own, which
/// `KeyboardView::move_brightness` skips, so nothing is sent. The Custom
/// sliders are put back the same way by `bind::put_back_sliders`.
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
