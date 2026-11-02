//! Reads the golden reports in `tests/fixtures/`.

use std::fs;
use std::path::PathBuf;

use razer_proto::REPORT_LEN;

/// Who sent a captured report.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `>`: the host sent it.
    Sent,
    /// `<`: the controller replied with it.
    Reply,
}

/// One captured report and the fixture it came from.
#[allow(dead_code)] // Not every test binary reads every field.
pub struct Captured {
    pub fixture: String,
    pub direction: Direction,
    /// The USB interface it went through (`mi=2`).
    pub interface: i32,
    pub bytes: [u8; REPORT_LEN],
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// A fixture line is `> mi=2 <90 hex bytes>` (sent) or `< mi=2 ...` (reply).
fn parse_line(fixture: &str, line: &str) -> Captured {
    let mut fields = line.split_whitespace();
    let direction = match fields.next() {
        Some(">") => Direction::Sent,
        Some("<") => Direction::Reply,
        other => panic!("{fixture}: unknown direction {other:?}"),
    };
    let interface = fields
        .next()
        .and_then(|field| field.strip_prefix("mi="))
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("{fixture}: no mi= field"));
    let bytes: Vec<u8> = fields
        .map(|hex| u8::from_str_radix(hex, 16).unwrap())
        .collect();
    Captured {
        fixture: fixture.to_owned(),
        direction,
        interface,
        bytes: bytes.try_into().unwrap(),
    }
}

/// Every report in every fixture.
pub fn captured_reports() -> Vec<Captured> {
    let mut reports = Vec::new();
    for entry in fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let fixture = path.file_name().unwrap().to_string_lossy().into_owned();
        for line in fs::read_to_string(&path).unwrap().lines() {
            reports.push(parse_line(&fixture, line));
        }
    }
    assert!(!reports.is_empty(), "no fixtures found");
    reports
}

/// Only the reports that went one way.
fn captured(direction: Direction) -> Vec<Captured> {
    captured_reports()
        .into_iter()
        .filter(|report| report.direction == direction)
        .collect()
}

/// Only the controller's replies.
#[allow(dead_code)] // Not every test binary uses it.
pub fn captured_replies() -> Vec<Captured> {
    captured(Direction::Reply)
}

/// Only the host's requests.
#[allow(dead_code)] // Not every test binary uses it.
pub fn captured_requests() -> Vec<Captured> {
    captured(Direction::Sent)
}
