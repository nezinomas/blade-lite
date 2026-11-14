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
        function Get-Item { throw 'Get-Item ran' }\n\
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
fn a_held_lock_or_a_running_app_is_refused() {
    let lock = format!(
        r"$LockName = 'Global\blade-lite-test-{}'",
        std::process::id()
    );
    let free = run(&format!("{lock}; (Enter-ControllerLock).Dispose(); 'free'"));
    assert_eq!(free, Ok("free".to_string()));
    let held = format!(
        "{lock}; $m = New-Object System.Threading.Mutex($false, $LockName); Enter-ControllerLock"
    );
    let running = format!("{lock}; function Get-Process {{ 'blade-lite' }}; Enter-ControllerLock");
    for body in [held, running] {
        let err = run(&body).unwrap_err();
        assert!(err.contains("Nothing changed"), "{body}: {err}");
    }
}

#[test]
fn only_a_dword_or_one_byte_of_0_or_1_is_a_value() {
    for (kind, data, want) in [
        ("DWord", "[int]0", "0"),
        ("DWord", "[int]1", "1"),
        ("Binary", "[byte[]]@(0)", "0"),
        ("Binary", "[byte[]]@(1)", "1"),
        ("DWord", "[int]2", "null"),
        ("Binary", "[byte[]]@(1, 0)", "null"),
        ("Binary", "[byte[]]@(2)", "null"),
        ("String", "'1'", "null"),
        ("QWord", "[long]1", "null"),
    ] {
        let got = run(&format!(
            "$v = ConvertTo-SuspendValue '{kind}' ({data}); if ($null -eq $v) {{ 'null' }} else {{ \"$v\" }}"
        ));
        assert_eq!(got, Ok(want.to_string()), "{kind} {data}");
    }
}

/// Where the interface number is in an interface's id (`MI_0x`).
const MI_AT: usize = 26;

/// Runs `Invoke-UsbSuspend` over fakes that log each read, write and
/// restart. `values` are the 4 values the interfaces hold; `fail_write`
/// makes that write (counted from 0) fail. Returns the log, and after a
/// ` | ` the error, if any.
fn invoke(undo: bool, values: &str, fail_write: i32) -> String {
    let list = devices(&laptop());
    let body = format!(
        r#"$script:log = @()
$script:values = @{values}
$script:writes = 0
function Get-PnpDevice {{ {list} }}
function Read-SuspendValue($Interface) {{
    $script:log += 'read'
    $script:values[[int]$Interface.InstanceId.Substring({MI_AT}, 1)]
}}
function Write-SuspendValue($Interface, $Value) {{
    if ($script:writes -eq {fail_write}) {{ throw 'access denied' }}
    $script:writes++
    $script:log += "write $Value"
    $script:values[[int]$Interface.InstanceId.Substring({MI_AT}, 1)] = $Value
}}
function pnputil {{ $script:log += 'restart'; $global:LASTEXITCODE = 0 }}
try {{ $null = Invoke-UsbSuspend -Undo:${undo}; $failure = '' }}
catch {{ $failure = " | $($_.Exception.Message)" }}
($script:log -join ',') + $failure"#
    );
    run(&body).expect("the run itself does not fail")
}

#[test]
fn the_fake_finds_the_interface_number() {
    assert_eq!(&interface(3).0[MI_AT..=MI_AT], "3");
}

#[test]
fn a_run_reads_all_writes_all_restarts_and_reads_back() {
    let reads = "read,read,read,read";
    let writes = "write 0,write 0,write 0,write 0";
    assert_eq!(
        invoke(false, "(1, 1, 0, 1)", -1),
        format!("{reads},{writes},restart,{reads}")
    );
}

#[test]
fn undo_writes_1() {
    let log = invoke(true, "(0, 0, 0, 0)", -1);
    assert!(
        log.contains("write 1,write 1,write 1,write 1,restart"),
        "{log}"
    );
}

#[test]
fn values_already_set_write_nothing() {
    assert_eq!(invoke(false, "(0, 0, 0, 0)", -1), "read,read,read,read");
}

#[test]
fn an_unknown_value_writes_nothing() {
    let log = invoke(false, "(1, 1, $null, 1)", -1);
    assert!(
        log.starts_with("read,read,read,read | Expected 4 values"),
        "{log}"
    );
    assert!(log.ends_with("Nothing changed."), "{log}");
}

#[test]
fn a_failed_write_says_how_many_changed_and_does_not_restart() {
    let log = invoke(false, "(1, 1, 1, 1)", 2);
    assert!(
        log.starts_with("read,read,read,read,write 0,write 0 | Changed 2 of 4"),
        "{log}"
    );
}

/// Runs `Read-SuspendValue` over a fake registry key: `kind` is a
/// `RegistryValueKind` name, or empty for a missing value.
fn read_value(kind: &str, data: &str) -> String {
    let get_kind = if kind.is_empty() {
        "throw 'no such value'".to_string()
    } else {
        format!("[Microsoft.Win32.RegistryValueKind]::{kind}")
    };
    let body = format!(
        r"function Get-Item {{
    $key = [pscustomobject]@{{}}
    $key | Add-Member ScriptMethod GetValueKind {{ param($name) {get_kind} }}
    $key | Add-Member ScriptMethod GetValue {{ param($name) ,({data}) }}
    $key
}}
$v = Read-SuspendValue ([pscustomobject]@{{ InstanceId = 'x' }}) 3>$null
if ($null -eq $v) {{ 'null' }} else {{ $v.GetType().Name + ' ' + $v }}"
    );
    run(&body).expect("the read itself does not fail")
}

#[test]
fn a_registry_read_takes_dword_and_one_byte_and_refuses_a_missing_value() {
    assert_eq!(read_value("DWord", "[int]1"), "Int32 1");
    assert_eq!(read_value("Binary", "[byte[]]@(0)"), "Int32 0");
    assert_eq!(read_value("Binary", "[byte[]]@(1, 0)"), "null");
    assert_eq!(read_value("", "[int]1"), "null");
}

#[test]
fn a_restart_that_did_not_run_or_a_wrong_read_back_is_an_error() {
    let list = devices(&laptop());
    let fakes = format!(
        r"function Get-PnpDevice {{ {list} }}
function Write-SuspendValue($Interface, $Value) {{ }}"
    );
    let restart_fails = format!(
        r"{fakes}
function Read-SuspendValue($Interface) {{ if ($script:written) {{ 0 }} else {{ 1 }} }}
function pnputil {{ $script:written = $true; $global:LASTEXITCODE = 3010 }}
Invoke-UsbSuspend"
    );
    let err = run(&restart_fails).unwrap_err();
    assert!(err.contains("did not restart (pnputil exit 3010)"), "{err}");
    let stays = format!(
        r"{fakes}
function Read-SuspendValue($Interface) {{ 1 }}
function pnputil {{ $global:LASTEXITCODE = 0 }}
Invoke-UsbSuspend"
    );
    let err = run(&stays).unwrap_err();
    assert!(err.contains("Read back: 1, 1, 1, 1, not 0"), "{err}");
}
