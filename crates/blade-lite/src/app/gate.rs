//! The controller, closed for good once the app has sent its quit preset.

use std::io;

use blade_core::{Controller, Query};
use razer_device::DeviceError;
use razer_proto::Command;

/// Wraps the controller. Once closed, every send and read is refused, so
/// nothing (a timer, a power event, a click, a nested message) can send
/// after the quit preset.
pub struct Gated<C> {
    /// The controller itself, for the tests to read their fake. Any use
    /// outside the tests goes around the gate.
    pub inner: C,
    closed: bool,
    /// While a Razer updater runs: nothing may meet a firmware update.
    held: bool,
}

impl<C> Gated<C> {
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            closed: false,
            held: false,
        }
    }

    /// Refuses every later send and read.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Refuses every send and read while `held`.
    pub fn hold(&mut self, held: bool) {
        self.held = held;
    }

    fn open(&mut self) -> Result<&mut C, DeviceError> {
        if self.closed {
            return Err(DeviceError::Io(io::Error::other("the app has quit")));
        }
        if self.held {
            return Err(DeviceError::Io(io::Error::other(
                "a Razer updater runs, so the app sends nothing",
            )));
        }
        Ok(&mut self.inner)
    }
}

impl<C: Controller> Controller for Gated<C> {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        self.open()?.run(commands)
    }
}

impl<C: Query> Query for Gated<C> {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        self.open()?.query(command)
    }
}
