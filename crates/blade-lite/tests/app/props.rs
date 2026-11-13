//! Property tests (ticket 165): the app over a controller that answers at
//! random. Each run is taken, refused, cut partway or finds the handle
//! closed; each read fails or gets random bytes. Whatever the answers, the
//! app does not panic; sends every value in the model's range, a boost
//! only under Custom and a fan target only under a manual Custom; on
//! battery or a weak charge sends no Performance, manual fan or boost and
//! leaves none running; and never shows a failed preset change as done.

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
    /// Busy for the first command, which may or may not have run.
    Busy,
    /// A held or closed handle: nothing is sent.
    Closed,
    /// Takes this many commands, then Busy for the next, which may or may
    /// not have run.
    CutAfter(usize),
}

/// What the controller replies to a read: `None` fails it (Busy).
type ReadAnswer = Option<Vec<u8>>;

/// What a charger read told the app: `None` if it failed, else whether the
/// reply was the full charger's (`None` if not understood).
type ChargerRead = Option<Option<bool>>;

/// Answers each run and read with the next answer of its list, round and
/// round. Keeps every command a run was given, what the controller ran,
/// and what each charger read said.
struct RandomController {
    runs: Vec<RunAnswer>,
    reads: Vec<ReadAnswer>,
    next_run: usize,
    next_read: usize,
    /// Every command of every run, failed ones too.
    tried: Vec<Command>,
    /// Each command the controller ran (`true`) or may have run (`false`),
    /// in order.
    ran: Vec<(Command, bool)>,
    failed_runs: usize,
    charger_reads: Vec<ChargerRead>,
}

impl RandomController {
    fn new(runs: Vec<RunAnswer>, reads: Vec<ReadAnswer>) -> Self {
        Self {
            runs,
            reads,
            next_run: 0,
            next_read: 0,
            tried: Vec::new(),
            ran: Vec::new(),
            failed_runs: 0,
            charger_reads: Vec::new(),
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
        self.tried.extend_from_slice(commands);
        let taken = match next(&self.runs, &mut self.next_run) {
            RunAnswer::Takes => commands.len(),
            RunAnswer::Busy => 0,
            RunAnswer::Closed => {
                self.failed_runs += 1;
                let closed = std::io::Error::new(std::io::ErrorKind::NotConnected, "held");
                return Err(DeviceError::Io(closed));
            }
            RunAnswer::CutAfter(n) => n,
        };
        let sure = commands.iter().take(taken).map(|c| (c.clone(), true));
        self.ran.extend(sure);
        let Some(unsure) = commands.get(taken) else {
            return Ok(());
        };
        self.ran.push((unsure.clone(), false));
        self.failed_runs += 1;
        Err(DeviceError::Busy)
    }
}

impl Query for RandomController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        let reply = next(&self.reads, &mut self.next_read);
        if *command == Charger::query() {
            let charger = BLADE_18.charger.expect("the Blade 18 reads its charger");
            let read = reply.as_ref().map(|args| charger.full_power(args));
            self.charger_reads.push(read);
        }
        reply.ok_or(DeviceError::Busy)
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
    /// The reads that send nothing, or only lighting.
    Firmware,
    CpuTemperature,
    DisplayView,
    BrightnessKeys,
}

impl Step {
    /// How the app takes a charger read in this step (ticket 136).
    fn read_kind(self) -> ReadKind {
        match self {
            Self::Poll => ReadKind::Poll,
            Self::Click(_) | Self::PowerKey => ReadKind::Click,
            _ => ReadKind::Switch,
        }
    }
}

/// When the app reads the charger.
#[derive(Debug, Clone, Copy)]
enum ReadKind {
    /// At start, a wake or a power event: the source just changed.
    Switch,
    /// Before a click or key press.
    Click,
    Poll,
}

/// The weak charge the app should know, from its charger reads (ticket
/// 136): a weak, failed or not understood read counts at once (a failed
/// poll read keeps the last state); a full read brings the full charger
/// back at a switch, never before a click, and on the polls only on the
/// second in a row.
#[derive(Debug, Default)]
struct Charge {
    weak: bool,
    full_once: bool,
}

impl Charge {
    fn after(&mut self, kind: ReadKind, read: ChargerRead) {
        match (kind, read) {
            (ReadKind::Poll, None) | (ReadKind::Click, Some(Some(true))) => {}
            (_, None | Some(None | Some(false))) => {
                *self = Self {
                    weak: true,
                    full_once: false,
                }
            }
            (ReadKind::Poll, Some(Some(true))) if self.weak && !self.full_once => {
                self.full_once = true;
            }
            (ReadKind::Switch | ReadKind::Poll, Some(Some(true))) => *self = Self::default(),
        }
    }
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
        1 => prop_oneof![
            Just(Step::Firmware),
            Just(Step::CpuTemperature),
            Just(Step::DisplayView),
            Just(Step::BrightnessKeys),
        ],
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
        Step::Firmware => app.check_firmware(),
        Step::CpuTemperature => app.read_cpu_temperature(),
        Step::DisplayView => drop(app.display_view()),
        Step::BrightnessKeys => drop(app.on_brightness_keys()),
    }
    None
}

/// The app over the random controller, and what the test knows of the
/// world around it.
struct World {
    app: RandomApp,
    /// From the power events alone.
    on_battery: bool,
    /// From the charger reads.
    charge: Charge,
    /// What each zone of the controller runs.
    zones: Zones,
    /// Between a sleep notice and the wake: the shell passes on only the
    /// wake then.
    asleep: bool,
}

impl World {
    fn controller(&mut self) -> &mut RandomController {
        &mut self.app.machine_mut().controller.inner
    }

    /// Does `step` and checks the rules for it, as the shell would pass it
    /// on: nothing but the wake after a sleep notice.
    fn step(&mut self, step: Step) {
        if self.asleep && !matches!(step, Step::Power(PowerEvent::Resumed)) {
            return;
        }
        let before = *self.app.config();
        let failed_before = self.controller().failed_runs;
        let took = take(&mut self.app, step);
        let failed_runs = self.controller().failed_runs - failed_before;
        match step {
            Step::Power(PowerEvent::OnBattery) => self.on_battery = true,
            Step::Power(PowerEvent::OnAc) => self.on_battery = false,
            Step::Power(PowerEvent::Suspending) => {
                self.asleep = true;
                // No manual fan targets may go after the sleep's quit (182).
                assert_eq!(self.app.fan_targets_due(), None);
            }
            Step::Power(PowerEvent::Resumed) => self.asleep = false,
            _ => {}
        }
        if let (Some(took), true) = (took, failed_runs > 0) {
            // A click sends once: a failed send is never shown as done.
            assert!(!took, "{step:?}");
            assert_eq!(*self.app.config(), before, "{step:?}");
        }
        self.check(step.read_kind(), failed_runs > 0);
    }

    /// The rules of ticket 146 for what the last step tried to send, and
    /// for what the controller runs after it.
    fn check(&mut self, kind: ReadKind, may_cut: bool) {
        let controller = self.controller();
        let tried = std::mem::take(&mut controller.tried);
        let ran = std::mem::take(&mut controller.ran);
        let reads = std::mem::take(&mut controller.charger_reads);
        for read in reads {
            self.charge.after(kind, read);
        }
        tried.iter().for_each(assert_in_range);
        for (command, sure) in ran {
            if sure {
                self.zones
                    .check_custom_writes(std::slice::from_ref(&command));
            } else {
                self.zones.forget(std::slice::from_ref(&command));
            }
        }
        if self.on_battery || self.charge.weak {
            assert_battery_safe(&tried, may_cut);
            // A failed send leaves the resends of the polls to put it right.
            if !self.app.preset_unsure() && !self.asleep {
                self.zones.assert_battery_safe();
            }
        }
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
        let mut world = World {
            app,
            on_battery: source.unwrap_or(true),
            charge: Charge::default(),
            zones: Zones::default(),
            asleep: false,
        };
        let failed_before = world.controller().failed_runs;
        drop(world.app.start());
        let failed = world.controller().failed_runs > failed_before;
        world.check(ReadKind::Switch, failed);
        for step in steps {
            world.step(step);
        }
        // The quit: no state is checked after it, as no app is left.
        world.asleep = true;
        let failed_before = world.controller().failed_runs;
        drop(world.app.quit_preset());
        assert_eq!(world.app.fan_targets_due(), None);
        let failed = world.controller().failed_runs > failed_before;
        world.check(ReadKind::Switch, failed);
    }
}
