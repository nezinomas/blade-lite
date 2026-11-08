//! What each tray menu item does.

use std::cell::RefCell;

use blade_core::{autostart, set_autostart};
use blade_lite::{AutostartItem, ShownErrors, autostart_item, report, takeover_note};
use platform::WindowsRegistry;

use crate::tray::{Tray, TrayAction};
use crate::{SHELL, TRAY};

thread_local! {
    /// The last autostart warning, so each one is logged once, not on each
    /// menu open. No warning clears it, so one that comes back is logged.
    static AUTOSTART_SAID: RefCell<ShownErrors> = RefCell::new(ShownErrors::default());
}

/// How "Start with Windows" should show, read from the Run key now. Not
/// ticked if it can't be read. A warning is logged once, not on each read.
pub fn autostart_shown() -> AutostartItem {
    let read = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| autostart(&WindowsRegistry, &exe).map_err(|error| error.to_string()));
    let item = autostart_item(&read);
    AUTOSTART_SAID.with(|said| {
        if let Some(text) = said
            .borrow_mut()
            .new_text(item.warning.as_deref().unwrap_or_default())
        {
            report(text);
        }
    });
    item
}

/// Turns autostart on or off as the tray tick now says. Either way the
/// item then shows what the registry holds.
fn sync_autostart(tray: &Tray) {
    let on = tray.autostart_ticked();
    let set = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| {
            let before = autostart(&WindowsRegistry, &exe).map_err(|error| error.to_string());
            if let Some(note) = takeover_note(&before, on) {
                report(&note);
            }
            set_autostart(on, &mut WindowsRegistry, &exe).map_err(|error| error.to_string())
        });
    if let Err(error) = set {
        report(&format!("could not set autostart: {error}"));
    }
    tray.show_autostart(&autostart_shown());
}

pub fn on_tray(action: TrayAction) {
    match action {
        TrayAction::Quit => {
            let _ = slint::quit_event_loop();
            return;
        }
        TrayAction::Autostart => {
            if let Some(tray) = TRAY.with(|cell| cell.borrow().clone()) {
                sync_autostart(&tray);
            }
            return;
        }
        TrayAction::MenuOpening => {
            if let Some(tray) = TRAY.with(|cell| cell.borrow().clone()) {
                tray.show_autostart(&autostart_shown());
            }
            return;
        }
        _ => {}
    }
    let Some(shell) = SHELL.with(|cell| cell.borrow().clone()) else {
        return;
    };
    match action {
        TrayAction::Clicked(icon) => shell.toggle_flyout(icon),
        TrayAction::OpenWindow => shell.show_window(),
        // Handled above; listed so the match stays whole.
        TrayAction::Quit | TrayAction::Autostart | TrayAction::MenuOpening => {}
    }
}
