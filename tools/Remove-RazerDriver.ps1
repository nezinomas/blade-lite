# Stage 2: removes the Razer driver packages so 02C7 falls back to the
# inbox Windows HID drivers. Uninstall Synapse in Settings first.
#   .\tools\Remove-RazerDriver.ps1           # dry run: show what is there
#   .\tools\Remove-RazerDriver.ps1 -Apply    # admin: restore point, remove, block
# Run the dry run again after a restart to check the result.

param(
    [switch]$Apply,
    [switch]$SkipRestorePoint
)

$ErrorActionPreference = 'Stop'

$metadataKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Device Metadata'

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

function Get-BladeDevices {
    Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -match 'VID_1532&PID_02C7' } |
        ForEach-Object {
            $inf = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_DriverInfPath).Data
            [pscustomobject]@{ Inf = $inf; Name = $_.FriendlyName; Id = $_.InstanceId }
        }
}

function Show-State {
    $apps = @(Get-RazerApps)
    $packages = @(Get-RazerDriverPackages)
    $devices = @(Get-BladeDevices)
    $onRazer = @($devices | Where-Object { $_.Inf -like 'oem*' })
    $metadata = (Get-ItemProperty $metadataKey -ErrorAction SilentlyContinue).PreventDeviceMetadataFromNetwork

    Write-Host "`nRazer apps installed: $($apps.Count)"
    $apps | ForEach-Object { Write-Host "  $_" }
    Write-Host "`nRazer driver packages: $($packages.Count)"
    $packages | ForEach-Object { Write-Host "  $($_.Published)  $($_.Original)" }
    Write-Host "`n02C7 devices on a Razer driver: $($onRazer.Count) of $($devices.Count)"
    $onRazer | ForEach-Object { Write-Host "  $($_.Inf)  $($_.Id)" }
    Write-Host "`nManufacturer apps from Windows Update blocked: $($metadata -eq 1)"

    $done = $packages.Count -eq 0 -and $onRazer.Count -eq 0 -and $metadata -eq 1
    Write-Host ("`nResult: " + $(if ($done) { 'DONE. 02C7 is on the inbox drivers.' } else { 'NOT DONE.' }))
}

function Assert-Admin {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run -Apply in an admin PowerShell.'
    }
}

function Assert-SynapseGone {
    $synapse = @(Get-RazerApps | Where-Object { $_ -match 'Synapse|Chroma' })
    if ($synapse.Count -gt 0) {
        throw "Uninstall these in Settings > Apps first, then restart: $($synapse -join ', ')"
    }
}

function New-RestorePoint {
    if ($SkipRestorePoint) { return }
    Enable-ComputerRestore -Drive "$env:SystemDrive\"
    Checkpoint-Computer -Description 'Before Razer driver removal' -RestorePointType MODIFY_SETTINGS
    Write-Host 'Restore point made.'
}

function Remove-RazerPackages {
    foreach ($package in Get-RazerDriverPackages) {
        Write-Host "Removing $($package.Published) ($($package.Original))"
        pnputil /delete-driver $package.Published /uninstall
        if ($LASTEXITCODE -ne 0) { Write-Warning "pnputil exit $LASTEXITCODE for $($package.Published)" }
    }
}

function Block-ManufacturerApps {
    New-Item $metadataKey -Force | Out-Null
    Set-ItemProperty $metadataKey -Name PreventDeviceMetadataFromNetwork -Value 1 -Type DWord
    Write-Host 'Turned off "Automatically get manufacturers'' apps and custom icons".'
}

if ($Apply) {
    Assert-Admin
    Assert-SynapseGone
    New-RestorePoint
    Remove-RazerPackages
    Block-ManufacturerApps
    Write-Host "`nRestart now. Then run this script again without -Apply to check."
} else {
    Show-State
}
