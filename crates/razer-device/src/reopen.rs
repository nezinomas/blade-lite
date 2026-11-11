//! A [`Transport`] that opens the controller again after its handle dies,
//! such as after a driver install or the USB device going and coming back.

use std::io;
use std::time::{Duration, Instant};

use razer_proto::REPORT_LEN;

use crate::Transport;

/// The controller, opened again at the next send after a HID error. Any
/// error but a short feature report drops the handle: hidapi gives no
/// kind to tell a dead handle from a one-off, and a new handle on a live
/// controller does no harm. Nothing is ever sent twice:
/// - the send or read that failed is not tried again;
/// - the send that opens the controller is not sent either, so the caller
///   can look for what took the controller (such as Razer's software) and
///   send its settings again before anything goes on the new handle;
/// - a read never opens: a new handle has no reply to read.
///
/// A failed open is not tried again until `gap` has passed, as each open
/// lists every HID device.
pub struct Reopening<T, O> {
    transport: Option<T>,
    open: O,
    gap: Duration,
    failed_at: Option<Instant>,
}

impl<T: Transport, O: FnMut() -> io::Result<T>> Reopening<T, O> {
    /// Wraps a controller that is already open; `open` opens it again, at
    /// most once per `gap` while it fails.
    pub fn new(transport: T, open: O, gap: Duration) -> Self {
        Self {
            transport: Some(transport),
            open,
            gap,
            failed_at: None,
        }
    }

    /// Drops the handle after a HID error, so the next send opens it again.
    /// A short feature report came from a live handle: it is kept.
    fn keep_if_alive<R>(&mut self, result: io::Result<R>) -> io::Result<R> {
        if result
            .as_ref()
            .is_err_and(|error| error.kind() != io::ErrorKind::UnexpectedEof)
        {
            self.transport = None;
        }
        result
    }

    /// Opens the controller, unless an open failed less than `gap` ago.
    /// The command that asked is never sent: see [`Reopening`].
    fn open_again(&mut self) -> io::Error {
        if self.failed_at.is_some_and(|at| at.elapsed() < self.gap) {
            return closed("the controller is closed, and was tried a moment ago");
        }
        match (self.open)() {
            Ok(transport) => {
                self.transport = Some(transport);
                self.failed_at = None;
                closed("the controller was opened again, so this command was not sent")
            }
            Err(error) => {
                self.failed_at = Some(Instant::now());
                error
            }
        }
    }
}

fn closed(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, text)
}

impl<T: Transport, O: FnMut() -> io::Result<T>> Transport for Reopening<T, O> {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        let Some(transport) = &mut self.transport else {
            return Err(self.open_again());
        };
        let sent = transport.send_report(report);
        self.keep_if_alive(sent)
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        let transport = self
            .transport
            .as_mut()
            .ok_or_else(|| closed("the controller is closed"))?;
        let read = transport.read_report();
        self.keep_if_alive(read)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    /// What each fake handle saw: which handle (by open count) did what.
    type Log = Rc<RefCell<Vec<(usize, &'static str)>>>;

    type Script = Rc<RefCell<VecDeque<Option<io::ErrorKind>>>>;

    /// A handle that fails each call with the next scripted error kind
    /// (`None` works).
    struct Fake {
        id: usize,
        script: Script,
        log: Log,
    }

    impl Fake {
        fn call(&self, what: &'static str) -> io::Result<()> {
            self.log.borrow_mut().push((self.id, what));
            match self.script.borrow_mut().pop_front().flatten() {
                Some(kind) => Err(io::Error::new(kind, "scripted")),
                None => Ok(()),
            }
        }
    }

    impl Transport for Fake {
        fn send_report(&mut self, _: &[u8; REPORT_LEN]) -> io::Result<()> {
            self.call("send")
        }

        fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
            self.call("read").map(|()| [0; REPORT_LEN])
        }
    }

    /// A reopening transport over handle 0, whose opens fail or work as
    /// `opens` says (then work), and whose calls go as `script` says.
    fn setup(
        script: &[Option<io::ErrorKind>],
        opens: &[bool],
        gap: Duration,
    ) -> (
        Log,
        Reopening<Fake, impl FnMut() -> io::Result<Fake> + use<>>,
    ) {
        let script: Script = Rc::new(RefCell::new(script.iter().copied().collect()));
        let log = Log::default();
        let mut opens: VecDeque<bool> = opens.iter().copied().collect();
        let first = Fake {
            id: 0,
            script: Rc::clone(&script),
            log: Rc::clone(&log),
        };
        let open_log = Rc::clone(&log);
        let mut count = 0;
        let open = move || {
            count += 1;
            open_log.borrow_mut().push((count, "open"));
            if opens.pop_front() == Some(false) {
                return Err(io::Error::other("not there"));
            }
            Ok(Fake {
                id: count,
                script: Rc::clone(&script),
                log: Rc::clone(&open_log),
            })
        };
        (log, Reopening::new(first, open, gap))
    }

    const REPORT: [u8; REPORT_LEN] = [0; REPORT_LEN];
    const DEAD: Option<io::ErrorKind> = Some(io::ErrorKind::Other);

    #[test]
    fn a_live_handle_is_never_opened_again() {
        let (log, mut controller) = setup(&[], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
    }

    #[test]
    fn after_a_failed_send_the_next_send_opens_and_sends_nothing() {
        let (log, mut controller) = setup(&[DEAD], &[], Duration::ZERO);
        assert!(controller.send_report(&REPORT).is_err());
        // No read on a dead handle, and no open for a read.
        assert_eq!(
            controller.read_report().unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        let opened = controller.send_report(&REPORT).unwrap_err();
        assert_eq!(opened.kind(), io::ErrorKind::NotConnected);
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();
        // Neither the failed send nor the one that opened went out again.
        assert_eq!(
            *log.borrow(),
            [(0, "send"), (1, "open"), (1, "send"), (1, "read")]
        );
    }

    #[test]
    fn a_failed_read_opens_at_the_next_send() {
        let (log, mut controller) = setup(&[None, DEAD], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        assert!(controller.send_report(&REPORT).is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *log.borrow(),
            [(0, "send"), (0, "read"), (1, "open"), (1, "send")]
        );
    }

    #[test]
    fn a_short_report_keeps_the_handle() {
        let short = Some(io::ErrorKind::UnexpectedEof);
        let (log, mut controller) = setup(&[None, short], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
    }

    #[test]
    fn a_failed_open_sends_nothing_and_a_later_send_tries_again() {
        let (log, mut controller) = setup(&[DEAD], &[false], Duration::ZERO);
        assert!(controller.send_report(&REPORT).is_err());
        assert_eq!(
            controller.send_report(&REPORT).unwrap_err().to_string(),
            "not there"
        );
        assert!(controller.send_report(&REPORT).is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *log.borrow(),
            [(0, "send"), (1, "open"), (2, "open"), (2, "send")]
        );
    }

    #[test]
    fn a_failed_open_is_not_tried_again_within_the_gap() {
        let gap = Duration::from_secs(3600);
        let (log, mut controller) = setup(&[DEAD], &[false], gap);
        assert!(controller.send_report(&REPORT).is_err());
        assert!(controller.send_report(&REPORT).is_err());
        let held = controller.send_report(&REPORT).unwrap_err();
        assert_eq!(held.kind(), io::ErrorKind::NotConnected);
        assert_eq!(*log.borrow(), [(0, "send"), (1, "open")]);
    }
}
