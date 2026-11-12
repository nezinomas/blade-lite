# Ticket 147 (b): logs what blade-lite.exe uses, once a minute, for the
# one-day check in the tray (laptop check 26). Reads only.
#   powershell -ExecutionPolicy Bypass -File tools\Watch-BladeLite.ps1
#   powershell -ExecutionPolicy Bypass -File tools\Watch-BladeLite.ps1 -Minutes 1440 -Out target\soak.csv
# Each line: the time, the working set and private memory in MB, handles,
# threads, and the CPU seconds used so far. A line with no values means
# the app was not running then. It runs for -Minutes of clock time, sleeps
# included. Stop it early with Ctrl+C; the file keeps what was written.
# A line that can't be written (the file open in Excel) is skipped with a
# warning, and the run goes on.

param(
    [ValidateRange(1, 100000)]
    [int]$Minutes = 1440,
    [string]$Out = 'target\blade-lite-soak.csv'
)

$ErrorActionPreference = 'Stop'

function Get-Sample {
    $time = Get-Date -Format 's'
    $app = Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $app) {
        return "$time,,,,,"
    }
    # A dot as the decimal mark whatever the PC's language, so the commas
    # stay field separators.
    [string]::Format([cultureinfo]::InvariantCulture,
        '{0},{1:F1},{2:F1},{3},{4},{5:F1}', $time,
        ($app.WorkingSet64 / 1MB), ($app.PrivateMemorySize64 / 1MB),
        $app.HandleCount, $app.Threads.Count, $app.TotalProcessorTime.TotalSeconds)
}

if (-not (Test-Path $Out)) {
    'time,working_set_mb,private_mb,handles,threads,cpu_seconds' | Out-File -FilePath $Out -Encoding ascii
}

$end = (Get-Date).AddMinutes($Minutes)
while ((Get-Date) -lt $end) {
    try {
        $line = Get-Sample
        $line | Out-File -FilePath $Out -Append -Encoding ascii
        Write-Host $line
    } catch {
        # Such as the app ending between two reads, or the file locked.
        Write-Warning "skipped a sample: $_"
    }
    Start-Sleep -Seconds 60
}
