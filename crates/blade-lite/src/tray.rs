//! The tray icon: a click opens the flyout, the menu opens the window or
//! quits.

use razer_proto::Rgb;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use blade_lite::Rect;

/// The icon's size in pixels. Windows scales it for the tray.
const SIZE: u16 = 32;

/// What the user did with the tray icon.
#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    /// A left click on the icon, which sits at this rectangle.
    Clicked(Rect),
    OpenWindow,
    Quit,
}

/// The icon, kept alive for as long as it should show.
pub struct Tray {
    _icon: TrayIcon,
    open: MenuId,
    quit: MenuId,
}

impl Tray {
    /// Shows the icon, drawn in the accent colour.
    ///
    /// # Errors
    ///
    /// If Windows refuses the icon.
    pub fn new(accent: Rgb) -> Result<Self, Box<dyn std::error::Error>> {
        let open = MenuItem::new("Open window", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append(&open)?;
        menu.append(&quit)?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("Blade Lite")
            .with_icon(Icon::from_rgba(
                icon_pixels(accent),
                SIZE.into(),
                SIZE.into(),
            )?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self {
            _icon: icon,
            open: open.id().clone(),
            quit: quit.id().clone(),
        })
    }

    /// Calls `handle` on the event-loop thread for each thing the user does.
    pub fn on_action(&self, handle: fn(TrayAction)) {
        TrayIconEvent::set_event_handler(Some(move |event| {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let icon = Rect {
                    x: round(rect.position.x),
                    y: round(rect.position.y),
                    width: px(rect.size.width),
                    height: px(rect.size.height),
                };
                post(handle, TrayAction::Clicked(icon));
            }
        }));
        let (open, quit) = (self.open.clone(), self.quit.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == open {
                post(handle, TrayAction::OpenWindow);
            } else if event.id == quit {
                post(handle, TrayAction::Quit);
            }
        }));
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

/// A rounded square in the accent colour with a white bolt, as in the mockup.
fn icon_pixels(accent: Rgb) -> Vec<u8> {
    let mut pixels = Vec::new();
    for row in 0..SIZE {
        for column in 0..SIZE {
            // Centre of the pixel, in the mockup's 20-unit icon space.
            let x = (f32::from(column) + 0.5) * 20.0 / f32::from(SIZE);
            let y = (f32::from(row) + 0.5) * 20.0 / f32::from(SIZE);
            let pixel = if in_bolt(x, y) {
                [0xFF, 0xFF, 0xFF, 0xFF]
            } else if in_rounded_square(x, y) {
                [accent.red, accent.green, accent.blue, 0xFF]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend(pixel);
        }
    }
    pixels
}

/// The square from 1 to 19 with corners of radius 5.
fn in_rounded_square(x: f32, y: f32) -> bool {
    let (low, high, radius) = (1.0, 19.0, 5.0);
    let cx = x.clamp(low + radius, high - radius);
    let cy = y.clamp(low + radius, high - radius);
    (low..=high).contains(&x)
        && (low..=high).contains(&y)
        && (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

/// The bolt from the mockup's icon, by the even-odd rule.
fn in_bolt(x: f32, y: f32) -> bool {
    const BOLT: [(f32, f32); 6] = [
        (11.0, 2.5),
        (4.5, 11.0),
        (10.0, 11.0),
        (9.0, 17.5),
        (15.5, 9.0),
        (10.0, 9.0),
    ];
    let mut inside = false;
    for (i, &(x1, y1)) in BOLT.iter().enumerate() {
        let (x2, y2) = BOLT[(i + 1) % BOLT.len()];
        if (y1 > y) != (y2 > y) && x < x1 + (y - y1) * (x2 - x1) / (y2 - y1) {
            inside = !inside;
        }
    }
    inside
}
