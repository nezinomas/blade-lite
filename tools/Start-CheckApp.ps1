# Ticket 166: builds this checkout's app (release, so no console) into
# target\app and starts it on its own, for a laptop check that ends the
# Windows session (sign out, restart, shut down).
#   powershell -ExecutionPolicy Bypass -File tools\Start-CheckApp.ps1
# Why not `cargo app`: `cargo run` puts the app in a job that ends it when
# cargo ends, and Windows ends cargo with its terminal as the session ends.
# The app is then killed before it hears the session end, so it sends
# nothing (laptop check 23, try 1). Started here, the app is not in cargo's
# job, as when it starts with Windows.
# The release build is needed, not only tidier: a debug build is a console
# program, and when its console closes, Windows may end it the same way.
# Run it from the root of the checkout that has the change. Quit any
# running Blade Lite first (tray, Quit).

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\app\release\blade-lite.exe'

# The one-copy lock would make a new start open the old copy, which may be
# an older build or one started by cargo.
function Assert-NoneRuns {
    if (Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue) {
        Write-Error 'Blade Lite runs already. Quit it from the tray (Quit), then run this again.'
    }
}

Assert-NoneRuns
Push-Location $root
try {
    # Cargo writes its progress to stderr, which 'Stop' would take as an
    # error when the output is captured. The exit code says if it failed.
    $ErrorActionPreference = 'Continue'
    cargo build -p blade-lite --release --target-dir target/app
    $built = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($built -ne 0) {
        Write-Error "The build failed (exit $built), so nothing was started."
    }
} finally {
    Pop-Location
}
# Again: a copy may have started during the build.
Assert-NoneRuns

# Through Explorer, as Windows starts it at login: a process started from
# here would be in any job this terminal is in, and such a job can end it
# with the terminal. Explorer's own job only allows breakaway (ticket 166).
explorer.exe $exe
Start-Sleep -Seconds 3
$app = Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $app) {
    Write-Error "$exe did not start. Look in %APPDATA%\blade-lite\blade-lite.log."
}
"Started $($app.Path) (process $($app.Id))."
'Check that its window or tray icon shows, with no error box.'
'It runs on its own: closing this window does not end it. Quit it from the tray.'
