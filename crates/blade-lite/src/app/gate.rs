//! The controller, closed for good once the app has sent its quit preset.

use std::io;
use std::ops::{Deref, DerefMut};

use blade_core::{Controller, Query};
use razer_device::DeviceError;
use razer_proto::Command;

/// Wraps the controller. Once closed, every send and read is refused, so
/// nothing (a timer, a power event, a click, a nested message) can send
/// after the quit preset.
pub struct Gated<C> {
    inner: C,
    closed: bool,
}

impl<C> Gated<C> {
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            closed: false,
        }
    }

    /// Refuses every later send and read.
    pub fn close(&mut self) {
        self.closed = true;
    }

    fn open(&mut self) -> Result<&mut C, DeviceError> {
        if self.closed {
            return Err(DeviceError::Io(io::Error::other("the app has quit")));
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

/// The tests read the fake controller through it.
impl<C> Deref for Gated<C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.inner
    }
}

impl<C> DerefMut for Gated<C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.inner
    }
}
