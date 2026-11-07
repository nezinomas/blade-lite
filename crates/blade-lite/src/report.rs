//! Where the app's errors go: stderr for a debug build, and the log file,
//! since a release build has no console.

use std::io::{self, Write};
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};
use std::time::SystemTime;

use blade_core::Log;

/// Writes each report to stderr and, if there is one, the log file.
#[derive(Debug)]
pub struct Reporter {
    log: Mutex<Option<Log>>,
}

impl Reporter {
    /// A reporter for `log`. `None` reports to stderr only.
    #[must_use]
    pub fn new(log: Option<Log>) -> Self {
        Self {
            log: Mutex::new(log),
        }
    }

    /// Reports `text`. A log file that can't be written is said on stderr
    /// and never stops the caller.
    pub fn report(&self, text: &str) {
        // One thread at a time, so a file is never moved aside mid-line.
        // A thread that panicked while holding it left nothing half done.
        let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        write(log.as_ref(), text);
    }

    /// Reports `text`, but to stderr only if another report is under way,
    /// such as one that panicked: waiting for it could hang.
    fn report_if_free(&self, text: &str) {
        match self.log.try_lock() {
            Ok(log) => write(log.as_ref(), text),
            Err(TryLockError::Poisoned(poisoned)) => write(poisoned.into_inner().as_ref(), text),
            Err(TryLockError::WouldBlock) => write(None, text),
        }
    }
}

/// Writes `text` to stderr and `log`. Never panics: a closed stderr is
/// ignored, and a log that can't be written is said on stderr.
fn write(log: Option<&Log>, text: &str) {
    let _ = writeln!(io::stderr(), "blade-lite: {text}");
    if let Some(log) = log
        && let Err(error) = log.write(SystemTime::now(), text)
    {
        let _ = writeln!(
            io::stderr(),
            "blade-lite: the log file can't be written: {error}"
        );
    }
}

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Sends the app's reports to `log` from now on. Only the first call
/// counts, and only if no report came before it: false if it was too late.
pub fn open_log(log: Log) -> bool {
    REPORTER.set(Reporter::new(Some(log))).is_ok()
}

/// Reports each panic too, then lets Rust say it as before. A release build
/// has no console, so a panic would otherwise leave no trace.
pub fn report_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reporter().report_if_free(&format!("panic: {info}"));
        before(info);
    }));
}

fn reporter() -> &'static Reporter {
    REPORTER.get_or_init(|| Reporter::new(None))
}

/// Reports `text` through the app's reporter: stderr, and the log file once
/// [`open_log`] has run.
pub fn report(text: &str) {
    reporter().report(text);
}

/// The error text the window shows, so each one is logged once, not on each
/// refresh.
#[derive(Debug, Default)]
pub struct ShownErrors {
    last: String,
}

impl ShownErrors {
    /// `text` if it is an error that was not shown just before, else `None`.
    pub fn new_text<'a>(&mut self, text: &'a str) -> Option<&'a str> {
        if text == self.last {
            return None;
        }
        text.clone_into(&mut self.last);
        (!text.is_empty()).then_some(text)
    }
}
