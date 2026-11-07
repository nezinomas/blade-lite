//! The power-mode key reader loop, over a fake source of key reports.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use blade_core::PowerKeyPresses;
use blade_lite::{KEY_IDLE_READS, KeyReportSource, ReaderEnd, step_on_presses};
use razer_proto::BLADE_18;

/// Gives its reads in order (`None` is a quiet read), then fails once
/// they run out.
struct FakeSource(VecDeque<Option<Vec<u8>>>);

impl KeyReportSource for FakeSource {
    fn read(&mut self, _wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::other("no more reads"))
    }
}

fn report(bytes: &[u8]) -> Vec<u8> {
    let mut report = vec![0; 16];
    report[..bytes.len()].copy_from_slice(bytes);
    report
}

fn presses() -> PowerKeyPresses {
    PowerKeyPresses::new(BLADE_18.power_key.unwrap())
}

fn source(reads: Vec<Option<Vec<u8>>>) -> FakeSource {
    FakeSource(reads.into())
}

#[test]
fn a_press_calls_back_once_then_a_failed_read_reopens() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        None,
        Some(report(&[0x04, 0x0A, 0xD3])),
        Some(report(&[0x04])),
    ]);
    let mut calls = 0;

    let end = step_on_presses(&mut source, presses(), || {
        calls += 1;
        true
    });

    assert_eq!(calls, 1);
    assert_eq!(end, ReaderEnd::Reopen);
}

#[test]
fn the_loop_ends_when_the_app_is_quitting() {
    let mut source = source(vec![Some(report(&[0x04])), Some(report(&[0x04, 0xD3]))]);

    let end = step_on_presses(&mut source, presses(), || false);

    assert_eq!(end, ReaderEnd::Quit);
}

#[test]
fn a_long_quiet_spell_reopens_the_reports() {
    let quiet = (0..KEY_IDLE_READS).map(|_| None).collect();
    let mut source = source(quiet);
    let mut calls = 0;

    let end = step_on_presses(&mut source, presses(), || {
        calls += 1;
        true
    });

    assert_eq!(end, ReaderEnd::Reopen);
    assert_eq!(calls, 0);
    // It did not read past the quiet spell to the failing read.
    assert!(source.0.is_empty());
}

#[test]
fn a_report_resets_the_quiet_count() {
    let mut reads: Vec<_> = (1..KEY_IDLE_READS).map(|_| None).collect();
    reads.push(Some(report(&[0x04])));
    reads.extend((1..KEY_IDLE_READS).map(|_| None));
    let mut source = source(reads);

    // The source fails after its reads, so the loop reached the end.
    let end = step_on_presses(&mut source, presses(), || true);

    assert_eq!(end, ReaderEnd::Reopen);
    assert!(source.0.is_empty());
}
