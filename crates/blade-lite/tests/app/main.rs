//! What each click in the window does, over fakes.

mod battery;
mod brightness_keys;
mod display;
mod fakes;
mod firmware;
mod header;
mod keyboard;
mod logo;
mod mic_key;
mod power_key;
mod presets;
mod settings;
mod touchpad_key;
mod weak_charge;

use std::path::{Path, PathBuf};

use blade_core::{Config, Preset};
use razer_proto::{BLADE_18, ChargeLimit, Command, NativeMode, Rgb, static_colour_touchpad_off};
use tempfile::TempDir;

use fakes::{SET_CHARGE_LIMIT, TestApp, app_on, brightness_commands, lighting_commands, logo};

const FHD: NativeMode = BLADE_18.native_modes[1];
const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);

const LIMIT_ON: u8 = 0xD0;
const LIMIT_OFF: u8 = 0x50;

fn charge_limit() -> ChargeLimit {
    BLADE_18.charge_limit.unwrap()
}

fn charge_limit_on(app: &TestApp) -> Option<bool> {
    app.performance_view().charge_limit.map(|switch| switch.on)
}

fn app_with(config: Config) -> (TestApp, TempDir, PathBuf) {
    app_on(&BLADE_18, config)
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

/// As [`lighting_commands`], with the touchpad key red.
fn red_lighting_commands(percent: u8, colour: Rgb) -> Vec<Command> {
    let mut commands = brightness_commands(percent, true);
    commands.extend(static_colour_touchpad_off(colour, &BLADE_18));
    commands
}

/// A folder in the file's place makes the save fail.
fn break_save(path: &Path) {
    std::fs::create_dir(path).unwrap();
}

fn performance_app() -> (TestApp, TempDir, PathBuf) {
    app_with(Config {
        preset: Preset::Performance,
        ..Config::default()
    })
}
