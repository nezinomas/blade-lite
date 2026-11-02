use std::fs;
use std::path::PathBuf;

use razer_proto::{CHECKSUM_INDEX, REPORT_LEN, checksum};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// A fixture line is `> mi=2 <90 hex bytes>` (sent) or `< mi=2 ...` (reply).
fn parse_report(line: &str) -> [u8; REPORT_LEN] {
    let bytes: Vec<u8> = line
        .split_whitespace()
        .skip(2)
        .map(|hex| u8::from_str_radix(hex, 16).unwrap())
        .collect();
    bytes.try_into().unwrap()
}

fn captured_reports() -> Vec<(String, [u8; REPORT_LEN])> {
    let mut reports = Vec::new();
    for entry in fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for line in fs::read_to_string(&path).unwrap().lines() {
            reports.push((name.clone(), parse_report(line)));
        }
    }
    reports
}

#[test]
fn checksum_of_every_captured_report_matches_its_checksum_byte() {
    let reports = captured_reports();
    assert!(!reports.is_empty(), "no fixtures found");

    for (fixture, report) in &reports {
        assert_eq!(
            checksum(report),
            report[CHECKSUM_INDEX],
            "{fixture}: {report:02X?}"
        );
    }
}
