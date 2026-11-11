//! A [`Transport`] that opens the controller again after its handle dies,
//! such as after a driver install or the USB device going and coming back.

use std::io;

use razer_proto::REPORT_LEN;

use crate::Transport;

/// The controller, opened again before the next send after a real HID
/// error. The send or read that failed is never tried again: the next
/// command is the first to use the new handle.
pub struct Reopening<T, O> {
    transport: Option<T>,
    open: O,
}

impl<T: Transport, O: FnMut() -> io::Result<T>> Reopening<T, O> {
    /// Wraps a controller that is already open; `open` opens it again.
    pub fn new(transport: T, open: O) -> Self {
        Self {
            transport: Some(transport),
            open,
        }
    }

    /// Drops the handle after a real HID error, so the next send opens it
    /// again. A short feature report came from a live handle: it is kept.
    fn keep_if_alive<R>(&mut self, result: io::Result<R>) -> io::Result<R> {
        if result
            .as_ref()
            .is_err_and(|error| error.kind() != io::ErrorKind::UnexpectedEof)
        {
            self.transport = None;
        }
        result
    }
}

impl<T: Transport, O: FnMut() -> io::Result<T>> Transport for Reopening<T, O> {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        let transport = match &mut self.transport {
            Some(transport) => transport,
            // A failed open sends nothing; the next send tries again.
            None => self.transport.insert((self.open)()?),
        };
        let sent = transport.send_report(report);
        self.keep_if_alive(sent)
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        // Never opens: a new handle has no reply to read.
        let transport = self.transport.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "the controller is closed")
        })?;
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

    /// What each fake handle saw: which handle (by open count) sent what.
    type Log = Rc<RefCell<Vec<(usize, &'static str)>>>;

    /// A handle that fails each call with the next scripted error kind
    /// (`None` works).
    struct Fake {
        id: usize,
        script: Rc<RefCell<VecDeque<Option<io::ErrorKind>>>>,
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

    struct Setup {
        script: Rc<RefCell<VecDeque<Option<io::ErrorKind>>>>,
        log: Log,
        opens: Rc<RefCell<VecDeque<bool>>>,
    }

    /// A reopening transport over handle 0, whose opens work or fail as
    /// `opens` says (then work), and whose calls go as `script` says.
    fn setup(
        script: &[Option<io::ErrorKind>],
        opens: &[bool],
    ) -> (Setup, Reopening<Fake, impl FnMut() -> io::Result<Fake>>) {
        let setup = Setup {
            script: Rc::new(RefCell::new(script.iter().copied().collect())),
            log: Rc::default(),
            opens: Rc::new(RefCell::new(opens.iter().copied().collect())),
        };
        let fake = |id| Fake {
            id,
            script: Rc::clone(&setup.script),
            log: Rc::clone(&setup.log),
        };
        let first = fake(0);
        let (script, log, opens) = (
            Rc::clone(&setup.script),
            Rc::clone(&setup.log),
            Rc::clone(&setup.opens),
        );
        let mut count = 0;
        let open = move || {
            count += 1;
            log.borrow_mut().push((count, "open"));
            if opens.borrow_mut().pop_front() == Some(false) {
                return Err(io::Error::other("not there"));
            }
            Ok(Fake {
                id: count,
                script: Rc::clone(&script),
                log: Rc::clone(&log),
            })
        };
        (setup, Reopening::new(first, open))
    }

    const REPORT: [u8; REPORT_LEN] = [0; REPORT_LEN];

    #[test]
    fn a_live_handle_is_never_opened_again() {
        let (setup, mut controller) = setup(&[], &[]);
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*setup.log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
    }

    #[test]
    fn a_failed_send_opens_again_before_the_next_send_only() {
        let (setup, mut controller) = setup(&[Some(io::ErrorKind::Other)], &[]);
        assert!(controller.send_report(&REPORT).is_err());
        // No read on a dead handle, and no open for a read.
        assert_eq!(
            controller.read_report().unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();
        // The failed send was not sent again: one send per call.
        assert_eq!(
            *setup.log.borrow(),
            [(0, "send"), (1, "open"), (1, "send"), (1, "read")]
        );
    }

    #[test]
    fn a_failed_read_opens_again_before_the_next_send() {
        let (setup, mut controller) = setup(&[None, Some(io::ErrorKind::Other)], &[]);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *setup.log.borrow(),
            [(0, "send"), (0, "read"), (1, "open"), (1, "send")]
        );
    }

    #[test]
    fn a_short_report_keeps_the_handle() {
        let (setup, mut controller) = setup(&[None, Some(io::ErrorKind::UnexpectedEof)], &[]);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*setup.log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
    }

    #[test]
    fn a_failed_open_sends_nothing_and_the_next_send_tries_again() {
        let (setup, mut controller) = setup(&[Some(io::ErrorKind::Other)], &[false]);
        assert!(controller.send_report(&REPORT).is_err());
        assert_eq!(
            controller.send_report(&REPORT).unwrap_err().to_string(),
            "not there"
        );
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *setup.log.borrow(),
            [(0, "send"), (1, "open"), (2, "open"), (2, "send")]
        );
        assert!(setup.opens.borrow().is_empty());
    }
}
