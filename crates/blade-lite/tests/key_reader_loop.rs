//! The power-mode key reader's state over its opens (ticket 277): the
//! reopen, the quiet spell, the read-all spell and the failure paths, over
//! a fake opener of key reports.

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use blade_core::PowerKeyPresses;
use blade_lite::{
    HeardReports, KEY_QUIET_SPELL, KEY_REOPEN_PAUSE, KeyReaderLoop, KeyReportOpener,
    KeyReportSource, OpenedKeyReports, ReaderEnd, UNHEARD_SPELLS_BEFORE_ALL,
};
use razer_device::DroppedReport;
use razer_proto::BLADE_18;

/// A key report with the power-mode key down.
fn key_down() -> Vec<u8> {
    let mut report = vec![0; 16];
    report[..2].copy_from_slice(&[0x04, 0xD3]);
    report
}

/// Key reports as opened: each read gives the key down; they name a
/// sender, a read path and drops as set. A path to `read_on` is the read
/// path once a report is read, as when one of many collections sends.
#[derive(Default)]
struct FakeReports {
    sender: Option<String>,
    path: Option<String>,
    read_on: Option<String>,
    dropped: Vec<DroppedReport>,
}

impl KeyReportSource for FakeReports {
    fn read(&mut self, _wait: Duration) -> io::Result<Option<Vec<u8>>> {
        if let Some(path) = self.read_on.take() {
            self.path = Some(path);
        }
        Ok(Some(key_down()))
    }

    fn single(&self) -> bool {
        true
    }
}

impl OpenedKeyReports for FakeReports {
    fn sender(&self) -> Option<&str> {
        self.sender.as_deref()
    }

    fn read_path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    fn take_dropped(&mut self) -> Vec<DroppedReport> {
        std::mem::take(&mut self.dropped)
    }
}

/// Keeps each open asked for ("all", or the sender preferred), and gives
/// the reports `next` makes, or an error once `fails` is set.
#[derive(Default)]
struct FakeOpener {
    opens: Vec<String>,
    fails: Option<String>,
    next: FakeReports,
}

impl FakeOpener {
    fn open(&mut self, what: String) -> io::Result<FakeReports> {
        self.opens.push(what);
        if let Some(error) = &self.fails {
            return Err(io::Error::other(error.clone()));
        }
        Ok(FakeReports {
            sender: self.next.sender.clone(),
            path: self.next.path.clone(),
            read_on: self.next.read_on.clone(),
            dropped: self.next.dropped.clone(),
        })
    }
}

impl KeyReportOpener for FakeOpener {
    type Reports = FakeReports;

    fn open_all(&mut self) -> io::Result<FakeReports> {
        self.open("all".to_owned())
    }

    fn open_preferring(&mut self, sender: Option<&str>) -> io::Result<FakeReports> {
        self.open(format!("preferring {sender:?}"))
    }
}

/// One run of reads on an open: how it ends, how long it ran, and whether
/// a key report was heard in it.
struct Run(ReaderEnd, Duration, bool);

const SPELL: Duration = KEY_QUIET_SPELL;

fn quiet(heard: bool) -> Run {
    Run(ReaderEnd::QuietSpell, SPELL, heard)
}

fn quit() -> Run {
    Run(ReaderEnd::Quit, Duration::ZERO, false)
}

fn failed(error: &str) -> Run {
    Run(ReaderEnd::Failed(error.to_owned()), SPELL, false)
}

fn short() -> Run {
    Run(ReaderEnd::QuietSpell, Duration::from_secs(1), false)
}

/// The reader, its fake opener, the runs still to come, and what it saw:
/// the lines said, and for each run whether the key down was a press (a
/// fresh key state: true; one kept over a quiet spell: false, as the key
/// was down at its end).
struct Harness {
    reader: KeyReaderLoop,
    opener: FakeOpener,
    runs: VecDeque<Run>,
    said: Vec<String>,
    pressed: Vec<bool>,
}

impl Harness {
    fn new(runs: impl IntoIterator<Item = Run>) -> Self {
        Self {
            reader: KeyReaderLoop::new(BLADE_18.power_key.unwrap()),
            opener: FakeOpener::default(),
            runs: runs.into_iter().collect(),
            said: Vec::new(),
            pressed: Vec::new(),
        }
    }

    /// One open and its runs: the pause before the next open, `None` once
    /// the app is quitting.
    fn open_and_read(&mut self) -> Option<Duration> {
        let Self {
            reader,
            opener,
            runs,
            said,
            pressed,
        } = self;
        reader.open_and_read(
            opener,
            |reports: &mut HeardReports<FakeReports>, presses: &mut PowerKeyPresses| {
                let Run(end, ran, heard) = runs.pop_front().expect("a run is scripted");
                pressed.push(presses.on_report(&key_down(), Instant::now()));
                if heard {
                    reports.read(Duration::ZERO).unwrap();
                }
                (end, ran)
            },
            |line| said.push(line.to_owned()),
        )
    }
}

#[test]
fn a_quiet_spell_reads_on_on_the_same_handle_and_keeps_the_key() {
    let mut harness = Harness::new([quiet(true), quiet(false), quit()]);

    assert_eq!(harness.open_and_read(), None);

    assert_eq!(harness.opener.opens, ["preferring None"]);
    assert_eq!(harness.pressed, [true, false, false]);
    assert!(harness.said.is_empty(), "{:?}", harness.said);
}

#[test]
fn a_failure_is_said_once_pauses_and_starts_the_key_and_sender_anew() {
    let mut harness = Harness::new([quiet(true), failed("gone"), failed("gone"), quit()]);
    harness.opener.next.sender = Some("col02".to_owned());

    assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.opener.opens,
        ["preferring None", "preferring None", "preferring None"]
    );
    assert_eq!(harness.pressed, [true, false, true, true]);
    assert_eq!(harness.said, ["the key reports stopped: gone"]);
}

#[test]
fn a_short_run_pauses_says_nothing_and_starts_the_key_anew() {
    let mut harness = Harness::new([short(), quit()]);
    harness.opener.next.sender = Some("col02".to_owned());

    assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(harness.opener.opens, ["preferring None", "preferring None"]);
    assert_eq!(harness.pressed, [true, true]);
    assert!(harness.said.is_empty(), "{:?}", harness.said);
}

#[test]
fn a_failed_open_is_said_once_pauses_and_starts_the_key_and_sender_anew() {
    // An hour unheard ends in a reopen that keeps the key and the sender.
    let mut harness = Harness::new(unheard_hour().chain([quit()]));
    harness.opener.next.sender = Some("col02".to_owned());
    assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    harness.opener.fails = Some("no keyboard".to_owned());

    assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    harness.opener.fails = None;
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.opener.opens[1..],
        ["all", "preferring None", "preferring None"]
    );
    assert_eq!(
        harness.said.last().map(String::as_str),
        Some("no power-mode key for now: no keyboard")
    );
    assert_eq!(harness.said.len(), 2, "{:?}", harness.said);
    assert_eq!(harness.pressed.last(), Some(&true));
}

/// The runs of an hour of quiet spells with no key report heard.
fn unheard_hour() -> impl Iterator<Item = Run> {
    (0..UNHEARD_SPELLS_BEFORE_ALL).map(|_| quiet(false))
}

#[test]
fn an_hour_unheard_reads_all_for_one_spell_then_the_sender_alone() {
    let runs = unheard_hour().chain([quiet(true), quit()]);
    let mut harness = Harness::new(runs);
    harness.opener.next.sender = Some("col03".to_owned());

    assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.opener.opens,
        ["preferring None", "all", "preferring Some(\"col03\")"]
    );
    // The key state is kept over each reopen after a quiet spell.
    assert!(harness.pressed.iter().skip(1).all(|pressed| !pressed));
    assert_eq!(
        harness.said,
        [format!(
            "no key report in {UNHEARD_SPELLS_BEFORE_ALL} quiet spells: \
             all key collections are read for one spell"
        )]
    );
}

#[test]
fn the_read_all_spell_is_said_once_a_run() {
    // The read-all spell's own unheard spell counts as the first of the
    // next hour.
    let runs = unheard_hour()
        .chain([quiet(false)])
        .chain(unheard_hour().skip(1))
        .chain([quit()]);
    let mut harness = Harness::new(runs);

    for _ in 0..3 {
        assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    }
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.opener.opens,
        ["preferring None", "all", "preferring None", "all"]
    );
    assert_eq!(harness.said.len(), 1, "{:?}", harness.said);
}

#[test]
fn the_read_path_is_said_once_each_time_it_changes() {
    let mut harness = Harness::new([failed("gone"), failed("gone"), quit()]);
    harness.opener.next.path = Some("col04".to_owned());

    harness.open_and_read();
    harness.open_and_read();
    harness.opener.next.path = Some("col05".to_owned());
    harness.open_and_read();

    assert_eq!(
        harness.said,
        [
            "the power-mode key is read on col04",
            "the key reports stopped: gone",
            "the power-mode key is read on col05",
        ]
    );
}

#[test]
fn a_drop_is_said_once_over_reopens() {
    let mut harness = Harness::new([failed("gone"), quit()]);
    harness.opener.next.dropped = vec![DroppedReport {
        id: 5,
        len: 16,
        path: "col04".to_owned(),
    }];

    harness.open_and_read();
    harness.open_and_read();

    let drops = harness
        .said
        .iter()
        .filter(|line| line.contains("not a key report"))
        .count();
    assert_eq!(drops, 1, "{:?}", harness.said);
}

#[test]
fn the_last_sender_is_kept_when_the_read_all_spell_hears_none() {
    let mut harness = Harness::new(unheard_hour().chain([quiet(false), quit()]));
    harness.opener.next.sender = Some("col03".to_owned());

    assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    harness.opener.next.sender = None;
    assert_eq!(harness.open_and_read(), Some(Duration::ZERO));
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.opener.opens,
        ["preferring None", "all", "preferring Some(\"col03\")"]
    );
}

#[test]
fn the_read_path_is_said_when_a_collection_sends_on_the_same_open() {
    let mut harness = Harness::new([quiet(true), quit()]);
    harness.opener.next.read_on = Some("col04".to_owned());

    assert_eq!(harness.open_and_read(), None);

    assert_eq!(harness.opener.opens.len(), 1);
    assert_eq!(harness.said, ["the power-mode key is read on col04"]);
}

#[test]
fn a_failure_is_said_again_after_a_run_that_ended_without_one() {
    let mut harness = Harness::new([failed("gone"), short(), failed("gone"), quit()]);

    for _ in 0..3 {
        assert_eq!(harness.open_and_read(), Some(KEY_REOPEN_PAUSE));
    }
    assert_eq!(harness.open_and_read(), None);

    assert_eq!(
        harness.said,
        [
            "the key reports stopped: gone",
            "the key reports stopped: gone"
        ]
    );
}
