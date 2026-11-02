//! Talks to the Blade 18 controller over a [`Transport`].

use std::io;
use std::time::Duration;

use razer_proto::{BuildError, ParseError, REPORT_LEN, Report, Request, Status};

/// Sends and reads the controller's feature reports.
pub trait Transport {
    /// Sends one report to the controller.
    ///
    /// # Errors
    ///
    /// Any I/O error from the HID layer.
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()>;

    /// Reads the controller's reply.
    ///
    /// # Errors
    ///
    /// Any I/O error from the HID layer.
    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]>;
}

/// Waits. A trait so that tests do not wait for real.
pub trait Clock {
    fn sleep(&self, duration: Duration);
}

/// How long the controller needs between a send and a read, and before a retry.
pub const PAUSE: Duration = Duration::from_millis(1);

/// How many times a command is sent while the controller replies Busy.
pub const MAX_ATTEMPTS: usize = 5;

/// Transaction IDs count from 0 to 31, then start again, as Synapse does.
const TRANSACTION_IDS: u8 = 32;

/// Why a command did not succeed.
#[derive(Debug)]
pub enum DeviceError {
    /// The command could not be built.
    Build(BuildError),
    /// The HID layer failed.
    Io(io::Error),
    /// The reply could not be parsed.
    Parse(ParseError),
    /// The controller was still busy after [`MAX_ATTEMPTS`] sends.
    Busy,
    /// The controller replied Failure or `NotSupported`.
    Rejected(Status),
}

/// The controller, with the pause and the Busy retry built in.
pub struct Device<T, C> {
    transport: T,
    clock: C,
    transaction_id: u8,
}

impl<T: Transport, C: Clock> Device<T, C> {
    pub fn new(transport: T, clock: C) -> Self {
        Self {
            transport,
            clock,
            transaction_id: 0,
        }
    }

    /// Sends a command and returns the controller's Success reply.
    /// While the controller replies Busy, waits and sends the command again.
    ///
    /// # Errors
    ///
    /// See [`DeviceError`].
    pub fn send(&mut self, command: u16, args: &[u8]) -> Result<Report, DeviceError> {
        let request = Request::new(self.next_transaction_id(), command, args)
            .map_err(DeviceError::Build)?
            .to_bytes();
        for _ in 0..MAX_ATTEMPTS {
            let report = self.exchange(&request)?;
            match report.status() {
                Status::Success => return Ok(report),
                Status::Busy => self.clock.sleep(PAUSE),
                rejected => return Err(DeviceError::Rejected(rejected)),
            }
        }
        Err(DeviceError::Busy)
    }

    /// Sends one report, pauses, and reads the reply.
    fn exchange(&mut self, request: &[u8; REPORT_LEN]) -> Result<Report, DeviceError> {
        self.transport
            .send_report(request)
            .map_err(DeviceError::Io)?;
        self.clock.sleep(PAUSE);
        let reply = self.transport.read_report().map_err(DeviceError::Io)?;
        Report::parse(&reply).map_err(DeviceError::Parse)
    }

    fn next_transaction_id(&mut self) -> u8 {
        let id = self.transaction_id;
        self.transaction_id = (id + 1) % TRANSACTION_IDS;
        id
    }
}
