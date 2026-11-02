//! Windows APIs behind small traits, so the rest can be tested with fakes.
//!
//! This is the only crate allowed `unsafe`: each block says why it is sound.

#![allow(unsafe_code)]

mod display;
mod keys;
mod power;
mod registry;

use std::io;

pub use display::WindowsDisplays;
pub use keys::WindowsKeys;
pub use power::WindowsPowerEvents;
pub use registry::WindowsRegistry;

/// Something Windows tells us about power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// The laptop woke from sleep or hibernation.
    Resumed,
}

/// Power events from Windows, one at a time.
pub trait PowerEvents {
    /// Waits for the next event. `None` means no more events will come.
    fn next_event(&mut self) -> Option<PowerEvent>;
}

/// Reads values under `HKEY_CURRENT_USER`.
pub trait Registry {
    /// Reads the DWORD `name` in `key`.
    ///
    /// # Errors
    ///
    /// The Windows error if the value is missing or is not a DWORD.
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32>;
}

/// A key that the app may send. The app never sends any other key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Win,
    Ctrl,
    F24,
}

/// Sends key presses to Windows, as if typed.
pub trait KeyInjector {
    /// Presses the keys down in order, then lets them go in reverse order.
    ///
    /// # Errors
    ///
    /// The Windows error if the presses could not be sent.
    fn press(&mut self, keys: &[Key]) -> io::Result<()>;
}

/// One way a display can be driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    /// The refresh rate, in Hz.
    pub hz: u32,
}

/// How a display is connected, as Windows reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTechnology {
    /// The laptop's own panel.
    Internal,
    /// Any other display, such as a monitor on a cable.
    External,
}

/// An active display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// The GDI name, such as `\\.\DISPLAY1`.
    pub name: String,
    pub technology: OutputTechnology,
    pub current: DisplayMode,
    /// Every mode Windows offers, in its own order, repeats and all.
    pub modes: Vec<DisplayMode>,
}

/// Reads the active displays and their modes.
pub trait DisplayApi {
    /// The active displays, in no set order.
    ///
    /// # Errors
    ///
    /// The Windows error if the displays could not be read.
    fn displays(&self) -> io::Result<Vec<Display>>;
}

/// The Windows error for a status code that a call returned.
fn windows_error(status: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(status).unwrap_or(i32::MAX))
}

/// The text as UTF-16 with a closing nul, as Windows wants it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
