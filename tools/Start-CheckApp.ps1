# Ticket 166: builds this checkout's app (release, so no console) into
# target\app and starts it on its own, for a laptop check that ends the
# Windows session (sign out, restart, shut down).
#   powershell -ExecutionPolicy Bypass -File tools\Start-CheckApp.ps1
# Why not `cargo app`: `cargo run` puts the app in a job that ends it when
# cargo ends, and Windows ends cargo with its terminal as the session ends.
# The app is then killed before it hears the session end, so it sends
# nothing (laptop check 23, try 1). Started here, the app is not in cargo's
# job and has no console, as when it starts with Windows.
# Run it from the root of the checkout that has the change. Quit any
# running Blade Lite first (tray, Quit).

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\app\release\blade-lite.exe'

# The one-copy lock would make a new start open the old copy, which may be
# an older build or one started by cargo.
if (Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue) {
    Write-Error 'Blade Lite runs already. Quit it from the tray (Quit), then run this again.'
}

Push-Location $root
try {
    cargo build -p blade-lite --release --target-dir target/app
    if ($LASTEXITCODE -ne 0) {
        Write-Error "The build failed (exit $LASTEXITCODE), so nothing was started."
    }
} finally {
    Pop-Location
}

# Through Explorer, as Windows starts it at login: a process started from
# here would be in any job this terminal is in, and such a job can end it
# with the terminal.
explorer.exe $exe
Start-Sleep -Seconds 3
$app = Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $exe } | Select-Object -First 1
if ($null -eq $app) {
    Write-Error "$exe did not start. Look in %APPDATA%\blade-lite\blade-lite.log."
}
"Started $exe (process $($app.Id))."
'It runs on its own: closing this window does not end it. Quit it from the tray.'
