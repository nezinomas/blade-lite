//! The popup above the taskbar clock: a new power mode, the touchpad on or
//! off, or a CPU or GPU that stays hot.

use std::rc::Rc;
use std::time::{Duration, Instant};

use blade_lite::{Rect, popup_position, report};
use platform::{raise_topmost, work_area};
use slint::winit_030::{WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, Timer};
use ui::{HEAT_HOLD, Sensor};
use winit::platform::windows::WindowExtWindows;

use super::windows::hwnd;
use super::{Parts, Shell};
use crate::tray::px;

/// How long the popup stays before it fades, and how long the
/// fade takes (a little longer than its animation in the markup).
pub(super) const POPUP_HOLD: Duration = Duration::from_millis(1500);
/// The heat warning stays longer: it shows once per hot spell.
const HEAT_POPUP_HOLD: Duration = Duration::from_secs(10);
const POPUP_FADE: Duration = Duration::from_millis(300);

/// The popup's captions.
pub(super) const POWER_MODE: &str = "Power mode";
pub(super) const TOUCHPAD: &str = "Touchpad";
pub(super) const TOO_HOT: &str = "Too hot";
/// The fan guard's popup caption.
pub(super) const FANS: &str = "Fans";

impl<P: Parts> Shell<P> {
    /// Notes the CPU and GPU temperatures read at `now`, `None` for one not
    /// read, and shows and logs the warnings of the sensors that have stayed
    /// hot, in one popup. Feeds the fan guard too, which may send a fan
    /// speed, only while `heat_reads_allowed`.
    pub fn watch_heat(self: &Rc<Self>, cpu: Option<u32>, gpu: Option<u32>, now: Instant) {
        self.guard_fans(cpu, gpu, now);
        let mut watch = self.heat_watch.get();
        let warnings: Vec<String> = [(Sensor::Cpu, cpu), (Sensor::Gpu, gpu)]
            .into_iter()
            .filter_map(|(sensor, celsius)| watch.note(sensor, celsius, now))
            .collect();
        self.heat_watch.set(watch);
        if warnings.is_empty() {
            return;
        }
        let text = warnings.join(" · ");
        report(&format!(
            "too hot for {} min: {text}",
            HEAT_HOLD.as_secs() / 60
        ));
        self.pop_up(TOO_HOT, &text, HEAT_POPUP_HOLD);
    }

    /// Feeds the fan guard (ticket 129). When the app's fan boost differs
    /// from the guard and Custom with a manual fan runs, the fans go to the
    /// model's top speed, or back to the saved speed; that is logged and
    /// shown in the popup. Nothing is sent while Razer software runs or a
    /// resume waits: the change waits for a later reading. A failed send
    /// shows its error.
    fn guard_fans(self: &Rc<Self>, cpu: Option<u32>, gpu: Option<u32>, now: Instant) {
        let mut guard = self.fan_guard.get();
        guard.note(cpu, gpu, now);
        self.fan_guard.set(guard);
        if !self.heat_reads_allowed() {
            return;
        }
        let sent = self.app.borrow_mut().set_fan_boost(guard.on());
        match sent {
            Ok(false) => {}
            Ok(true) => {
                let text = if guard.on() {
                    "Full speed"
                } else {
                    "Back to the set speed"
                };
                report(&format!(
                    "fan guard: {text} (CPU {}, GPU {})",
                    shown(cpu),
                    shown(gpu)
                ));
                self.pop_up(FANS, text, HEAT_POPUP_HOLD);
            }
            Err(error) => self.act(|_| Err(error)),
        }
        self.say_gpu_fault(guard.held_by_gpu_fault(), cpu, gpu);
    }

    /// Logs once when the guard starts to hold a manual fan at full speed
    /// only for failed GPU reads, and once when that ends (ticket 163):
    /// else only a throttled read failure says why the fans stay loud.
    fn say_gpu_fault(&self, held: bool, cpu: Option<u32>, gpu: Option<u32>) {
        let app = self.app.borrow();
        // Not while its full speed waits for a resend: the fans may still
        // run at the saved speed.
        let held = held && app.manual_fan_runs() && !app.boost_unsent();
        drop(app);
        if held == self.gpu_fault_said.replace(held) {
            return;
        }
        if held {
            report(&format!(
                "fan guard: Full speed held only by failed GPU reads (CPU {})",
                shown(cpu)
            ));
        } else {
            report(&format!(
                "fan guard: failed GPU reads no longer hold a manual fan at full speed (CPU {}, GPU {})",
                shown(cpu),
                shown(gpu)
            ));
        }
    }

    /// Shows the popup with `caption` over `mode` above the taskbar clock,
    /// without taking the focus, for `hold`, then fades it out. A new one
    /// starts the time again.
    pub(super) fn pop_up(self: &Rc<Self>, caption: &str, mode: &str, hold: Duration) {
        self.popup.set_caption(caption.into());
        self.popup.set_mode(mode.into());
        let count = self.popups.get().wrapping_add(1);
        self.popups.set(count);
        // Placed before the show. The first time the size is not known yet,
        // so setup places it again.
        self.place_popup();
        // Every window shows without taking the focus (see main), so this
        // show leaves it where the user is typing.
        if let Err(error) = self.popup.show() {
            report(&format!("cannot show the {caption} popup: {error}"));
            return;
        }
        if self.popup_ready.get() {
            self.fade_in_on_top();
        } else {
            self.once_shown(
                |shell| shell.popup.window(),
                |shell, _| shell.set_up_popup(),
            );
        }
        self.after(hold, count, move |shell| {
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
        self.fade_in_on_top();
    }

    /// Fades the popup in above every other topmost window, such as a
    /// full-screen game (ticket 155). If it can't go there it still shows,
    /// and the log says why.
    fn fade_in_on_top(&self) {
        let raised = hwnd(self.popup.window())
            .ok_or_else(|| std::io::Error::other("no window"))
            .and_then(raise_topmost);
        if let Err(error) = raised {
            report(&format!("the popup can't go on top: {error}"));
        }
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

/// A reading for the log: `61 °C`, or `unread`.
fn shown(celsius: Option<u32>) -> String {
    celsius.map_or("unread".to_owned(), |c| format!("{c} °C"))
}
