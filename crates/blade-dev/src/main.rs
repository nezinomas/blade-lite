//! `blade-dev probe`: finds the controller and reads its firmware version.
//! `blade-dev keys`: prints the Razer key reports, and the keyboard and
//! vapor light levels as they change, for a minute.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use blade_core::{APP_NAME, Start, start_instance};
use blade_dev::Command;
use platform::WindowsInstance;
use razer_device::{Device, DeviceError, HidKeyReports, HidTransport, SystemClock};
use razer_proto::BLADE_18;

/// How long `keys` runs.
const KEYS_RUN: Duration = Duration::from_secs(60);
/// Get LED brightness. Args are `[1, led, 0]`, the reply args `[1, led, level]`.
const GET_BRIGHTNESS: u16 = 0x0383;
/// How long one key read waits; the brightness is read between reads.
const KEY_READ_WAIT: Duration = Duration::from_millis(250);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match Command::parse(args.iter().map(String::as_str)) {
        Some(Command::Probe) => probe(),
        Some(Command::Keys) => keys(),
        None => {
            eprintln!("usage: blade-dev probe | keys");
            ExitCode::FAILURE
        }
    }
}

/// Takes the one-copy lock, so the app can't drive the controller too.
/// The lock is held while the returned instance lives.
fn take_lock() -> Option<WindowsInstance> {
    let mut instance = match WindowsInstance::new(APP_NAME) {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("cannot take the one-copy lock: {error}");
            return None;
        }
    };
    match start_instance(&mut instance, false) {
        Ok(Start::First) => Some(instance),
        Ok(Start::Second) => {
            eprintln!("Blade Lite is running. Quit it from the tray first.");
            None
        }
        Err(error) => {
            eprintln!("cannot take the one-copy lock: {error}");
            None
        }
    }
}

fn keys() -> ExitCode {
    let Some(_instance) = take_lock() else {
        return ExitCode::FAILURE;
    };
    let (Some(&keyboard), Some(vapor), true) = (
        BLADE_18.brightness_leds.first(),
        BLADE_18.vapor_light,
        BLADE_18.power_key.is_some(),
    ) else {
        eprintln!("this model has no key reports, brightness or vapor light");
        return ExitCode::FAILURE;
    };
    let mut reports = match HidKeyReports::open(&BLADE_18) {
        Ok(reports) => reports,
        Err(error) => {
            eprintln!("cannot open the key reports: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut device = match HidTransport::open(&BLADE_18) {
        Ok(transport) => Device::new(transport, SystemClock),
        Err(error) => {
            eprintln!("cannot open the controller: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("press keys for {} s", KEYS_RUN.as_secs());
    let start = Instant::now();
    let mut shown = None;
    while start.elapsed() < KEYS_RUN {
        let at = start.elapsed().as_millis();
        match reports.read(KEY_READ_WAIT) {
            Ok(Some(report)) => println!("{at:>6} ms  report {report:02X?}"),
            Ok(None) => {}
            Err(error) => {
                eprintln!("cannot read a key report: {error}");
                return ExitCode::FAILURE;
            }
        }
        let levels = (level(&mut device, keyboard), level(&mut device, vapor.led));
        if let (Err(DeviceError::Io(error)), _) | (_, Err(DeviceError::Io(error))) = &levels {
            eprintln!("the controller went away: {error}");
            return ExitCode::FAILURE;
        }
        let text = format!("keyboard {}, vapor {}", show(&levels.0), show(&levels.1));
        if shown.as_ref() != Some(&text) {
            println!("{at:>6} ms  {text}");
            shown = Some(text);
        }
    }
    ExitCode::SUCCESS
}

/// Reads one LED's brightness level. `None` if the reply is for another
/// storage or LED.
fn level(
    device: &mut Device<HidTransport, SystemClock>,
    led: u8,
) -> Result<Option<u8>, DeviceError> {
    let reply = device.send(GET_BRIGHTNESS, &[1, led, 0])?;
    Ok(match reply.args() {
        [1, echoed, level, ..] if *echoed == led => Some(*level),
        _ => None,
    })
}

fn show(level: &Result<Option<u8>, DeviceError>) -> String {
    match level {
        Ok(Some(level)) => format!("level {level}"),
        Ok(None) => "an unknown reply".into(),
        Err(error) => format!("read failed ({error:?})"),
    }
}

fn probe() -> ExitCode {
    let Some(_instance) = take_lock() else {
        return ExitCode::FAILURE;
    };
    let transport = match HidTransport::open(&BLADE_18) {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("cannot open the controller: {error}");
            return ExitCode::FAILURE;
        }
    };
    let info = transport.info().clone();
    println!(
        "controller {:04X}:{:04X} MI_{:02} usage {:02X}:{:02X}",
        info.vendor_id, info.product_id, info.interface_number, info.usage_page, info.usage
    );
    match Device::new(transport, SystemClock).firmware_version() {
        Ok(version) => {
            println!("firmware {}.{}", version.major, version.minor);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cannot read the firmware version: {error}");
            ExitCode::FAILURE
        }
    }
}
