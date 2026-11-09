//! Where the app's errors go: stderr for a debug build, and the log file,
//! since a release build has no console.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};
use std::time::{Duration, Instant, SystemTime};

use blade_core::Log;

/// Writes each report to stderr (unless quiet) and, if there is one, the
/// log file.
#[derive(Debug)]
pub struct Reporter {
    log: Mutex<Option<Log>>,
    stderr: bool,
}

impl Reporter {
    /// A reporter for `log`. `None` reports to stderr only.
    #[must_use]
    pub fn new(log: Option<Log>) -> Self {
        Self {
            log: Mutex::new(log),
            stderr: true,
        }
    }

    /// A reporter for `log` that writes nothing to stderr, for tests: the
    /// test harness does not capture what is written there. `None` reports
    /// nothing at all.
    #[must_use]
    pub fn quiet(log: Option<Log>) -> Self {
        Self {
            log: Mutex::new(log),
            stderr: false,
        }
    }

    /// Reports `text`. A log file that can't be written is said on stderr
    /// (unless quiet) and never stops the caller.
    pub fn report(&self, text: &str) {
        // One thread at a time, so a file is never moved aside mid-line.
        // A thread that panicked while holding it left nothing half done.
        let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        write(log.as_ref(), text, self.stderr);
    }

    /// Reports `text`, but to stderr only if another report is under way,
    /// such as one that panicked: waiting for it could hang.
    fn report_if_free(&self, text: &str) {
        match self.log.try_lock() {
            Ok(log) => write(log.as_ref(), text, self.stderr),
            Err(TryLockError::Poisoned(poisoned)) => {
                write(poisoned.into_inner().as_ref(), text, self.stderr);
            }
            Err(TryLockError::WouldBlock) => write(None, text, self.stderr),
        }
    }
}

/// Writes `text` to `log`, and to stderr if `stderr`. Never panics: a closed
/// stderr is ignored, and a log that can't be written is said on stderr.
fn write(log: Option<&Log>, text: &str, stderr: bool) {
    let say = |line: &str| {
        if stderr {
            let _ = writeln!(io::stderr(), "blade-lite: {line}");
        }
    };
    say(text);
    if let Some(log) = log
        && let Err(error) = log.write(SystemTime::now(), text)
    {
        say(&format!("the log file can't be written: {error}"));
    }
}

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Sends the app's reports to `log` from now on. Only the first call
/// counts, and only if no report came before it: false if it was too late.
pub fn open_log(log: Log) -> bool {
    REPORTER.set(Reporter::new(Some(log))).is_ok()
}

/// As [`open_log`], but nothing goes to stderr. For the window tests only;
/// the app uses [`open_log`].
pub fn open_quiet_log(log: Log) -> bool {
    REPORTER.set(Reporter::quiet(Some(log))).is_ok()
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

/// Errors that can come and go on each read, such as a busy fan read, so
/// each one is said once, then not again until a quiet time has passed.
#[derive(Debug)]
pub struct SaidLately {
    quiet: Duration,
    said: HashMap<String, Instant>,
}

impl SaidLately {
    #[must_use]
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            said: HashMap::new(),
        }
    }

    /// The errors not said in the quiet time before `now`. They count as
    /// said from `now` on. Two errors that differ only in their details
    /// count as one.
    pub fn to_say<'a>(&mut self, errors: &'a [String], now: Instant) -> Vec<&'a str> {
        // Forget the old ones, so the map stays small.
        self.said
            .retain(|_, &mut at| now.saturating_duration_since(at) < self.quiet);
        let mut new = Vec::new();
        for error in errors {
            let kind = kind(error);
            if !self.said.contains_key(kind) {
                self.said.insert(kind.to_owned(), now);
                new.push(error.as_str());
            }
        }
        new
    }
}

/// The error without its details in braces or brackets, such as a
/// transaction ID, since they can change on each read.
fn kind(error: &str) -> &str {
    error.find(['{', '(']).map_or(error, |at| &error[..at])
}
