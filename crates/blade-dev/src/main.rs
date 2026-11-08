//! `blade-dev probe`: finds the controller and reads its firmware version.

use std::process::ExitCode;

use blade_core::{APP_NAME, Start, start_instance};
use blade_dev::Command;
use platform::WindowsInstance;
use razer_device::{Device, HidTransport, SystemClock};
use razer_proto::BLADE_18;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match Command::parse(args.iter().map(String::as_str)) {
        Some(Command::Probe) => probe(),
        None => {
            eprintln!("usage: blade-dev probe");
            ExitCode::FAILURE
        }
    }
}

fn probe() -> ExitCode {
    // Held until the probe ends, so the app can't drive the controller too.
    let mut instance = match WindowsInstance::new(APP_NAME) {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("cannot take the one-copy lock: {error}");
            return ExitCode::FAILURE;
        }
    };
    match start_instance(&mut instance, false) {
        Ok(Start::First) => {}
        Ok(Start::Second) => {
            eprintln!("Blade Lite is running. Quit it from the tray first.");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("cannot take the one-copy lock: {error}");
            return ExitCode::FAILURE;
        }
    }
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
