//! Guards of `tools/Remove-RazerDriver.ps1` (ticket 268). The script is only
//! dot-sourced here, with every command that changes the laptop replaced by
//! one that fails, so a test can never change a device or a setting.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

use blade_core::APP_NAME;
use razer_proto::BLADE_18;

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/Remove-RazerDriver.ps1")
}

/// Runs `body` after the script is dot-sourced. Returns stdout, or the
/// error text if the run failed.
fn run(body: &str) -> Result<String, String> {
    let prelude = "$ErrorActionPreference = 'Stop'\n\
        function pnputil { throw 'pnputil ran' }\n\
        function Get-PnpDevice { throw 'Get-PnpDevice ran' }\n\
        function Set-ItemProperty { throw 'Set-ItemProperty ran' }\n\
        function Remove-ItemProperty { throw 'Remove-ItemProperty ran' }\n\
        function New-Item { throw 'New-Item ran' }\n\
        function Set-Content { throw 'Set-Content ran' }\n\
        function Remove-Item { throw 'Remove-Item ran' }\n\
        function Enable-ComputerRestore { throw 'Enable-ComputerRestore ran' }\n\
        function Checkpoint-Computer { throw 'Checkpoint-Computer ran' }\n\
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

#[test]
fn dot_sourcing_changes_nothing() {
    assert_eq!(run("'loaded'"), Ok("loaded".to_string()));
}

#[test]
fn the_script_checks_the_apps_lock() {
    assert_eq!(run("$LockName"), Ok(format!(r"Global\{APP_NAME}")));
}

#[test]
fn the_script_names_the_profiles_product_id() {
    let id = run("$ProductId").unwrap();
    assert_eq!(id, format!("{:04X}", BLADE_18.usb.product_id));
}

/// On a lock of its own, so the test never takes the app's.
#[test]
fn a_held_lock_or_a_running_app_is_refused() {
    let lock = format!(
        r"$LockName = 'Global\blade-lite-test-driver-{}'",
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
fn a_setting_undo_could_not_put_back_is_refused_first() {
    let err = run("function Get-MetadataValue { '2' }\nAssert-MetadataValueKnown").unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
    for known in ["absent", "0", "1"] {
        let ok = run(&format!(
            "function Get-MetadataValue {{ '{known}' }}\nAssert-MetadataValueKnown; 'ok'"
        ));
        assert_eq!(ok, Ok("ok".to_string()), "{known}");
    }
}

#[test]
fn a_failed_restore_point_read_stops_the_removal() {
    let err = run("function Get-ComputerRestorePoint { throw 'WMI failed' }\nGet-LastRestorePoint")
        .unwrap_err();
    assert!(err.contains("WMI failed"), "{err}");
}

/// Packages and devices: `oem81` is the Blade's, `oem90` a Razer mouse's.
const PACKAGES_AND_DEVICES: &str = "$packages = @(\
    [pscustomobject]@{ Published = 'oem81.inf'; Original = 'rzdevu_02c7_mou.inf' },\
    [pscustomobject]@{ Published = 'oem90.inf'; Original = 'rzdevu_0099.inf' })\n\
    $devices = @(\
    [pscustomobject]@{ Inf = 'oem81.inf'; Id = 'HID\\VID_1532&PID_02C7&MI_02\\x' },\
    [pscustomobject]@{ Inf = 'oem90.inf'; Id = 'HID\\VID_1532&PID_0099&MI_00\\x' })\n";

#[test]
fn a_package_another_razer_device_uses_is_kept() {
    let picked = run(&format!(
        "{PACKAGES_AND_DEVICES}\
         (Select-PackagesToRemove $packages $devices | ForEach-Object Published) -join ','"
    ))
    .unwrap();
    assert_eq!(picked.lines().last(), Some("oem81.inf"), "{picked}");
    assert!(picked.contains("Kept oem90.inf"), "{picked}");
}

#[test]
fn only_the_blades_devices_are_shown() {
    let shown = run(&format!(
        "{PACKAGES_AND_DEVICES}(Select-BladeDevices $devices | ForEach-Object Inf) -join ','"
    ));
    assert_eq!(shown, Ok("oem81.inf".to_string()));
}

/// Restore points as `Get-ComputerRestorePoint` gives them, number and
/// description, before and after the checkpoint.
fn restore_points(after: &str) -> String {
    format!(
        "$script:points = @([pscustomobject]@{{ SequenceNumber = 7; Description = 'old' }})\n\
         function Get-ComputerRestorePoint {{ $script:points }}\n\
         function Enable-ComputerRestore {{ }}\n\
         function Checkpoint-Computer {{ $script:points += {after} }}\n"
    )
}

#[test]
fn a_restore_point_windows_skipped_stops_the_removal() {
    let err = run(&format!(
        "{}New-RestorePoint\n'went on'",
        restore_points("@()")
    ))
    .unwrap_err();
    assert!(err.contains("Nothing removed"), "{err}");
}

#[test]
fn a_new_restore_point_lets_the_removal_go_on() {
    let made = run(&format!(
        "{}New-RestorePoint",
        restore_points(
            "[pscustomobject]@{ SequenceNumber = 8; Description = 'Before Razer driver removal' }"
        )
    ));
    assert_eq!(made, Ok("Restore point made.".to_string()));
}

#[test]
fn an_old_restore_point_with_the_same_name_does_not_count() {
    let err = run(
        "$script:points = @([pscustomobject]@{ SequenceNumber = 7; Description = 'Before Razer driver removal' })\n\
         function Get-ComputerRestorePoint { $script:points }\n\
         function Enable-ComputerRestore { }\n\
         function Checkpoint-Computer { }\n\
         New-RestorePoint",
    )
    .unwrap_err();
    assert!(err.contains("Nothing removed"), "{err}");
}

/// Runs the undo with the kept value file at a temp path holding `kept`, and
/// the setting's writes recorded instead of made.
fn undo_with(kept: &str) -> Result<String, String> {
    // A file of its own per test run and value, so tests at once don't mix.
    let file =
        std::env::temp_dir().join(format!("blade-lite-undo-{}-{kept}.txt", std::process::id()));
    std::fs::write(&file, kept).unwrap();
    let path = file.display().to_string().replace('\'', "''");
    let done = run(&format!(
        "$beforeFile = '{path}'\n\
         function Set-ItemProperty {{ param($Path, $Name, $Value, $Type) \"set $Name $Value\" }}\n\
         function Remove-ItemProperty {{ param($Path, $Name) \"removed $Name\" }}\n\
         function Remove-Item {{ }}\n\
         (Restore-ManufacturerApps) -join '|'"
    ));
    let _ = std::fs::remove_file(&file);
    done
}

#[test]
fn undo_puts_back_a_kept_value() {
    let done = undo_with("0").unwrap();
    assert!(
        done.contains("set PreventDeviceMetadataFromNetwork 0"),
        "{done}"
    );
}

#[test]
fn undo_removes_a_value_that_was_absent() {
    let done = undo_with("absent").unwrap();
    assert!(
        done.contains("removed PreventDeviceMetadataFromNetwork"),
        "{done}"
    );
}

#[test]
fn undo_refuses_a_kept_value_it_cannot_read() {
    for kept in ["7", ""] {
        let err = undo_with(kept).unwrap_err();
        assert!(err.contains("Nothing changed"), "{kept:?}: {err}");
    }
}

#[test]
fn undo_with_nothing_kept_is_refused() {
    let err = run("$beforeFile = 'C:\\no\\such\\blade-lite-file.txt'\n\
         Restore-ManufacturerApps")
    .unwrap_err();
    assert!(err.contains("Nothing changed"), "{err}");
}
