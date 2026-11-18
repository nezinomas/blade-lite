# Removing Synapse and the Razer driver (stage 2)

After this, the Blade's `1532:02C7` interfaces use the inbox Windows HID
drivers, so our app can talk to MI_02 with no Razer software.

## Before (2026-11-02)

`tools/Remove-RazerDriver.ps1` (dry run) found:

| Item | Found |
| --- | --- |
| Razer apps | Razer Synapse 4.0.896, Razer Chroma 4.0.896, RazerXTU 1.0.7, Razer IntelOverClock 1.0.3.0 |
| Driver packages | `oem75` rzdevu_02c7_dkm, `oem76` rzdevu_02c7_vcon, `oem79` rzcommonu, `oem80` rzdevu_02c7_kbd, `oem81` rzdevu_02c7_mou |
| 02C7 nodes on a Razer driver | 5 of 17, including HID MI_02 (`oem81`, the mouse filter) |
| THX Spatial Audio | Publisher THX, not Razer. It is the audio driver. Keep it |

## Steps

1. Settings > Apps > Installed apps. Uninstall Razer Synapse, Razer Chroma,
   RazerXTU and Razer IntelOverClock. Restart.
2. In an admin PowerShell, in the repo folder:
   `powershell -ExecutionPolicy Bypass -File .\tools\Remove-RazerDriver.ps1 -Apply`.
   Quit Blade Lite first (the script takes its lock). It makes a restore
   point and stops if Windows made none (Windows skips one if any restore
   point was made in the last 24 hours; then run it the next day, or add
   `-SkipRestorePoint`). It removes each Razer driver package that no other
   Razer device Windows knows has as its main driver (a Razer mouse keeps
   its driver, plugged in or not; Bluetooth devices and extension drivers
   are not seen) with `pnputil /delete-driver <oem#.inf> /uninstall`, keeps the
   old value of "Automatically get manufacturers' apps and custom icons",
   and turns it off.
3. Restart.
4. In a normal PowerShell:
   `powershell -ExecutionPolicy Bypass -File .\tools\Remove-RazerDriver.ps1`.
   It must say `Result: DONE`.
5. Check by hand: typing, the touchpad, the touchpad key, Fn + keyboard
   brightness, Fn + volume, and the lid logo.

Windows Update offers the Razer drivers again (seen 2026-11-09). Hide them
with `tools/Block-RazerDriverUpdates.ps1 -Apply` (ticket 74). A device ID
block (DeviceInstall restriction keys) was not used: it could also block the
inbox drivers the keyboard needs.

## Rollback

Use the restore point "Before Razer driver removal" (System Properties >
System Protection > System Restore), or install Synapse again from
razer.com. `tools/Remove-RazerDriver.ps1 -Undo` (admin) puts back only the
"manufacturers' apps" setting as it was before `-Apply`; it can't put back
the removed driver packages.

## After (2026-11-02)

Uninstalling Synapse in Settings already removed all 5 Razer driver
packages. `-Apply` then only made the restore point and the Windows Update
block.

| Check | Result |
| --- | --- |
| Script check | `Result: DONE`. 0 Razer apps, 0 Razer packages, 0 of 13 nodes on a Razer driver |
| HID MI_02 | `msmouse.inf` (inbox "HID-compliant mouse"), was `oem81` |
| Typing, touchpad | Work |
| Touchpad key | Turns the touchpad on and off. The key does NOT turn red any more |
| Fn + keyboard brightness | Works (up and down) |
| Fn + volume | Works |

So the firmware handles the Fn keys, not Synapse. The red touchpad key came
from Synapse after all (likely through `0059`), so our app must paint it.
