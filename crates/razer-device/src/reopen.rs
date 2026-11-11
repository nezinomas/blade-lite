//! A [`Transport`] that opens the controller again after its handle dies,
//! such as after a driver install or the USB device going and coming back.

use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use razer_proto::REPORT_LEN;

use crate::Transport;

/// How many gaps to wait before opening again after a handle that died
/// before any send on it worked: such a controller opens but can't be
/// used (still coming up, or held by a driver).
const DEAD_HANDLE_GAPS: u32 = 6;

/// The controller, opened again after a HID error. Any error but a short
/// feature report drops the handle: hidapi gives no kind to tell a dead
/// handle from a one-off, and a new handle on a live controller does no
/// harm. Nothing is ever sent twice, and nothing goes on a new handle
/// before the app is ready:
/// - the send or read that failed is not tried again;
/// - the next send opens the controller and is not sent;
/// - a new handle is held: every send and read is refused until the app
///   clears `held` (after a pause and a look for Razer software, it sends
///   its settings again);
/// - a read never opens.
///
/// An open is not tried sooner than `gap` after the last one, whether it
/// worked or not, as each open lists every HID device; after a handle that
/// died before any send worked, not sooner than 6 gaps.
pub struct Reopening<T, O> {
    transport: Option<T>,
    open: O,
    gap: Duration,
    held: Rc<Cell<bool>>,
    opened_at: Option<Instant>,
    /// Whether a send on the handle (the last one, if it is closed) worked.
    worked: bool,
}

impl<T: Transport, O: FnMut(bool) -> io::Result<T>> Reopening<T, O> {
    /// Wraps a controller that is already open. `open` opens it again; it
    /// is told whether a send on the last handle worked, so the first open
    /// of a run of failures can be told from the next ones. A new handle
    /// sets `held`.
    pub fn new(transport: T, open: O, gap: Duration, held: Rc<Cell<bool>>) -> Self {
        Self {
            transport: Some(transport),
            open,
            gap,
            held,
            opened_at: None,
            worked: true,
        }
    }

    /// The handle, unless it is held. A held one sends and reads nothing.
    fn usable(&mut self) -> io::Result<Option<&mut T>> {
        if self.held.get() {
            return Err(closed(
                "the controller was opened again and waits for the app",
            ));
        }
        Ok(self.transport.as_mut())
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

    /// Opens the controller, unless the last open was too short a time ago.
    /// The command that asked is never sent: see [`Reopening`].
    fn open_again(&mut self) -> io::Error {
        let gap = if self.worked {
            self.gap
        } else {
            self.gap * DEAD_HANDLE_GAPS
        };
        if self.opened_at.is_some_and(|at| at.elapsed() < gap) {
            return closed("the controller is closed, and was tried a moment ago");
        }
        self.opened_at = Some(Instant::now());
        match (self.open)(self.worked) {
            Ok(transport) => {
                self.transport = Some(transport);
                self.worked = false;
                self.held.set(true);
                closed("the controller was opened again, so this command was not sent")
            }
            Err(error) => error,
        }
    }
}

fn closed(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, text)
}

impl<T: Transport, O: FnMut(bool) -> io::Result<T>> Transport for Reopening<T, O> {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        let Some(transport) = self.usable()? else {
            return Err(self.open_again());
        };
        let sent = transport.send_report(report);
        self.worked |= sent.is_ok();
        self.keep_if_alive(sent)
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        let transport = self
            .usable()?
            .ok_or_else(|| closed("the controller is closed"))?;
        let read = transport.read_report();
        self.keep_if_alive(read)
    }
}
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

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

    struct Setup {
        log: Log,
        held: Rc<Cell<bool>>,
        /// What each open was told: whether the last handle worked.
        told: Rc<RefCell<Vec<bool>>>,
    }

    /// A reopening transport over handle 0, whose opens fail or work as
    /// `opens` says (then work), and whose calls go as `script` says.
    fn setup(
        script: &[Option<io::ErrorKind>],
        opens: &[bool],
        gap: Duration,
    ) -> (
        Setup,
        Reopening<Fake, impl FnMut(bool) -> io::Result<Fake> + use<>>,
    ) {
        let script: Script = Rc::new(RefCell::new(script.iter().copied().collect()));
        let setup = Setup {
            log: Log::default(),
            held: Rc::default(),
            told: Rc::default(),
        };
        let mut opens: VecDeque<bool> = opens.iter().copied().collect();
        let first = Fake {
            id: 0,
            script: Rc::clone(&script),
            log: Rc::clone(&setup.log),
        };
        let (log, told) = (Rc::clone(&setup.log), Rc::clone(&setup.told));
        let mut count = 0;
        let open = move |worked| {
            count += 1;
            log.borrow_mut().push((count, "open"));
            told.borrow_mut().push(worked);
            if opens.pop_front() == Some(false) {
                return Err(io::Error::other("not there"));
            }
            Ok(Fake {
                id: count,
                script: Rc::clone(&script),
                log: Rc::clone(&log),
            })
        };
        let held = Rc::clone(&setup.held);
        (setup, Reopening::new(first, open, gap, held))
    }

    const REPORT: [u8; REPORT_LEN] = [0; REPORT_LEN];
    const DEAD: Option<io::ErrorKind> = Some(io::ErrorKind::Other);
    const SHORT: Option<io::ErrorKind> = Some(io::ErrorKind::UnexpectedEof);

    fn not_connected<R: std::fmt::Debug>(result: io::Result<R>) -> bool {
        result.unwrap_err().kind() == io::ErrorKind::NotConnected
    }

    #[test]
    fn a_live_handle_is_never_opened_again() {
        let (s, mut controller) = setup(&[], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*s.log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
        assert!(!s.held.get());
    }

    #[test]
    fn a_new_handle_is_held_until_the_app_clears_it() {
        let (s, mut controller) = setup(&[DEAD], &[], Duration::ZERO);
        assert!(controller.send_report(&REPORT).is_err());
        // No read on a dead handle, and no open for a read.
        assert!(not_connected(controller.read_report()));
        // The send that opens is not sent.
        assert!(not_connected(controller.send_report(&REPORT)));
        assert!(s.held.get());
        assert!(not_connected(controller.send_report(&REPORT)));
        assert!(not_connected(controller.read_report()));

        s.held.set(false);
        controller.send_report(&REPORT).unwrap();
        controller.read_report().unwrap();

        assert_eq!(
            *s.log.borrow(),
            [(0, "send"), (1, "open"), (1, "send"), (1, "read")]
        );
        assert_eq!(*s.told.borrow(), [true]);
    }

    #[test]
    fn a_failed_read_opens_at_the_next_send() {
        let (s, mut controller) = setup(&[None, DEAD], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        assert!(controller.send_report(&REPORT).is_err());
        s.held.set(false);
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *s.log.borrow(),
            [(0, "send"), (0, "read"), (1, "open"), (1, "send")]
        );
    }

    #[test]
    fn a_short_report_keeps_the_handle() {
        let (s, mut controller) = setup(&[None, SHORT], &[], Duration::ZERO);
        controller.send_report(&REPORT).unwrap();
        assert!(controller.read_report().is_err());
        controller.send_report(&REPORT).unwrap();
        assert_eq!(*s.log.borrow(), [(0, "send"), (0, "read"), (0, "send")]);
    }

    #[test]
    fn a_failed_open_sends_nothing_and_a_later_send_tries_again() {
        let (s, mut controller) = setup(&[DEAD], &[false], Duration::ZERO);
        assert!(controller.send_report(&REPORT).is_err());
        assert_eq!(
            controller.send_report(&REPORT).unwrap_err().to_string(),
            "not there"
        );
        assert!(!s.held.get(), "no handle to hold");
        assert!(controller.send_report(&REPORT).is_err());
        s.held.set(false);
        controller.send_report(&REPORT).unwrap();
        assert_eq!(
            *s.log.borrow(),
            [(0, "send"), (1, "open"), (2, "open"), (2, "send")]
        );
        // The handle that died first had worked; none since.
        assert_eq!(*s.told.borrow(), [true, true]);
    }

    #[test]
    fn no_open_within_the_gap_of_the_last_whether_it_worked_or_not() {
        let gap = Duration::from_secs(3600);
        let (s, mut controller) = setup(&[DEAD], &[false], gap);
        assert!(controller.send_report(&REPORT).is_err());
        assert!(controller.send_report(&REPORT).is_err());
        assert!(not_connected(controller.send_report(&REPORT)));
        assert_eq!(*s.log.borrow(), [(0, "send"), (1, "open")]);

        let (s, mut controller) = setup(&[DEAD], &[], gap);
        assert!(controller.send_report(&REPORT).is_err());
        assert!(controller.send_report(&REPORT).is_err());
        s.held.set(false);
        controller.send_report(&REPORT).unwrap();
        // A handle that opens is not opened again within the gap either.
        assert_eq!(*s.log.borrow(), [(0, "send"), (1, "open"), (1, "send")]);
    }

    #[test]
    fn a_handle_that_dies_before_any_send_worked_waits_longer() {
        let gap = Duration::from_millis(20);
        let (s, mut controller) = setup(&[DEAD, DEAD], &[], gap);
        assert!(controller.send_report(&REPORT).is_err());
        assert!(controller.send_report(&REPORT).is_err());
        s.held.set(false);
        // The new handle dies at once.
        assert!(controller.send_report(&REPORT).is_err());
        std::thread::sleep(gap * 2);
        assert!(not_connected(controller.send_report(&REPORT)));
        assert_eq!(s.told.borrow().len(), 1, "one gap is not enough");
        std::thread::sleep(gap * DEAD_HANDLE_GAPS);
        assert!(controller.send_report(&REPORT).is_err());
        assert_eq!(*s.told.borrow(), [true, false]);
    }
}
