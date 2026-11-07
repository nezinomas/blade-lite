//! Where the app's errors go: stderr for a debug build, and the log file,
//! since a release build has no console.

use std::sync::{Mutex, OnceLock, PoisonError};
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
        eprintln!("blade-lite: {text}");
        // One thread at a time, so a file is never moved aside mid-line.
        // A thread that panicked while holding it left nothing half done.
        let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(log) = log.as_ref()
            && let Err(error) = log.write(SystemTime::now(), text)
        {
            eprintln!("blade-lite: the log file can't be written: {error}");
        }
    }
}

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Sends the app's reports to `log` from now on. Only the first call counts.
pub fn open_log(log: Log) {
    let _ = REPORTER.set(Reporter::new(Some(log)));
}

/// Reports `text` through the app's reporter: stderr, and the log file once
/// [`open_log`] has run.
pub fn report(text: &str) {
    REPORTER.get_or_init(|| Reporter::new(None)).report(text);
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
