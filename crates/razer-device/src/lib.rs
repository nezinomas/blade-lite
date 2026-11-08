//! Talks to the Blade 18 controller over a [`Transport`].

use std::hash::{BuildHasher, Hasher, RandomState};
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

/// How long the controller needs between a send and a read, and before a read again.
pub const PAUSE: Duration = Duration::from_millis(1);

/// How long to wait before reading again after a Busy reply. Longer than
/// [`PAUSE`], so a slow command can finish: up to 200 ms over all Busy reads.
/// A guess: no Busy reply has been seen on the laptop.
pub const BUSY_PAUSE: Duration = Duration::from_millis(50);

/// How many replies are read while the controller replies Busy. The command
/// itself is sent only once.
pub const MAX_BUSY_READS: usize = 5;

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
    /// The controller was still busy after [`MAX_BUSY_READS`] reads. The command
    /// was sent once and may still run.
    Busy,
    /// The controller replied Failure or `NotSupported`.
    Rejected(Status),
    /// The reply has fewer args than the command needs.
    ShortReply,
    /// The reply is for another request: it echoes another transaction ID or
    /// command. Holds what the reply echoed. A reply with another transaction ID
    /// is read once more after the pause. The command is never sent again, since
    /// it is not known what the controller did.
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

/// The controller, with the pause and the Busy read-again built in.
pub struct Device<T, C> {
    transport: T,
    clock: C,
    transaction_id: u8,
}

impl<T: Transport, C: Clock> Device<T, C> {
    /// A device whose first transaction ID is chosen at random, so a reply left
    /// in the controller by an earlier run is unlikely to match its first request.
    pub fn new(transport: T, clock: C) -> Self {
        Self::with_first_id(transport, clock, random_id())
    }

    /// A device whose first transaction ID is `first_id`, modulo 32. For tests:
    /// the app uses [`Device::new`], so a reply from an earlier run is unlikely to match.
    #[doc(hidden)]
    pub fn with_first_id(transport: T, clock: C, first_id: u8) -> Self {
        Self {
            transport,
            clock,
            transaction_id: first_id % TRANSACTION_IDS,
        }
    }

    /// Sends a command once and returns the controller's Success reply.
    /// The command is never sent again, since it is not known what the
    /// controller did with it. A first reply with another transaction ID
    /// (most likely one read too soon) is read once more after the pause.
    /// While the controller replies Busy, the reply is read again after the
    /// [`BUSY_PAUSE`], up to [`MAX_BUSY_READS`] Busy reads: Busy may mean the controller
    /// took the command and is still working on it.
    ///
    /// # Errors
    ///
    /// See [`DeviceError`].
    pub fn send(&mut self, command: u16, args: &[u8]) -> Result<Report, DeviceError> {
        let transaction_id = self.next_transaction_id();
        let request = Request::new(transaction_id, command, args)
            .map_err(DeviceError::Build)?
            .to_bytes();
        let mut report = self.exchange(&request)?;
        if report.transaction_id() != transaction_id {
            report = self.read_after(PAUSE)?;
        }
        let mut busy_reads = 0;
        while is_busy(&report, transaction_id, command)? {
            busy_reads += 1;
            if busy_reads == MAX_BUSY_READS {
                return Err(DeviceError::Busy);
            }
            report = self.read_after(BUSY_PAUSE)?;
        }
        Ok(report)
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
        self.read_after(PAUSE)
    }

    /// Pauses, then reads and parses a reply.
    fn read_after(&mut self, pause: Duration) -> Result<Report, DeviceError> {
        self.clock.sleep(pause);
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
/// Bytes 2 and 3 (remaining packets) are not compared: this model sets them in
/// some replies, such as `00 02` for `078C`, though the request had `00 00`.
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

/// Whether a reply to the request is Busy. A Success reply is not; any other
/// status, or a reply to another request, is an error.
fn is_busy(report: &Report, transaction_id: u8, command: u16) -> Result<bool, DeviceError> {
    ensure_matches(report, transaction_id, command)?;
    match report.status() {
        Status::Success => Ok(false),
        Status::Busy => Ok(true),
        rejected => Err(DeviceError::Rejected(rejected)),
    }
}

/// A transaction ID from the random seed std gives each hasher.
fn random_id() -> u8 {
    let random = RandomState::new().build_hasher().finish();
    random.to_le_bytes()[0] % TRANSACTION_IDS
}
