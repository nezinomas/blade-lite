//! The app's state. The views built from it, what each click does and
//! what the app does on start and power events are in the submodules.

mod clicks;
mod error;
mod events;
mod settings;
mod views;

use std::path::PathBuf;

use blade_core::{
    BatteryRate, Config, Controller, Lighting, Query, charge_limit_on, full_power_charger, logo_on,
    touchpad_on,
};
use platform::{DisplayApi, KeyInjector, Registry};
use razer_proto::{ModelProfile, RedKey, RedKeys, Rgb};

pub use error::{AppError, took_effect};

use crate::report;

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
    /// the controller may not run the preset the app shows (a failed start
    /// may also be a lighting send; a resend then does no harm). The next
    /// power event sends it, even if the source is the same.
    preset_unsure: bool,
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
            logo_on: None,
            charge_limit_on: None,
            key_red: None,
            num_lock_on: false,
            mic_muted: None,
            read_errors: Vec::new(),
            cpu_celsius: None,
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
    /// preset, not the saved one.
    pub fn take_on_battery(&mut self, on_battery: bool) {
        self.power = Power::from_battery(on_battery);
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

    fn on_battery(&self) -> bool {
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
    fn running(&self, config: Config) -> Config {
        Config {
            preset: config.preset.running(self.battery_presets(), self.model),
            ..config
        }
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
