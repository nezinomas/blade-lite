//! Property tests: any stream of replies and HID errors, never a panic, the
//! command sent once, and the reads bounded (ticket 146).

use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use razer_device::{Clock, Device, MAX_BUSY_READS, Transport};
use razer_proto::{CHECKSUM_INDEX, REPORT_LEN, Status, checksum};

/// One reply the fake hands out.
#[derive(Debug, Clone)]
enum Reply {
    /// A HID error.
    Fails,
    /// Any bytes at all.
    Noise(Vec<u8>),
    /// A well-formed reply to the request sent, with this status byte.
    Echo(u8),
    /// A well-formed reply with this transaction ID and status byte.
    Other(u8, u8),
    /// A well-formed reply to another command, with this status byte.
    OtherCommand(u8, u8),
    /// Any bytes with a good checksum.
    SignedNoise(Vec<u8>),
}

fn reply() -> impl Strategy<Value = Reply> {
    let noise = || prop::collection::vec(any::<u8>(), REPORT_LEN);
    prop_oneof![
        1 => Just(Reply::Fails),
        1 => noise().prop_map(Reply::Noise),
        1 => noise().prop_map(Reply::SignedNoise),
        // Busy often, so runs of it reach the read limit.
        6 => Just(Reply::Echo(0x01)),
        2 => prop::sample::select(vec![0x02_u8, 0x03, 0x04, 0x05, 0x00, 0x42]).prop_map(Reply::Echo),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(id, status)| Reply::Other(id, status)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(id, status)| Reply::OtherCommand(id, status)),
    ]
}

/// Hands out the replies in turn; an empty list fails each read.
struct Fake {
    replies: Vec<Reply>,
    last_request: [u8; REPORT_LEN],
    sends: Rc<Cell<usize>>,
    reads: Rc<Cell<usize>>,
}

impl Transport for Fake {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        self.sends.set(self.sends.get() + 1);
        self.last_request = *report;
        Ok(())
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        self.reads.set(self.reads.get() + 1);
        if self.replies.is_empty() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let signed = |mut bytes: [u8; REPORT_LEN], status: u8| {
            bytes[0] = status;
            bytes[CHECKSUM_INDEX] = checksum(&bytes);
            bytes
        };
        match self.replies.remove(0) {
            Reply::Fails => Err(io::ErrorKind::BrokenPipe.into()),
            Reply::Noise(bytes) => Ok(bytes.try_into().unwrap_or([0; REPORT_LEN])),
            Reply::Echo(status) => Ok(signed(self.last_request, status)),
            Reply::Other(id, status) => {
                let mut bytes = self.last_request;
                bytes[1] = id;
                Ok(signed(bytes, status))
            }
            Reply::OtherCommand(id, status) => {
                let mut bytes = self.last_request;
                bytes[7] = id;
                Ok(signed(bytes, status))
            }
            Reply::SignedNoise(bytes) => {
                let bytes: [u8; REPORT_LEN] = bytes.try_into().unwrap_or([0; REPORT_LEN]);
                Ok(signed(bytes, bytes[0]))
            }
        }
    }
}

struct NoWait;

impl Clock for NoWait {
    fn sleep(&self, _: Duration) {}
}

proptest! {
    #[test]
    fn any_replies_send_the_command_once_and_read_a_bounded_number_of_times(
        replies in prop::collection::vec(reply(), 0..14),
        first_id in any::<u8>(),
    ) {
        let (sends, reads) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let fake = Fake {
            replies,
            last_request: [0; REPORT_LEN],
            sends: Rc::clone(&sends),
            reads: Rc::clone(&reads),
        };
        let mut device = Device::with_first_id(fake, NoWait, first_id);

        // Any command does; this is the preset query.
        let sent = device.send(0x0D82, &[1, 1, 0, 0]);

        prop_assert_eq!(sends.get(), 1);
        // A reply taken is a Success reply to this very request.
        if let Ok(report) = sent {
            prop_assert_eq!(report.transaction_id(), first_id % 32);
            prop_assert_eq!(report.command(), 0x0D82);
            prop_assert_eq!(report.status(), Status::Success);
        }
        prop_assert!(reads.get() <= MAX_BUSY_READS + 1, "{} reads", reads.get());
    }
}
