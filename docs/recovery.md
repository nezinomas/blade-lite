# When the fans or the power mode are stuck

Blade Lite sets the fans and the power mode on the laptop's controller.
The controller keeps the last setting, even with no app running.
This page says how to get back to normal.

## What you may see

- The fans stay fast, and the laptop is cool.
- The fans stay slow or still, and the laptop gets hot under load.
- Performance stays on after you pull the charger.

## Fans stuck on a manual speed

This is the way checked on the laptop (laptop check 18):

1. Plug in the charger. On battery the app can't send Custom, and the
   steps below need it.
2. Start Blade Lite. Pick no other preset first.
3. Pick **Quit** in the tray menu at once.

If Custom with a manual fan is the saved preset, Quit first sets Custom
with the fans on auto, then sends Balanced.

Another way: pick **Custom**, set the fan to **Auto**, then pick
**Balanced**.

Picking **Balanced** alone, straight from a manual fan, is not checked
yet (laptop check 12).

With no Blade Lite running, `blade-lite.exe --reset` sends the same two
steps (Custom with the fans on auto and the lowest boosts, then Balanced)
and ends, with no window (ticket 148; laptop check 64). It sends nothing
while Blade Lite or Razer software runs.

## Performance stays on, or another preset

Start Blade Lite and pick **Balanced**.

## When Blade Lite can't open the controller

Restart the laptop.
After a restart a manual fan kept its speed through the boot.
A few seconds after the desktop came up, it went back to normal.
This was heard once, with "Start with Windows" off; the fan state was not
read from the controller (laptop check 21, 2026-11-11).
With "Start with Windows" on, the app sends the saved preset again at
login.
Whether a restart ends Performance is not checked.
Whether a shutdown or a long press of the power button does the same is
not checked.

## Before a Razer firmware or BIOS update

Quit Blade Lite first (tray menu, **Quit**). If a box said Blade Lite can't
open the controller yet, it still runs with no tray icon: end
`blade-lite.exe` in Task Manager.
A firmware update must not meet the app's commands.
Blade Lite sends nothing while it sees a Razer updater, but it can miss one.

## What Blade Lite leaves running

- **Quit** (tray menu): sends Balanced, or Silent if Silent runs. If Custom
  runs with a manual fan, it first sets the fans to auto.
- **`blade-lite.exe --quit`** (for an installer, ticket 148): asks the
  running copy to quit as Quit does. It waits up to 20 s for the copy to
  end; a copy that took the ask in that time gets 120 s more (140 s in
  all). It reaches only a copy in the same Windows session. Exit codes
  (the log says why):
  - 0: no copy ran, or the copy quit with its quit preset sent.
  - 1: it failed, such as when the copy ended but its quit preset was
    not sent, or the copy did not take the ask. If a copy still runs, run
    `--quit` once more; if it gives 1 again, end the copy in Task
    Manager. Then run `--reset`.
  - 2: the copy may be quitting but has not ended, or an error came while
    it may be quitting. Don't end it. Run `--quit` again until it gives 0
    or 1, then run `--reset`.
- **A shutdown, restart or sign-out**: tries to send the same as Quit
  first. Checked on the laptop on AC (checks 23 and 38); on battery with
  the panel at 60 Hz not yet (check 38). It waits at most 10 s
  for the app to be free, such as after a rate switch (ticket 159); if the
  app stays busy longer, it may send nothing.
- **Most bugs inside the app**: it sends the same as Quit, then shows a
  message box. A crash during the start, or a hard crash, sends nothing,
  as a kill does.
- **Ended in Task Manager** (a kill): sends nothing. A manual Custom fan,
  the fan guard's full speed or Performance keeps running. Use the steps
  above.
- **While Blade Lite sees a Razer updater**: it sends nothing, also not at
  Quit. A manual fan stays as set, also after the updater ends. Then use
  the steps above.
