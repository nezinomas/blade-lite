//! Property tests: any stream of replies and HID errors, never a panic, the
//! command sent once, and the reads bounded (ticket 146).

use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use razer_device::{Clock, Device, MAX_BUSY_READS, Transport};
use razer_proto::{CHECKSUM_INDEX, REPORT_LEN, checksum};

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
}

fn reply() -> impl Strategy<Value = Reply> {
    prop_oneof![
        Just(Reply::Fails),
        prop::collection::vec(any::<u8>(), REPORT_LEN).prop_map(Reply::Noise),
        prop::sample::select(vec![0x01_u8, 0x02, 0x03, 0x05, 0x00, 0x42]).prop_map(Reply::Echo),
        (any::<u8>(), any::<u8>()).prop_map(|(id, status)| Reply::Other(id, status)),
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
        replies in prop::collection::vec(reply(), 0..12),
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

        let _ = device.send(0x0D82, &[1, 1, 0, 0]);

        prop_assert_eq!(sends.get(), 1);
        prop_assert!(reads.get() <= MAX_BUSY_READS + 1, "{} reads", reads.get());
    }
}
