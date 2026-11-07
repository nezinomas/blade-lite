//! A small rolling log file in `%APPDATA%`, since a release build has no
//! console to show errors on.
//!
//! Nothing leaves the PC: the log is only written to disk, and the oldest
//! file is deleted once there are [`LOG_FILES`] of them.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::config::APP_DIR;

/// The log file's name in the app's folder.
const FILE_NAME: &str = "blade-lite.log";
/// A file is moved aside once one more line would make it bigger than this.
/// One line is at most about 4 KB, so only a file of one line can pass it.
pub const LOG_MAX_BYTES: u64 = 1024 * 1024;
/// How many files are kept: the log itself, then `.log.1` (the newest old
/// one) up to `.log.4`.
pub const LOG_FILES: usize = 5;
/// The most characters one line holds. A longer text is cut.
pub const LOG_LINE_MAX: usize = 1024;

const SECONDS_PER_DAY: u64 = 86_400;

/// The rolling log file.
#[derive(Debug)]
pub struct Log {
    path: PathBuf,
    max_bytes: u64,
    files: usize,
}

impl Log {
    /// The log's path for an `%APPDATA%` folder, beside the config.
    #[must_use]
    pub fn path_in(appdata: &Path) -> PathBuf {
        appdata.join(APP_DIR).join(FILE_NAME)
    }

    /// A log at `path` with the planned limits.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self::with_limits(path, LOG_MAX_BYTES, LOG_FILES)
    }

    /// A log with other limits. At least one file is kept.
    #[must_use]
    pub fn with_limits(path: PathBuf, max_bytes: u64, files: usize) -> Self {
        Self {
            path,
            max_bytes,
            files: files.max(1),
        }
    }

    /// Adds one line for `text`, stamped with `at`. Moves the file aside
    /// first if the line would make it too big.
    ///
    /// # Errors
    ///
    /// The I/O error if the folder or file can't be made or written.
    pub fn write(&self, at: SystemTime, text: &str) -> io::Result<()> {
        let line = log_line(at, text);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Two copies of the app could both roll at once. That leaves a gap
        // in the numbers, never more files, so it is not guarded against.
        if self.is_full_for(line.len() as u64)?
            && let Err(error) = self.roll()
        {
            // A file another program holds open can stop the roll. Starting
            // the log again keeps it small and keeps it going.
            let note = log_line(at, &format!("the log could not be rolled: {error}"));
            return self.open(true)?.write_all((note + &line).as_bytes());
        }
        self.open(false)?.write_all(line.as_bytes())
    }

    /// Opens the log to add lines, emptied first if `empty`.
    fn open(&self, empty: bool) -> io::Result<fs::File> {
        let mut options = OpenOptions::new();
        if empty {
            options.write(true).truncate(true);
        } else {
            options.append(true);
        }
        options.create(true).open(&self.path)
    }

    /// Whether the file holds lines and one more of `len` bytes won't fit.
    fn is_full_for(&self, len: u64) -> io::Result<bool> {
        match fs::metadata(&self.path) {
            Ok(meta) => Ok(meta.len() > 0 && meta.len().saturating_add(len) > self.max_bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Drops the oldest file and moves each other one up by one.
    fn roll(&self) -> io::Result<()> {
        let oldest = self.files - 1;
        if oldest == 0 {
            return remove_if_there(&self.path);
        }
        remove_if_there(&self.old(oldest))?;
        for n in (1..oldest).rev() {
            rename_if_there(&self.old(n), &self.old(n + 1))?;
        }
        rename_if_there(&self.path, &self.old(1))
    }

    /// The path of old file `n`, such as `blade-lite.log.1`.
    fn old(&self, n: usize) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn rename_if_there(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// One log line: the UTC time, the text on one line and cut to
/// [`LOG_LINE_MAX`] characters, then a newline.
#[must_use]
pub fn log_line(at: SystemTime, text: &str) -> String {
    let body: String = text
        .chars()
        .take(LOG_LINE_MAX)
        .map(|c| if breaks_a_line(c) { ' ' } else { c })
        .collect();
    format!("{} {body}\n", utc_stamp(at))
}

/// Whether `c` could split a line or turn its text round in a viewer.
fn breaks_a_line(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `at` as `YYYY-MM-DD hh:mm:ssZ`. A time before 1970 shows as 1970.
fn utc_stamp(at: SystemTime) -> String {
    let seconds = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (year, month, day) = civil_date(seconds / SECONDS_PER_DAY);
    let time = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

/// The Gregorian date of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, for days on or after the epoch).
fn civil_date(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
