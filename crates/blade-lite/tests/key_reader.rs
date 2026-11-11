//! The power-mode key reader loop, over a fake source of key reports.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use blade_core::{BRIGHTNESS_READ_GAP, PowerKeyPresses};
use blade_lite::{
    ENTER_READ_WAIT, FnKeys, KEY_IDLE_READS, KEY_READ_WAIT, KeyEvent, KeyReportSource, ReaderEnd,
    step_on_presses,
};
use razer_proto::BLADE_18;

/// Gives its reads in order (`None` is a quiet read), then fails once
/// they run out. Keeps each wait it was given.
struct FakeSource(VecDeque<Option<Vec<u8>>>, Vec<Duration>);

impl KeyReportSource for FakeSource {
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.1.push(wait);
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::other("no more reads"))
    }
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
    FakeSource(reads.into(), Vec::new())
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

    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |event| {
        calls += usize::from(event == KeyEvent::PowerKey);
        true
    });

    assert_eq!(calls, 1);
    assert_eq!(end, ReaderEnd::Failed("no more reads".into()));
}

#[test]
fn the_loop_ends_when_the_app_is_quitting() {
    let mut source = source(vec![Some(report(&[0x04])), Some(report(&[0x04, 0xD3]))]);

    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |_| false);

    assert_eq!(end, ReaderEnd::Quit);
}

#[test]
fn a_long_quiet_spell_reopens_the_reports() {
    let quiet = (0..KEY_IDLE_READS).map(|_| None).collect();
    let mut source = source(quiet);
    let mut calls = 0;

    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |event| {
        calls += usize::from(event == KeyEvent::PowerKey);
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
    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |_| true);

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

    step_on_presses(&mut source, &mut enter, presses(), |event| {
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

    step_on_presses(&mut source, &mut enter, presses(), |_| true);

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

    step_on_presses(&mut source, &mut keys, presses(), |event| {
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

    step_on_presses(&mut source, &mut keys, presses(), |_| true);

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

    let end = step_on_presses(&mut source, &mut keys, presses(), |event| {
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

    step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |_| true);

    assert_eq!(
        source.1,
        [KEY_READ_WAIT, KEY_READ_WAIT, ENTER_READ_WAIT, KEY_READ_WAIT]
    );
}

#[test]
fn quiet_reads_while_fn_is_held_do_not_reopen() {
    // Short reads while Fn is held must not end the quiet spell early.
    let mut reads = vec![Some(report(&[0x04])), Some(report(&[0x04, 0x0A]))];
    reads.extend((0..KEY_IDLE_READS).map(|_| None));
    let mut source = source(reads);

    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |_| true);

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

    let end = step_on_presses(&mut source, &mut enter, presses(), |_| false);

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

    step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |event| {
        events.push(event);
        true
    });

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

    step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |event| {
        events.push(event);
        true
    });

    assert_eq!(events, [KeyEvent::PowerKey, KeyEvent::FnUp]);
}

#[test]
fn the_loop_ends_when_the_app_quits_on_brightness_keys() {
    let mut source = source(vec![
        Some(report(&[0x04, 0x0A])),
        Some(report(&[0x04])),
        Some(report(&[0x04])),
    ]);

    let end = step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |_| false);

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

    step_on_presses(&mut source, &mut FakeEnter::default(), presses(), |event| {
        events.push(event);
        true
    });

    // Real time passes, so a slow runner may fit in one more read.
    let (up, held) = events.split_last().unwrap();
    assert_eq!(*up, KeyEvent::FnUp);
    assert!((2..=3).contains(&held.len()), "{events:?}");
    assert!(held.iter().all(|&event| event == KeyEvent::FnHeld));
}
