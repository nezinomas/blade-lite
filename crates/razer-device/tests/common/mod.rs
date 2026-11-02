//! A fake controller and a fake clock that write into one shared log.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use razer_device::{Clock, Transport};
use razer_proto::{REPORT_LEN, Request};

/// Status bytes the controller puts in byte 0 of a reply.
pub const SUCCESS: u8 = 0x02;
pub const BUSY: u8 = 0x01;
pub const FAILURE: u8 = 0x03;

/// One thing the device did, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Sent([u8; REPORT_LEN]),
    Slept(Duration),
    Read,
}

pub type Log = Rc<RefCell<Vec<Event>>>;

/// Records the reports sent and replays canned replies.
pub struct FakeTransport {
    log: Log,
    replies: VecDeque<[u8; REPORT_LEN]>,
}

impl Transport for FakeTransport {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        self.log.borrow_mut().push(Event::Sent(*report));
        Ok(())
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        self.log.borrow_mut().push(Event::Read);
        self.replies
            .pop_front()
            .ok_or_else(|| io::Error::other("no canned reply left"))
    }
}

/// Records each sleep instead of waiting.
pub struct FakeClock {
    log: Log,
}

impl Clock for FakeClock {
    fn sleep(&self, duration: Duration) {
        self.log.borrow_mut().push(Event::Slept(duration));
    }
}

/// A fake transport that replays `replies`, a fake clock, and their shared log.
pub fn fakes(replies: Vec<[u8; REPORT_LEN]>) -> (FakeTransport, FakeClock, Log) {
    let log = Log::default();
    let transport = FakeTransport {
        log: Rc::clone(&log),
        replies: replies.into(),
    };
    let clock = FakeClock {
        log: Rc::clone(&log),
    };
    (transport, clock, log)
}

/// The bytes of a request, as the device should send them.
pub fn request(transaction_id: u8, command: u16, args: &[u8]) -> [u8; REPORT_LEN] {
    Request::new(transaction_id, command, args)
        .unwrap()
        .to_bytes()
}

/// A reply to a request: the same bytes with a status. Byte 0 is outside the checksum.
pub fn reply(status: u8, transaction_id: u8, command: u16) -> [u8; REPORT_LEN] {
    let mut bytes = request(transaction_id, command, &[]);
    bytes[0] = status;
    bytes
}

/// Only the reports the device sent.
pub fn sent(log: &Log) -> Vec<[u8; REPORT_LEN]> {
    log.borrow()
        .iter()
        .filter_map(|event| match event {
            Event::Sent(bytes) => Some(*bytes),
            _ => None,
        })
        .collect()
}

/// A Success reply that carries `args`.
#[allow(dead_code)] // Not every test binary uses it.
pub fn reply_with_args(transaction_id: u8, command: u16, args: &[u8]) -> [u8; REPORT_LEN] {
    let mut bytes = request(transaction_id, command, args);
    bytes[0] = SUCCESS;
    bytes
}
