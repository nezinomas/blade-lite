# Ticket 284: for laptop check 30, a Windows restart inside the app's 2 s
# pause after a wake (RESUME_PAUSE), which a hand can't pick in time. It
# watches the app's log for its wake line and, when armed, starts a normal
# restart at once (shutdown /r /t 0: with no time-out, /f is not implied,
# so the app hears the session end as at a Start-menu Restart; Microsoft's
# shutdown page). Then the log shows whether the quit waited for the pause.
#   powershell -ExecutionPolicy Bypass -File tools\Restart-OnWake.ps1 -Armed
# Without -Armed it only says when it saw the wake line (a dry run). Armed,
# it restarts at the first wake of any kind (a sleep too), so save your work
# first, and stop it with Ctrl + C if the try is called off. It acts once,
# then ends; it ends with nothing done when Blade Lite quits, or after
# -WaitMinutes of wall time (time asleep counts). A log roll skips what the
# old file got after the last read: a try may be missed, never an old line
# taken as new. The times it saw the line and asked for the restart go to
# %TEMP%\Restart-OnWake.txt, as its window is gone after the restart.
# Blade Lite must run (started with tools\Start-CheckApp.ps1, ticket 166).

param(
    [switch]$Armed,
    [ValidateRange(1, 120)]
    [int]$WaitMinutes = 30,
    # Only for a dry run against a copy: the app writes its own log.
    [string]$LogPath = (Join-Path $env:APPDATA 'blade-lite\blade-lite.log')
)

$ErrorActionPreference = 'Stop'

# The app's line at Windows' wake notice (shell\ticks.rs), which begins the
# pause, after the line's time stamp.
$wakeLine = '(?m)^\d{4}-\d\d-\d\d \d\d:\d\d:\d\dZ Windows says the laptop woke \('
$record = Join-Path $env:TEMP 'Restart-OnWake.txt'
$utf8 = [System.Text.UTF8Encoding]::new($false)

function Test-AppRuns {
    [bool](Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue)
}

if (-not (Test-AppRuns)) {
    Write-Error 'Blade Lite does not run, so no wake line can come. Start it first, then run this again.'
}
if (-not (Test-Path -LiteralPath $LogPath -PathType Leaf)) {
    Write-Error "No log at $LogPath."
}
if ($Armed -and $LogPath -ne (Join-Path $env:APPDATA 'blade-lite\blade-lite.log')) {
    Write-Error 'Armed only with the app''s own log, so no other file can restart Windows.'
}

# The whole lines of $path from byte $from on, and the byte count they take.
# The app opens the log for each line; delete is shared so its roll (a
# rename) is never refused while this reads.
function Read-NewLines([string]$path, [long]$from) {
    $stream = [System.IO.File]::Open($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
        [System.IO.FileShare]'ReadWrite, Delete')
    try {
        $null = $stream.Seek($from, 'Begin')
        $new = [System.IO.StreamReader]::new($stream, $utf8, $false).ReadToEnd()
    } finally {
        $stream.Dispose()
    }
    # A line cut mid-write is read again whole next time.
    $whole = $new.Substring(0, $new.LastIndexOf("`n") + 1)
    @($whole, $utf8.GetByteCount($whole))
}

# Reads what is new since $script:offset; true if it holds the wake line.
function Test-NewWakeLine {
    try {
        $length = (Get-Item -LiteralPath $LogPath).Length
        if ($length -lt $script:offset) {
            # Rolled or emptied: the new file from its start. The old one is
            # not read, as `.1` may be an older file (an emptied log).
            Write-Host "The log was rolled at $((Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff'))Z."
            $script:offset = 0
        }
        if ($length -le $script:offset) {
            return $false
        }
        $read = Read-NewLines $LogPath $script:offset
    } catch [System.IO.IOException], [System.Management.Automation.ItemNotFoundException] {
        # Mid-roll the log is gone for a moment: the next poll reads it.
        return $false
    }
    $script:offset += $read[1]
    $read[0] -match $wakeLine
}

# Lines already in the log are old: only new ones count.
$script:offset = (Get-Item -LiteralPath $LogPath).Length
$until = (Get-Date).AddMinutes($WaitMinutes)
$mode = if ($Armed) { 'ARMED: Windows restarts at the wake line' } else { 'dry run: nothing is restarted' }
"Watching $LogPath ($mode). Ends by itself at $($until.ToString('HH:mm:ss')). Ctrl + C stops it."

$found = $false
$polls = 0
while (-not $found -and (Get-Date) -lt $until) {
    $found = Test-NewWakeLine
    if (-not $found) {
        # Each second: an app that quit writes no wake line.
        if ((++$polls % 50) -eq 0 -and -not (Test-AppRuns)) {
            'Blade Lite quit, so nothing was done.'
            exit 0
        }
        Start-Sleep -Milliseconds 20
    }
}
if (-not $found) {
    "No wake line in $WaitMinutes min. Nothing was done."
    exit 0
}
$seen = (Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff')
if (-not $Armed) {
    "Wake line seen at ${seen}Z. Dry run, so no restart."
    exit 0
}
Add-Content -LiteralPath $record -Value "wake line seen at ${seen}Z, restart asked" -Encoding utf8
"Wake line seen at ${seen}Z. Restarting Windows now."
shutdown.exe /r /t 0
$started = $LASTEXITCODE
Add-Content -LiteralPath $record -Value "shutdown.exe ended with $started at $((Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff'))Z" -Encoding utf8
exit $started
