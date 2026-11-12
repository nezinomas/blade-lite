# When the fans or the power mode are stuck

Blade Lite sets the fans and the power mode on the laptop's controller.
The controller keeps the last setting, even with no app running.
This page says how to get back to normal.

## What you may see

- The fans stay fast, and the laptop is cool.
- The fans stay slow or still, and the laptop gets hot under load.
- Performance stays on after you pull the charger.

## With Blade Lite

1. Start Blade Lite.
2. Pick **Balanced**.

Or start Blade Lite and pick **Quit** in the tray menu.
Quit sends Balanced, with the fans on auto (ticket 130).

If a Razer updater runs, Blade Lite sends nothing until it ends.
Wait for the update to finish, then do the steps above.

## When Blade Lite can't open the controller

Restart the laptop.
After a restart the fans kept their manual speed through the boot.
A few seconds after the desktop came up, they went back to normal.
This was heard once, not read (laptop check 21, 2026-11-11).
Whether a shutdown or a long press of the power button does the same is not checked.

## Before a Razer firmware or BIOS update

Quit Blade Lite first (tray menu, **Quit**).
A firmware update must not meet the app's commands.

## What Blade Lite leaves running

- **Quit** (tray menu): sends Balanced, with the fans on auto.
- **A shutdown, restart or sign-out**: sends Balanced first (ticket 142).
- **A crash inside the app**: sends Balanced, then shows a message box (ticket 141).
- **Ended in Task Manager** (a kill): sends nothing. A manual Custom fan, the fan guard's full speed or Performance keeps running.
  Start Blade Lite again and pick Balanced.
- **While a Razer updater runs**: Blade Lite sends nothing, also not at Quit.
  A manual fan stays as set until the updater ends.
