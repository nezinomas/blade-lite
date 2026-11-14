# Ticket 212: turns off Windows' USB selective suspend for the Razer USB
# device (the keyboard, lights and fan controller). With suspend on, Windows
# lets the device sleep 2 to 5 s after its last use. Each wake drops the
# keyboard lights (tickets 202 and 204, docs/NOTES.md).
#   powershell -ExecutionPolicy Bypass -File tools\Set-RazerUsbSuspend.ps1         # admin: suspend off
#   powershell -ExecutionPolicy Bypass -File tools\Set-RazerUsbSuspend.ps1 -Undo   # admin: suspend on (the value before ticket 204)
# Quit Blade Lite from the tray first. The script sets the value
# SelectiveSuspendEnabled on the 4 HidUsb interfaces (MI_00 to MI_03), then
# restarts the device, so the keyboard and lights go off for a moment.
# It writes the value as one byte (REG_BINARY), the kind Windows uses on the
# laptop's other HidUsb interfaces.
# It changes nothing unless it finds exactly what it expects.

[CmdletBinding()]
param([switch]$Undo)

$ErrorActionPreference = 'Stop'

# The Blade 18 profile's USB ids (crates/razer-proto/src/profile.rs). A test
# checks that they match.
$VendorId = '1532'
$ProductId = '02C7'
$InterfaceCount = 4
# The one-copy lock of the app, blade-dev and the cargo hw tests
# (APP_NAME in crates/blade-core/src/instance.rs). A test checks the name.
$LockName = 'Global\blade-lite'

function Assert-Admin {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this in an admin PowerShell. Nothing changed.'
    }
}

# A device restart in the middle of a send could cut it off, so nothing that
# drives the controller may run. The script holds the lock for its whole run,
# so none can start meanwhile either. Returns the lock; dispose it at the end.
function Enter-ControllerLock {
    $created = $false
    $lock = New-Object System.Threading.Mutex($false, $LockName, [ref]$created)
    if (-not $created -or (Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue)) {
        $lock.Dispose()
        throw 'Blade Lite, blade-dev or a cargo hw test runs. Quit it (Blade Lite: tray, Quit), then run this again. Nothing changed.'
    }
    $lock
}

# The value to set: 0 turns suspend off, 1 (the value before ticket 204) turns it on.
function Get-TargetValue([switch]$Undo) {
    if ($Undo) { 1 } else { 0 }
}

# The HidUsb interfaces MI_00 to MI_03 of the device, one of each.
function Select-RazerInterfaces($Devices) {
    $pattern = "^USB\\VID_$VendorId&PID_$ProductId&MI_0[0-3]\\"
    $found = @($Devices | Where-Object { $_.InstanceId -match $pattern })
    $hid = @($found | Where-Object { $_.Service -eq 'HidUsb' })
    $interfaceIds = @($hid | ForEach-Object { ($_.InstanceId -split '\\')[1] } | Sort-Object -Unique)
    if ($found.Count -ne $InterfaceCount -or $hid.Count -ne $InterfaceCount -or $interfaceIds.Count -ne $InterfaceCount) {
        throw "Expected $InterfaceCount HidUsb interfaces MI_00 to MI_03, found $($found.Count) ($($hid.Count) HidUsb, $($interfaceIds.Count) different). Nothing changed."
    }
    $hid | Sort-Object InstanceId
}

# The composite device that holds the interfaces. Its restart makes the HID
# driver read the new value.
function Select-RazerComposite($Devices) {
    $pattern = "^USB\\VID_$VendorId&PID_$ProductId\\"
    $found = @($Devices | Where-Object { $_.InstanceId -match $pattern })
    if ($found.Count -ne 1 -or $found[0].Service -ne 'usbccgp') {
        throw "Expected 1 usbccgp composite device, found $($found.Count). Nothing changed."
    }
    $found[0]
}

# A registry value as 0 or 1: a DWORD, or a one-byte REG_BINARY (Windows'
# kind). Anything else is $null, a state we do not know.
function ConvertTo-SuspendValue($Kind, $Data) {
    $number = switch ($Kind) {
        'DWord' { if ($Data -is [int]) { $Data } }
        'Binary' { if ($Data -is [byte[]] -and $Data.Length -eq 1) { [int]$Data[0] } }
    }
    if ($number -eq 0 -or $number -eq 1) { $number } else { $null }
}

# Each interface must hold 0 or 1 now.
function Assert-SuspendValues($Values) {
    $Values = @($Values)
    $bad = @($Values | Where-Object { $_ -isnot [int] -or ($_ -ne 0 -and $_ -ne 1) })
    if ($Values.Count -ne $InterfaceCount -or $bad.Count -ne 0) {
        throw "Expected $InterfaceCount values of 0 or 1, read: $($Values -join ', ') (an empty one could not be read). Nothing changed."
    }
}

function Get-ParametersKey($Interface) {
    "HKLM:\SYSTEM\CurrentControlSet\Enum\$($Interface.InstanceId)\Device Parameters"
}

function Read-SuspendValue($Interface) {
    try {
        $key = Get-Item -LiteralPath (Get-ParametersKey $Interface)
        ConvertTo-SuspendValue $key.GetValueKind('SelectiveSuspendEnabled') $key.GetValue('SelectiveSuspendEnabled')
    } catch {
        Write-Warning "$($Interface.InstanceId): $($_.Exception.Message)"
        $null
    }
}

function Write-SuspendValue($Interface, $Value) {
    Set-ItemProperty -LiteralPath (Get-ParametersKey $Interface) -Name SelectiveSuspendEnabled -Value ([byte[]]@($Value)) -Type Binary
}

# The whole change. Every check comes before the first write.
function Invoke-UsbSuspend([switch]$Undo) {
    $target = Get-TargetValue -Undo:$Undo
    $devices = @(Get-PnpDevice -PresentOnly)
    $interfaces = @(Select-RazerInterfaces $devices)
    $composite = Select-RazerComposite $devices
    $old = @($interfaces | ForEach-Object { Read-SuspendValue $_ })
    Assert-SuspendValues $old

    if (@($old | Where-Object { $_ -ne $target }).Count -eq 0) {
        Write-Output "SelectiveSuspendEnabled is $target on all $InterfaceCount interfaces already. Nothing changed."
        return
    }

    for ($i = 0; $i -lt $interfaces.Count; $i++) {
        try {
            Write-SuspendValue $interfaces[$i] $target
        } catch {
            throw "Changed $i of $InterfaceCount interfaces (listed above), then: $($_.Exception.Message) The device was not restarted. Run this again, or with -Undo."
        }
        Write-Output "$($interfaces[$i].InstanceId): SelectiveSuspendEnabled $($old[$i]) -> $target"
    }

    Write-Output 'Restarting the Razer USB device. The keyboard and lights go off for a moment.'
    pnputil /restart-device "$($composite.InstanceId)"
    $restart = $LASTEXITCODE

    $new = @($interfaces | ForEach-Object { Read-SuspendValue $_ })
    if (@($new | Where-Object { $_ -ne $target }).Count -ne 0) {
        throw "Read back: $($new -join ', '), not $target. Run this again, or with -Undo."
    }
    if ($restart -ne 0) {
        throw "The value is set, but the device did not restart (pnputil exit $restart). Restart Windows so it takes effect."
    }
    Write-Output "Done. SelectiveSuspendEnabled is $target on all $InterfaceCount interfaces."
}

# Dot-sourced (by the tests): define the functions only, change nothing.
if ($MyInvocation.InvocationName -eq '.') { return }

Assert-Admin
$lock = Enter-ControllerLock
try {
    Invoke-UsbSuspend -Undo:$Undo
} finally {
    $lock.Dispose()
}
