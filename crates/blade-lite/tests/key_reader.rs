//! The power-mode key reader loop, over a fake source of key reports.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use blade_core::{BRIGHTNESS_READ_GAP, PowerKeyPresses};
use blade_lite::{
    ENTER_READ_WAIT, FnKeys, KEY_QUIET_SPELL, KEY_READ_WAIT, KEY_REOPEN_PAUSE, KeyEvent,
    KeyReportSource, ReaderEnd, step_on_presses,
};
use razer_proto::BLADE_18;

/// Gives its reads in order (`None` is a quiet read), then fails once
/// they run out. Keeps each wait it was given. Reads more than one
/// collection unless made `single`.
struct FakeSource(VecDeque<Option<Vec<u8>>>, Vec<Duration>, bool);

impl KeyReportSource for FakeSource {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.1.push(wait);
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::other("no more reads"))
    }

    fn single(&self) -> bool {
        self.2
    }
}

/// Quiet reads of [`KEY_READ_WAIT`] that make up a quiet spell.
fn idle_reads() -> u32 {
    u32::try_from(KEY_QUIET_SPELL.as_millis() / KEY_READ_WAIT.as_millis()).unwrap()
}

/// Gives its Enter states in order, then up, and counts its Enter reads.
/// The same for the mic keys.
#[derive(Default)]
struct FakeEnter(VecDeque<bool>, usize, VecDeque<bool>, usize);

impl FnKeys for FakeEnter {
    fn enter_down(&mut self) -> bool {
        self.1 += 1;
        self.0.pop_front().unwrap_or(false)
    }

    fn mic_down(&mut self) -> bool {
        self.3 += 1;
        self.2.pop_front().unwrap_or(false)
    }
}

fn enter(states: &[bool]) -> FakeEnter {
    FakeEnter(states.iter().copied().collect(), 0, VecDeque::new(), 0)
}

fn mic(states: &[bool]) -> FakeEnter {
    FakeEnter(VecDeque::new(), 0, states.iter().copied().collect(), 0)
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
    FakeSource(reads.into(), Vec::new(), false)
}

fn single_source(reads: Vec<Option<Vec<u8>>>) -> FakeSource {
    FakeSource(reads.into(), Vec::new(), true)
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

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |event| {
            calls += usize::from(event == KeyEvent::PowerKey);
            true
        },
    );

    assert_eq!(calls, 1);
    assert_eq!(end, ReaderEnd::Failed("no more reads".into()));
}

#[test]
fn the_loop_ends_when_the_app_is_quitting() {
    let mut source = source(vec![Some(report(&[0x04])), Some(report(&[0x04, 0xD3]))]);

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| false,
    );

    assert_eq!(end, ReaderEnd::Quit);
}

#[test]
fn a_long_quiet_spell_reopens_the_reports() {
    let quiet = (0..idle_reads()).map(|_| None).collect();
    let mut source = source(quiet);
    let mut calls = 0;

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |event| {
            calls += usize::from(event == KeyEvent::PowerKey);
            true
        },
    );

    assert_eq!(end, ReaderEnd::Reopen);
    assert_eq!(calls, 0);
    // It did not read past the quiet spell to the failing read.
    assert!(source.0.is_empty());
}

#[test]
fn only_a_quiet_spell_of_full_length_opens_again_at_once() {
    let spell = KEY_QUIET_SPELL;
    assert_eq!(ReaderEnd::Reopen.pause_before_open(spell), Duration::ZERO);
    assert_eq!(
        ReaderEnd::Reopen.pause_before_open(spell / 2),
        Duration::ZERO
    );
    assert_eq!(
        ReaderEnd::Reopen.pause_before_open((spell / 2).saturating_sub(Duration::from_millis(1))),
        KEY_REOPEN_PAUSE
    );
    // Reads that came back at once are a broken handle: pause.
    assert_eq!(
        ReaderEnd::Reopen.pause_before_open(Duration::from_secs(1)),
        KEY_REOPEN_PAUSE
    );
    assert_eq!(
        ReaderEnd::Failed("gone".into()).pause_before_open(spell),
        KEY_REOPEN_PAUSE
    );
}

#[test]
fn a_report_resets_the_quiet_count() {
    let mut reads: Vec<_> = (1..idle_reads()).map(|_| None).collect();
    reads.push(Some(report(&[0x04])));
    reads.extend((1..idle_reads()).map(|_| None));
    let mut source = source(reads);

    // The source fails after its reads, so the loop reached the end.
    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| true,
    );

    assert!(matches!(end, ReaderEnd::Failed(_)), "{end:?}");
    assert!(source.0.is_empty());
}

#[test]
fn enter_pressed_while_fn_is_held_calls_back_once() {
    // Num Lock on: the keyboard sends only Fn, Windows gets the Enter.
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        None,
        None,
        None,
    ]);
    let mut enter = enter(&[false, true, true]);
    let mut calls = 0;

    step_on_presses(&mut source, &mut enter, &mut presses(), |event| {
        calls += usize::from(event == KeyEvent::PowerKey);
        true
    });

    assert_eq!(calls, 1);
}

#[test]
fn enter_is_read_only_while_fn_is_held() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        None,
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        None,
    ]);
    let mut enter = enter(&[]);

    step_on_presses(&mut source, &mut enter, &mut presses(), |_| true);

    // Once, after the Fn report.
    assert_eq!(enter.1, 1);
}

#[test]
fn fn_and_mic_call_back_once_for_the_mic() {
    // Fn + mic: the keyboard sends only Fn, Windows gets Win + Alt + K.
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        None,
        None,
        None,
    ]);
    let mut keys = mic(&[false, true, true]);
    let mut events = Vec::new();

    step_on_presses(&mut source, &mut keys, &mut presses(), |event| {
        events.push(event);
        true
    });

    assert_eq!(
        events
            .iter()
            .filter(|&&event| event != KeyEvent::FnHeld)
            .collect::<Vec<_>>(),
        [&KeyEvent::MicKey]
    );
}

#[test]
fn the_mic_keys_are_read_only_while_fn_is_held() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        None,
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        None,
    ]);
    let mut keys = mic(&[]);

    step_on_presses(&mut source, &mut keys, &mut presses(), |_| true);

    // Once, after the Fn report.
    assert_eq!(keys.3, 1);
}

#[test]
fn the_loop_ends_when_the_app_quits_on_a_mic_press() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        None,
    ]);
    let mut keys = mic(&[false, true]);

    let end = step_on_presses(&mut source, &mut keys, &mut presses(), |event| {
        event != KeyEvent::MicKey
    });

    assert_eq!(end, ReaderEnd::Quit);
}

#[test]
fn reads_wait_less_while_fn_is_held() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
    ]);

    step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| true,
    );

    assert_eq!(
        source.1,
        [KEY_READ_WAIT, KEY_READ_WAIT, ENTER_READ_WAIT, KEY_READ_WAIT]
    );
}

#[test]
fn quiet_reads_while_fn_is_held_do_not_reopen() {
    // Short reads while Fn is held must not end the quiet spell early.
    let mut reads = vec![Some(report(&[0x04])), Some(report(&[0x04, 0x0A]))];
    reads.extend((0..idle_reads()).map(|_| None));
    let mut source = source(reads);

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| true,
    );

    assert!(matches!(end, ReaderEnd::Failed(_)), "{end:?}");
}

#[test]
fn the_loop_ends_when_the_app_quits_on_an_enter_press() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        None,
    ]);
    let mut enter = enter(&[false, true]);

    let end = step_on_presses(&mut source, &mut enter, &mut presses(), |_| false);

    assert_eq!(end, ReaderEnd::Quit);
}

#[test]
fn fn_coming_up_calls_back_once() {
    // Fn + F11: the laptop sends only Fn, and sets the brightness itself.
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        None,
        Some(report(&[0x04])),
    ]);
    let mut events = Vec::new();

    step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |event| {
            events.push(event);
            true
        },
    );

    assert_eq!(events, [KeyEvent::FnUp]);
}

#[test]
fn the_power_key_then_fn_up_call_back_in_order() {
    let mut source = source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A, 0xD3])),
        Some(report(&[0x04])),
    ]);
    let mut events = Vec::new();

    step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |event| {
            events.push(event);
            true
        },
    );

    assert_eq!(events, [KeyEvent::PowerKey, KeyEvent::FnUp]);
}

#[test]
fn the_loop_ends_when_the_app_quits_on_brightness_keys() {
    let mut source = source(vec![
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        Some(report(&[0x04])),
    ]);

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| false,
    );

    assert_eq!(end, ReaderEnd::Quit);
    assert_eq!(source.0.len(), 1);
}

/// Fn down, then quiet reads that take their full wait, so time passes as
/// on the laptop, then Fn up once Fn was held `release_after`, then a
/// failed read.
struct HeldFn {
    reads: u32,
    release_after: Duration,
    fn_down: Option<std::time::Instant>,
    released: bool,
}

impl KeyReportSource for HeldFn {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.reads += 1;
        if self.reads == 1 {
            return Ok(Some(report(&[0x04])));
        }
        let Some(fn_down) = self.fn_down else {
            self.fn_down = Some(std::time::Instant::now());
            return Ok(Some(report(&[0x04, 0x0A])));
        };
        if self.released {
            return Err(io::Error::other("no more reads"));
        }
        if fn_down.elapsed() >= self.release_after {
            self.released = true;
            return Ok(Some(report(&[0x04])));
        }
        std::thread::sleep(wait);
        Ok(None)
    }

    fn single(&self) -> bool {
        false
    }
}

#[test]
fn the_brightness_is_read_while_fn_is_held_then_once_more_on_brightness_keys() {
    let mut source = HeldFn {
        reads: 0,
        release_after: BRIGHTNESS_READ_GAP * 2 + BRIGHTNESS_READ_GAP / 2,
        fn_down: None,
        released: false,
    };
    let mut events = Vec::new();

    step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |event| {
            events.push(event);
            true
        },
    );

    // Real time passes, so a slow runner may fit in one more read.
    let (up, held) = events.split_last().unwrap();
    assert_eq!(*up, KeyEvent::FnUp);
    assert!((2..=3).contains(&held.len()), "{events:?}");
    assert!(held.iter().all(|&event| event == KeyEvent::FnHeld));
}

#[test]
fn only_a_full_quiet_spell_keeps_the_key_state() {
    let spell = KEY_QUIET_SPELL;
    assert!(ReaderEnd::Reopen.keeps_key_state(spell));
    assert!(ReaderEnd::Reopen.keeps_key_state(spell / 2));
    // Quiet reads that did not wait: a broken handle, as a failure.
    let short = (spell / 2).saturating_sub(Duration::from_millis(1));
    assert!(!ReaderEnd::Reopen.keeps_key_state(short));
    assert!(!ReaderEnd::Failed("gone".into()).keeps_key_state(spell));
    assert!(!ReaderEnd::Quit.keeps_key_state(spell));
}

/// Presses after a run that saw the key up and ended as `end` after `ran`,
/// then given `04 D3` (numpad Enter, Num Lock off) as the first report.
fn power_key_calls_after(end: &ReaderEnd, ran: Duration) -> usize {
    power_key_calls_after_seeing(&[0x04], end, ran)
}

/// As [`power_key_calls_after`], with `last` the run's last report.
fn power_key_calls_after_seeing(last: &[u8], end: &ReaderEnd, ran: Duration) -> usize {
    let mut before = presses();
    let mut quiet_run = source(vec![Some(report(last))]);
    step_on_presses(
        &mut quiet_run,
        &mut FakeEnter::default(),
        &mut before,
        |_| true,
    );
    let mut presses = end.next_presses(ran, &before);
    let mut calls = 0;
    step_on_presses(
        &mut source(vec![Some(report(&[0x04, 0xD3]))]),
        &mut FakeEnter::default(),
        &mut presses,
        |event| {
            calls += usize::from(event == KeyEvent::PowerKey);
            true
        },
    );
    calls
}

#[test]
fn the_first_press_after_any_reopen_counts() {
    // A first report listing the key counts even when the key's earlier
    // state is unknown, as after a broken handle or a failure (ticket 174).
    let spell = KEY_QUIET_SPELL;
    assert_eq!(power_key_calls_after(&ReaderEnd::Reopen, spell), 1);
    assert_eq!(power_key_calls_after(&ReaderEnd::Reopen, Duration::ZERO), 1);
    assert_eq!(
        power_key_calls_after(&ReaderEnd::Failed("gone".into()), spell),
        1
    );
}

#[test]
fn a_key_stuck_down_does_not_step_again_after_a_quiet_spell() {
    let spell = KEY_QUIET_SPELL;
    let held = [0x04, 0xD3];
    let reopen = power_key_calls_after_seeing(&held, &ReaderEnd::Reopen, spell);
    assert_eq!(reopen, 0);
    let failed = power_key_calls_after_seeing(&held, &ReaderEnd::Failed("gone".into()), spell);
    assert_eq!(failed, 1);
}

#[test]
fn the_first_press_after_the_app_starts_counts_once() {
    // Numpad Enter (Num Lock off), then Fn + numpad Enter, each the first
    // report of a new key state, as at the app start (check 16).
    for first in [&[0x04, 0xD3][..], &[0x04, 0x0A, 0xD3]] {
        let mut calls = 0;
        step_on_presses(
            &mut source(vec![Some(report(first)), Some(report(&[0x04]))]),
            &mut enter(&[true, true]),
            &mut presses(),
            |event| {
                calls += usize::from(event == KeyEvent::PowerKey);
                true
            },
        );
        assert_eq!(calls, 1, "{first:02X?}");
    }
}

#[test]
fn a_single_collection_waits_out_the_quiet_spell_in_one_read() {
    // One read waits it all: no wake to count quiet reads (ticket 188).
    let mut source = single_source(vec![None]);

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| true,
    );

    assert_eq!(end, ReaderEnd::Reopen);
    assert_eq!(source.1, [KEY_QUIET_SPELL]);
}

#[test]
fn a_single_collection_waits_less_while_fn_is_held_and_the_spell_starts_again() {
    let mut source = single_source(vec![
        Some(report(&[0x04])),
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        None,
    ]);

    let end = step_on_presses(
        &mut source,
        &mut FakeEnter::default(),
        &mut presses(),
        |_| true,
    );

    assert_eq!(end, ReaderEnd::Reopen);
    assert_eq!(
        source.1,
        [
            KEY_QUIET_SPELL,
            KEY_QUIET_SPELL,
            ENTER_READ_WAIT,
            KEY_QUIET_SPELL
        ]
    );
}
