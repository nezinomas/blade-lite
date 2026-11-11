//! The tray icon: a click opens the flyout, the menu opens the window,
//! turns autostart on or off, or quits.

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
        TrayIconEvent::set_event_handler(Some(move |event| {
            let TrayIconEvent::Click {
                rect,
                button,
                button_state,
                ..
            } = event
            else {
                return;
            };
            match (button, button_state) {
                (MouseButton::Left, MouseButtonState::Up) => {
                    let icon = Rect {
                        x: round(rect.position.x),
                        y: round(rect.position.y),
                        width: px(rect.size.width),
                        height: px(rect.size.height),
                    };
                    post(handle, TrayAction::Clicked(icon));
                }
                // tray-icon opens the menu right after this call returns,
                // on this same thread (the event loop's). So the item is
                // read again now, not posted, or the menu could show first.
                (MouseButton::Right, MouseButtonState::Up) => {
                    handle(TrayAction::MenuOpening);
                }
                _ => {}
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
                handle(TrayAction::Autostart);
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
fn post(handle: fn(TrayAction), action: TrayAction) {
    // The loop is gone only while the app quits, so the action can go.
    let _ = slint::invoke_from_event_loop(move || handle(action));
}
