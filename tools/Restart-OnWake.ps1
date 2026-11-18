# Ticket 284: for laptop check 30, a Windows restart inside the app's 2 s
# pause after a wake (RESUME_PAUSE), which a hand can't pick in time. It
# watches the app's log for its wake line and, when armed, starts a normal
# restart at once (shutdown /r /t 0: with no time-out, /f is not implied,
# so the app hears the session end as at a Start-menu Restart; Microsoft's
# shutdown page). Then the log shows whether the quit waited for the pause.
#   powershell -ExecutionPolicy Bypass -File tools\Restart-OnWake.ps1 -Armed
# Without -Armed it only says when it saw the wake line (a dry run). It acts
# on the first wake line written after it starts, then ends; it ends with
# nothing done after -WaitMinutes. Blade Lite must run (started with
# tools\Start-CheckApp.ps1 for the check, ticket 166).

param(
    [switch]$Armed,
    [ValidateRange(1, 120)]
    [int]$WaitMinutes = 30,
    # Only for a dry run against a copy: the app writes its own log.
    [string]$LogPath = (Join-Path $env:APPDATA 'blade-lite\blade-lite.log')
)

$ErrorActionPreference = 'Stop'

# The app's line at Windows' wake notice (shell\ticks.rs), which begins the pause.
$wakeLine = 'Windows says the laptop woke ('

if (-not (Get-Process -Name 'blade-lite' -ErrorAction SilentlyContinue)) {
    Write-Error 'Blade Lite does not run, so no wake line can come. Start it first, then run this again.'
}
if (-not (Test-Path -LiteralPath $LogPath -PathType Leaf)) {
    Write-Error "No log at $LogPath."
}
if ($Armed -and $LogPath -ne (Join-Path $env:APPDATA 'blade-lite\blade-lite.log')) {
    Write-Error 'Armed only with the app''s own log, so no other file can restart Windows.'
}

# Lines already in the log are old: only new ones count.
$offset = (Get-Item -LiteralPath $LogPath).Length
$until = (Get-Date).AddMinutes($WaitMinutes)
$mode = if ($Armed) { 'ARMED: Windows restarts at the wake line' } else { 'dry run: nothing is restarted' }
"Watching $LogPath ($mode). Ends by itself at $($until.ToString('HH:mm:ss')). Ctrl + C stops it."

while ((Get-Date) -lt $until) {
    $length = (Get-Item -LiteralPath $LogPath).Length
    # A rolled log starts again small: read it from its start.
    if ($length -lt $offset) {
        $offset = 0
    }
    if ($length -gt $offset) {
        # The app holds the file open to write: share it.
        $stream = [System.IO.File]::Open($LogPath, 'Open', 'Read', 'ReadWrite')
        try {
            $null = $stream.Seek($offset, 'Begin')
            $new = [System.IO.StreamReader]::new($stream).ReadToEnd()
        } finally {
            $stream.Dispose()
        }
        # A line cut mid-write is read again whole next time.
        $end = $new.LastIndexOf("`n")
        if ($end -ge 0) {
            $offset += [System.Text.Encoding]::UTF8.GetByteCount($new.Substring(0, $end + 1))
            $whole = $new.Substring(0, $end + 1)
            if ($whole.Contains($wakeLine)) {
                $seen = (Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff')
                if (-not $Armed) {
                    "Wake line seen at ${seen}Z. Dry run, so no restart."
                    exit 0
                }
                "Wake line seen at ${seen}Z. Restarting Windows now."
                shutdown.exe /r /t 0
                exit $LASTEXITCODE
            }
        }
    }
    Start-Sleep -Milliseconds 20
}
"No wake line in $WaitMinutes min. Nothing was done."
