//! Windows APIs behind small traits, so the rest can be tested with fakes.
//!
//! This is the only crate allowed `unsafe`: each block says why it is sound.

#![allow(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

mod backdrop;
mod display;
mod files;
mod instance;
mod keys;
mod popup;
mod power;
mod programs;
mod registry;
mod sensors;
mod watch;

use std::io;

pub use backdrop::{hide_caption_buttons, round_corners, set_mica};
pub use display::WindowsDisplays;
pub use files::{FileDialog, pick_file};
pub use instance::{WakeListener, WindowsInstance};
pub use keys::{WindowsKeys, enter_down, num_lock_on};
pub use popup::work_area;
pub use power::{WindowsPowerEvents, on_battery};
pub use programs::WindowsPrograms;
pub use registry::WindowsRegistry;
pub use sensors::WindowsSensors;
pub use watch::RegistryWatch;

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

/// Reads, writes and removes text values under `HKEY_CURRENT_USER`.
pub trait UserStrings {
    /// Reads the text `name` in `key`.
    ///
    /// # Errors
    ///
    /// The Windows error if the value is missing or is not text.
    fn read_user_string(&self, key: &str, name: &str) -> io::Result<String>;

    /// Writes the text `name` in `key`, making the key if needed.
    ///
    /// # Errors
    ///
    /// The Windows error if the value could not be written.
    fn write_user_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()>;

    /// Removes `name` from `key`. A value that is already gone is no error.
    ///
    /// # Errors
    ///
    /// The Windows error if the value could not be removed.
    fn delete_user_value(&mut self, key: &str, name: &str) -> io::Result<()>;
}

/// Reads binary values under `HKEY_CURRENT_USER`.
pub trait UserBytes {
    /// Reads the binary value `name` in `key`.
    ///
    /// # Errors
    ///
    /// The Windows error if the value is missing or is not binary.
    fn read_user_bytes(&self, key: &str, name: &str) -> io::Result<Vec<u8>>;
}

/// Reads what runs on the laptop now.
pub trait Programs {
    /// The file name of each running program, and the name and display
    /// name of each running service and driver, in no set order.
    ///
    /// # Errors
    ///
    /// The Windows error if they could not be read.
    fn running(&self) -> io::Result<Vec<String>>;
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

/// How long a display keeps a mode it is switched to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeLife {
    /// Saved in the user's settings, so it is kept after a restart.
    Saved,
    /// Not saved: a restart or a sign-in starts at the saved mode. Windows
    /// or another program (such as a game on exit) can also put the saved
    /// mode back sooner, for example after a display is plugged in (general
    /// Windows behaviour, not checked on this laptop).
    UntilRestart,
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

    /// Switches the display named `name` to `mode`, for as long as `life` says.
    ///
    /// # Errors
    ///
    /// The Windows error if the display did not take the mode.
    fn set_mode(&mut self, name: &str, mode: DisplayMode, life: ModeLife) -> io::Result<()>;

    /// The mode saved for the display named `name`, which a restart starts
    /// at, or `None` if Windows has none.
    fn saved_mode(&self, name: &str) -> Option<DisplayMode>;
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
    /// Whether the battery charges now. `None` with no battery, or off AC.
    pub charging: Option<bool>,
}

/// The battery's wear, as Windows reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatteryHealth {
    /// The full charge capacity against the design capacity, in percent.
    /// `None` if Windows does not know either.
    pub percent: Option<u32>,
    /// The charge cycles. `None` if the battery does not count them.
    pub cycles: Option<u32>,
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

    /// The battery's wear and cycle count.
    ///
    /// # Errors
    ///
    /// The Windows error if it could not be read, or `NotFound` with no
    /// battery.
    fn battery_health(&mut self) -> io::Result<BatteryHealth>;

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
