//! The app's state, the views built from it, and what each click does.

use std::path::PathBuf;

use blade_core::{
    ApplyError, BatteryRate, ChargeError, Config, ConfigError, Controller, DisplayError, Lighting,
    LogoError, Preset, Query, Readings, TouchpadError, apply, apply_lighting, apply_performance,
    charge_limit_on, chosen_native_mode, current_rate, logo_on, native_mode_state, next_preset,
    paint_touchpad_key, read_controller, refresh_rates, set_charge_limit, set_logo,
    set_native_mode, set_refresh_rate, set_touchpad, touchpad_on,
};
use platform::{DisplayApi, KeyInjector, PowerEvent, Registry};
use razer_device::DeviceError;
use razer_proto::{Brightness, ModelProfile, Rgb};
use ui::{
    DisplayCommand, DisplayState, DisplayView, Header, HeaderState, KeyboardCommand, KeyboardState,
    KeyboardView, PerformanceCommand, PerformanceState, PerformanceView,
};

use crate::report;

/// What the window shows before the user sets the lighting.
const DEFAULT_LIGHTING: Lighting = Lighting {
    brightness: 100,
    colour: Rgb::new(0xFF, 0xFF, 0xFF),
    vapor_light: true,
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
/// applied first and saved only when it worked. A logo switch that worked
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

    /// Reads the charge limit state, sends the saved settings, then reads
    /// the logo state they leave. The preset that runs in place of the
    /// saved one on battery is sent. The touchpad key is red in that frame
    /// if the touchpad is off.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn start(&mut self) -> Result<(), AppError> {
        self.read_charge_limit();
        let config = self.running(self.config);
        let red = self.touchpad_off();
        let applied = apply(&config, self.model, &mut self.machine.controller, red);
        self.key_red = applied.is_ok().then_some(red);
        self.logo_on = logo_on(&mut self.machine.controller, self.model)
            .ok()
            .flatten();
        Ok(applied?)
    }

    /// Paints the touchpad key red if the touchpad is now off, or back in
    /// the lighting colour if it is on. Nothing is sent if the key already
    /// shows the state, or the user has not set the lighting, since the
    /// app then leaves the keyboard's own effect alone.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. Nothing is sent if the touchpad state can't be
    /// read.
    pub fn on_touchpad_changed(&mut self) -> Result<(), AppError> {
        let Some(lighting) = self.config.lighting else {
            return Ok(());
        };
        let red = !touchpad_on(&self.machine.registry)?;
        if self.key_red == Some(red) {
            return Ok(());
        }
        let painted = paint_touchpad_key(red, lighting, self.model, &mut self.machine.controller);
        self.key_red = painted.is_ok().then_some(red);
        Ok(painted?)
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

    /// The logo switch is hidden if its state could not be read, the vapor
    /// light switch if the model has none.
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
            vapor_light_on: self.vapor_light_on(),
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

    /// Steps to the next preset, as the power-mode key asks.
    ///
    /// # Errors
    ///
    /// See [`AppError`]. Nothing is sent if the model has no key, or no
    /// other preset to step to; the config is left as it was.
    pub fn on_power_key(&mut self) -> Result<(), AppError> {
        if self.model.power_key.is_none() {
            return Ok(());
        }
        let running = self.config.preset.running(self.on_battery);
        match next_preset(running, self.model, self.on_battery) {
            Some(next) if next != running => {
                self.on_performance(PerformanceCommand::SetPreset(next))
            }
            _ => Ok(()),
        }
    }

    /// # Errors
    ///
    /// See [`AppError`]. The config is left as it was, except that a logo
    /// switch that was sent but not saved is kept in memory.
    pub fn on_keyboard(&mut self, command: KeyboardCommand) -> Result<(), AppError> {
        let mut lighting = self.lighting();
        match command {
            KeyboardCommand::SetBrightness(brightness) => {
                lighting.brightness = brightness.percent();
            }
            KeyboardCommand::SetColour(colour) => lighting.colour = colour,
            // A model without the light is left alone.
            KeyboardCommand::SetVaporLight(_) if self.model.vapor_light.is_none() => {
                return Ok(());
            }
            KeyboardCommand::SetVaporLight(on) => lighting.vapor_light = on,
            KeyboardCommand::SetTouchpad(on) => {
                let machine = &mut self.machine;
                return Ok(set_touchpad(on, &machine.registry, &mut machine.keys)?);
            }
            // Saved, as the laptop turns the logo on again after a wake.
            // Kept in memory even if the save fails, so the next resume
            // sends what the laptop shows now.
            KeyboardCommand::SetLogo(on) => {
                set_logo(on, &mut self.machine.controller, self.model)?;
                self.logo_on = Some(on);
                self.config.lid_logo = Some(on);
                return Ok(self.config.save(&self.config_path)?);
            }
        }
        let red = self.touchpad_off();
        let applied = apply_lighting(lighting, self.model, &mut self.machine.controller, red);
        self.key_red = applied.is_ok().then_some(red);
        applied?;
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
            DisplayCommand::SetSixtyHzOnBattery(on) => self.set_sixty_hz_on_battery(on)?,
        }
        Ok(())
    }

    /// Flips the 60 Hz on battery switch. It reads the saved value, so it
    /// works while the controller or the panel can't be read.
    ///
    /// # Errors
    ///
    /// See [`AppError`].
    pub fn toggle_sixty_hz_on_battery(&mut self) -> Result<(), AppError> {
        self.set_sixty_hz_on_battery(!self.config.sixty_hz_on_battery)
    }

    /// Saves the switch and acts on it at once. If 60 Hz can't be set on
    /// battery, the old value is saved again, so the rates don't lock while
    /// the panel runs another rate.
    fn set_sixty_hz_on_battery(&mut self, on: bool) -> Result<(), AppError> {
        let old = self.config;
        if on == old.sixty_hz_on_battery {
            return Ok(());
        }
        self.commit(Config {
            sixty_hz_on_battery: on,
            ..old
        })?;
        if !self.on_battery {
            return Ok(());
        }
        // As if the laptop had just gone on battery, or back on AC.
        if !on {
            return self.follow_power(PowerEvent::OnAc);
        }
        let dropped = self.follow_power(PowerEvent::OnBattery);
        if dropped.is_err() {
            // The rate error is the one to show, even if this save fails too.
            let _ = self.commit(old);
        }
        dropped
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

    /// The saved vapor light switch, or `None` if the model has no light.
    #[must_use]
    pub fn vapor_light_on(&self) -> Option<bool> {
        self.model.vapor_light.map(|_| self.lighting().vapor_light)
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
