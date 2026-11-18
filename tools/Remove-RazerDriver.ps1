# Stage 2: removes the Razer driver packages so 02C7 falls back to the
# inbox Windows HID drivers. Uninstall Synapse in Settings first.
#   .\tools\Remove-RazerDriver.ps1           # dry run: show what is there
#   .\tools\Remove-RazerDriver.ps1 -Apply    # admin: restore point, remove, block
#   .\tools\Remove-RazerDriver.ps1 -Undo     # admin: put the Windows Update setting back
# Run the dry run again after a restart to check the result.
# Quit Blade Lite from the tray first: -Apply and -Undo take its lock.
# -Apply removes only Razer packages that no other Razer device Windows
# knows (plugged in or not) has as its driver (a Razer mouse keeps its
# driver). Only USB devices' main driver INF is matched: a package another
# Razer device uses as an extension, or a Bluetooth Razer device's, is not
# seen. It stops, with nothing removed, unless a new restore point is
# there: Windows skips one if any restore point was made in the last 24
# hours (Microsoft's SRSetRestorePointW and Checkpoint-Computer pages), so
# then run it the next day, or with -SkipRestorePoint. The removed packages can't be put back by this
# script: use the restore point, or install Synapse again (docs/driver-removal.md).

param(
    [switch]$Apply,
    [switch]$Undo,
    [switch]$SkipRestorePoint
)

$ErrorActionPreference = 'Stop'

$metadataKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Device Metadata'
$metadataName = 'PreventDeviceMetadataFromNetwork'
# Where -Apply keeps the setting's old value for -Undo.
$beforeFile = Join-Path $env:ProgramData 'blade-lite\Remove-RazerDriver.before.txt'
$restoreDescription = 'Before Razer driver removal'
# The Blade 18 profile's USB id (crates/razer-proto/src/profile.rs).
$ProductId = '02C7'
# The one-copy lock of the app, blade-dev and the cargo hw tests
# (APP_NAME in crates/blade-core/src/instance.rs). A test checks the name.
$LockName = 'Global\blade-lite'

function Get-RazerApps {
    $roots = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
             'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
             'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*'
    Get-ItemProperty $roots -ErrorAction SilentlyContinue |
        Where-Object { $_.Publisher -match '^Razer' } |
        Select-Object -ExpandProperty DisplayName -Unique
}

function Get-RazerDriverPackages {
    $blocks = ((pnputil /enum-drivers) -join "`n") -split "`n\s*`n"
    foreach ($block in $blocks) {
        if ($block -notmatch 'Provider Name:\s+Razer') { continue }
        [pscustomobject]@{
            Published = [regex]::Match($block, 'Published Name:\s+(\S+)').Groups[1].Value
            Original  = [regex]::Match($block, 'Original Name:\s+(\S+)').Groups[1].Value
        }
    }
}

# Every Razer device, with the INF its driver comes from: those plugged in
# now, or with -All every one Windows knows.
function Get-RazerDevices([switch]$All) {
    $found = if ($All) { Get-PnpDevice } else { Get-PnpDevice -PresentOnly }
    $found | Where-Object { $_.InstanceId -match 'VID_1532&PID_' } |
        ForEach-Object {
            $inf = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_DriverInfPath).Data
            [pscustomobject]@{ Inf = $inf; Name = $_.FriendlyName; Id = $_.InstanceId }
        }
}

function Select-BladeDevices($Devices) {
    @($Devices | Where-Object { $_.Id -match "VID_1532&PID_$ProductId" })
}

# The packages that no other Razer device uses: those are kept, and named.
function Select-PackagesToRemove($Packages, $Devices) {
    $others = @($Devices | Where-Object { $_.Id -notmatch "VID_1532&PID_$ProductId" })
    $blade = @(Select-BladeDevices $Devices)
    foreach ($package in $Packages) {
        $users = @($others | Where-Object { $_.Inf -eq $package.Published })
        if ($users.Count -gt 0) {
            Write-Host "Kept $($package.Published) ($($package.Original)): used by $(($users | ForEach-Object Id) -join ', ')"
            if (@($blade | Where-Object { $_.Inf -eq $package.Published }).Count -gt 0) {
                Write-Warning "$($package.Published) is also the Blade's driver: those $ProductId interfaces stay on it."
            }
            continue
        }
        $package
    }
}

function Show-State {
    $apps = @(Get-RazerApps)
    $packages = @(Get-RazerDriverPackages)
    $devices = @(Select-BladeDevices @(Get-RazerDevices))
    $onRazer = @($devices | Where-Object { $_.Inf -like 'oem*' })
    $metadata = (Get-ItemProperty $metadataKey -ErrorAction SilentlyContinue).$metadataName

    Write-Host "`nRazer apps installed: $($apps.Count)"
    $apps | ForEach-Object { Write-Host "  $_" }
    Write-Host "`nRazer driver packages: $($packages.Count)"
    $packages | ForEach-Object { Write-Host "  $($_.Published)  $($_.Original)" }
    Write-Host "`n$ProductId devices on a Razer driver: $($onRazer.Count) of $($devices.Count)"
    $onRazer | ForEach-Object { Write-Host "  $($_.Inf)  $($_.Id)" }
    Write-Host "`nManufacturer apps from Windows Update blocked: $($metadata -eq 1)"

    $done = $devices.Count -gt 0 -and $onRazer.Count -eq 0 -and $metadata -eq 1
    Write-Host ("`nResult: " + $(if ($done) { "DONE. $ProductId is on the inbox drivers." } else { 'NOT DONE.' }))
}

function Assert-Admin {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run -Apply or -Undo in an admin PowerShell. Nothing changed.'
    }
}

# A driver removal restarts the device, which could cut off a send, so
# nothing that drives the controller may run. Held for the whole run.
# Returns the lock; dispose it at the end.
function Enter-ControllerLock {
    $created = $false
    $lock = New-Object System.Threading.Mutex($false, $LockName, [ref]$created)
    if (-not $created -or (Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue)) {
        $lock.Dispose()
        throw 'Blade Lite, blade-dev or a cargo hw test runs. Quit it (Blade Lite: tray, Quit), then run this again. Nothing changed.'
    }
    $lock
}

function Assert-SynapseGone {
    $synapse = @(Get-RazerApps | Where-Object { $_ -match 'Synapse|Chroma' })
    if ($synapse.Count -gt 0) {
        throw "Uninstall these in Settings > Apps first, then restart: $($synapse -join ', '). Nothing changed."
    }
}

# The highest restore point number, 0 if none. A failed read stops the run:
# a 0 would make an old point count as new.
function Get-LastRestorePoint {
    $last = @(Get-ComputerRestorePoint) |
        Measure-Object -Property SequenceNumber -Maximum
    if ($last.Count -eq 0) { 0 } else { [int64]$last.Maximum }
}

# Makes a restore point and checks that a new one is there: Windows may
# skip it (one each 24 hours) with only a warning.
function New-RestorePoint {
    if ($SkipRestorePoint) {
        Write-Host 'No restore point made (-SkipRestorePoint).'
        return
    }
    $before = Get-LastRestorePoint
    Enable-ComputerRestore -Drive "$env:SystemDrive\"
    Checkpoint-Computer -Description $restoreDescription -RestorePointType MODIFY_SETTINGS -WarningAction Continue
    $made = @(Get-ComputerRestorePoint -ErrorAction SilentlyContinue | Where-Object {
        $_.SequenceNumber -gt $before -and $_.Description -eq $restoreDescription
    })
    if ($made.Count -eq 0) {
        throw 'Windows made no new restore point (it skips one if any was made in the last 24 hours). Nothing removed. Run again tomorrow, or with -SkipRestorePoint to go on without one.'
    }
    Write-Host 'Restore point made.'
}

function Remove-RazerPackages {
    $packages = @(Select-PackagesToRemove @(Get-RazerDriverPackages) @(Get-RazerDevices -All))
    foreach ($package in $packages) {
        Write-Host "Removing $($package.Published) ($($package.Original))"
        pnputil /delete-driver $package.Published /uninstall
        if ($LASTEXITCODE -ne 0) { Write-Warning "pnputil exit $LASTEXITCODE for $($package.Published)" }
    }
}

# The setting's value now, as text: 'absent' when it is not set.
function Get-MetadataValue {
    $value = (Get-ItemProperty $metadataKey -ErrorAction SilentlyContinue).$metadataName
    if ($null -eq $value) { 'absent' } else { [string]$value }
}

# Refuses a value -Undo could not put back, before anything changes.
function Assert-MetadataValueKnown {
    $now = Get-MetadataValue
    if ($now -notmatch '^(absent|0|1)$') {
        throw "$metadataName is '$now', not absent, 0 or 1: -Undo could not put it back. Nothing changed."
    }
}

# Keeps the old value for -Undo, unless one is kept already (a second
# -Apply would keep its own 1).
function Save-MetadataValue {
    if (Test-Path $beforeFile) { return }
    New-Item (Split-Path $beforeFile) -ItemType Directory -Force | Out-Null
    Set-Content $beforeFile (Get-MetadataValue) -Encoding ascii
}

function Block-ManufacturerApps {
    Save-MetadataValue
    # Only when missing: -Force on a key that is there would make it again
    # with no other values.
    if (-not (Test-Path $metadataKey)) { New-Item $metadataKey | Out-Null }
    Set-ItemProperty $metadataKey -Name $metadataName -Value 1 -Type DWord
    Write-Host 'Turned off "Automatically get manufacturers'' apps and custom icons".'
}

# Puts back the value -Apply kept. Refuses a kept value it can't read.
function Restore-ManufacturerApps {
    if (-not (Test-Path $beforeFile)) {
        throw "No value kept in $beforeFile (-Apply was not run since this was added). Nothing changed."
    }
    $before = "$(Get-Content $beforeFile -TotalCount 1)".Trim()
    if ($before -eq 'absent') {
        Remove-ItemProperty $metadataKey -Name $metadataName -ErrorAction SilentlyContinue
    } elseif ($before -match '^[01]$') {
        Set-ItemProperty $metadataKey -Name $metadataName -Value ([int]$before) -Type DWord
    } else {
        throw "The kept value '$before' in $beforeFile is not absent, 0 or 1. Nothing changed."
    }
    Remove-Item $beforeFile
    Write-Host "Put back $metadataName ($before). The removed drivers come back with Synapse, or with Windows Update after tools\Block-RazerDriverUpdates.ps1 -Undo."
}

# Dot-sourced (by the tests): define the functions only, change nothing.
if ($MyInvocation.InvocationName -eq '.') { return }

if ($Apply -and $Undo) { throw 'Give -Apply or -Undo, not both. Nothing changed.' }
if ($Apply -or $Undo) {
    Assert-Admin
    $lock = Enter-ControllerLock
    try {
        if ($Undo) {
            Restore-ManufacturerApps
        } else {
            Assert-SynapseGone
            Assert-MetadataValueKnown
            New-RestorePoint
            Remove-RazerPackages
            Block-ManufacturerApps
            Write-Host "`nRestart now. Then run this script again without -Apply to check."
        }
    } finally {
        $lock.Dispose()
    }
} else {
    Show-State
}
