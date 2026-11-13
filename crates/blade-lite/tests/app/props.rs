//! Property tests (ticket 165): the app over a controller that answers at
//! random. Each run is taken, refused, cut partway or finds the handle
//! closed; each read fails or gets random bytes. Whatever the answers, the
//! app does not panic, sends no Performance, manual fan or boost on
//! battery or a weak charge, sends every value in the model's range, and
//! never shows a failed preset change as done.

use blade_core::{Config, Controller, Preset, Query};
use blade_lite::{App, Machine, took_effect};
use platform::PowerEvent;
use proptest::prelude::*;
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Charger, Command};
use ui::PerformanceCommand;

use crate::fakes::{FakeDisplays, FakeKeys, FakeRegistry};
use crate::rules::{Zones, assert_battery_safe, assert_in_range};

/// How the controller answers a run.
#[derive(Debug, Clone, Copy)]
enum RunAnswer {
    Takes,
    /// Busy before anything is sent.
    Busy,
    /// A held or closed handle.
    Closed,
    /// Takes this many commands, then Busy, as a reply to one it still ran.
    CutAfter(usize),
}

/// What the controller replies to a read: `None` fails it (Busy).
type ReadAnswer = Option<Vec<u8>>;

/// Answers each run and read with the next answer of its list, round and
/// round. Keeps what a run sent, and what the charger reads said.
struct RandomController {
    runs: Vec<RunAnswer>,
    reads: Vec<ReadAnswer>,
    next_run: usize,
    next_read: usize,
    sent: Vec<Command>,
    failed_runs: usize,
    /// Whether the last charger read that worked found the full charger.
    /// A failed read leaves it: the app may keep the full charger then.
    full_charger_read: bool,
}

impl RandomController {
    fn new(runs: Vec<RunAnswer>, reads: Vec<ReadAnswer>) -> Self {
        Self {
            runs,
            reads,
            next_run: 0,
            next_read: 0,
            sent: Vec::new(),
            failed_runs: 0,
            full_charger_read: false,
        }
    }
}

/// The next of `answers`, round and round.
fn next<T: Clone>(answers: &[T], at: &mut usize) -> T {
    let answer = answers[*at % answers.len()].clone();
    *at += 1;
    answer
}

impl Controller for RandomController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        let taken = match next(&self.runs, &mut self.next_run) {
            RunAnswer::Takes => {
                self.sent.extend_from_slice(commands);
                return Ok(());
            }
            RunAnswer::Busy => 0,
            RunAnswer::Closed => {
                self.failed_runs += 1;
                let closed = std::io::Error::new(std::io::ErrorKind::NotConnected, "held");
                return Err(DeviceError::Io(closed));
            }
            RunAnswer::CutAfter(n) => n.min(commands.len()),
        };
        self.sent.extend_from_slice(&commands[..taken]);
        self.failed_runs += 1;
        Err(DeviceError::Busy)
    }
}

impl Query for RandomController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        let reply = next(&self.reads, &mut self.next_read).ok_or(DeviceError::Busy)?;
        if *command == Charger::query() {
            let charger = BLADE_18.charger.expect("the Blade 18 reads its charger");
            self.full_charger_read = charger.full_power(&reply) == Some(true);
        }
        Ok(reply)
    }
}

type RandomApp = App<RandomController, FakeDisplays, FakeRegistry, FakeKeys>;

/// What the app is asked to do next.
#[derive(Debug, Clone, Copy)]
enum Step {
    Power(PowerEvent),
    /// A poll: the charger read, then a resend if one is due.
    Poll,
    FanGuard(bool),
    FanTargetsAgain,
    Click(PerformanceCommand),
    PowerKey,
}

fn preset() -> impl Strategy<Value = Preset> {
    prop_oneof![
        Just(Preset::Silent),
        Just(Preset::Balanced),
        Just(Preset::Performance),
        Just(Preset::Custom),
    ]
}

/// A fan speed: mostly in the model's range, sometimes out of it.
fn fan_rpm() -> impl Strategy<Value = Option<u16>> {
    let range = BLADE_18.custom.fan_rpm;
    prop_oneof![
        Just(None),
        (range.min / 100..=range.max / 100).prop_map(|speed| Some(speed * 100)),
        any::<u16>().prop_map(Some),
    ]
}

/// A boost level: in the model's range or just over it.
fn boost() -> impl Strategy<Value = u8> {
    0..=BLADE_18.custom.cpu_max.max(BLADE_18.custom.gpu_max) + 2
}

fn click() -> impl Strategy<Value = PerformanceCommand> {
    prop_oneof![
        4 => preset().prop_map(PerformanceCommand::SetPreset),
        1 => boost().prop_map(PerformanceCommand::SetCpuBoost),
        1 => boost().prop_map(PerformanceCommand::SetGpuBoost),
        2 => fan_rpm().prop_map(PerformanceCommand::SetFanRpm),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    let event = prop_oneof![
        Just(PowerEvent::Suspending),
        Just(PowerEvent::Resumed),
        Just(PowerEvent::OnBattery),
        Just(PowerEvent::OnAc),
    ];
    prop_oneof![
        2 => event.prop_map(Step::Power),
        3 => Just(Step::Poll),
        1 => any::<bool>().prop_map(Step::FanGuard),
        1 => Just(Step::FanTargetsAgain),
        4 => click().prop_map(Step::Click),
        1 => Just(Step::PowerKey),
    ]
}

fn run_answer() -> impl Strategy<Value = RunAnswer> {
    prop_oneof![
        5 => Just(RunAnswer::Takes),
        1 => Just(RunAnswer::Busy),
        1 => Just(RunAnswer::Closed),
        1 => (0_usize..8).prop_map(RunAnswer::CutAfter),
    ]
}

/// A read's reply: the full charger, a weak one, random bytes, or none.
fn read_answer() -> impl Strategy<Value = ReadAnswer> {
    prop_oneof![
        2 => Just(Some(vec![0x13, 0x13])),
        1 => Just(Some(vec![0x04, 0x13])),
        2 => prop::collection::vec(any::<u8>(), 0..8).prop_map(Some),
        1 => Just(None),
    ]
}

fn saved_config() -> impl Strategy<Value = Config> {
    (preset(), boost(), boost(), fan_rpm()).prop_map(|(preset, cpu, gpu, fan_rpm)| {
        let mut config = Config {
            preset,
            ..Config::default()
        };
        config.custom.cpu_boost = cpu;
        config.custom.gpu_boost = gpu;
        config.custom.fan_rpm = fan_rpm;
        config
    })
}

/// Does `step`. For a click or key press, returns whether it took effect.
fn take(app: &mut RandomApp, step: Step) -> Option<bool> {
    // Every error is allowed: only what was sent and shown is checked.
    match step {
        Step::Power(event) => drop(app.on_power(event)),
        Step::Poll => {
            drop(app.follow_charger());
            drop(app.resend_preset());
        }
        Step::FanGuard(on) => drop(app.set_fan_boost(on)),
        Step::FanTargetsAgain => drop(app.send_fan_targets_again()),
        Step::Click(command) => return Some(took_effect(&app.on_performance(command))),
        Step::PowerKey => return Some(took_effect(&app.on_power_key())),
    }
    None
}

/// Whether the app should take the laptop to run on its battery after
/// `step`, from the power events alone.
fn on_battery_after(step: Step, before: bool) -> bool {
    match step {
        Step::Power(PowerEvent::OnBattery) => true,
        Step::Power(PowerEvent::OnAc) => false,
        _ => before,
    }
}

proptest! {
    #[test]
    fn the_app_keeps_the_send_rules_whatever_the_controller_answers(
        config in saved_config(),
        source in prop_oneof![Just(None), any::<bool>().prop_map(Some)],
        runs in prop::collection::vec(run_answer(), 1..40),
        reads in prop::collection::vec(read_answer(), 1..40),
        steps in prop::collection::vec(step(), 0..60),
    ) {
        let dir = tempfile::tempdir().expect("a temp dir");
        let machine = Machine {
            controller: RandomController::new(runs, reads),
            displays: FakeDisplays::default(),
            registry: FakeRegistry::default(),
            keys: FakeKeys::default(),
        };
        let mut app = App::new(&BLADE_18, config, dir.path().join("config.toml"), machine);
        app.take_on_battery(source);
        let mut on_battery = source.unwrap_or(true);
        let mut zones = Zones::default();
        drop(app.start());
        check(&mut app, &mut zones, on_battery, 0);
        for step in steps {
            let before = *app.config();
            let failed_before = controller(&app).failed_runs;
            let took = take(&mut app, step);
            on_battery = on_battery_after(step, on_battery);
            let failed_runs = controller(&app).failed_runs - failed_before;
            if let (Some(took), true) = (took, failed_runs > 0) {
                // A click sends once: a failed send is never shown as done.
                prop_assert!(!took, "{:?}", step);
                prop_assert_eq!(*app.config(), before, "{:?}", step);
            }
            check(&mut app, &mut zones, on_battery, failed_runs);
        }
        let failed_before = controller(&app).failed_runs;
        drop(app.quit_preset());
        let failed_runs = controller(&app).failed_runs - failed_before;
        check(&mut app, &mut zones, on_battery, failed_runs);
    }
}

fn controller(app: &RandomApp) -> &RandomController {
    &app.machine().controller.inner
}

/// The rules of ticket 146 for what the last step sent. A weak charge is
/// any charger read but the full charger's since the last one that worked.
fn check(app: &mut RandomApp, zones: &mut Zones, on_battery: bool, failed_runs: usize) {
    let controller = &mut app.machine_mut().controller.inner;
    let sent = std::mem::take(&mut controller.sent);
    sent.iter().for_each(assert_in_range);
    zones.check_fan_targets(&sent);
    if on_battery || !controller.full_charger_read {
        assert_battery_safe(&sent, failed_runs > 0);
    }
}
