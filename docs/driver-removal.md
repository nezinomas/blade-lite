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
   `.\tools\Remove-RazerDriver.ps1 -Apply`.
   It makes a restore point, removes every Razer driver package with
   `pnputil /delete-driver <oem#.inf> /uninstall`, and turns off
   "Automatically get manufacturers' apps and custom icons".
3. Restart.
4. In a normal PowerShell: `.\tools\Remove-RazerDriver.ps1`.
   It must say `Result: DONE`.
5. Check by hand: typing, the touchpad, the touchpad key, Fn + keyboard
   brightness, Fn + volume, and the lid logo.

If Windows Update puts the driver back later, block the device ID with the
DeviceInstall restriction keys under `HKLM\SOFTWARE\Policies` (Home has no
Group Policy editor). Not done now (YAGNI).

## Rollback

Use the restore point "Before Razer driver removal" (System Properties >
System Protection > System Restore), or install Synapse again from
razer.com.

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
