//! Talks to the Blade 18 controller over a [`Transport`].

use std::io;
use std::time::Duration;

use razer_proto::{BuildError, Command, ParseError, REPORT_LEN, Report, Request, Status};

mod discovery;
mod hid;

pub use discovery::{HidInfo, find_controller, find_key_collections};
pub use hid::{HidKeyReports, HidTransport};

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

/// Waits for real.
pub struct SystemClock;

impl Clock for SystemClock {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
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
    /// The reply has fewer args than the command needs.
    ShortReply,
    /// The reply is for another request: it echoes another transaction ID or
    /// command. Holds what the reply echoed. Not retried, since it is not known
    /// what the controller did.
    WrongReply { transaction_id: u8, command: u16 },
}

/// Get firmware version. Args are `[0, 0]`, the reply args are `[major, minor]`.
const FIRMWARE_VERSION: u16 = 0x0081;

/// The controller's firmware version, such as 1.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
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
        let transaction_id = self.next_transaction_id();
        let request = Request::new(transaction_id, command, args)
            .map_err(DeviceError::Build)?
            .to_bytes();
        for _ in 0..MAX_ATTEMPTS {
            let report = self.exchange(&request)?;
            ensure_matches(&report, transaction_id, command)?;
            match report.status() {
                Status::Success => return Ok(report),
                Status::Busy => self.clock.sleep(PAUSE),
                rejected => return Err(DeviceError::Rejected(rejected)),
            }
        }
        Err(DeviceError::Busy)
    }

    /// Sends each command in order, and stops at the first that fails.
    ///
    /// # Errors
    ///
    /// See [`DeviceError`].
    pub fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        for command in commands {
            self.send(command.id, &command.args)?;
        }
        Ok(())
    }

    /// Reads the controller's firmware version.
    ///
    /// # Errors
    ///
    /// See [`DeviceError`].
    pub fn firmware_version(&mut self) -> Result<FirmwareVersion, DeviceError> {
        let report = self.send(FIRMWARE_VERSION, &[0, 0])?;
        match *report.args() {
            [major, minor, ..] => Ok(FirmwareVersion { major, minor }),
            _ => Err(DeviceError::ShortReply),
        }
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

/// Checks that `report` echoes the transaction ID and command it was sent for.
fn ensure_matches(report: &Report, transaction_id: u8, command: u16) -> Result<(), DeviceError> {
    if report.transaction_id() == transaction_id && report.command() == command {
        Ok(())
    } else {
        Err(DeviceError::WrongReply {
            transaction_id: report.transaction_id(),
            command: report.command(),
        })
    }
}
