//! Windows APIs behind small traits, so the rest can be tested with fakes.
//!
//! This is the only crate allowed `unsafe`: each block says why it is sound.

#![allow(unsafe_code)]

mod backdrop;
mod display;
mod instance;
mod keys;
mod power;
mod registry;
mod sensors;

use std::io;

pub use backdrop::{round_corners, set_mica};
pub use display::WindowsDisplays;
pub use instance::{WakeListener, WindowsInstance};
pub use keys::WindowsKeys;
pub use power::WindowsPowerEvents;
pub use registry::WindowsRegistry;
pub use sensors::WindowsSensors;

/// Something Windows tells us about power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// The laptop woke from sleep or hibernation.
    Resumed,
    /// The laptop now runs on its battery.
    OnBattery,
    /// The laptop now runs on its charger.
    OnAc,
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

/// Keeps the app to one running copy.
pub trait InstanceLock {
    /// Claims the app for this copy. `false` means another copy has it.
    ///
    /// # Errors
    ///
    /// The Windows error if the claim could not be made.
    fn claim(&mut self) -> io::Result<bool>;

    /// Asks the copy that has the app to open its window.
    ///
    /// # Errors
    ///
    /// The Windows error if the other copy could not be told.
    fn wake_first(&self) -> io::Result<()>;
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

/// Reads the active displays and their modes, and changes a display's mode.
pub trait DisplayApi {
    /// The active displays, in no set order.
    ///
    /// # Errors
    ///
    /// The Windows error if the displays could not be read.
    fn displays(&self) -> io::Result<Vec<Display>>;

    /// Switches the display named `name` to `mode`, and keeps it after a restart.
    ///
    /// # Errors
    ///
    /// The Windows error if the display did not take the mode.
    fn set_mode(&mut self, name: &str, mode: DisplayMode) -> io::Result<()>;
}

/// How long the CPUs have run since boot, in 100 ns units, summed over all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    /// Time spent idle.
    pub idle: u64,
    /// All time, idle included.
    pub total: u64,
}

/// The battery as Windows reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerReading {
    /// `None` if Windows does not know it.
    pub battery_percent: Option<u8>,
    /// Milliwatts into the battery: below zero while it drains. `None` if
    /// Windows does not know it.
    pub battery_rate_mw: Option<i32>,
}

/// What the NVIDIA GPU reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuReading {
    pub celsius: u32,
    pub load_percent: u32,
}

/// Reads the sensors Windows offers.
pub trait Sensors {
    /// The CPU time since boot.
    ///
    /// # Errors
    ///
    /// The Windows error if it could not be read.
    fn cpu_times(&mut self) -> io::Result<CpuTimes>;

    /// The battery charge and rate.
    ///
    /// # Errors
    ///
    /// The Windows error if it could not be read.
    fn power(&mut self) -> io::Result<PowerReading>;

    /// Whether the NVIDIA GPU is awake. Asking does not wake it.
    ///
    /// # Errors
    ///
    /// The Windows error if its power state could not be read.
    fn gpu_awake(&mut self) -> io::Result<bool>;

    /// The GPU's temperature and load. This wakes a sleeping GPU, so ask
    /// [`gpu_awake`](Self::gpu_awake) first.
    ///
    /// # Errors
    ///
    /// The error if the GPU could not be read.
    fn gpu(&mut self) -> io::Result<GpuReading>;
}

/// The Windows error for a status code that a call returned.
fn windows_error(status: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(status).unwrap_or(i32::MAX))
}

/// The text as UTF-16 with a closing nul, as Windows wants it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
