//! The tray's "Start with Windows" item, from what the Run key starts.

use blade_core::Autostart;

/// The item's text.
pub const AUTOSTART_LABEL: &str = "Start with Windows";

/// The item's text while the app's Run value starts another command, such
/// as another copy of the app. A click points it at this copy.
pub const AUTOSTART_OTHER_LABEL: &str = "Start with Windows (set to another program)";

/// The item's text while the Run key can't be read, so the tick is not
/// known.
pub const AUTOSTART_UNKNOWN_LABEL: &str = "Start with Windows (unknown)";

/// How the item shows, and what to log about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartItem {
    pub ticked: bool,
    pub label: &'static str,
    /// Why the tick may not be what the user expects, if it may not.
    pub warning: Option<String>,
}

/// The item for what the Run key starts, or for the error that kept it
/// from being read. Only this exe starting is ticked.
#[must_use]
pub fn autostart_item(read: &Result<Autostart, String>) -> AutostartItem {
    let (label, warning) = match read {
        Ok(Autostart::On | Autostart::Off) => (AUTOSTART_LABEL, None),
        Ok(Autostart::Other(command)) => (
            AUTOSTART_OTHER_LABEL,
            Some(format!("autostart runs another command: {command}")),
        ),
        Err(error) => (
            AUTOSTART_UNKNOWN_LABEL,
            Some(format!("autostart unknown: {error}")),
        ),
    };
    AutostartItem {
        ticked: matches!(read, Ok(Autostart::On)),
        label,
        warning,
    }
}

/// What to log before a click ticks the item over a Run value that is not
/// this copy's, so the change can be traced. `read` is the state just
/// before the click; `on` is the new tick.
#[must_use]
pub fn takeover_note(read: &Result<Autostart, String>, on: bool) -> Option<String> {
    if !on {
        return None;
    }
    match read {
        Ok(Autostart::Other(command)) => {
            Some(format!("autostart now starts this copy, not: {command}"))
        }
        Err(_) => Some("autostart set while the old value was unknown".to_owned()),
        Ok(Autostart::On | Autostart::Off) => None,
    }
}
