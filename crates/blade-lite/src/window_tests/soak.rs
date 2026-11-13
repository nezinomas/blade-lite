//! A long run over the fakes with mock time (ticket 147): many thousand
//! one-second ticks with random sleeps, charger swaps, opens again, failed
//! sends, clicks and heat. After each step the rules of ticket 146 hold
//! for what was sent, and the lists the shell keeps stay small. A usual
//! day's set commands are counted too, for the flash wear question.

use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Lighting, Preset};
use i_slint_backend_testing::AccessibleRole::{self, Button, Switch};
use i_slint_backend_testing::ElementHandle;
use platform::PowerEvent;
use razer_proto::{Charger, Command, Rgb};
use slint::platform::PointerEventButton;

use super::{
    FAKE_MIC, FAKE_ON_BATTERY, FAKE_REOPENED, FAKE_SOURCE_UNKNOWN, Fixture, fixture,
    selected_preset,
};
use crate::fakes::{FakeCharger, FakeController};
use crate::rules::{
    SET_BOOST, SET_FAN, SET_PRESET, VARIABLE_STORAGE, Zones, assert_battery_safe, assert_in_range,
};
use crate::shell::Reopened;

/// The set commands with a storage byte first (the logo's is 0).
const STORED: [u16; 5] = [SET_PRESET, SET_FAN, SET_BOOST, 0x0303, 0x0300];

const SECOND: Duration = Duration::from_secs(1);

/// The preset buttons, as the window labels them.
const PRESETS: [&str; 4] = ["Silent", "Balanced", "Performance", "Custom"];

/// A small seeded random source, so a failing run can be run again.
struct Rng(u64);

impl Rng {
    /// A number below `n` (xorshift64).
    fn below(&mut self, n: u16) -> u16 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        u16::try_from(self.0 % u64::from(n)).expect("below n")
    }

    /// One of `items`.
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let n = u16::try_from(items.len()).expect("a short list");
        items[usize::from(self.below(n))]
    }

    fn chance(&mut self, per_thousand: u16) -> bool {
        self.below(1000) < per_thousand
    }
}

/// What the app may know of a weak charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Weak {
    No,
    /// Plugged in, and no charger read came since.
    Unread,
    /// Plugged in, and read.
    Read,
}

/// The shell over fakes, the world around it, and the mock clocks.
struct Soak {
    f: Fixture,
    power: Sender<PowerEvent>,
    now: Instant,
    wall: SystemTime,
    rng: Rng,
    asleep: bool,
    weak: Weak,
    /// Steps left of a spell of failed sends.
    failing_left: u32,
    /// Every command sent so far, as the fake's list is emptied each step.
    sent: Vec<Command>,
    zones: Zones,
    /// Whether a run may have failed partway since the last check.
    may_cut: bool,
    /// Steps left of a spell of failed reads.
    reads_failing_left: u32,
    seen: Seen,
}

/// How often the run met each case, so a run that misses them fails.
#[derive(Debug, Default)]
struct Seen {
    clicks: u32,
    failed_clicks: u32,
    sleeps: u32,
    opens_again: u32,
    weak_reads: u32,
    battery_sends: u32,
    read_spells: u32,
}

impl Soak {
    fn new(config: Config, seed: u64) -> Self {
        let f = fixture(config);
        let (power, events) = channel();
        f.shell.listen_for_power(events);
        fake(&f, |c| c.cpu_celsius = Some(50));
        Self {
            f,
            power,
            now: Instant::now(),
            wall: SystemTime::now(),
            rng: Rng(seed),
            asleep: false,
            weak: Weak::No,
            failing_left: 0,
            sent: Vec::new(),
            zones: Zones::default(),
            may_cut: false,
            reads_failing_left: 0,
            seen: Seen::default(),
        }
    }

    /// One second of the world: maybe one random event, then a tick.
    fn random_step(&mut self) {
        let roll = self.rng.below(1000);
        let mut clicked = None;
        match roll {
            0..=2 => self.sleep_or_wake(),
            _ if self.asleep => {}
            3..=6 => self.swap_battery(),
            7..=9 => self.swap_weak(),
            10..=11 => self.open_again(),
            12..=16 => self.fail_some_runs(),
            17..=36 => clicked = self.click_something(),
            37..=46 => self.change_heat(),
            47..=49 => FAKE_MIC.set(self.rng.pick(&[None, Some(false), Some(true)])),
            50..=52 => self.fail_some_reads(),
            _ => {}
        }
        self.second();
        self.check(clicked);
    }

    /// One second passes: the Slint timers (the pauses) run, then a tick,
    /// unless the laptop sleeps: Windows freezes the app then, and only the
    /// wall clock goes on.
    fn second(&mut self) {
        self.wall += SECOND;
        // A run cut partway, or a closed handle that fails the second run
        // of the quit preset, can leave a send half done.
        self.may_cut |= fake(&self.f, |c| {
            c.fails_after.is_some() || c.not_connected_runs > 0
        });
        if !self.asleep {
            self.now += SECOND;
            i_slint_backend_testing::mock_elapsed_time(SECOND);
            self.f.shell.tick_at(self.now, self.wall);
        }
        if self.failing_left > 0 {
            self.failing_left -= 1;
            if self.failing_left == 0 {
                fake(&self.f, |c| {
                    c.fails = false;
                    c.fails_after = None;
                });
            }
        }
        if self.reads_failing_left > 0 {
            self.reads_failing_left -= 1;
            if self.reads_failing_left == 0 {
                self.reads_work_again();
            }
        }
    }

    /// A spell of failed reads: the charger (only with the Razer one in, so
    /// a weak charge stays known), the lid logo, the firmware, or the power
    /// source (unknown: the battery presets run).
    fn fail_some_reads(&mut self) {
        let kind = self.rng.below(4);
        let weak = self.weak;
        fake(&self.f, |c| match kind {
            0 if weak == Weak::No => c.charger = FakeCharger::Fails,
            1 => c.logo_reply = None,
            2 => c.firmware = None,
            _ => FAKE_SOURCE_UNKNOWN.set(true),
        });
        self.reads_failing_left = self
            .reads_failing_left
            .max(1 + u32::from(self.rng.below(60)));
        self.seen.read_spells += 1;
    }

    fn reads_work_again(&self) {
        fake(&self.f, |c| {
            if matches!(c.charger, FakeCharger::Fails) {
                c.charger = FakeCharger::Razer;
            }
            // The laptop turns the logo on again by itself after a wake.
            c.logo_reply.get_or_insert_with(|| vec![0, 4, 1]);
            c.firmware.get_or_insert_with(|| vec![1, 6]);
        });
        FAKE_SOURCE_UNKNOWN.set(false);
    }

    /// Goes to sleep, or wakes. Sometimes both are read at one tick, and
    /// sometimes the charger is swapped in the sleep, with no event.
    fn sleep_or_wake(&mut self) {
        if self.asleep {
            if self.rng.chance(300) {
                FAKE_ON_BATTERY.set(!FAKE_ON_BATTERY.get());
            }
            self.wake();
        } else if self.rng.chance(100) {
            self.send(PowerEvent::Suspending);
            self.send(PowerEvent::Resumed);
        } else {
            self.sleep();
        }
    }

    fn sleep(&mut self) {
        self.send(PowerEvent::Suspending);
        // The app reads it at the next tick, then Windows freezes it.
        self.second();
        self.asleep = true;
        self.seen.sleeps += 1;
    }

    fn wake(&mut self) {
        self.send(PowerEvent::Resumed);
        self.asleep = false;
    }

    fn swap_battery(&mut self) {
        let on_battery = !FAKE_ON_BATTERY.get();
        FAKE_ON_BATTERY.set(on_battery);
        self.send(if on_battery {
            PowerEvent::OnBattery
        } else {
            PowerEvent::OnAc
        });
    }

    /// A USB-C monitor in place of the Razer charger, or back: Windows sees
    /// no power change.
    fn swap_weak(&mut self) {
        let weak = self.weak == Weak::No;
        self.weak = if weak { Weak::Unread } else { Weak::No };
        let charger = if weak {
            FakeCharger::UsbC
        } else {
            FakeCharger::Razer
        };
        fake(&self.f, |c| c.charger = charger);
    }

    fn open_again(&mut self) {
        let opened = if self.rng.chance(250) {
            Reopened::Failed
        } else {
            Reopened::Opened
        };
        FAKE_REOPENED.set(opened);
        self.seen.opens_again += 1;
    }

    /// A spell of failed sends: all fail, or each run is cut after a few
    /// commands, or a few runs find the handle closed.
    fn fail_some_runs(&mut self) {
        let kind = self.rng.below(3);
        let spell = 1 + u32::from(self.rng.below(30));
        let cut = usize::from(self.rng.below(6));
        fake(&self.f, |c| match kind {
            0 => c.fails = true,
            1 => c.fails_after = Some(cut),
            _ => c.not_connected_runs = cut,
        });
        self.failing_left = self.failing_left.max(spell);
    }

    /// The CPU heat now, so the fan guard comes and goes; sometimes the
    /// read fails.
    fn change_heat(&mut self) {
        let celsius = (!self.rng.chance(100)).then(|| self.rng.pick(&[40, 55, 70, 85, 95, 99]));
        fake(&self.f, |c| c.cpu_celsius = celsius);
    }

    /// Clicks a preset, the lid logo or the vapor light. Returns the preset
    /// clicked, if one was, and whether its send was sure to fail.
    fn click_something(&mut self) -> Option<(&'static str, bool)> {
        let pick = usize::from(self.rng.below(6));
        if let Some(&preset) = PRESETS.get(pick) {
            let before = self.f.shell.app().config().preset;
            let sure_fail = fake(&self.f, |c| {
                c.fails || c.fails_after.is_some() || c.not_connected_runs > 0
            });
            if !click_if_there(&self.f, Button, preset) {
                return None;
            }
            self.seen.clicks += 1;
            if sure_fail {
                // Rule of ticket 146: a failed send is never kept.
                assert_eq!(self.f.shell.app().config().preset, before, "{preset}");
                self.seen.failed_clicks += 1;
            }
            return Some((preset, sure_fail));
        }
        let switch = ["Lid logo", "Vapor chamber light"][pick - PRESETS.len()];
        click_if_there(&self.f, Switch, switch);
        None
    }

    fn send(&self, event: PowerEvent) {
        self.power.send(event).unwrap();
    }

    /// The rules of ticket 146 for what this step sent, and the kept lists.
    fn check(&mut self, clicked: Option<(&str, bool)>) {
        let may_cut = std::mem::take(&mut self.may_cut);
        let (sent, asked) = fake(&self.f, |c| {
            (std::mem::take(&mut c.sent), std::mem::take(&mut c.asked))
        });
        sent.iter().for_each(assert_in_range);
        self.zones.check_custom_writes(&sent);
        // A weak charge counts from the step after the first read of it:
        // only a read tells the app (ticket 136).
        let battery_presets = FAKE_ON_BATTERY.get() || self.weak == Weak::Read;
        if battery_presets {
            assert_battery_safe(&sent, may_cut);
            self.seen.battery_sends += u32::from(sent.iter().any(|c| c.id == SET_PRESET));
        }
        if self.weak == Weak::Unread && asked.contains(&Charger::query()) {
            self.weak = Weak::Read;
            self.seen.weak_reads += 1;
        }
        if let Some((preset, true)) = clicked
            && !battery_presets
        {
            assert!(
                selected_preset(&self.f) != preset
                    || format!("{:?}", self.f.shell.app().config().preset) == preset,
                "a failed {preset} shown as done"
            );
        }
        let kept = self.f.shell.kept();
        assert!(kept.said_lately <= 16, "{kept:?}");
        assert_eq!(kept.stopped_parts, 0, "{kept:?}");
        assert!(kept.sampler_errors <= 8, "{kept:?}");
        assert!(kept.app_read_errors <= 8, "{kept:?}");
        assert!(kept.pauses <= 3, "{kept:?}");
        self.sent.extend(sent);
    }
}

/// Clicks the one control with `label` in the window; it must show.
fn click(soak: &Soak, role: AccessibleRole, label: &str) {
    assert!(click_if_there(&soak.f, role, label), "{label}");
}

/// Runs `change` on the fake controller.
fn fake<T>(f: &Fixture, change: impl FnOnce(&mut FakeController) -> T) -> T {
    change(&mut f.shell.app().machine_mut().controller.inner)
}

/// Clicks the one control with `label`, if one shows. Returns whether it did.
fn click_if_there(f: &Fixture, role: AccessibleRole, label: &str) -> bool {
    let found: Vec<_> = ElementHandle::find_by_accessible_label(f.shell.window(), label)
        .filter(|element| element.accessible_role() == Some(role))
        .collect();
    let [control] = &found[..] else {
        return false;
    };
    control.mock_single_click(PointerEventButton::Left);
    true
}

fn manual_fan(preset: Preset) -> Config {
    let mut config = Config {
        preset,
        lighting: Some(Lighting {
            brightness: 80,
            colour: Rgb::new(0x20, 0x80, 0xFF),
            vapor_light: true,
        }),
        ..Config::default()
    };
    config.custom.fan_rpm = Some(3500);
    config
}

/// Ends the run calm: awake, no failures, and checks that every pause
/// ended.
fn settle(soak: &mut Soak) {
    if soak.asleep {
        soak.wake();
    }
    fake(&soak.f, |c| {
        c.fails = false;
        c.fails_after = None;
        c.not_connected_runs = 0;
    });
    soak.failing_left = 0;
    soak.reads_failing_left = 0;
    soak.reads_work_again();
    for _ in 0..10 {
        soak.second();
        soak.check(None);
    }
    assert_eq!(soak.f.shell.kept().pauses, 0);
}

fn soak(preset: Preset, seed: u64) {
    let mut soak = Soak::new(manual_fan(preset), seed);
    for _ in 0..20_000 {
        soak.random_step();
    }
    settle(&mut soak);
    let met = &soak.seen;
    let each = [
        met.clicks,
        met.failed_clicks,
        met.sleeps,
        met.opens_again,
        met.weak_reads,
        met.battery_sends,
        met.read_spells,
    ];
    assert!(each.iter().all(|&count| count >= 5), "{met:?}");
}

#[test]
fn a_long_run_from_a_manual_custom_keeps_the_rules_and_its_lists_small() {
    soak(Preset::Custom, 0x9E37_79B9_7F4A_7C15);
}

#[test]
fn a_long_run_from_performance_keeps_the_rules_and_its_lists_small() {
    soak(Preset::Performance, 0x2545_F491_4F6C_DD1D);
}

/// A usual day, in the tray: the controller is read every 10 s, a few
/// clicks, two charger pulls, a lunch sleep and the night's. Counts the set
/// commands, for the flash wear question (ticket 147 (c)): the polls send
/// none, so the count comes only from the starts, wakes, sleeps, swaps and
/// clicks.
#[test]
fn a_usual_day_sends_set_commands_only_at_its_events() {
    let mut soak = Soak::new(manual_fan(Preset::Balanced), 1);
    let hour = |h: u64| h * 3600;
    let mut quiet_from = None;
    for second in 0..hour(24) {
        match second {
            s if s == hour(1) => click(&soak, Button, "Silent"),
            // Unplugged, then plugged in again an hour later.
            s if [hour(2), hour(3), hour(10), hour(11)].contains(&s) => soak.swap_battery(),
            s if s == hour(4) => click(&soak, Button, "Performance"),
            s if s == hour(5) || s == hour(15) => soak.sleep(),
            s if s == hour(6) => soak.wake(),
            s if s == hour(7) => click(&soak, Switch, "Lid logo"),
            s if s == hour(8) => click(&soak, Button, "Balanced"),
            // After the click's own step.
            s if s == hour(8) + 1 => quiet_from = Some(soak.sent.len()),
            s if s == hour(9) => {
                assert_eq!(
                    soak.sent[quiet_from.unwrap()..].len(),
                    0,
                    "an hour with no event sends nothing"
                );
            }
            _ => {}
        }
        soak.second();
        soak.check(None);
    }
    let stored = soak
        .sent
        .iter()
        .filter(|c| STORED.contains(&c.id) && c.args.first() == Some(&VARIABLE_STORAGE))
        .count();
    // The counts in docs/NOTES.md (ticket 147): 7 preset sends of 4 zones,
    // 2 lighting sends, one lid logo switch.
    assert_eq!((soak.sent.len(), stored), (50, 32), "{:?}", soak.sent);
}
