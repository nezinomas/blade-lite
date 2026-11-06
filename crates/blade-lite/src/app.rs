//! The app's state, the views built from it, and what each click does.

use std::path::PathBuf;

use blade_core::{
    ApplyError, BatteryRate, ChargeError, Config, ConfigError, Controller, DisplayError, Lighting,
    LogoError, Preset, Query, Readings, TouchpadError, apply, apply_lighting, apply_performance,
    charge_limit_on, chosen_native_mode, current_rate, logo_on, native_mode_state, read_controller,
    refresh_rates, set_charge_limit, set_logo, set_native_mode, set_refresh_rate, set_touchpad,
    touchpad_on,
};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};
use razer_device::DeviceError;
use razer_proto::{Brightness, ModelProfile, Rgb};
use ui::{
    DisplayCommand, DisplayState, DisplayView, Header, HeaderState, KeyboardCommand, KeyboardState,
    KeyboardView, PerformanceCommand, PerformanceState, PerformanceView,
};

/// What the window shows before the user sets the lighting.
const DEFAULT_LIGHTING: Lighting = Lighting {
    brightness: 100,
    colour: Rgb::new(0xFF, 0xFF, 0xFF),
};

/// Why a click or a view failed.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Display(#[from] DisplayError),
    #[error(transparent)]
    Touchpad(#[from] TouchpadError),
    #[error(transparent)]
    Logo(#[from] LogoError),
    #[error(transparent)]
    Charge(#[from] ChargeError),
    #[error("the controller did not answer: {0:?}")]
    Device(DeviceError),
    #[error("the controller chose a native mode the model lacks")]
    UnknownNativeMode,
    #[error("this preset is not offered on battery: {0:?}")]
    NotOnBattery(Preset),
}

impl From<DeviceError> for AppError {
    fn from(error: DeviceError) -> Self {
        Self::Device(error)
    }
}

/// The parts of the laptop the app talks to.
pub struct Machine<C, D, R, K> {
    pub controller: C,
    pub displays: D,
    pub registry: R,
    pub keys: K,
}

/// The app's state. Each click becomes a command, and each command is
/// applied first and saved only when it worked.
pub struct App<C, D, R, K> {
    model: &'static ModelProfile,
    config: Config,
    config_path: PathBuf,
    machine: Machine<C, D, R, K>,
    battery_rate: BatteryRate,
    on_battery: bool,
    /// The lid logo state, read at start and after each resume. `None` if
    /// the model has none or it could not be read.
    logo_on: Option<bool>,
    /// The battery charge limit state, read like the logo's.
    charge_limit_on: Option<bool>,
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

    /// Reads the logo and charge limit states, then sends the saved
    /// settings, with the preset that runs in place of the saved one on
    /// battery.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn start(&mut self) -> Result<(), AppError> {
        self.logo_on = logo_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
        self.read_charge_limit();
        let config = self.running(self.config);
        Ok(apply(&config, self.model, &mut self.machine.controller)?)
    }

    /// The header, with the controller's readings added to `readings`.
    pub fn header(&mut self, readings: Readings) -> Header {
        let controller = read_controller(&mut self.machine.controller, self.model);
        Header::new(&HeaderState {
            model: self.model,
            preset: self.config.preset,
            on_battery: self.on_battery,
            readings,
            cpu_celsius: controller.cpu,
            fans: controller.fans,
        })
    }

    #[must_use]
    pub fn performance_view(&self) -> PerformanceView {
        PerformanceView::new(&PerformanceState {
            model: self.model,
            preset: self.config.preset,
            on_battery: self.on_battery,
            custom: self.config.custom,
            charge_limit_on: self.charge_limit_on,
        })
    }

    /// # Errors
    ///
    /// See [`AppError`].
    pub fn display_view(&mut self) -> Result<DisplayView, AppError> {
        let chosen = chosen_native_mode(&mut self.machine.controller, self.model)?
            .ok_or(AppError::UnknownNativeMode)?;
        let displays = &self.machine.displays;
        Ok(DisplayView::new(&DisplayState {
            model: self.model,
            offered: refresh_rates(displays)?,
            current: current_rate(displays)?,
            native: native_mode_state(chosen, self.model, displays)?,
            sixty_hz_on_battery: self.config.sixty_hz_on_battery,
            on_battery: self.on_battery,
        }))
    }

    /// The logo switch is hidden if its state could not be read.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn keyboard_view(&self) -> Result<KeyboardView, AppError> {
        let lighting = self.lighting();
        let brightness = Brightness::new(lighting.brightness).map_err(ApplyError::Brightness)?;
        Ok(KeyboardView::new(&KeyboardState {
            brightness,
            colour: lighting.colour,
            touchpad_on: touchpad_on(&self.machine.registry)?,
            logo_on: self.logo_on,
        }))
    }

    /// # Errors
    ///
    /// See [`AppError`]. The config is left as it was.
    pub fn on_performance(&mut self, command: PerformanceCommand) -> Result<(), AppError> {
        let mut config = self.config;
        match command {
            PerformanceCommand::SetPreset(preset) => {
                if self.on_battery && !preset.offered_on_battery() {
                    return Err(AppError::NotOnBattery(preset));
                }
                config.preset = preset;
            }
            PerformanceCommand::SetCpuBoost(level) => config.custom.cpu_boost = level,
            PerformanceCommand::SetGpuBoost(level) => config.custom.gpu_boost = level,
            PerformanceCommand::SetFanRpm(rpm) => config.custom.fan_rpm = rpm,
            // The controller keeps the limit, so nothing is saved.
            PerformanceCommand::SetChargeLimit(on) => return self.set_charge_limit(on),
        }
        let running = self.running(config);
        apply_performance(&running, self.model, &mut self.machine.controller)?;
        self.commit(config)
    }

    /// # Errors
    ///
    /// See [`AppError`]. The config is left as it was.
    pub fn on_keyboard(&mut self, command: KeyboardCommand) -> Result<(), AppError> {
        let mut lighting = self.lighting();
        match command {
            KeyboardCommand::SetBrightness(brightness) => {
                lighting.brightness = brightness.percent();
            }
            KeyboardCommand::SetColour(colour) => lighting.colour = colour,
            KeyboardCommand::SetTouchpad(on) => {
                let machine = &mut self.machine;
                return Ok(set_touchpad(on, &machine.registry, &mut machine.keys)?);
            }
            // The controller keeps the logo state, so nothing is saved.
            KeyboardCommand::SetLogo(on) => {
                set_logo(on, &mut self.machine.controller, self.model)?;
                self.logo_on = Some(on);
                return Ok(());
            }
        }
        apply_lighting(lighting, self.model, &mut self.machine.controller)?;
        self.commit(Config {
            lighting: Some(lighting),
            ..self.config
        })
    }

    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_display(&mut self, command: DisplayCommand) -> Result<(), AppError> {
        match command {
            DisplayCommand::SetRefreshRate(rate) => {
                set_refresh_rate(&mut self.machine.displays, rate)?;
            }
            DisplayCommand::SetNativeMode(mode) => {
                set_native_mode(mode, &mut self.machine.controller)?;
            }
            DisplayCommand::SetSixtyHzOnBattery(on) => {
                self.commit(Config {
                    sixty_hz_on_battery: on,
                    ..self.config
                })?;
                if self.on_battery {
                    // As if the laptop had just gone on battery, or back on AC.
                    let event = if on {
                        PowerEvent::OnBattery
                    } else {
                        PowerEvent::OnAc
                    };
                    self.follow_power(event)?;
                }
            }
        }
        Ok(())
    }

    /// Follows the power source, and applies everything again after a
    /// resume. On battery only Silent or Balanced runs; the saved preset
    /// comes back on AC. The caller waits [`blade_core::RESUME_PAUSE`]
    /// before passing on a resume.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn on_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        match event {
            PowerEvent::Resumed => self.start(),
            PowerEvent::OnBattery | PowerEvent::OnAc => {
                self.set_on_battery(event == PowerEvent::OnBattery)?;
                self.follow_power(event)
            }
        }
    }

    /// Sets the charge limit. If that fails, the switch shows what the
    /// controller reads back, or hides if that read fails too.
    fn set_charge_limit(&mut self, on: bool) -> Result<(), AppError> {
        let result = set_charge_limit(on, &mut self.machine.controller, self.model);
        match result {
            Ok(()) => self.charge_limit_on = Some(on),
            Err(_) => self.read_charge_limit(),
        }
        Ok(result?)
    }

    fn read_charge_limit(&mut self) {
        self.charge_limit_on = charge_limit_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
    }

    /// Sends the preset again if the power source changes the one that runs.
    fn set_on_battery(&mut self, on_battery: bool) -> Result<(), AppError> {
        let before = self.running(self.config).preset;
        self.on_battery = on_battery;
        let config = self.running(self.config);
        if config.preset != before {
            apply_performance(&config, self.model, &mut self.machine.controller)?;
        }
        Ok(())
    }

    /// The config with the preset that runs in place of its saved one.
    fn running(&self, config: Config) -> Config {
        Config {
            preset: config.preset.running(self.on_battery),
            ..config
        }
    }

    fn follow_power(&mut self, event: PowerEvent) -> Result<(), AppError> {
        let displays = &mut self.machine.displays;
        Ok(self.battery_rate.on_event(event, &self.config, displays)?)
    }

    fn lighting(&self) -> Lighting {
        self.config.lighting.unwrap_or(DEFAULT_LIGHTING)
    }

    fn commit(&mut self, config: Config) -> Result<(), AppError> {
        config.save(&self.config_path)?;
        self.config = config;
        Ok(())
    }
}
