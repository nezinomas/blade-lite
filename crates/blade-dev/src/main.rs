//! `blade-dev probe`: finds the controller and reads its firmware version.

use std::process::ExitCode;

use blade_dev::Command;
use razer_device::{Device, HidTransport, SystemClock};

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
    let transport = match HidTransport::open() {
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
            eprintln!("cannot read the firmware version: {error:?}");
            ExitCode::FAILURE
        }
    }
}
