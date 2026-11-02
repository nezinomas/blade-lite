# Walks through the stage 1 capture: one USBPcap file per Synapse action.
# Run in an admin PowerShell, with Synapse open.
#   .\capture\Start-SynapseCapture.ps1            # all steps
#   .\capture\Start-SynapseCapture.ps1 -From 12   # resume at step 12
# Raw files go to capture\raw (not committed: they also hold your key presses).

param(
    [int]$From = 0,
    [string]$OutDir = (Join-Path $PSScriptRoot 'raw')
)

$ErrorActionPreference = 'Stop'

$steps = @(
    @{ Name = 'idle-dashboard';     Seconds = 60; Do = 'Open the Synapse dashboard for the laptop. Do not touch anything. The script waits 60 seconds.' }
    @{ Name = 'preset-silent';      Do = 'Performance tab: pick Silent.' }
    @{ Name = 'preset-balanced';    Do = 'Pick Balanced.' }
    @{ Name = 'preset-performance'; Do = 'Pick Performance.' }
    @{ Name = 'custom-cpu-boost';   Do = 'Pick Custom. Set CPU to each level from lowest to highest, 3 seconds apart. Note the level names.' }
    @{ Name = 'custom-gpu-boost';   Do = 'Still Custom. Set GPU to each level from lowest to highest, 3 seconds apart. Note the level names.' }
    @{ Name = 'fan-manual-min';     Do = 'Fan: pick Manual. Move the slider to the LOWEST value. Note the RPM it shows.' }
    @{ Name = 'fan-manual-max';     Do = 'Move the fan slider to the HIGHEST value. Note the RPM it shows.' }
    @{ Name = 'fan-auto';           Do = 'Fan: pick Auto. Then pick Balanced.' }
    @{ Name = 'charge-limit-on';    Do = 'Battery: turn the charge limit ON. Note the limit it shows.' }
    @{ Name = 'charge-limit-levels';Do = 'If Synapse lets you pick other limits (like 60% or 70%), pick each one, 3 seconds apart. Note them. If not, just press Enter.' }
    @{ Name = 'charge-limit-off';   Do = 'Turn the charge limit OFF.' }
    @{ Name = 'charge-limit-80';    Do = 'Turn the charge limit ON again at 80% (your normal setting).' }
    @{ Name = 'brightness-0';       Do = 'Lighting: set keyboard brightness to 0%.' }
    @{ Name = 'brightness-1';       Do = 'Set keyboard brightness to 1% (or the lowest step above 0).' }
    @{ Name = 'brightness-50';      Do = 'Set keyboard brightness to 50%.' }
    @{ Name = 'brightness-100';     Do = 'Set keyboard brightness to 100%.' }
    @{ Name = 'colour-a-red';       Do = 'Effect: Static. Pick pure red (FF0000).' }
    @{ Name = 'colour-b-blue';      Do = 'Static, pick pure blue (0000FF).' }
    @{ Name = 'logo-off';           Do = 'Turn the lid logo OFF.' }
    @{ Name = 'logo-on';            Do = 'Turn the lid logo ON.' }
    @{ Name = 'touchpad-key-blue';  Do = 'Keyboard is static blue. Press the touchpad key (numpad ".") once: touchpad OFF. Wait 3 seconds. Press it again: touchpad ON. Note if the key turned red.' }
    @{ Name = 'colour-green';       Do = 'Static, pick pure green (00FF00).' }
    @{ Name = 'touchpad-key-green'; Do = 'Press the touchpad key: OFF. Wait 3 seconds. Press it: ON.' }
    @{ Name = 'brightness-low';     Do = 'Set keyboard brightness to 10%.' }
    @{ Name = 'touchpad-key-low';   Do = 'Press the touchpad key: OFF. Wait 3 seconds. Press it: ON. Note if the red key is dim too.' }
    @{ Name = 'touchpad-key-zero';  Do = 'Set brightness to 0%. Press the touchpad key: OFF. Wait 3 seconds. Note if the key still glows. Press it: ON. Then set brightness back to your normal value.' }
    @{ Name = 'fn-keys';            Do = 'Press Fn + keyboard-brightness keys up and down, and Fn + R if you have it. Note what happened.' }
    @{ Name = 'display-mode';       Procmon = $true; Do = 'Change the native display mode to the other option. If Synapse asks to restart: choose LATER. Then change it back. Choose LATER again.' }
    @{ Name = 'gpu-mode';           Procmon = $true; Do = 'Change GPU mode to the other option. If Synapse asks to restart: choose LATER. Then change it back. Choose LATER again.' }
)

function Assert-Admin {
    $id = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $id.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this in an admin PowerShell (Terminal (Admin)).'
    }
}

function Find-Tool([string[]]$Paths) {
    $hit = $Paths | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $hit) { throw "Not found: $($Paths -join ', ')" }
    $hit
}

function Get-UsbPcapInterfaces {
    # USBPcapCMD output does not reach a PowerShell pipe, so go through a file.
    $tmp = New-TemporaryFile
    Start-Process -FilePath $script:UsbPcap -ArgumentList '--extcap-interfaces' `
        -RedirectStandardOutput $tmp -NoNewWindow -Wait
    $list = Get-Content $tmp
    Remove-Item $tmp
    $list | ForEach-Object { if ($_ -match '\{value=([^}]+)\}') { $Matches[1] } }
}

function Start-Capture([string]$Base) {
    $i = 0
    foreach ($iface in $script:Interfaces) {
        $i++
        $file = "$Base.hub$i.pcap"
        Start-Process -FilePath $script:UsbPcap -WindowStyle Hidden -PassThru `
            -ArgumentList @('-d', $iface, '-o', $file, '-A', '-s', '65535')
    }
}

function Start-Procmon([string]$Base) {
    if (-not $script:Procmon) { return $null }
    Start-Process -FilePath $script:Procmon -ArgumentList @('/Quiet', '/Minimized', '/BackingFile', "$Base.pml")
    Start-Sleep -Seconds 5
    $true
}

function Stop-Procmon {
    & $script:Procmon /Terminate
    Start-Sleep -Seconds 3
}

function Invoke-Step([int]$Index, [hashtable]$Step) {
    $base = Join-Path $OutDir ('{0:D2}-{1}' -f $Index, $Step.Name)
    Write-Host ''
    Write-Host ("[{0}/{1}] {2}" -f $Index, ($steps.Count - 1), $Step.Name) -ForegroundColor Cyan
    Write-Host $Step.Do
    Read-Host 'Press Enter to START recording'
    # USBPcapCMD cannot write over an old file, so a redone step starts clean.
    Remove-Item "$base.*" -ErrorAction SilentlyContinue
    $pm = if ($Step.Procmon) { Start-Procmon $base }
    $procs = Start-Capture $base
    try {
        Start-Sleep -Seconds 1
        Write-Host 'RECORDING. Do the step in Synapse now.' -ForegroundColor Yellow
        if ($Step.Seconds) { Start-Sleep -Seconds $Step.Seconds } else { Read-Host 'Press Enter when DONE' }
        Start-Sleep -Seconds 2
    } finally {
        # Also runs on Ctrl+C, so no recorder is left holding a file.
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
        if ($pm) { Stop-Procmon }
    }
    $note = Read-Host 'What did you see? (type a note, or just Enter)'
    Add-Content -Path (Join-Path $OutDir 'notes.txt') -Value ('{0:D2}-{1}: {2}' -f $Index, $Step.Name, $note)
}

Assert-Admin
$script:UsbPcap = Find-Tool @('C:\Program Files\USBPcap\USBPcapCMD.exe')
$script:Procmon = (Get-Command Procmon64.exe, Procmon.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
$script:Interfaces = @(Get-UsbPcapInterfaces)
if (-not $script:Interfaces) { throw 'No USBPcap interface found. Did you restart after installing USBPcap?' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Write-Host "USBPcap interfaces: $($script:Interfaces -join ', ')"
if (-not $script:Procmon) { Write-Host 'Process Monitor not found: display and GPU steps record USB only.' }

for ($i = $From; $i -lt $steps.Count; $i++) { Invoke-Step $i $steps[$i] }
Write-Host ''
Write-Host "All done. Files are in $OutDir" -ForegroundColor Green
