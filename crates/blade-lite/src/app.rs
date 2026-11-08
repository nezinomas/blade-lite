//! The app's state. The views built from it, what each click does and
//! what the app does on start and power events are in the submodules.

mod clicks;
mod error;
mod events;
mod views;

use std::path::PathBuf;

use blade_core::{BatteryRate, Config, Controller, Lighting, Query, charge_limit_on, touchpad_on};
use platform::{DisplayApi, KeyInjector, Registry};
use razer_proto::{ModelProfile, Rgb};

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

/// The app's state. Each click becomes a command, and each command is
/// applied first and saved only when it worked. A change the laptop took
/// is kept in memory even if its save fails.
pub struct App<C, D, R, K> {
    model: &'static ModelProfile,
    config: Config,
    config_path: PathBuf,
    machine: Machine<C, D, R, K>,
    battery_rate: BatteryRate,
    on_battery: bool,
    /// The lid logo state, read after the settings are sent at start and
    /// after each resume. `None` if the model has none or it could not be
    /// read.
    logo_on: Option<bool>,
    /// The battery charge limit state, read like the logo's.
    charge_limit_on: Option<bool>,
    /// Whether the frame the app last painted has the touchpad key red.
    /// `None` if unknown, such as after a failed send: the next change
    /// then paints it.
    key_red: Option<bool>,
    /// The controller reads that failed when the header was last made.
    read_errors: Vec<String>,
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
            on_battery: false,
            logo_on: None,
            charge_limit_on: None,
            key_red: None,
            read_errors: Vec::new(),
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

    /// Whether the touchpad key should be red. If the touchpad state can't
    /// be read, the key keeps the plain colour.
    fn touchpad_off(&self) -> bool {
        touchpad_on(&self.machine.registry).map_or_else(
            |error| {
                report(&format!("the touchpad key stays plain: {error}"));
                false
            },
            |on| !on,
        )
    }

    fn read_charge_limit(&mut self) {
        self.charge_limit_on = charge_limit_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
    }

    /// The config with the preset that runs in place of its saved one.
    fn running(&self, config: Config) -> Config {
        Config {
            preset: config.preset.running(self.on_battery),
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
