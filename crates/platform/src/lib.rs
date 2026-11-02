//! Windows APIs behind small traits, so the rest can be tested with fakes.
//!
//! This is the only crate allowed `unsafe`: each block says why it is sound.

#![allow(unsafe_code)]

mod power;

use std::io;

pub use power::WindowsPowerEvents;

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
