//! The tray's "Start with Windows" item, from what the Run key starts.

use blade_core::Autostart;

/// The item's text.
pub const AUTOSTART_LABEL: &str = "Start with Windows";

/// The item's text while the Run key starts another copy of the app. A
/// click points it at this copy.
pub const AUTOSTART_OTHER_LABEL: &str = "Start with Windows (set for another copy)";

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
            Some(format!("autostart starts another copy: {command}")),
        ),
        Err(error) => (AUTOSTART_LABEL, Some(format!("autostart unknown: {error}"))),
    };
    AutostartItem {
        ticked: matches!(read, Ok(Autostart::On)),
        label,
        warning,
    }
}
