//! Guards of `tools/Set-RazerUsbSuspend.ps1` (ticket 212). The script is
//! only dot-sourced here, with every command that changes the laptop
//! replaced by one that fails, so a test can never change a device.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

use blade_core::APP_NAME;
use razer_proto::BLADE_18;

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/Set-RazerUsbSuspend.ps1")
}

/// Runs `body` after the script is dot-sourced. Returns stdout, or the
/// error text if the run failed.
fn run(body: &str) -> Result<String, String> {
    let prelude = "$ErrorActionPreference = 'Stop'\n\
        function Get-PnpDevice { throw 'Get-PnpDevice ran' }\n\
        function Set-ItemProperty { throw 'Set-ItemProperty ran' }\n\
        function Get-ItemProperty { throw 'Get-ItemProperty ran' }\n\
        function pnputil { throw 'pnputil ran' }\n\
        function Get-Process { }\n";
    let path = script().display().to_string().replace('\'', "''");
    let command = format!("{prelude}. '{path}'\n{body}");
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ])
        .args(["-Command", &command])
        .output()
        .expect("powershell starts");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("{stdout}{stderr}"))
    }
}

/// A list of devices as `Get-PnpDevice` gives them: id and service.
fn devices(list: &[(String, &str)]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|(id, service)| {
            format!("[pscustomobject]@{{ InstanceId = '{id}'; Service = '{service}' }}")
        })
        .collect();
    format!("@({})", items.join(", "))
}

const COMPOSITE: (&str, &str) = (r"USB\VID_1532&PID_02C7\00000000001A", "usbccgp");

fn interface(mi: u8) -> (String, &'static str) {
    (
        format!(r"USB\VID_1532&PID_02C7&MI_0{mi}\6&2B1C2A3F&0&000{mi}"),
        "HidUsb",
    )
}

fn laptop() -> Vec<(String, &'static str)> {
    let mut list: Vec<(String, &'static str)> = (0..4).map(interface).collect();
    list.push((COMPOSITE.0.to_string(), COMPOSITE.1));
    // Other devices that must not be picked.
    list.push((r"USB\VID_1532&PID_02C7&MI_04\x".to_string(), "HidUsb"));
    list.push((r"USB\VID_1532&PID_0099&MI_00\x".to_string(), "HidUsb"));
    list.push((r"USB\VID_046D&PID_C52B&MI_00\x".to_string(), "HidUsb"));
    list.push((r"HID\VID_1532&PID_02C7&MI_00&COL01\x".to_string(), "HidUsb"));
    list
}

#[test]
fn dot_sourcing_changes_nothing() {
    assert_eq!(run("'loaded'"), Ok("loaded".to_string()));
}

#[test]
fn the_script_names_the_profiles_usb_ids() {
    let ids = run("\"$VendorId $ProductId\"").unwrap();
    let want = format!(
        "{:04X} {:04X}",
        BLADE_18.usb.vendor_id, BLADE_18.usb.product_id
    );
    assert_eq!(ids, want);
}

#[test]
fn the_four_hid_interfaces_are_picked() {
    let list = devices(&laptop());
    let picked = run(&format!(
        "(Select-RazerInterfaces {list} | ForEach-Object InstanceId) -join ','"
    ))
    .unwrap();
    let want: Vec<String> = (0..4).map(|mi| interface(mi).0).collect();
    assert_eq!(picked, want.join(","));
}

#[test]
fn three_interfaces_are_refused() {
    let mut list = laptop();
    list.remove(3);
    let err = run(&format!("Select-RazerInterfaces {}", devices(&list))).unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
}

#[test]
fn an_interface_with_another_driver_is_refused() {
    let mut list = laptop();
    list[1].1 = "RzDev";
    let err = run(&format!("Select-RazerInterfaces {}", devices(&list))).unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
}

#[test]
fn a_twice_listed_interface_is_refused() {
    let mut list = laptop();
    list[3] = (interface(2).0.replace("0002", "0009"), "HidUsb");
    let err = run(&format!("Select-RazerInterfaces {}", devices(&list))).unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
}

#[test]
fn the_one_composite_device_is_picked() {
    let list = devices(&laptop());
    let picked = run(&format!("(Select-RazerComposite {list}).InstanceId")).unwrap();
    assert_eq!(picked, COMPOSITE.0);
}

#[test]
fn no_or_two_composite_devices_are_refused() {
    let mut none = laptop();
    none.retain(|(_, service)| *service != "usbccgp");
    let mut two = laptop();
    two.push((format!("{}B", COMPOSITE.0), "usbccgp"));
    for list in [none, two] {
        let err = run(&format!("Select-RazerComposite {}", devices(&list))).unwrap_err();
        assert!(err.contains("Nothing changed"), "{err}");
    }
}

#[test]
fn only_known_suspend_values_are_taken() {
    assert!(run("Assert-SuspendValues @(1, 1, 0, 1)").is_ok());
    for bad in ["@(1, 1, 2, 1)", "@(1, $null, 1, 1)", "@(1, 1, 1)"] {
        let err = run(&format!("Assert-SuspendValues {bad}")).unwrap_err();
        assert!(err.contains("Nothing changed"), "{bad}: {err}");
    }
}

#[test]
fn undo_puts_suspend_back_on() {
    assert_eq!(run("Get-TargetValue -Undo:$false"), Ok("0".to_string()));
    assert_eq!(run("Get-TargetValue -Undo:$true"), Ok("1".to_string()));
}

#[test]
fn the_script_checks_the_apps_lock() {
    assert_eq!(run("$LockName"), Ok(format!(r"Global\{APP_NAME}")));
}

#[test]
fn a_held_lock_is_refused() {
    let lock = format!(r"Global\blade-lite-test-{}", std::process::id());
    let free = run(&format!("$LockName = '{lock}'; Assert-NoneRuns; 'free'"));
    assert_eq!(free, Ok("free".to_string()));
    let err = run(&format!(
        "$LockName = '{lock}'; $m = New-Object System.Threading.Mutex($true, $LockName); Assert-NoneRuns"
    ))
    .unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
}
