<#
.SYNOPSIS
Prints, with the UTC time, each change in what Windows says about the
touchpad (ticket 239): the registry value the app reads
(HKCU\...\PrecisionTouchPad\Status, Enabled), the touchpad fields of
SystemParametersInfo(SPI_GETTOUCHPADPARAMETERS) (Windows 11 24H2 and later),
and the status of the ELAN touchpad device (ELAN0406, this laptop's
touchpad on the Blade 18 2025; elsewhere it prints device=missing). Read-only: it writes nothing,
sends no keys and opens no device.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File tools\Watch-Touchpad.ps1 -Minutes 10
#>
param(
    [ValidateRange(1, 60)][int]$Minutes = 10,
    [ValidateRange(100, 5000)][int]$GapMs = 250,
    # Each printed line is also added to this file, so it outlives a crash.
    [string]$LogPath
)

$ErrorActionPreference = 'Stop'

Add-Type -Namespace Probe -Name Native -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)]
public static extern bool SystemParametersInfoW(uint action, uint param, byte[] data, uint winIni);
'@

# winuser.h (SDK 10.0.26100): SPI_GETTOUCHPADPARAMETERS 0xAE,
# TOUCHPAD_PARAMETERS_V1 is 11 UINTs, versionNumber 1 first.
$SPI_GETTOUCHPADPARAMETERS = 0xAE
$SIZE = 44

function Get-Spi {
    $buffer = [byte[]]::new($SIZE)
    [BitConverter]::GetBytes([uint32]1).CopyTo($buffer, 0)
    if (-not [Probe.Native]::SystemParametersInfoW($SPI_GETTOUCHPADPARAMETERS, $SIZE, $buffer, 0)) {
        return "spi error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $status = [BitConverter]::ToUInt32($buffer, 12)
    $settings = [BitConverter]::ToUInt32($buffer, 16)
    $bit = { param($word, $n) [int](($word -shr $n) -band 1) }
    'present={0} mouse={1} enabled={2} active={3} activeWithMouse={4}' -f `
    (& $bit $status 0), (& $bit $status 2), (& $bit $status 3), (& $bit $status 4), (& $bit $settings 0)
}

function Get-Registry {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad\Status'
    try { "registry=$((Get-ItemProperty -Path $key -Name Enabled).Enabled)" }
    catch { 'registry=unreadable' }
}

function Get-Device {
    $device = Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'HID\ELAN0406&COL03*' } |
        Select-Object -First 1
    if ($null -eq $device) { return 'device=missing' }
    "device=$($device.Status)"
}

$end = (Get-Date).AddMinutes($Minutes)
$last = ''
Write-Host "Watching the touchpad for $Minutes min. Ctrl+C stops."
while ((Get-Date) -lt $end) {
    $now = "$(Get-Registry) $(Get-Spi) $(Get-Device)"
    if ($now -ne $last) {
        $line = "$((Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff'))Z $now"
        Write-Host $line
        if ($LogPath) { Add-Content -Path $LogPath -Value $line -Encoding utf8 }
        $last = $now
    }
    Start-Sleep -Milliseconds $GapMs
}
