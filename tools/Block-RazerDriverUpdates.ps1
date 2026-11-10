# Ticket 74: hides the Razer driver updates that Windows Update offers, so
# they are not installed with the next update. Other drivers are left alone.
#   .\tools\Block-RazerDriverUpdates.ps1           # dry run: list them
#   .\tools\Block-RazerDriverUpdates.ps1 -Apply    # admin: hide them
#   .\tools\Block-RazerDriverUpdates.ps1 -Undo     # admin: show them again
# A newer Razer driver comes as a new update: run the dry run now and then.

param(
    [switch]$Apply,
    [switch]$Undo
)

$ErrorActionPreference = 'Stop'

function Get-RazerDriverUpdates {
    $session = New-Object -ComObject Microsoft.Update.Session
    $searcher = $session.CreateUpdateSearcher()
    # Hidden ones are asked for apart, so they are listed too.
    foreach ($hidden in 0, 1) {
        $result = $searcher.Search("IsInstalled=0 and Type='Driver' and IsHidden=$hidden")
        $result.Updates | Where-Object { $_.DriverManufacturer -match '^Razer' }
    }
}

function Assert-Admin {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run -Apply or -Undo in an admin PowerShell.'
    }
}

if ($Apply -and $Undo) { throw 'Use -Apply or -Undo, not both.' }

Write-Host 'Asking Windows Update. This can take a minute.'
$updates = @(Get-RazerDriverUpdates)
if ($Apply -or $Undo) {
    Assert-Admin
    foreach ($update in $updates) { $update.IsHidden = [bool]$Apply }
    $updates = @(Get-RazerDriverUpdates)
}

Write-Host "`nRazer driver updates offered: $($updates.Count)"
foreach ($update in $updates) {
    $state = if ($update.IsHidden) { 'hidden ' } else { 'OFFERED' }
    Write-Host "  $state  $($update.Title)  ($($update.DriverHardwareID))"
}
$offered = @($updates | Where-Object { -not $_.IsHidden })
Write-Host ("`nResult: " + $(if ($offered.Count -eq 0) { 'DONE. No Razer driver will install.' } else { 'NOT DONE.' }))
