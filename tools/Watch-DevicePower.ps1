<#
.SYNOPSIS
Prints when the Razer USB device, the NVIDIA GPU or the touchpad changes its
power state (D0 awake, D1 to D3 asleep), with the UTC time, as the app log
has it. Only reads Windows' device properties: it opens no device and wakes
nothing (ticket 202). The three are read in one call each round, so a
round takes longer than -GapMs, and a wake shorter than a round can be
missed. The GPU's ID is this laptop's (RTX 5090 Laptop).

.EXAMPLE
powershell -ExecutionPolicy Bypass -File tools\Watch-DevicePower.ps1 -Minutes 10
#>
param(
    [ValidateRange(1, 60)][int]$Minutes = 10,
    [ValidateRange(100, 5000)][int]$GapMs = 250
)

$ErrorActionPreference = 'Stop'

# The devices to watch, by the start of their instance ID.
$watched = [ordered]@{
    'Razer USB'  = 'USB\VID_1532&PID_02C7\'
    'NVIDIA GPU' = 'PCI\VEN_10DE&DEV_2C58'
    'Touchpad'   = 'ACPI\ELAN0406\'
}

$ids = [ordered]@{}
$present = Get-PnpDevice -PresentOnly
foreach ($name in $watched.Keys) {
    $device = $present | Where-Object { $_.InstanceId.StartsWith($watched[$name]) } | Select-Object -First 1
    if ($null -eq $device) { throw "not found: $name ($($watched[$name]))" }
    $ids[$name] = $device.InstanceId
}

# CM_POWER_DATA: a 4-byte size, then the most recent power state (1 is D0).
function Get-PowerState($Data) {
    if ($null -eq $Data -or $Data.Length -lt 8) { return 'unknown' }
    $data = $Data
    $state = [BitConverter]::ToInt32($data, 4)
    if ($state -ge 1 -and $state -le 4) { return "D$($state - 1)" }
    return "unknown ($state)"
}

$last = @{}
$end = (Get-Date).AddMinutes($Minutes)
Write-Output "watching for $Minutes min, rounds at least $GapMs ms apart"
while ((Get-Date) -lt $end) {
    $time = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss.fff') + 'Z'
    $read = @{}
    foreach ($item in Get-PnpDeviceProperty -InstanceId @($ids.Values) -KeyName 'DEVPKEY_Device_PowerData') {
        $read[$item.InstanceId] = $item.Data
    }
    foreach ($name in $ids.Keys) {
        $state = Get-PowerState $read[$ids[$name]]
        if ($last[$name] -ne $state) {
            Write-Output "$time  $name  $state"
            $last[$name] = $state
        }
    }
    Start-Sleep -Milliseconds $GapMs
}
