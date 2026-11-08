//! The views the windows show, built from the app's state.

use blade_core::{
    ApplyError, Controller, Preset, Query, Readings, chosen_native_mode, current_rate,
    native_mode_state, read_controller, refresh_rates, touchpad_on,
};
use platform::{DisplayApi, KeyInjector, Registry};
use razer_proto::Brightness;
use ui::{
    DisplayState, DisplayView, Header, HeaderState, KeyboardState, KeyboardView, PerformanceState,
    PerformanceView,
};

use super::{App, AppError};

impl<C, D, R, K> App<C, D, R, K>
where
    C: Controller + Query,
    D: DisplayApi,
    R: Registry,
    K: KeyInjector,
{
    /// The header, with the controller's readings added to `readings`.
    /// The reads that failed are kept in [`read_errors`](Self::read_errors).
    pub fn header(&mut self, readings: Readings) -> Header {
        let controller = read_controller(&mut self.machine.controller, self.model);
        self.read_errors = controller.errors;
        Header::new(&HeaderState {
            model: self.model,
            preset: self.config.preset,
            on_battery: self.on_battery,
            readings,
            cpu_celsius: controller.cpu,
            fans: controller.fans,
        })
    }

    /// One line per controller read that failed when the header was last
    /// made, such as `fan 1: Busy`.
    #[must_use]
    pub fn read_errors(&self) -> &[String] {
        &self.read_errors
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

    /// The preset that runs now: on battery a preset not offered there
    /// runs as another one.
    #[must_use]
    pub fn running_preset(&self) -> Preset {
        self.config.preset.running(self.on_battery)
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

    /// The saved vapor light switch, or `None` if the model has no light.
    #[must_use]
    pub fn vapor_light_on(&self) -> Option<bool> {
        self.model.vapor_light.map(|_| self.lighting().vapor_light)
    }
}
