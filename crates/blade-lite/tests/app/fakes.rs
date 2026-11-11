//! Fakes for the laptop's parts, shared by the app tests and the window
//! tests in the app's binary.

use std::io;
use std::path::PathBuf;

use blade_core::{Config, Controller, Query};
use blade_lite::{App, Machine};
use platform::{
    Display, DisplayApi, DisplayMode, Key, KeyInjector, ModeLife, OutputTechnology, Registry,
};
use razer_device::DeviceError;
use razer_proto::{
    BLADE_18, Brightness, Celsius, ChargeLimit, Charger, Command, Logo, ModelProfile, NativeMode,
    Rgb, static_colour,
};
use tempfile::TempDir;

pub const SET_CHARGE_LIMIT: u16 = 0x0712;
pub const SET_LOGO: u16 = 0x0300;

pub fn logo() -> Logo {
    BLADE_18.logo.unwrap()
}

/// What the charger read gives, as read on the laptop.
#[derive(Debug, Default, Clone, Copy)]
pub enum FakeCharger {
    #[default]
    Razer,
    /// A USB-C monitor that powers the laptop: a weak charge.
    UsbC,
    Fails,
}

/// Records the commands it runs. It answers the native mode query with
/// `native_byte`, the logo query with `logo_reply`, the charge limit query with
/// `charge`, the brightness query with `brightness_level` and the
/// temperature query with `cpu_celsius`, the charger query with the Razer
/// `charger`, and fails
/// every run while `fails` is set. A logo command
/// changes `logo_reply`. While `fails_after` is set, a run takes only
/// that many commands, then fails.
#[derive(Default)]
pub struct FakeController {
    pub sent: Vec<Command>,
    pub fails: bool,
    /// Fails this many runs before anything is sent, as a held or closed
    /// handle does (`NotConnected`).
    pub not_connected_runs: usize,
    /// How many runs were asked for, failed ones too.
    pub runs: usize,
    /// Fails the native mode query, as a Busy reply.
    pub native_fails: bool,
    /// The native mode byte it reads back: 0 is the first mode.
    pub native_byte: u8,
    /// Takes this many commands of each run, then fails it, as a Busy
    /// reply to a command the controller still ran. It stays set, and a
    /// run of `n` or fewer commands still fails.
    pub fails_after: Option<usize>,
    /// Takes this many runs, then fails each run after them, as a Busy
    /// reply before anything is sent.
    pub ok_runs: Option<usize>,
    /// `None` fails the logo query.
    pub logo_reply: Option<Vec<u8>>,
    /// The charge limit byte. A charge limit command changes it, unless
    /// `charge_stuck` is set. `None` fails the query.
    pub charge: Option<u8>,
    pub charge_stuck: bool,
    /// The keyboard brightness level the controller reads back. `None`
    /// fails the query.
    pub brightness_level: Option<u8>,
    /// The CPU temperature the controller reads back. `None` fails the query.
    pub cpu_celsius: Option<u8>,
    pub charger: FakeCharger,
    /// Every query asked, in order.
    pub asked: Vec<Command>,
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        self.runs += 1;
        if self.not_connected_runs > 0 {
            self.not_connected_runs -= 1;
            let closed = std::io::Error::new(std::io::ErrorKind::NotConnected, "held");
            return Err(DeviceError::Io(closed));
        }
        if self.fails {
            return Err(DeviceError::Busy);
        }
        if let Some(left) = self.ok_runs.as_mut() {
            if *left == 0 {
                return Err(DeviceError::Busy);
            }
            *left -= 1;
        }
        let taken = self
            .fails_after
            .map_or(commands, |n| commands.get(..n).unwrap_or(commands));
        for command in taken {
            if command.id == SET_CHARGE_LIMIT && !self.charge_stuck {
                self.charge = Some(command.args[0]);
            }
            if command.id == SET_LOGO {
                self.logo_reply = Some(command.args.clone());
            }
        }
        self.sent.extend_from_slice(taken);
        match self.fails_after {
            Some(_) => Err(DeviceError::Busy),
            None => Ok(()),
        }
    }
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        self.asked.push(command.clone());
        if *command == NativeMode::query() {
            if self.native_fails {
                Err(DeviceError::Busy)
            } else {
                Ok(vec![self.native_byte])
            }
        } else if *command == logo().query() {
            self.logo_reply.clone().ok_or(DeviceError::Busy)
        } else if *command == ChargeLimit::query() {
            self.charge.map(|byte| vec![byte]).ok_or(DeviceError::Busy)
        } else if Some(command) == Brightness::query(&BLADE_18).as_ref() {
            let led = BLADE_18.brightness_leds[0];
            self.brightness_level
                .map(|level| vec![1, led, level])
                .ok_or(DeviceError::Busy)
        } else if *command == Charger::query() {
            match self.charger {
                FakeCharger::Razer => Ok(vec![0x13, 0x13]),
                FakeCharger::UsbC => Ok(vec![0x04, 0x13]),
                FakeCharger::Fails => Err(DeviceError::Busy),
            }
        } else if *command == Celsius::query() {
            self.cpu_celsius
                .map(|celsius| vec![2, celsius, 0, 0])
                .ok_or(DeviceError::Busy)
        } else {
            Err(DeviceError::Busy)
        }
    }
}

/// The panel at 3840 × 2400, 60 to 240 Hz, and how many of the next sets
/// fail (Windows refuses one just after a wake), and the mode Windows has
/// saved.
pub struct FakeDisplays(pub Display, pub usize, pub Option<DisplayMode>);

impl Default for FakeDisplays {
    fn default() -> Self {
        let mode = |hz| DisplayMode {
            width: 3840,
            height: 2400,
            hz,
        };
        Self(
            Display {
                name: r"\\.\DISPLAY1".into(),
                technology: OutputTechnology::Internal,
                current: mode(240),
                modes: vec![mode(60), mode(120), mode(240)],
            },
            0,
            None,
        )
    }
}

impl DisplayApi for FakeDisplays {
    fn saved_mode(&self, _name: &str) -> Option<DisplayMode> {
        self.2
    }

    fn displays(&self) -> io::Result<Vec<Display>> {
        Ok(vec![self.0.clone()])
    }

    fn set_mode(&mut self, _name: &str, mode: DisplayMode, _life: ModeLife) -> io::Result<()> {
        if self.1 > 0 {
            self.1 -= 1;
            return Err(io::ErrorKind::Other.into());
        }
        self.0.current = mode;
        Ok(())
    }
}

/// The touchpad state: 1 is on. `None` fails the read.
pub struct FakeRegistry {
    pub touchpad: Option<u32>,
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
pub struct FakeKeys(pub Vec<Key>);

impl KeyInjector for FakeKeys {
    fn press(&mut self, keys: &[Key]) -> io::Result<()> {
        self.0.extend_from_slice(keys);
        Ok(())
    }
}

pub type TestApp = App<FakeController, FakeDisplays, FakeRegistry, FakeKeys>;

pub fn app_on(model: &'static ModelProfile, config: Config) -> (TestApp, TempDir, PathBuf) {
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

/// The brightness commands: the keyboard, then the vapor light.
pub fn brightness_commands(percent: u8, vapor_on: bool) -> Vec<Command> {
    let brightness = Brightness::new(percent).unwrap();
    let mut commands = brightness.commands(&BLADE_18);
    commands.push(BLADE_18.vapor_light.unwrap().command(brightness, vapor_on));
    commands
}

pub fn lighting_commands(percent: u8, colour: Rgb) -> Vec<Command> {
    let mut commands = brightness_commands(percent, true);
    commands.extend(static_colour(colour, &BLADE_18));
    commands
}
