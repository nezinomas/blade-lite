//! The tray icon: a click opens the flyout, the menu opens the window,
//! turns autostart on or off, or quits.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};

use razer_proto::Rgb;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use blade_lite::{AutostartItem, Rect, icon};

/// The icon's size in pixels. Windows scales it for the tray.
const SIZE: u16 = 32;

/// What the user did with the tray icon.
#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    /// A left click on the icon, which sits at this rectangle.
    Clicked(Rect),
    /// A right click on the icon, just before the menu opens.
    MenuOpening,
    OpenWindow,
    /// A click on "Start with Windows", which has already flipped its tick.
    Autostart,
    Quit,
}

/// The icon, kept alive for as long as it should show.
pub struct Tray {
    _icon: TrayIcon,
    open: MenuId,
    autostart: CheckMenuItem,
    quit: MenuId,
}

impl Tray {
    /// Shows the icon, drawn in the accent colour, with "Start with Windows"
    /// shown as `autostart` says.
    ///
    /// # Errors
    ///
    /// If Windows refuses the icon.
    pub fn new(accent: Rgb, autostart: &AutostartItem) -> Result<Self, Box<dyn std::error::Error>> {
        let open = MenuItem::new("Open window", true, None);
        let autostart = CheckMenuItem::new(autostart.label, true, autostart.ticked, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append(&open)?;
        menu.append(&autostart)?;
        menu.append(&quit)?;
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Blade Lite")
            .with_icon(Icon::from_rgba(
                icon::pixels([accent.red, accent.green, accent.blue], SIZE),
                SIZE.into(),
                SIZE.into(),
            )?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self {
            _icon: tray_icon,
            open: open.id().clone(),
            autostart,
            quit: quit.id().clone(),
        })
    }

    /// Calls `handle` on the event-loop thread for each thing the user does.
    pub fn on_action(&self, handle: fn(TrayAction)) {
        let after_double = AtomicBool::new(false);
        TrayIconEvent::set_event_handler(Some(move |event| {
            match action_for(&event, &after_double) {
                // tray-icon opens the menu right after this call returns,
                // on this same thread (the event loop's). So the item is
                // read again now, not posted, or the menu could show first.
                Some(action @ TrayAction::MenuOpening) => handle_now(handle, action),
                Some(action) => post(handle, action),
                None => {}
            }
        }));
        let (open, quit) = (self.open.clone(), self.quit.clone());
        let autostart = self.autostart.id().clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == open {
                post(handle, TrayAction::OpenWindow);
            } else if event.id == autostart {
                // Handled now, on this thread (the event loop's), so no
                // menu refresh can change the tick before it is read.
                handle_now(handle, TrayAction::Autostart);
            } else if event.id == quit {
                post(handle, TrayAction::Quit);
            }
        }));
    }

    /// Whether "Start with Windows" is ticked.
    pub fn autostart_ticked(&self) -> bool {
        self.autostart.is_checked()
    }

    /// Shows "Start with Windows" as `item` says.
    pub fn show_autostart(&self, item: &AutostartItem) {
        self.autostart.set_checked(item.ticked);
        self.autostart.set_text(item.label);
    }
}

/// What a tray icon event asks for, if anything. Windows sends a left
/// double click as down, up, double click, up: the first up opens the
/// flyout, the double click opens the window (ticket 176), and its own up
/// is dropped, or it would open the flyout again. `after_double` keeps that
/// across events; a new press clears it, so a lost up eats no later click.
fn action_for(event: &TrayIconEvent, after_double: &AtomicBool) -> Option<TrayAction> {
    match event {
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state,
            ..
        } => match button_state {
            MouseButtonState::Down => {
                after_double.store(false, Ordering::Relaxed);
                None
            }
            MouseButtonState::Up if after_double.swap(false, Ordering::Relaxed) => None,
            MouseButtonState::Up => Some(TrayAction::Clicked(Rect {
                x: round(rect.position.x),
                y: round(rect.position.y),
                width: px(rect.size.width),
                height: px(rect.size.height),
            })),
        },
        TrayIconEvent::Click {
            button: MouseButton::Right,
            button_state: MouseButtonState::Up,
            ..
        } => Some(TrayAction::MenuOpening),
        TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            after_double.store(true, Ordering::Relaxed);
            Some(TrayAction::OpenWindow)
        }
        _ => None,
    }
}

/// A screen length in pixels as a coordinate.
pub fn px(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A screen coordinate, to the nearest pixel.
#[allow(clippy::cast_possible_truncation)] // Screen coordinates fit an i32.
fn round(value: f64) -> i32 {
    value.round() as i32
}

/// Runs `handle` on the event-loop thread, where the windows live.
/// Runs `action` now, inside tray-icon's own window procedure. A panic
/// can't unwind out of that (`extern "system"`: Rust aborts), so it is
/// caught and raised again on the event loop, which then ends the run with
/// the quit preset (`Shell::serve`).
fn handle_now(handle: fn(TrayAction), action: TrayAction) {
    if let Err(panic) = std::panic::catch_unwind(AssertUnwindSafe(move || handle(action))) {
        // The loop is gone only while the app quits.
        let _ = slint::invoke_from_event_loop(move || std::panic::resume_unwind(panic));
    }
}

fn post(handle: fn(TrayAction), action: TrayAction) {
    // The loop is gone only while the app quits, so the action can go.
    let _ = slint::invoke_from_event_loop(move || handle(action));
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use tray_icon::dpi::{PhysicalPosition, PhysicalSize};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent, TrayIconId};

    use super::{TrayAction, action_for};

    fn rect() -> tray_icon::Rect {
        tray_icon::Rect {
            size: PhysicalSize::new(24, 24),
            position: PhysicalPosition::new(100.4, 200.6),
        }
    }

    fn click(button: MouseButton, button_state: MouseButtonState) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
            button,
            button_state,
        }
    }

    fn double(button: MouseButton) -> TrayIconEvent {
        TrayIconEvent::DoubleClick {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
            button,
        }
    }

    /// The actions for `events`, in Windows' order, from a fresh start.
    fn actions(events: Vec<TrayIconEvent>) -> Vec<Option<TrayAction>> {
        let after_double = AtomicBool::new(false);
        events
            .into_iter()
            .map(|event| action_for(&event, &after_double))
            .collect()
    }

    const DOWN: MouseButtonState = MouseButtonState::Down;
    const UP: MouseButtonState = MouseButtonState::Up;
    const LEFT: MouseButton = MouseButton::Left;

    #[test]
    fn a_left_click_opens_the_flyout_at_the_icon() {
        let got = actions(vec![click(LEFT, DOWN), click(LEFT, UP)]);

        assert!(got[0].is_none());
        let Some(TrayAction::Clicked(icon)) = got[1] else {
            panic!("got {got:?}");
        };
        assert_eq!(
            (icon.x, icon.y, icon.width, icon.height),
            (100, 201, 24, 24)
        );
    }

    #[test]
    fn a_left_double_click_opens_the_window_and_its_second_release_does_nothing() {
        // Windows sends down, up, double click, up.
        let got = actions(vec![
            click(LEFT, DOWN),
            click(LEFT, UP),
            double(LEFT),
            click(LEFT, UP),
        ]);

        assert!(matches!(got[1], Some(TrayAction::Clicked(_))));
        assert!(matches!(got[2], Some(TrayAction::OpenWindow)), "{got:?}");
        assert!(got[3].is_none(), "{got:?}");
    }

    #[test]
    fn a_click_after_a_double_click_opens_the_flyout_again() {
        let got = actions(vec![
            click(LEFT, DOWN),
            click(LEFT, UP),
            double(LEFT),
            click(LEFT, UP),
            click(LEFT, DOWN),
            click(LEFT, UP),
        ]);

        assert!(matches!(got[5], Some(TrayAction::Clicked(_))), "{got:?}");
    }

    #[test]
    fn a_double_click_with_its_release_lost_does_not_eat_the_next_click() {
        // The release went elsewhere (the pointer left the icon).
        let got = actions(vec![double(LEFT), click(LEFT, DOWN), click(LEFT, UP)]);

        assert!(matches!(got[2], Some(TrayAction::Clicked(_))), "{got:?}");
    }

    #[test]
    fn a_right_or_middle_double_click_does_nothing() {
        let got = actions(vec![
            double(MouseButton::Right),
            double(MouseButton::Middle),
        ]);

        assert!(got.iter().all(Option::is_none), "{got:?}");
    }

    #[test]
    fn a_right_release_reads_the_menu_before_it_opens() {
        let got = actions(vec![
            click(MouseButton::Right, DOWN),
            click(MouseButton::Right, UP),
        ]);

        assert!(got[0].is_none());
        assert!(matches!(got[1], Some(TrayAction::MenuOpening)), "{got:?}");
    }
}
