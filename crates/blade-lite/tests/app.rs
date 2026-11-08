//! What each click in the window does, over fakes.

use std::io;
use std::path::{Path, PathBuf};

use blade_core::{Config, Controller, Lighting, Preset, Query, Readings, RefreshRate};
use blade_lite::{App, Machine};
use platform::{
    Display, DisplayApi, DisplayMode, Key, KeyInjector, OutputTechnology, PowerEvent, Registry,
};
use razer_device::DeviceError;
use razer_proto::{
    BLADE_18, Brightness, ChargeLimit, Command, Logo, ModelProfile, NativeMode, PerfMode, Rgb,
    static_colour, static_colour_touchpad_off,
};
use tempfile::TempDir;
use ui::{DisplayCommand, KeyboardCommand, PerformanceCommand, PowerPill};

const FHD: NativeMode = BLADE_18.native_modes[1];
const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);

const SET_CHARGE_LIMIT: u16 = 0x0712;
const SET_LOGO: u16 = 0x0300;
const LIMIT_ON: u8 = 0xD0;
const LIMIT_OFF: u8 = 0x50;

fn charge_limit() -> ChargeLimit {
    BLADE_18.charge_limit.unwrap()
}

fn charge_limit_on(app: &TestApp) -> Option<bool> {
    app.performance_view().charge_limit.map(|switch| switch.on)
}

fn logo() -> Logo {
    BLADE_18.logo.unwrap()
}

/// Records the commands it runs. It answers the native mode query with
/// mode 0, the logo query with `logo_reply` and the charge limit query with
/// `charge`, and fails every run while `fails` is set. A logo command
/// changes `logo_reply`.
#[derive(Default)]
struct FakeController {
    sent: Vec<Command>,
    fails: bool,
    /// `None` fails the logo query.
    logo_reply: Option<Vec<u8>>,
    /// The charge limit byte. A charge limit command changes it, unless
    /// `charge_stuck` is set. `None` fails the query.
    charge: Option<u8>,
    charge_stuck: bool,
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        for command in commands {
            if command.id == SET_CHARGE_LIMIT && !self.charge_stuck {
                self.charge = Some(command.args[0]);
            }
            if command.id == SET_LOGO {
                self.logo_reply = Some(command.args.clone());
            }
        }
        self.sent.extend_from_slice(commands);
        Ok(())
    }
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        if *command == NativeMode::query() {
            Ok(vec![0])
        } else if *command == logo().query() {
            self.logo_reply.clone().ok_or(DeviceError::Busy)
        } else if *command == ChargeLimit::query() {
            self.charge.map(|byte| vec![byte]).ok_or(DeviceError::Busy)
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

/// The touchpad state: 1 is on. `None` fails the read.
struct FakeRegistry {
    touchpad: Option<u32>,
}

impl Default for FakeRegistry {
    fn default() -> Self {
        Self { touchpad: Some(1) }
    }
}

impl Registry for FakeRegistry {
    fn read_user_dword(&self, _key: &str, _name: &str) -> io::Result<u32> {
        self.touchpad
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
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
    app_on(&BLADE_18, config)
}

fn app_on(model: &'static ModelProfile, config: Config) -> (TestApp, TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let machine = Machine {
        controller: FakeController::default(),
        displays: FakeDisplays::default(),
        registry: FakeRegistry::default(),
        keys: FakeKeys::default(),
    };
    (App::new(model, config, path.clone(), machine), dir, path)
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

/// The brightness commands: the keyboard, then the vapor light.
fn brightness_commands(percent: u8, vapor_on: bool) -> Vec<Command> {
    let brightness = Brightness::new(percent).unwrap();
    let mut commands = brightness.commands(&BLADE_18);
    commands.push(BLADE_18.vapor_light.unwrap().command(brightness, vapor_on));
    commands
}

fn lighting_commands(percent: u8, colour: Rgb) -> Vec<Command> {
    let mut commands = brightness_commands(percent, true);
    commands.extend(static_colour(colour, &BLADE_18));
    commands
}

/// As [`lighting_commands`], with the touchpad key red.
fn red_lighting_commands(percent: u8, colour: Rgb) -> Vec<Command> {
    let mut commands = brightness_commands(percent, true);
    commands.extend(static_colour_touchpad_off(colour, &BLADE_18));
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

    let error = result.unwrap_err();
    assert!(!error.was_sent());
    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(!path.exists());
}

/// A folder in the file's place makes the save fail.
fn break_save(path: &Path) {
    std::fs::create_dir(path).unwrap();
}

#[test]
fn a_preset_sent_but_not_saved_is_kept_and_shown() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    let error = app
        .on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap_err();

    assert!(error.was_sent());
    assert_eq!(app.running_preset(), Preset::Silent);
    let view = app.performance_view();
    let selected = view.presets.iter().find(|option| option.selected).unwrap();
    assert_eq!(selected.preset, Preset::Silent);
}

#[test]
fn a_power_key_preset_not_saved_is_kept_for_the_next_press() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    assert!(app.on_power_key().unwrap_err().was_sent());
    let first = app.running_preset();
    assert_ne!(first, Preset::Balanced);
    assert!(app.on_power_key().unwrap_err().was_sent());

    assert_ne!(app.running_preset(), first);
}

#[test]
fn a_custom_value_sent_but_not_saved_is_kept() {
    let (mut app, _dir, path) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    break_save(&path);

    assert!(
        app.on_performance(PerformanceCommand::SetCpuBoost(3))
            .unwrap_err()
            .was_sent()
    );

    assert_eq!(app.config().custom.cpu_boost, 3);
}

#[test]
fn lighting_sent_but_not_saved_is_kept() {
    let (mut app, _dir, path) = app();
    break_save(&path);
    let half = Brightness::new(50).unwrap();

    let error = app
        .on_keyboard(KeyboardCommand::SetBrightness(half))
        .unwrap_err();

    assert!(error.was_sent());
    assert_eq!(app.keyboard_view().unwrap().brightness, 50);
}

#[test]
fn the_error_says_the_change_was_sent_but_not_saved() {
    let (mut app, _dir, path) = app();
    break_save(&path);

    let error = app
        .on_performance(PerformanceCommand::SetPreset(Preset::Silent))
        .unwrap_err();

    assert!(error.to_string().starts_with("sent, but not saved: "));
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
            colour: WHITE,
            vapor_light: true,
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
            vapor_light: true,
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

const GREEN: Rgb = Rgb::new(0x44, 0xD6, 0x2C);

/// An app with the lighting set and the touchpad off.
fn touchpad_off_app() -> (TestApp, TempDir, PathBuf) {
    let (mut app, dir, path) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 36,
            colour: GREEN,
            vapor_light: true,
        }),
        ..Config::default()
    });
    app.machine_mut().registry.touchpad = Some(0);
    (app, dir, path)
}

fn red_key() -> Vec<Command> {
    static_colour_touchpad_off(GREEN, &BLADE_18)
}

fn plain_key() -> Vec<Command> {
    static_colour(GREEN, &BLADE_18)
}

/// The commands sent after the first `count`.
fn sent_after(app: &TestApp, count: usize) -> &[Command] {
    &sent(app)[count..]
}

#[test]
fn start_paints_the_touchpad_key_red_if_the_touchpad_is_off() {
    let (mut app, _dir, _) = touchpad_off_app();

    app.start().unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(red_lighting_commands(36, GREEN));
    assert_eq!(sent(&app), expected);
}

#[test]
fn without_saved_lighting_the_keyboard_effect_is_left_alone() {
    let (mut app, _dir, _) = app();
    app.machine_mut().registry.touchpad = Some(0);

    app.start().unwrap();
    app.on_touchpad_changed().unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_touchpad_change_paints_the_key_red_or_back() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    let started = sent(&app).len();

    app.machine_mut().registry.touchpad = Some(0);
    app.on_touchpad_changed().unwrap();
    app.machine_mut().registry.touchpad = Some(1);
    app.on_touchpad_changed().unwrap();

    let mut expected = red_key();
    expected.extend(plain_key());
    assert_eq!(sent_after(&app, started), expected);
}

#[test]
fn a_change_that_keeps_the_touchpad_state_sends_nothing() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_touchpad_changed().unwrap();
    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, started), []);
}

#[test]
fn a_new_colour_keeps_the_touchpad_key_red() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_keyboard(KeyboardCommand::SetColour(WHITE)).unwrap();

    assert_eq!(sent_after(&app, started), red_lighting_commands(36, WHITE));
}

#[test]
fn a_resume_paints_the_touchpad_key_red_again() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    let started = sent(&app).len();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(sent_after(&app, started).last(), red_key().last());
    assert_eq!(sent(&app).len(), 2 * started);
}

#[test]
fn an_unreadable_touchpad_state_leaves_the_key_and_the_start_alone() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = None;

    app.start().unwrap();
    let started = sent(&app).len();
    assert!(app.on_touchpad_changed().is_err());

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(36, GREEN));
    assert_eq!(sent(&app), expected);
    assert_eq!(sent(&app).len(), started);
}

#[test]
fn after_a_failed_lighting_send_the_next_change_paints_the_key() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.start().unwrap();
    app.machine_mut().controller.fails = true;
    assert!(app.on_keyboard(KeyboardCommand::SetColour(WHITE)).is_err());
    app.machine_mut().controller.fails = false;
    let before = sent(&app).len();

    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, before), red_key());
}

#[test]
fn a_failed_key_paint_is_tried_again_on_the_next_change() {
    let (mut app, _dir, _) = touchpad_off_app();
    app.machine_mut().registry.touchpad = Some(1);
    app.start().unwrap();
    app.machine_mut().registry.touchpad = Some(0);
    app.machine_mut().controller.fails = true;
    assert!(app.on_touchpad_changed().is_err());
    app.machine_mut().controller.fails = false;
    let before = sent(&app).len();

    app.on_touchpad_changed().unwrap();

    assert_eq!(sent_after(&app, before), red_key());
}

#[test]
fn the_keyboard_view_shows_the_touchpad_state() {
    let (app, _dir, _) = app();

    assert!(app.keyboard_view().unwrap().touchpad_on);
}

#[test]
fn start_reads_the_logo_state_for_the_keyboard_view() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 0]);

    app.start().unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn the_keyboard_view_does_not_ask_the_controller_for_the_logo() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    // A later read would now fail, and the view would hide the switch.
    app.machine_mut().controller.logo_reply = None;

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn a_failed_logo_read_hides_the_switch_and_keeps_the_rest() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();
    let view = app.keyboard_view().unwrap();

    assert_eq!(view.logo_on, None);
    assert!(view.touchpad_on);
}

#[test]
fn a_resume_reads_the_logo_state_again() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 0]);
    app.start().unwrap();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn the_logo_switch_shows_the_new_state() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();

    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_failed_logo_switch_keeps_the_old_state() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn the_logo_switch_sends_the_logo_commands_and_saves_the_state() {
    let (mut app, _dir, path) = app();

    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();

    assert_eq!(sent(&app), logo().commands(false));
    let saved = Config {
        lid_logo: Some(false),
        ..Config::default()
    };
    assert_eq!(Config::load(&path).unwrap(), saved);
}

#[test]
fn a_logo_switch_that_cannot_be_saved_is_still_sent_at_resume() {
    let (mut app, _dir, path) = app_with(Config {
        lid_logo: Some(true),
        ..Config::default()
    });
    // A folder in the file's place makes the save fail.
    std::fs::create_dir(&path).unwrap();
    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
}

#[test]
fn a_failed_logo_switch_saves_nothing() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.fails = true;

    assert!(app.on_keyboard(KeyboardCommand::SetLogo(false)).is_err());

    assert!(!path.exists());
}

#[test]
fn a_resume_turns_a_saved_off_logo_off_again() {
    let (mut app, _dir, _) = app();
    app.start().unwrap();
    app.on_keyboard(KeyboardCommand::SetLogo(false)).unwrap();
    // The laptop turns the logo on again while asleep.
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
}

#[test]
fn a_failed_start_shows_the_logo_state_the_controller_reads() {
    let (mut app, _dir, _) = app_with(Config {
        lid_logo: Some(false),
        ..Config::default()
    });
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);
    app.machine_mut().controller.fails = true;

    assert!(app.start().is_err());

    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(true));
}

#[test]
fn start_sends_the_saved_logo_state() {
    let (mut app, _dir, _) = app_with(Config {
        lid_logo: Some(false),
        ..Config::default()
    });
    app.machine_mut().controller.logo_reply = Some(vec![0, 4, 1]);

    app.start().unwrap();

    assert!(sent(&app).ends_with(&logo().commands(false)));
    assert_eq!(app.keyboard_view().unwrap().logo_on, Some(false));
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
fn setting_sixty_hz_on_battery_to_what_it_is_saves_nothing() {
    for on in [false, true] {
        let (mut app, _dir, path) = app_with(Config {
            sixty_hz_on_battery: on,
            ..Config::default()
        });

        app.on_display(DisplayCommand::SetSixtyHzOnBattery(on))
            .unwrap();

        assert!(!path.exists());
    }
}

#[test]
fn turning_it_on_on_battery_when_60_hz_fails_keeps_it_off() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut()
        .displays
        .0
        .modes
        .retain(|mode| mode.hz != 60);

    assert!(
        app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
            .is_err()
    );

    assert!(!app.config().sixty_hz_on_battery);
    assert!(!Config::load(&path).unwrap().sixty_hz_on_battery);
    assert!(app.display_view().unwrap().rates_enabled);
    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn the_switch_flips_the_saved_value() {
    let (mut app, _dir, path) = app();

    app.toggle_sixty_hz_on_battery().unwrap();
    assert!(Config::load(&path).unwrap().sixty_hz_on_battery);

    app.toggle_sixty_hz_on_battery().unwrap();
    assert!(!Config::load(&path).unwrap().sixty_hz_on_battery);
}

#[test]
fn turning_it_on_twice_on_battery_keeps_the_rate_to_put_back() {
    let (mut app, _dir, _) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();
    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn turning_it_on_or_off_on_ac_leaves_the_rate() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();
    assert_eq!(panel_hz(&app), 240);
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
            vapor_light: true,
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

#[test]
fn the_header_keeps_the_controller_read_errors() {
    let (mut app, _dir, _) = app();

    app.header(Readings::default());

    assert!(
        app.read_errors().iter().any(|e| e.starts_with("fan 1: ")),
        "{:?}",
        app.read_errors()
    );
}

fn performance_app() -> (TestApp, TempDir, PathBuf) {
    app_with(Config {
        preset: Preset::Performance,
        ..Config::default()
    })
}

#[test]
fn on_battery_performance_runs_as_balanced_and_comes_back_on_ac() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Performance);

    app.on_power(PowerEvent::OnAc).unwrap();
    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(PerfMode::Performance.commands(&BLADE_18).unwrap());
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_power_event_that_keeps_the_running_preset_sends_nothing() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn performance_is_refused_on_battery() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_resume_on_battery_applies_balanced_for_a_saved_performance() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn start_reads_the_charge_limit_for_the_performance_view() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_failed_charge_limit_read_hides_the_switch_and_keeps_the_start() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), None);
    assert_ne!(sent(&app), []);
}

#[test]
fn a_resume_reads_the_charge_limit_again() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(charge_limit_on(&app), Some(false));
}

#[test]
fn the_charge_limit_switch_sends_its_command_and_saves_nothing() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(false))
        .unwrap();

    assert_eq!(sent(&app), [charge_limit().command(false).unwrap()]);
    assert_eq!(charge_limit_on(&app), Some(false));
    assert!(!path.exists());
}

#[test]
fn a_failed_charge_limit_switch_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_that_does_not_take_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.machine_mut().controller.charge_stuck = true;
    app.start().unwrap();

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_switch_does_not_touch_the_preset() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);
    app.start().unwrap();
    let before = *app.config();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(true))
        .unwrap();

    assert!(sent(&app).iter().all(|c| c.id == SET_CHARGE_LIMIT));
    assert_eq!(*app.config(), before);
}

#[test]
fn the_power_key_sends_the_next_preset_and_saves_it() {
    let (mut app, _dir, path) = app();

    app.on_power_key().unwrap();

    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
    assert_eq!(Config::load(&path).unwrap().preset, Preset::Performance);
    assert_eq!(
        app.performance_view()
            .presets
            .iter()
            .find(|o| o.selected)
            .unwrap()
            .preset,
        Preset::Performance
    );
}

#[test]
fn the_power_key_wraps_from_performance_to_silent() {
    let (mut app, _dir, _) = performance_app();

    app.on_power_key().unwrap();

    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn the_power_key_steps_from_custom_to_silent() {
    let (mut app, _dir, _) = app_with(Config {
        preset: Preset::Custom,
        ..Config::default()
    });

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
}

#[test]
fn on_battery_the_power_key_steps_from_the_running_preset() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();

    // Balanced runs in place of Performance, so Silent is next.
    app.on_power_key().unwrap();

    assert_eq!(sent(&app), PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Silent);
}

#[test]
fn a_power_key_step_the_controller_refuses_is_neither_kept_nor_saved() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.fails = true;

    assert!(app.on_power_key().is_err());
    assert_eq!(app.config().preset, Preset::Balanced);
    assert!(!path.exists());
}

static NO_POWER_KEY: ModelProfile = ModelProfile {
    power_key: None,
    ..BLADE_18
};

static ONE_PRESET: ModelProfile = ModelProfile {
    presets: &[PerfMode::Balanced],
    ..BLADE_18
};

#[test]
fn a_model_without_the_power_key_ignores_it() {
    let (mut app, _dir, path) = app_on(&NO_POWER_KEY, Config::default());

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_power_key_with_only_the_running_preset_sends_nothing() {
    let (mut app, _dir, path) = app_on(&ONE_PRESET, Config::default());

    app.on_power_key().unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

static NO_VAPOR_LIGHT: ModelProfile = ModelProfile {
    vapor_light: None,
    ..BLADE_18
};

/// An app lit at 30% in white, with the vapor light as given.
fn vapor_app(vapor_light: bool) -> (TestApp, TempDir, PathBuf) {
    app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light,
        }),
        ..Config::default()
    })
}

#[test]
fn the_vapor_light_switch_shows_the_saved_state() {
    assert_eq!(
        vapor_app(true).0.keyboard_view().unwrap().vapor_light_on,
        Some(true)
    );
    assert_eq!(
        vapor_app(false).0.keyboard_view().unwrap().vapor_light_on,
        Some(false)
    );
}

#[test]
fn the_vapor_light_switch_sends_and_saves_it() {
    let (mut app, _dir, path) = vapor_app(true);

    app.on_keyboard(KeyboardCommand::SetVaporLight(false))
        .unwrap();

    let mut expected = brightness_commands(30, false);
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
    let saved = Config::load(&path).unwrap().lighting.unwrap();
    assert!(!saved.vapor_light);
    assert_eq!(app.keyboard_view().unwrap().vapor_light_on, Some(false));
}

#[test]
fn a_brightness_change_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.on_keyboard(KeyboardCommand::SetBrightness(Brightness::new(80).unwrap()))
        .unwrap();

    let mut expected = brightness_commands(80, false);
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_failed_vapor_light_send_saves_nothing() {
    let (mut app, _dir, path) = vapor_app(true);
    app.machine_mut().controller.fails = true;

    assert!(
        app.on_keyboard(KeyboardCommand::SetVaporLight(false))
            .is_err()
    );

    assert!(app.config().lighting.unwrap().vapor_light);
    assert!(!path.exists());
}

#[test]
fn a_model_without_a_vapor_light_hides_the_switch_and_ignores_it() {
    let (mut app, _dir, path) = app_on(&NO_VAPOR_LIGHT, Config::default());

    assert_eq!(app.keyboard_view().unwrap().vapor_light_on, None);
    app.on_keyboard(KeyboardCommand::SetVaporLight(false))
        .unwrap();

    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_resume_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.on_power(PowerEvent::Resumed).unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(brightness_commands(30, false));
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
}

#[test]
fn start_keeps_the_vapor_light_off() {
    let (mut app, _dir, _) = vapor_app(false);

    app.start().unwrap();

    let vapor = BLADE_18
        .vapor_light
        .unwrap()
        .command(Brightness::new(30).unwrap(), false);
    assert!(sent(&app).contains(&vapor), "{:?}", sent(&app));
    assert_eq!(vapor.args, [0x01, 0x26, 0]);
}

#[test]
fn the_running_preset_is_the_one_the_power_source_lets_run() {
    let (mut app, _dir, _) = performance_app();
    assert_eq!(app.running_preset(), Preset::Performance);

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(app.running_preset(), Preset::Balanced);
}
