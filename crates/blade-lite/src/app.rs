//! The app's state. The views built from it, what each click does and
//! what the app does on start and power events are in the submodules.

mod clicks;
mod error;
mod events;
mod settings;
mod views;

use std::path::PathBuf;

use blade_core::{
    ApplyError, BatteryRate, Config, Controller, Lighting, Preset, Query, apply_performance,
    charge_limit_on, full_power_charger, logo_on, touchpad_on,
};
use platform::{DisplayApi, KeyInjector, Registry};
use razer_proto::{FirmwareVersion, ModelProfile, NativeMode, RedKey, RedKeys, Rgb, Rpm};

pub use error::{AppError, took_effect};

use crate::report;

/// A firmware version as people write it, such as 1.6.
fn firmware_text(version: FirmwareVersion) -> String {
    format!("{}.{}", version.major, version.minor)
}

/// How many polls send the preset again after a failed send. Then the
/// next power event or change sends it.
pub const RESEND_TRIES: u8 = 3;

/// What the window shows before the user sets the lighting.
const DEFAULT_LIGHTING: Lighting = Lighting {
    brightness: 100,
    colour: Rgb::new(0xFF, 0xFF, 0xFF),
    vapor_light: true,
};

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
    machine: Machine<C, D, R, K>,
    battery_rate: BatteryRate,
    power: Power,
    /// Whether the last start, preset change or power switch failed, so
    /// the controller may not run the preset the app shows. Only a failed
    /// preset run marks it, not a lid logo or lighting send after it. The
    /// next poll or power event sends it, even if the source is the same.
    preset_unsure: bool,
    /// The polls left to send the preset again after a failed send.
    resends_left: u8,
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
    /// The CPU temperature read when the header was last made, in °C.
    cpu_celsius: Option<u32>,
    /// The native mode last read or set, for a display view made without
    /// reading the controller. `None` if none yet, or the last read or set
    /// failed.
    native_mode: Option<NativeMode>,
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
            machine,
            battery_rate: BatteryRate::default(),
            power: Power::Charger,
            preset_unsure: false,
            resends_left: 0,
            preset_failures: 0,
            held_rate_error: None,
            firmware_warning: None,
            fan_boost: None,
            source_known: true,
            logo_on: None,
            charge_limit_on: None,
            key_red: None,
            num_lock_on: false,
            mic_muted: None,
            read_errors: Vec::new(),
            cpu_celsius: None,
            native_mode: None,
        }
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn machine(&self) -> &Machine<C, D, R, K> {
        &self.machine
    }

    pub fn machine_mut(&mut self) -> &mut Machine<C, D, R, K> {
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
    /// preset, not the saved one. `None` (unknown) counts as battery.
    pub fn take_on_battery(&mut self, on_battery: Option<bool>) {
        if on_battery.is_none() && self.source_known {
            report("the battery presets run");
        }
        self.power = Power::from_battery(on_battery.unwrap_or(true));
        self.source_known = on_battery.is_some();
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

    /// Notes a start, preset change or switch: one the controller may have
    /// taken in part is sent again on the next few polls. A value refused
    /// before anything was sent leaves the controller, and so the flag, as
    /// they were. A preset taken before a lid logo or lighting error is
    /// sure, and drops a rate error held for a resend.
    fn note_preset_send(&mut self, applied: &Result<(), ApplyError>) {
        match applied {
            Ok(()) | Err(ApplyError::AfterPreset(_)) => {
                self.preset_unsure = false;
                self.held_rate_error = None;
            }
            Err(ApplyError::Device(_)) => self.preset_send_cut(),
            Err(_) => {}
        }
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

    fn read_charge_limit(&mut self) {
        self.charge_limit_on = charge_limit_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
    }

    /// Reads the controller's firmware version and logs it. A version the
    /// model profile does not list, or one that can't be read, gets a
    /// warning, shown in the window; the app still sends (user pick,
    /// ticket 135). Called once, at start.
    pub fn check_firmware(&mut self) {
        let version = match self.machine.controller.query(&FirmwareVersion::query()) {
            Ok(args) => {
                FirmwareVersion::from_reply(&args).ok_or_else(|| "reply not understood".to_owned())
            }
            Err(error) => Err(error.to_string()),
        };
        let found = match version {
            Ok(version) if self.model.firmware.contains(&version) => {
                report(&format!("controller firmware {}", firmware_text(version)));
                return;
            }
            Ok(version) => format!("is {}", firmware_text(version)),
            Err(error) => {
                report(&format!("can't read the controller firmware: {error}"));
                "could not be read".to_owned()
            }
        };
        let checked: Vec<String> = self
            .model
            .firmware
            .iter()
            .map(|&v| firmware_text(v))
            .collect();
        let warning = format!(
            "The controller firmware {found}, and Blade Lite is checked only on {}. Settings are still sent.",
            checked.join(" and ")
        );
        report(&warning);
        self.firmware_warning = Some(warning);
    }

    /// The warning for a firmware the model profile does not list, if any.
    #[must_use]
    pub fn firmware_warning(&self) -> Option<&str> {
        self.firmware_warning.as_deref()
    }

    /// Reads the lid logo state. `None` if the read fails.
    fn read_logo(&mut self) {
        self.logo_on = logo_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
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

    /// Reads whether the charger is weak. On battery nothing is read. A
    /// failed read or a reply not understood counts as a weak charge (fail
    /// safe) until the next read, with a log line.
    pub(super) fn read_charger(&mut self) {
        if self.on_battery() {
            return;
        }
        self.power = match full_power_charger(&mut self.machine.controller, self.model) {
            Ok(Some(true)) => Power::Charger,
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
        let running = self.running(self.config);
        let manual = running.preset == Preset::Custom && running.custom.fan_rpm.is_some();
        if self.fan_boost == boost {
            // A failed full-speed send whose resends ran out is tried
            // again while the guard stays on (the polls send it).
            if on && manual && self.preset_unsure && self.resends_left == 0 {
                self.resends_left = RESEND_TRIES;
            }
            return Ok(false);
        }
        self.fan_boost = boost;
        let running = self.running(self.config);
        // Already at the top speed: nothing changes on the laptop.
        if !manual || self.config.custom.fan_rpm == Some(top) {
            return Ok(false);
        }
        let applied = apply_performance(&running, self.model, &mut self.machine.controller);
        self.note_preset_send(&applied);
        applied?;
        Ok(true)
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
