# Ticket 147 (b): logs what blade-lite.exe uses, once a minute, for the
# one-day check in the tray (laptop check 26). Reads only.
#   powershell -ExecutionPolicy Bypass -File tools\Watch-BladeLite.ps1
#   powershell -ExecutionPolicy Bypass -File tools\Watch-BladeLite.ps1 -Minutes 1440 -Out soak.csv
# Each line: the time, the working set and private memory in MB, handles,
# threads, and the CPU seconds used so far. A line with no process means
# the app was not running then. Stop it with Ctrl+C; the file keeps what
# was written.

param(
    [int]$Minutes = 1440,
    [string]$Out = 'blade-lite-soak.csv'
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Out)) {
    'time,working_set_mb,private_mb,handles,threads,cpu_seconds' | Out-File -FilePath $Out -Encoding utf8
}

for ($i = 0; $i -lt $Minutes; $i++) {
    $time = Get-Date -Format 's'
    $app = Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $app) {
        $line = "$time,,,,,"
    } else {
        # A dot as the decimal mark whatever the PC's language, so the
        # commas stay field separators.
        $line = [string]::Format([cultureinfo]::InvariantCulture,
            '{0},{1:F1},{2:F1},{3},{4},{5:F1}', $time,
            ($app.WorkingSet64 / 1MB), ($app.PrivateMemorySize64 / 1MB),
            $app.HandleCount, $app.Threads.Count, $app.TotalProcessorTime.TotalSeconds)
    }
    $line | Out-File -FilePath $Out -Append -Encoding utf8
    Write-Host $line
    if ($i -lt $Minutes - 1) {
        Start-Sleep -Seconds 60
    }
}
