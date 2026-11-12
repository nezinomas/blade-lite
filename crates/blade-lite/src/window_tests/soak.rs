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
use razer_proto::{BLADE_18, Charger, Command, Rgb};
use slint::platform::PointerEventButton;

use super::{FAKE_MIC, FAKE_ON_BATTERY, FAKE_REOPENED, Fixture, fixture, selected_preset};
use crate::fakes::{FakeCharger, FakeController};
use crate::shell::Reopened;

const SET_PRESET: u16 = 0x0D02;
const SET_FAN: u16 = 0x0D01;

/// The storage byte `OpenRazer` calls the stored one.
const VARIABLE_STORAGE: u8 = 0x01;

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
            _ => {}
        }
        self.second();
        self.check(clicked);
    }

    /// One second passes: the Slint timers (the pauses) run, then a tick
    /// unless the laptop sleeps (Windows freezes the app then).
    fn second(&mut self) {
        self.now += SECOND;
        self.wall += SECOND;
        i_slint_backend_testing::mock_elapsed_time(SECOND);
        if !self.asleep {
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
    /// clicked, if one was.
    fn click_something(&mut self) -> Option<&'static str> {
        let pick = usize::from(self.rng.below(6));
        if let Some(&preset) = PRESETS.get(pick) {
            let before = self.f.shell.app().config().preset;
            let sure_fail = fake(&self.f, |c| c.fails || c.fails_after.is_some());
            if !click_if_there(&self.f, Button, preset) {
                return None;
            }
            if sure_fail {
                // Rule of ticket 146: a failed send is never kept.
                assert_eq!(self.f.shell.app().config().preset, before, "{preset}");
            }
            return Some(preset);
        }
        let switch = ["Lid logo", "Vapor chamber light"][pick - PRESETS.len()];
        click_if_there(&self.f, Switch, switch);
        None
    }

    fn send(&self, event: PowerEvent) {
        self.power.send(event).unwrap();
    }

    /// The rules of ticket 146 for what this step sent, and the kept lists.
    fn check(&mut self, clicked: Option<&str>) {
        let (sent, asked) = fake(&self.f, |c| {
            (std::mem::take(&mut c.sent), std::mem::take(&mut c.asked))
        });
        for command in sent.iter().filter(|c| c.id == SET_FAN) {
            let rpm = u16::from(command.args[2]) * 100;
            let range = BLADE_18.custom.fan_rpm;
            assert!((range.min..=range.max).contains(&rpm), "{command:?}");
        }
        // A weak charge counts from the step after the first read of it:
        // only a read tells the app (ticket 136).
        let battery_presets = FAKE_ON_BATTERY.get() || self.weak == Weak::Read;
        if battery_presets {
            assert_battery_safe(&sent);
        }
        if self.weak == Weak::Unread && asked.contains(&Charger::query()) {
            self.weak = Weak::Read;
        }
        if let Some(preset) = clicked
            && fake(&self.f, |c| c.fails)
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

/// On battery or a weak charge, no Performance and no manual fan goes.
/// Custom with the fans on auto goes only as the first part of the quit
/// preset, so a Silent or Balanced write comes after it.
fn assert_battery_safe(sent: &[Command]) {
    let bytes = BLADE_18.mode_bytes;
    let quiet = |c: &Command| {
        c.id == SET_PRESET && (c.args[2] == bytes.silent || c.args[2] == bytes.balanced)
    };
    for (at, command) in sent.iter().enumerate() {
        assert_ne!(command.id, SET_FAN, "a fan target on battery: {sent:?}");
        if command.id != SET_PRESET {
            continue;
        }
        let (mode, fan) = (command.args[2], command.args[3]);
        assert!(
            mode != bytes.performance && fan == 0,
            "on battery: {sent:?}"
        );
        if mode == BLADE_18.custom.mode {
            assert!(
                sent[at..].iter().any(quiet),
                "Custom left on battery: {sent:?}"
            );
        }
    }
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
            s if s == hour(1) => {
                click_if_there(&soak.f, Button, "Silent");
            }
            s if s == hour(2) || s == hour(10) => soak.swap_battery(),
            s if s == hour(3) || s == hour(11) => soak.swap_battery(),
            s if s == hour(4) => {
                click_if_there(&soak.f, Button, "Performance");
            }
            s if s == hour(5) || s == hour(15) => soak.sleep(),
            s if s == hour(6) => soak.wake(),
            s if s == hour(7) => {
                click_if_there(&soak.f, Switch, "Lid logo");
            }
            s if s == hour(8) => {
                click_if_there(&soak.f, Button, "Balanced");
            }
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
        .filter(|c| c.args.first() == Some(&VARIABLE_STORAGE))
        .count();
    println!(
        "a usual day: {} set commands, {stored} with storage byte 1",
        soak.sent.len()
    );
    assert!(soak.sent.len() < 400, "{}", soak.sent.len());
}
