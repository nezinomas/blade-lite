//! What each click in the window does, over fakes.

use std::io;
use std::path::PathBuf;

use blade_core::{Config, Controller, Lighting, Preset, Query, Readings, RefreshRate};
use blade_lite::{App, Machine};
use platform::{
    Display, DisplayApi, DisplayMode, Key, KeyInjector, OutputTechnology, PowerEvent, Registry,
};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Brightness, Command, NativeMode, PerfMode, Rgb, static_colour};
use tempfile::TempDir;
use ui::{DisplayCommand, KeyboardCommand, PerformanceCommand, PowerPill};

const FHD: NativeMode = BLADE_18.native_modes[1];
const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);

/// Records the commands it runs. It answers the native mode query with
/// mode 0, and fails everything while `fails` is set.
#[derive(Default)]
struct FakeController {
    sent: Vec<Command>,
    fails: bool,
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        self.sent.extend_from_slice(commands);
        Ok(())
    }
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        if *command == NativeMode::query() {
            Ok(vec![0])
        } else {
            Err(DeviceError::Busy)
        }
    }
}

/// The panel at 3840 × 2400, 60 to 240 Hz.
struct FakeDisplays(Display);

impl Default for FakeDisplays {
    fn default() -> Self {
        let mode = |hz| DisplayMode {
            width: 3840,
            height: 2400,
            hz,
        };
        Self(Display {
            name: r"\\.\DISPLAY1".into(),
            technology: OutputTechnology::Internal,
            current: mode(240),
            modes: vec![mode(60), mode(120), mode(240)],
        })
    }
}

impl DisplayApi for FakeDisplays {
    fn displays(&self) -> io::Result<Vec<Display>> {
        Ok(vec![self.0.clone()])
    }

    fn set_mode(&mut self, _name: &str, mode: DisplayMode) -> io::Result<()> {
        self.0.current = mode;
        Ok(())
    }
}

/// The touchpad is on.
struct FakeRegistry;

impl Registry for FakeRegistry {
    fn read_user_dword(&self, _key: &str, _name: &str) -> io::Result<u32> {
        Ok(1)
    }
}

#[derive(Default)]
struct FakeKeys(Vec<Key>);

impl KeyInjector for FakeKeys {
    fn press(&mut self, keys: &[Key]) -> io::Result<()> {
        self.0.extend_from_slice(keys);
        Ok(())
    }
}

type TestApp = App<FakeController, FakeDisplays, FakeRegistry, FakeKeys>;

fn app_with(config: Config) -> (TestApp, TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let machine = Machine {
        controller: FakeController::default(),
        displays: FakeDisplays::default(),
        registry: FakeRegistry,
        keys: FakeKeys::default(),
    };
    (
        App::new(&BLADE_18, config, path.clone(), machine),
        dir,
        path,
    )
}

fn app() -> (TestApp, TempDir, PathBuf) {
    app_with(Config::default())
}

fn sent(app: &TestApp) -> &[Command] {
    &app.machine().controller.sent
}

fn panel_hz(app: &TestApp) -> u32 {
    app.machine().displays.0.current.hz
}

fn lighting_commands(percent: u8, colour: Rgb) -> Vec<Command> {
    let mut commands = Brightness::new(percent).unwrap().commands(&BLADE_18);
    commands.extend(static_colour(colour, &BLADE_18));
    commands
}

#[test]
fn start_applies_the_saved_settings() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn picking_a_preset_sends_it_saves_it_and_shows_it() {
    let (mut app, _dir, path) = app();

    app.on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(Config::load(&path).unwrap().preset, Preset::Silent);
    let view = app.performance_view();
    let selected = view.presets.iter().find(|option| option.selected).unwrap();
    assert_eq!(selected.preset, Preset::Silent);
}

#[test]
fn a_preset_the_controller_refuses_is_neither_kept_nor_saved() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.fails = true;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Silent));

    assert!(result.is_err());
    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(!path.exists());
}

#[test]
fn a_custom_slider_sends_the_custom_values() {
    let (mut app, _dir, path) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });

    app.on_performance(PerformanceCommand::SetCpuBoost(3))
        .unwrap();

    assert_eq!(Config::load(&path).unwrap().custom.cpu_boost, 3);
    // The four zones, then the CPU boost first among the boosts.
    assert_eq!(sent(&app)[4].args, [1, 1, 3]);
}

#[test]
fn brightness_sends_and_saves_the_lighting() {
    let (mut app, _dir, path) = app();
    let half = Brightness::new(50).unwrap();

    app.on_keyboard(KeyboardCommand::SetBrightness(half))
        .unwrap();

    assert_eq!(sent(&app), lighting_commands(50, WHITE));
    let saved = Config::load(&path).unwrap().lighting;
    assert_eq!(
        saved,
        Some(Lighting {
            brightness: 50,
            colour: WHITE
        })
    );
    assert_eq!(app.keyboard_view().unwrap().brightness, 50);
}

#[test]
fn a_colour_keeps_the_saved_brightness() {
    let red = Rgb::new(0xE8, 0x11, 0x23);
    let (mut app, _dir, _) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
        }),
        ..Config::default()
    });

    app.on_keyboard(KeyboardCommand::SetColour(red)).unwrap();

    assert_eq!(sent(&app), lighting_commands(30, red));
}

#[test]
fn the_touchpad_switch_sends_the_shortcut() {
    let (mut app, _dir, _) = app();

    app.on_keyboard(KeyboardCommand::SetTouchpad(false))
        .unwrap();

    assert_eq!(app.machine().keys.0, [Key::Win, Key::Ctrl, Key::F24]);
}

#[test]
fn the_keyboard_view_shows_the_touchpad_state() {
    let (app, _dir, _) = app();

    assert!(app.keyboard_view().unwrap().touchpad_on);
}

#[test]
fn a_rate_button_switches_the_panel() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetRefreshRate(RefreshRate(120)))
        .unwrap();

    assert_eq!(panel_hz(&app), 120);
}

#[test]
fn the_display_view_shows_the_chosen_native_mode() {
    let (mut app, _dir, _) = app();

    let view = app.display_view().unwrap();

    let selected = view.native_modes.iter().find(|o| o.selected).unwrap();
    assert_eq!(selected.mode, BLADE_18.native_modes[0]);
    assert!(!view.restart_infobar);
}

#[test]
fn a_native_mode_is_sent_to_the_controller() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetNativeMode(FHD)).unwrap();

    assert_eq!(sent(&app), [FHD.command()]);
}

#[test]
fn turning_on_sixty_hz_on_battery_while_on_battery_drops_the_rate() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();

    assert!(Config::load(&path).unwrap().sixty_hz_on_battery);
    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn turning_it_off_on_battery_puts_the_rate_back() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(false))
        .unwrap();

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn the_power_source_drives_the_rate_and_the_pill() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });

    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(panel_hz(&app), 60);
    assert_eq!(app.header(Readings::default()).power, PowerPill::OnBattery);

    app.on_power(PowerEvent::OnAc).unwrap();
    assert_eq!(panel_hz(&app), 240);
    assert_eq!(app.header(Readings::default()).power, PowerPill::PluggedIn);
}

#[test]
fn a_resume_applies_everything_again() {
    let (mut app, _dir, _) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 10,
            colour: WHITE,
        }),
        ..Config::default()
    });

    app.on_power(PowerEvent::Resumed).unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(10, WHITE));
    assert_eq!(sent(&app), expected);
}

#[test]
fn readings_the_controller_cannot_give_show_dashes() {
    let (mut app, _dir, _) = app();

    let header = app.header(Readings::default());

    let fan = header.stats.iter().find(|s| s.label == "Fan 1").unwrap();
    assert_eq!(fan.value, ui::MISSING);
}
