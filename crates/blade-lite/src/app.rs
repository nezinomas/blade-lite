//! The app's state. The views built from it, what each click does and
//! what the app does on start and power events are in the submodules.

mod clicks;
mod error;
mod events;
mod gate;
mod settings;
mod views;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use blade_core::{
    ApplyError, BatteryRate, Config, Controller, Lighting, Preset, Query, apply_fan_targets,
    apply_performance, charge_limit_on, full_power_charger, logo_on, preset_differs, touchpad_on,
};
use platform::{DisplayApi, KeyInjector, Registry};
use razer_proto::{
    Brightness, FirmwareVersion, ModelProfile, NativeMode, RedKey, RedKeys, Rgb, Rpm,
};

pub use error::{AppError, took_effect};
pub use gate::Gated;

use crate::report;

/// How the firmware warning says the version could not be read.
const UNREAD: &str = "could not be read";

/// A firmware version as people write it, such as 1.6.
fn firmware_text(version: FirmwareVersion) -> String {
    format!("{}.{}", version.major, version.minor)
}

/// How many polls send the preset again after a failed send. Then the
/// next power event or change sends it.
pub const RESEND_TRIES: u8 = 3;

/// How often the preset is sent again, once its tries ran out, while a
/// manual fan the window does not show may run (ticket 250).
pub const MANUAL_RESEND_SPACING: Duration = Duration::from_mins(1);

/// How many times the lighting after a charger change is sent before it is
/// given up until the next change, wake or click (ticket 244).
pub const SOURCE_LIGHTING_TRIES: u8 = 3;

/// The least time between two sends of the lighting after a charger change,
/// so a plug that bounces can't send its stored writes at each poll (ticket
/// 246). A change inside it waits, so the last change still gets its send.
pub const SOURCE_LIGHTING_SPACING: Duration = Duration::from_secs(10);

/// What the window shows before the user sets the lighting.
const DEFAULT_LIGHTING: Lighting = Lighting {
    brightness: 100,
    colour: Rgb::new(0xFF, 0xFF, 0xFF),
    vapor_light: true,
};

/// Whether the fan guard's own full-speed send reached the controller
/// (ticket 163).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullSpeed {
    /// Sent, or nothing is left to send.
    Sent,
    /// Its send failed and no send has taken the full speed since.
    Unsent,
}

/// Whether the controller may run a manual fan (ticket 250).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SentFan {
    /// The last preset send that went whole had the fans on auto, and no
    /// cut send since carried the manual flag.
    Auto,
    /// The last preset send that went whole carried the manual flag, or a
    /// cut one since did.
    MaybeManual,
}

/// What the read-back after a preset send found (ticket 152).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PresetCheck {
    /// No send yet, or the last failed partway.
    Unknown,
    /// Every zone reads back as sent.
    Runs,
    /// The zones that read back otherwise, as text.
    Differs(String),
    /// The read failed, with its error.
    Unread(String),
}

/// The parts of the laptop the app talks to.
pub struct Machine<C, D, R, K> {
    pub controller: C,
    pub displays: D,
    pub registry: R,
    pub keys: K,
}

/// Logged when the charger reply is not understood.
pub(super) const NOT_UNDERSTOOD: &str = "charger reply not understood: the battery presets run";

/// What powers the laptop. Only the battery presets run on the battery or
/// a weak charger, as Synapse does. The charger is read at start, after
/// each resume and power event, before a click or key asks for a preset
/// not offered on battery, and on the polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Power {
    Charger,
    /// A charger too weak for every preset, such as a USB-C monitor.
    /// `full_once` is set after one poll read full power: only a second
    /// one in a row brings the full charger back, so a reply that flickers
    /// in a swap does not send the presets back and forth.
    WeakCharger {
        full_once: bool,
    },
    Battery,
}

const WEAK: Power = Power::WeakCharger { full_once: false };

impl Power {
    fn from_battery(on_battery: bool) -> Self {
        if on_battery {
            Self::Battery
        } else {
            Self::Charger
        }
    }

    /// After a poll's read: weak at once, full power only on the second
    /// full power read in a row.
    fn after_poll(self, full: bool) -> Self {
        match (self, full) {
            (_, false) => WEAK,
            (Self::WeakCharger { full_once: false }, true) => Self::WeakCharger { full_once: true },
            (_, true) => Self::Charger,
        }
    }

    fn is_weak(self) -> bool {
        matches!(self, Self::WeakCharger { .. })
    }
}

/// The app's state. Each click becomes a command, and each command is
/// applied first and saved only when it worked. A change the laptop took
/// is kept in memory even if its save fails.
pub struct App<C, D, R, K> {
    model: &'static ModelProfile,
    config: Config,
    config_path: PathBuf,
    machine: Machine<Gated<C>, D, R, K>,
    battery_rate: BatteryRate,
    power: Power,
    /// Whether the last start, preset change or power switch failed, so
    /// the controller may not run the preset the app shows. A failed preset
    /// run marks it (not a lid logo or lighting send after it), and so does
    /// a charger found gone at the quit or sleep (`battery_at_quit`). The
    /// next poll or power event sends it, even if the source is the same.
    preset_unsure: bool,
    /// The number of the last preset send, if it was a manual Custom taken
    /// whole: its fan targets go once more (ticket 173). A new number tells
    /// the shell to wait its 2 s again.
    fan_targets_due: Option<u32>,
    /// Counts the manual Custom sends, for `fan_targets_due`.
    manual_sends: u32,
    /// The polls left to send the preset again after a failed send.
    resends_left: u8,
    /// Whether the controller may run a manual fan. Unknown at the start
    /// (a crash or Razer software may have left one), so it starts as
    /// maybe: it counts only while the preset is unsure.
    sent_fan: SentFan,
    /// When the last slow resend was armed (see
    /// [`MANUAL_RESEND_SPACING`]).
    slow_resend_at: Option<Instant>,
    /// How many preset sends failed (resends too), so the shell can wait a
    /// poll after each.
    preset_failures: u32,
    /// A rate the panel does not offer, from a power event whose preset
    /// send failed too, so only the preset error was shown. The resend
    /// that works shows it. The next power event, a display click, the last
    /// failed resend or a preset send that works (a click, or a weak
    /// charger's send on AC) drops it unshown.
    held_rate_error: Option<AppError>,
    /// The warning for a controller firmware the model profile does not
    /// list, read once at start.
    firmware_warning: Option<String>,
    /// While the fan guard is on, the speed a manual Custom fan runs at:
    /// the model's top speed.
    fan_boost: Option<u16>,
    full_speed: FullSpeed,
    /// Whether the power source was known at the last read. An unknown one
    /// runs the battery presets, but leaves the rate alone at a wake.
    source_known: bool,
    /// The lid logo state, read after the settings are sent at start and
    /// after each resume. `None` if the model has none or it could not be
    /// read.
    logo_on: Option<bool>,
    /// The battery charge limit state, read like the logo's.
    charge_limit_on: Option<bool>,
    /// Which keys the frame the app last painted has red. `None` if
    /// unknown, such as after a failed send: the next change then paints
    /// it.
    key_red: Option<RedKeys>,
    /// Whether Num Lock is on. A red key that is then a numpad key keeps
    /// the plain colour.
    num_lock_on: bool,
    /// Whether the default mic is muted. `None` if it can't be read: the
    /// mic key then keeps the plain colour.
    mic_muted: Option<bool>,
    /// The controller reads that failed when the header was last made.
    read_errors: Vec<String>,
    /// Reads that failed outside a view, each once, until the shell takes
    /// them to log (ticket 259): the charger on a poll, the charge limit
    /// and the lid logo.
    missed_reads: Vec<String>,
    /// The CPU temperature read when the header was last made, in °C.
    cpu_celsius: Option<u32>,
    /// The native mode last read or set, for a display view made without
    /// reading the controller. `None` if none yet, or the last read or set
    /// failed.
    native_mode: Option<NativeMode>,
    /// What the read-back after the last preset send found (ticket 152).
    /// A difference is shown and logged; never sent again for it.
    preset_check: PresetCheck,
    /// The keyboard's level read before the app made the keys dark for a
    /// screen off (ticket 241), to put back. `None` while the app has not
    /// made them dark.
    dark_keys: Option<Brightness>,
    /// The screen state (off or not) the keys last followed, so the shell
    /// looks again only after a change, a failed put-back or a send of
    /// the saved lighting. `None` until the first look.
    keys_follow: Option<bool>,
    /// The tries left of the lighting a charger change asks for again
    /// (ticket 244); `None` when none is due.
    source_lighting_due: Option<u8>,
    /// When the lighting after a charger change was last sent or tried.
    source_lighting_sent: Option<Instant>,
    /// The slot (battery or not) the app left at 0 for a screen off when
    /// the controller swapped it out, with its level to put back at the
    /// next swap into it (ticket 245). The controller keeps one level per
    /// power source (ticket 243).
    left_dark: Option<(bool, Brightness)>,
}

impl<C, D, R, K> App<C, D, R, K>
where
    C: Controller + Query,
    D: DisplayApi,
    R: Registry,
    K: KeyInjector,
{
    #[must_use]
    pub fn new(
        model: &'static ModelProfile,
        config: Config,
        config_path: PathBuf,
        machine: Machine<C, D, R, K>,
    ) -> Self {
        Self {
            model,
            config,
            config_path,
            machine: Machine {
                controller: Gated::new(machine.controller),
                displays: machine.displays,
                registry: machine.registry,
                keys: machine.keys,
            },
            battery_rate: BatteryRate::default(),
            power: Power::Charger,
            preset_unsure: false,
            fan_targets_due: None,
            manual_sends: 0,
            resends_left: 0,
            sent_fan: SentFan::MaybeManual,
            slow_resend_at: None,
            preset_failures: 0,
            held_rate_error: None,
            firmware_warning: None,
            fan_boost: None,
            full_speed: FullSpeed::Sent,
            source_known: true,
            logo_on: None,
            charge_limit_on: None,
            key_red: None,
            num_lock_on: false,
            mic_muted: None,
            read_errors: Vec::new(),
            missed_reads: Vec::new(),
            cpu_celsius: None,
            native_mode: None,
            preset_check: PresetCheck::Unknown,
            dark_keys: None,
            keys_follow: None,
            source_lighting_due: None,
            source_lighting_sent: None,
            left_dark: None,
        }
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn machine(&self) -> &Machine<Gated<C>, D, R, K> {
        &self.machine
    }

    pub fn machine_mut(&mut self) -> &mut Machine<Gated<C>, D, R, K> {
        &mut self.machine
    }

    #[must_use]
    pub fn num_lock_on(&self) -> bool {
        self.num_lock_on
    }

    /// Takes the Num Lock state without painting, for use before
    /// [`start`](Self::start).
    pub fn set_num_lock(&mut self, on: bool) {
        self.num_lock_on = on;
    }

    #[must_use]
    pub fn mic_muted(&self) -> Option<bool> {
        self.mic_muted
    }

    /// Takes the mic mute without painting, for use before
    /// [`start`](Self::start).
    pub fn set_mic_muted(&mut self, muted: Option<bool>) {
        self.mic_muted = muted;
    }

    /// Takes the power source without sending, for use before
    /// [`start`](Self::start), so a start on battery sends the battery
    /// preset, not the saved one. `None` (unknown) counts as battery. A
    /// known change from a known source follows the key slots too.
    pub fn take_on_battery(&mut self, on_battery: Option<bool>) {
        if on_battery.is_none() && self.source_known {
            report("the battery presets run");
        }
        let swapped = self.source_known && on_battery.is_some_and(|now| now != self.on_battery());
        self.power = Power::from_battery(on_battery.unwrap_or(true));
        self.source_known = on_battery.is_some();
        // A swap may go unseen now: a kept level could be stale later.
        if on_battery.is_none() {
            self.left_dark = None;
        }
        if swapped {
            self.follow_dark_slots();
        }
    }

    /// Notes a known read that matches the source the app follows, so a
    /// wake after it follows the rate again. Nothing is sent.
    pub fn source_read(&mut self, on_battery: bool) {
        if on_battery == self.on_battery() {
            self.source_known = true;
        }
    }

    /// Whether the last preset send failed, so the controller may not run
    /// the preset the app shows.
    #[must_use]
    pub fn preset_unsure(&self) -> bool {
        self.preset_unsure
    }

    /// Sends `config`'s preset, then reads it back (ticket 152).
    fn send_preset(&mut self, config: &Config) -> Result<(), ApplyError> {
        let applied = apply_performance(config, self.model, &mut self.machine.controller);
        self.check_preset_if_sent(config, &applied);
        self.note_fan_targets(config, &applied);
        self.note_manual_sent(config, &applied);
        applied
    }

    /// Notes whether the controller may run a manual fan after a send of
    /// `sent`: a send that went whole sets what it carried, a cut one adds
    /// its manual flag to what may run, and one refused before anything
    /// was sent changes nothing.
    fn note_manual_sent(&mut self, sent: &Config, applied: &Result<(), ApplyError>) {
        let manual = sent.preset == Preset::Custom && sent.custom.fan_rpm.is_some();
        let went = matches!(applied, Ok(()) | Err(ApplyError::AfterPreset(_)));
        let cut = matches!(applied, Err(ApplyError::Device(_)));
        if manual && (went || cut) {
            self.sent_fan = SentFan::MaybeManual;
        } else if went {
            self.sent_fan = SentFan::Auto;
        }
    }

    /// Notes whether the fan targets of `config` are due to go once more:
    /// only after a manual Custom taken whole. A send cut partway drops it
    /// (the controller may run part of it; the resend goes whole), and one
    /// refused before anything was sent leaves it as it was.
    pub(crate) fn note_fan_targets(&mut self, config: &Config, applied: &Result<(), ApplyError>) {
        match applied {
            Ok(()) | Err(ApplyError::AfterPreset(_)) => {
                let manual = config.preset == Preset::Custom && config.custom.fan_rpm.is_some();
                self.fan_targets_due = None;
                if manual {
                    self.manual_sends = self.manual_sends.wrapping_add(1);
                    self.fan_targets_due = Some(self.manual_sends);
                }
            }
            Err(ApplyError::Device(_)) => self.fan_targets_due = None,
            Err(_) => {}
        }
    }

    /// The number of the manual Custom send whose fan targets are due to go
    /// once more, if any: see [`Self::send_fan_targets_again`].
    #[must_use]
    pub fn fan_targets_due(&self) -> Option<u32> {
        self.fan_targets_due
    }

    /// Drops fan targets due to go once more, such as while Razer software
    /// may have changed the fan mode.
    pub fn forget_fan_targets(&mut self) {
        self.fan_targets_due = None;
    }

    /// Sends the fan targets of the Custom that runs once more, if a manual
    /// Custom was the last preset sent (ticket 173). Sent while the fans ran
    /// on auto (another preset), a Custom with a manual fan leaves them
    /// stalled near 2900 RPM on firmware 1.6, whatever the target, until the
    /// next target write; the shell calls this about 2 s after the send. It
    /// sends the speed that should run now (the fan guard's, if it runs),
    /// and nothing if Custom with a manual fan no longer runs. Once per
    /// send: a failure is not tried again.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn send_fan_targets_again(&mut self) -> Result<(), AppError> {
        // After a failed send the controller may run part of another
        // preset: the resend of the whole preset comes first.
        if self.fan_targets_due.take().is_none() || self.preset_unsure {
            return Ok(());
        }
        let running = self.running(self.config);
        apply_fan_targets(&running, self.model, &mut self.machine.controller)?;
        Ok(())
    }

    /// Reads back the preset of `config` if `applied` sent it whole, and
    /// keeps, logs and shows a zone that differs. After a send that failed
    /// partway, or a failed read, nothing is known: an old difference is
    /// dropped. Each change is logged once.
    fn check_preset_if_sent(&mut self, config: &Config, applied: &Result<(), ApplyError>) {
        let check = match applied {
            Ok(()) | Err(ApplyError::AfterPreset(_)) => {
                match preset_differs(config, self.model, &mut self.machine.controller) {
                    Ok(None) => PresetCheck::Runs,
                    Ok(Some(differs)) => PresetCheck::Differs(differs),
                    Err(error) => PresetCheck::Unread(error.to_string()),
                }
            }
            Err(ApplyError::Device(_)) => PresetCheck::Unknown,
            // Nothing was sent.
            Err(_) => return,
        };
        if check != self.preset_check {
            match (&check, &self.preset_check) {
                (PresetCheck::Differs(differs), _) => report(&format!(
                    "the controller does not run the preset sent ({differs})"
                )),
                (PresetCheck::Unread(error), _) => {
                    report(&format!("can't read the preset back: {error}"));
                }
                (PresetCheck::Runs, PresetCheck::Differs(_)) => {
                    report("the controller runs the preset sent again");
                }
                _ => {}
            }
        }
        self.preset_check = check;
    }

    /// The zones that read back other than the last preset sent, if any.
    #[must_use]
    pub fn preset_mismatch(&self) -> Option<&str> {
        match &self.preset_check {
            PresetCheck::Differs(differs) => Some(differs),
            _ => None,
        }
    }

    /// Notes a start, preset change or switch: one the controller may have
    /// taken in part is sent again on the next few polls. A value refused
    /// before anything was sent leaves the controller, and so the flag, as
    /// they were. A preset taken before a lid logo or lighting error is
    /// sure, and drops a rate error held for a resend. It takes the guard's
    /// full speed only if `sent` carried it (ticket 180).
    fn note_preset_send(&mut self, sent: &Config, applied: &Result<(), ApplyError>) {
        match applied {
            Ok(()) | Err(ApplyError::AfterPreset(_)) => {
                self.preset_unsure = false;
                self.held_rate_error = None;
                self.note_full_speed(sent);
            }
            Err(ApplyError::Device(_)) => self.preset_send_cut(),
            Err(_) => {}
        }
    }

    /// Closes the controller for good, after the quit preset: every later
    /// send and read is refused.
    pub fn close_controller(&mut self) {
        self.machine.controller.close();
    }

    /// Whether Custom with a manual fan runs (or should run), or may run
    /// after a cut send (ticket 250), so the fan guard is the only thing
    /// that can speed the fans up.
    #[must_use]
    pub fn manual_fan_runs(&self) -> bool {
        self.manual_fan_shown() || self.manual_fan_unshown()
    }

    /// Whether a manual fan may run that the window does not show: a cut
    /// send carried the manual flag, or one cut after a manual Custom ran,
    /// and no send went whole since (ticket 250).
    #[must_use]
    pub fn manual_fan_unshown(&self) -> bool {
        self.preset_unsure && self.sent_fan == SentFan::MaybeManual && !self.manual_fan_shown()
    }

    /// Whether Custom with a manual fan should run.
    fn manual_fan_shown(&self) -> bool {
        let running = self.running(self.config);
        running.preset == Preset::Custom && running.custom.fan_rpm.is_some()
    }

    /// Arms one more resend of the preset that should run, at most each
    /// [`MANUAL_RESEND_SPACING`], while a manual fan the window does not
    /// show may run and the tries ran out: nothing else ends it.
    pub fn keep_resending(&mut self, now: Instant) {
        if !self.manual_fan_unshown() || self.resends_left > 0 {
            return;
        }
        if self
            .slow_resend_at
            .is_some_and(|at| now.saturating_duration_since(at) < MANUAL_RESEND_SPACING)
        {
            return;
        }
        self.slow_resend_at = Some(now);
        self.resends_left = 1;
    }

    /// Notes the guard's full speed sent if `sent`, a preset that went,
    /// carried it: Custom with the manual fan at the boost (ticket 180).
    fn note_full_speed(&mut self, sent: &Config) {
        if self.fan_boost.is_some()
            && sent.preset == Preset::Custom
            && sent.custom.fan_rpm == self.fan_boost
        {
            self.full_speed = FullSpeed::Sent;
        }
    }

    /// Holds every send and read while a Razer updater runs, or lets them
    /// go again.
    pub fn hold_controller(&mut self, held: bool) {
        self.machine.controller.hold(held);
    }

    /// Whether every send and read is held for a Razer updater.
    #[must_use]
    pub fn controller_held(&self) -> bool {
        self.machine.controller.held()
    }

    /// Notes a panic (a bug) that may have cut a send: the preset is then
    /// unsure, and a manual fan may run, so a quit sends the fans on auto
    /// first, as after a failed send.
    pub fn note_panic(&mut self) {
        self.sent_fan = SentFan::MaybeManual;
        self.preset_send_failed();
    }

    /// Marks the preset unsure, with all its resends left.
    fn preset_send_cut(&mut self) {
        self.resends_left = RESEND_TRIES;
        self.preset_send_failed();
    }

    /// Marks the preset unsure and counts the failure, so the next poll
    /// waits. The tries left stay.
    fn preset_send_failed(&mut self) {
        self.preset_unsure = true;
        self.preset_failures = self.preset_failures.wrapping_add(1);
    }

    /// Whether the fan guard's own full-speed send failed and no send has
    /// taken the full speed since: a resend that works then sends it.
    #[must_use]
    pub fn boost_unsent(&self) -> bool {
        self.full_speed == FullSpeed::Unsent
    }

    /// How many preset sends failed so far, resends too.
    #[must_use]
    pub fn preset_failures(&self) -> u32 {
        self.preset_failures
    }

    /// Which keys should be red. If the touchpad state can't be read, its
    /// key keeps the plain colour.
    fn key_should_be_red(&self) -> RedKeys {
        let touchpad_on = touchpad_on(&self.machine.registry).unwrap_or_else(|error| {
            report(&format!("the touchpad key stays plain: {error}"));
            true
        });
        self.red_for(touchpad_on)
    }

    /// The touchpad key red while the touchpad is off, the mic key while
    /// the mic is known to be muted, each unless Num Lock is on and the key
    /// is then a numpad key.
    fn red_for(&self, touchpad_on: bool) -> RedKeys {
        RedKeys {
            touchpad: !touchpad_on && !self.numpad_now(self.model.touchpad_key),
            mic: self.mic_muted == Some(true) && !self.numpad_now(self.model.mic_key),
        }
    }

    /// Whether `key` is a numpad key now: Num Lock is on and the key is one
    /// then.
    fn numpad_now(&self, key: Option<RedKey>) -> bool {
        self.num_lock_on && key.is_some_and(|key| key.plain_with_num_lock)
    }

    /// Reads whether the charge limit is on. `None` if the read fails; the
    /// failure is kept for the log.
    fn read_charge_limit(&mut self) {
        let read = charge_limit_on(&mut self.machine.controller, self.model);
        self.charge_limit_on = self.unless_missed("charge limit", read);
    }

    /// The value of a read, or `None` if it failed: the failure is kept
    /// for the shell to log, once per quiet time, as a view's failed reads
    /// are (ticket 259).
    pub(super) fn unless_missed<T>(
        &mut self,
        what: &str,
        read: Result<Option<T>, impl std::fmt::Display>,
    ) -> Option<T> {
        read.unwrap_or_else(|error| {
            let line = format!("{what}: {error}");
            if !self.missed_reads.contains(&line) {
                self.missed_reads.push(line);
            }
            None
        })
    }

    /// The reads that failed outside a view since the last take, for the
    /// shell to log (ticket 259).
    pub fn take_missed_reads(&mut self) -> Vec<String> {
        std::mem::take(&mut self.missed_reads)
    }

    /// Reads the controller's firmware version and logs it. A version the
    /// model profile does not list, or one that can't be read, gets a
    /// warning, shown in the window; the app still sends (user pick,
    /// ticket 135). Called at start, after the controller is opened again or
    /// a Razer updater ends (the firmware may have changed), and each
    /// minute while the last read failed (ticket 138). A read that keeps
    /// failing is logged once.
    pub fn check_firmware(&mut self) {
        // A held controller refuses the read: the read after the updater
        // tells, so the last result stays.
        if self.controller_held() {
            return;
        }
        let version = match self.machine.controller.query(&FirmwareVersion::query()) {
            Ok(args) => {
                FirmwareVersion::from_reply(&args).ok_or_else(|| "reply not understood".to_owned())
            }
            Err(error) => Err(error.to_string()),
        };
        let found = match version {
            Ok(version) if self.model.firmware.contains(&version) => {
                report(&format!("controller firmware {}", firmware_text(version)));
                None
            }
            Ok(version) => Some(format!("is {}", firmware_text(version))),
            Err(error) => {
                if !self.firmware_unread() {
                    report(&format!("can't read the controller firmware: {error}"));
                }
                Some(UNREAD.to_owned())
            }
        };
        let warning = found.map(|found| self.firmware_warning_text(&found));
        if warning.is_some() && warning != self.firmware_warning {
            report(warning.as_deref().unwrap_or_default());
        }
        self.firmware_warning = warning;
    }

    /// The warning for a firmware that is `found` (such as "is 1.9") when
    /// the model profile does not list it.
    fn firmware_warning_text(&self, found: &str) -> String {
        let checked: Vec<String> = self
            .model
            .firmware
            .iter()
            .map(|&v| firmware_text(v))
            .collect();
        format!(
            "The controller firmware {found}, and Blade Lite is checked only on {}. Settings are still sent.",
            checked.join(" and ")
        )
    }

    /// Whether the last firmware read failed.
    #[must_use]
    pub fn firmware_unread(&self) -> bool {
        self.firmware_warning
            .as_deref()
            .is_some_and(|warning| warning.contains(UNREAD))
    }

    /// The warning for a firmware the model profile does not list, if any.
    #[must_use]
    pub fn firmware_warning(&self) -> Option<&str> {
        self.firmware_warning.as_deref()
    }

    /// Reads the lid logo state. `None` if the read fails; the failure is
    /// kept for the log.
    fn read_logo(&mut self) {
        let read = logo_on(&mut self.machine.controller, self.model);
        self.logo_on = self.unless_missed("lid logo", read);
    }

    /// Whether only the battery presets run: on battery or a weak charge.
    fn battery_presets(&self) -> bool {
        self.power != Power::Charger
    }

    /// Whether the app takes the laptop to run on its battery now.
    #[must_use]
    pub fn on_battery(&self) -> bool {
        self.power == Power::Battery
    }

    /// Reads whether the charger is weak, at a start, a power event or a
    /// wake: the source just changed, so one full read is the full
    /// charger. See [`read_charger_at_click`](Self::read_charger_at_click).
    pub(super) fn read_charger(&mut self) {
        self.read_charger_with(|_| Power::Charger);
    }

    /// Reads whether the charger is weak before a click or a power-mode key
    /// press (ticket 136): a weak or failed read counts at once, but a full
    /// read changes nothing, so only the polls, 1 s or more apart, take the
    /// full charger back (two full reads in a row). Two quick presses can't
    /// count as those two reads.
    pub(super) fn read_charger_at_click(&mut self) {
        self.read_charger_with(|power| power);
    }

    /// Reads the charger as [`read_charger_at_click`](Self::read_charger_at_click)
    /// does, before a send that may run a preset not offered on battery: a
    /// charger swap gives no power event, and the last poll may be 10 s
    /// old. Returns the preset that ran before the read.
    pub(super) fn read_charger_before_send(&mut self) -> Preset {
        let before = self.running_preset();
        self.read_charger_at_click();
        before
    }

    /// Reads the charger; `full` gives the power after a full read. On
    /// battery nothing is read. A failed read or a reply not understood
    /// counts as a weak charge (fail safe) until the next read, with a log
    /// line.
    fn read_charger_with(&mut self, full: fn(Power) -> Power) {
        if self.on_battery() {
            return;
        }
        self.power = match full_power_charger(&mut self.machine.controller, self.model) {
            Ok(Some(true)) => full(self.power),
            Ok(Some(false)) => WEAK,
            Ok(None) => {
                report(NOT_UNDERSTOOD);
                WEAK
            }
            Err(error) => {
                report(&format!(
                    "charger read failed: the battery presets run: {error}"
                ));
                WEAK
            }
        };
    }

    /// The config with the preset that runs in place of its saved one.
    /// While the fan guard is on, a manual Custom fan runs at the model's
    /// top speed; the saved speed is kept. A saved speed the model does not
    /// take is left as it is, so the send still refuses it.
    fn running(&self, config: Config) -> Config {
        let mut running = Config {
            preset: config.preset.running(self.battery_presets(), self.model),
            ..config
        };
        if let Some(rpm) = self.fan_boost
            && running.preset == Preset::Custom
            && running
                .custom
                .fan_rpm
                .is_some_and(|saved| Rpm::new(saved, self.model).is_ok())
        {
            running.custom.fan_rpm = Some(rpm);
        }
        running
    }

    /// Brings the fan boost in line with the fan guard (ticket 129), on each
    /// allowed reading; usually nothing changes. While it is on, a manual
    /// Custom fan runs at the model's top speed. Sends Custom again only if
    /// Custom with a manual fan runs; a later Custom send takes the guard
    /// too. Returns whether it sent.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. A failed send marks the preset unsure, so the polls
    /// send it again. A send that works drops a held rate error unshown, as
    /// a click does.
    pub fn set_fan_boost(&mut self, on: bool) -> Result<bool, AppError> {
        let top = self.model.custom.fan_rpm.max;
        let boost = on.then_some(top);
        // A failed send whose resends ran out is tried again while the
        // guard is on and a manual fan may run (the polls send it, at the
        // poll rate: for a manual fan the window does not show, faster
        // than `MANUAL_RESEND_SPACING`, which only sends a preset with the
        // fans on auto).
        if on && self.manual_fan_runs() && self.preset_unsure && self.resends_left == 0 {
            self.resends_left = RESEND_TRIES;
        }
        if self.fan_boost == boost {
            return Ok(false);
        }
        self.fan_boost = boost;
        self.full_speed = FullSpeed::Sent;
        let running = self.running(self.config);
        // Already at the top speed: nothing changes on the laptop. A manual
        // fan the window does not show is ended by the resends of the
        // preset that should run (ticket 250).
        if !self.manual_fan_shown() || self.config.custom.fan_rpm == Some(top) {
            return Ok(false);
        }
        let applied = self.send_preset(&running);
        self.note_preset_send(&running, &applied);
        if on && matches!(applied, Err(ApplyError::Device(_))) {
            self.full_speed = FullSpeed::Unsent;
        }
        applied?;
        Ok(true)
    }

    /// The saved lighting was sent, so the keys run its level: none is
    /// kept to put back, and the screen needs a new look (ticket 241).
    fn keys_lit_by_lighting(&mut self) {
        self.dark_keys = None;
        self.keys_follow = None;
        self.source_lighting_due = None;
    }

    fn lighting(&self) -> Lighting {
        self.config.lighting.unwrap_or(DEFAULT_LIGHTING)
    }

    /// Keeps a config the laptop already runs, then saves it. Kept even if
    /// the save fails, so the views, the next change and the next resume
    /// match what the laptop does now.
    fn keep_sent(&mut self, config: Config) -> Result<(), AppError> {
        self.config = config;
        config.save(&self.config_path).map_err(AppError::NotSaved)
    }

    fn commit(&mut self, config: Config) -> Result<(), AppError> {
        config.save(&self.config_path)?;
        self.config = config;
        Ok(())
    }
}
