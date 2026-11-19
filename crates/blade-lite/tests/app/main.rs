//! What each click in the window does, over fakes.

mod battery;
mod brightness_keys;
mod charger_lighting;
mod display;
mod fakes;
mod fan_guard;
mod fan_targets;
mod firmware;
mod header;
mod keyboard;
mod logo;
mod manual_fan_unsure;
mod mic_key;
mod power_key;
mod presets;
mod props;
mod rules;
mod screen_off;
mod settings;
mod start_manual_fan;
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
    &app.machine().controller.inner.sent
}

/// The two steps that end a manual fan (ticket 298): Custom with the fans
/// on auto and the lowest boosts, then `mode`.
fn auto_fans_then(mode: razer_proto::PerfMode) -> Vec<Command> {
    let mut sent = razer_proto::Custom {
        cpu: razer_proto::CpuBoost::new(0, &BLADE_18).unwrap(),
        gpu: razer_proto::GpuBoost::new(0, &BLADE_18).unwrap(),
        fan: razer_proto::FanMode::Auto,
    }
    .commands(&BLADE_18);
    sent.extend(mode.commands(&BLADE_18).unwrap());
    sent
}

/// Two ticks [`CHARGER_STEADY`](blade_lite::CHARGER_STEADY) apart: the
/// full charger back has waited its time (ticket 263).
fn charger_steady(app: &mut TestApp) {
    let now = std::time::Instant::now();
    app.follow_charger_back(now).unwrap();
    app.follow_charger_back(now + blade_lite::CHARGER_STEADY)
        .unwrap();
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
