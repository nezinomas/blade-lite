# Stage plan

One stage is one branch. Tick a stage (`[x]`) in the same branch that finishes it.
Each stage's red and green tests and the open questions are in the full plan:
https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P

How a stage is built, reviewed and merged: the `stage` skill and the hardware safety rules in `CLAUDE.md`.
Every stage is reviewed before merge by subagents (`.claude/agents/`): both the `informed-reviewer` and the `fresh-reviewer` when it can change what is sent to the laptop, the `informed-reviewer` alone when it can't ("Review tiers" in `CLAUDE.md`).
Checks only a person can do on the laptop wait in "Still to check on the laptop" below until the user confirms them.

## Before code
- [x] 0 Try r-helper and read its 02C7 code
- [x] 1 Capture Synapse USB traffic
- [x] 2 Remove Synapse and its driver

## Protocol, over fakes
- [x] 3 Checksum
- [x] 4 Reply parser
- [x] 5 Packet builder
- [x] 6 Device with retry
- [x] 7 Probe on the real laptop

## Performance and keyboard
- [x] 8 Keyboard brightness (and the model profile)
- [x] 9 Static colour
- [x] 10 Presets
- [x] 11 Custom
- [x] 12 Config
- [x] 13 Apply and re-apply

## Windows features
- [x] 14 Touchpad
- [x] 15 Refresh rates on offer
- [x] 16 Set refresh rate
- [x] 17 60 Hz on battery
- [x] 18 Native display mode

## Sensors
- [x] 19 Controller readings
- [x] 20 Windows readings
- [x] 21 Poll schedule

## UI
- [x] 22 Mica spike
- [x] 23 Header view model
- [x] 24 Display view model
- [x] 25 Performance view model
- [x] 26 Keyboard and touchpad view model
- [x] 27 Window shell
- [x] 28 Tray flyout
- [x] 29 Single instance and autostart

## Extras
- [x] 30 Lid logo
- [x] 31 Battery charge limit
- 32 GPU mode: dropped (no switch on this model)
- [x] 33 Red touchpad key (follows the Windows touchpad switch; the physical key is 47)
- [x] 34 Power-mode key (Fn + numpad Enter steps Silent, Balanced, Performance; read from the keyboard's key report, no hook; checked on the laptop)
- [x] 35 60 Hz on battery toggle (in the Display group and the flyout; a failed 60 Hz set saves the switch off again)
- [x] 48 Power-mode popup above the taskbar clock (no focus, fades out; shows each mode change that worked; checked on the laptop)
- [ ] 49 Same popup for touchpad on and off (the physical key needs 47)
- [x] 52 Vapor-chamber light switch (LED `0x26`, off stays off after a brightness change and a hibernate; checked on the laptop)

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [ ] 37 Fans stay loud after Performance to Silent
- [x] 38 Keyboard colour input needs guards (only `#RRGGBB` sends a colour)
- [x] 41 `cargo hw` takes the one-copy lock, so it can't run beside the app
- [x] 42 A log file (`%APPDATA%\blade-lite\blade-lite.log`, 5 files of 1 MB, panics too)
- [x] 43 Autostart tick re-read when the menu opens; says "(set to another program)" when the Run key names another exe
- [x] 44 Lid logo comes back on after a sleep (now saved and sent at start and after each resume; checked after a sleep and a hibernate)
- [x] 45 Each reply must echo its request's transaction ID and command, or `Device::send` returns `WrongReply` and does not send again
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [x] 50 Log the sensor and fan read errors (each kind once per 10 minutes)
- [x] 51 A reply with an older transaction ID is read again once, never sent again
- [x] 53 A new transaction ID per send, and a new `Device` starts at a random ID
- [x] 54 Num Lock off after a wake from hibernate: a Windows setting, not the app (fixed in the registry, see `docs/NOTES.md`)
- [x] 55 On Busy, read again (50 ms apart, up to 5 reads), never send again
- [x] 56 Plain `DeviceError` text, commands in hex
- [x] 57 A change the laptop took is kept in memory even if the config save fails; the error says `sent, but not saved`
- [x] 58 Flyout header without min, max and close (winit keeps their styles on a frameless window and Mica's frame showed them; a style hook now takes them out; checked on the laptop)
- [x] 59 Power-mode key with Num Lock on too: the keyboard then sends only Fn and Windows a plain Enter, so while Fn is held (up to 5 s) and Num Lock is on, the reader reads Enter's state (no hook) and an Enter press steps the preset. The Enter still reaches the open app; either Enter key counts (user's pick)
- [ ] 60 On a weak charge (no charger, but a monitor powers the laptop over USB-C, so it charges slowly), Synapse did not allow Performance, for lack of power. Do the same
- [x] 61 `Config::save` writes a temp file, then renames it over the config
- [ ] 62 A send that fails part way (such as the zones sent, then a boost Busy) leaves the controller with part of the new state, but the app keeps the old one until the next resume sends it again. Decide whether to read back or say so in the error
- [ ] 63 The config rename can fail with access denied while another program (such as Defender or the search indexer) holds `config.toml` or its temp file open. The change stays in memory (ticket 57), but the next start sends the old config. Retry only the rename a few times (such as 3 tries, 20 ms apart) on `PermissionDenied`, if the log ever shows it
- [x] 64 Quiet `cargo test`: tests build `Reporter::quiet` (no stderr), the app's reporter is unchanged; about 600 lines down to a few (the app-reporter and touchpad-key tests still print, on purpose)
- [x] 65 The native mode, lid logo and touchpad `cargo hw` tests keep each step's `Result`, put the state back, then assert the put-back first; a touchpad switch not seen in 3 s is waited out before the restore (checked on the laptop)
- [x] 66 Split by job, code moved only: `app.rs` into `app/` (error, views, clicks, events), `shell.rs` into `shell/` (windows, popup, ticks), `main.rs` threads and tray menu out, `tests/app.rs` into `tests/app/` by topic. Other long Rust files each do one job, so they stay
- [x] 67 `window.slint` split: shared parts, the header and one file per group (display, performance, keyboard); markup moved only, bindings and callbacks the same (checked on the laptop)
- [x] 68 Brightness percent (window and flyout) follows the slider while it is dragged; still sent only on release. After a release, a failed send or a change elsewhere, the slider goes back to the laptop's value (Slint now writes the text, Rust's text is gone)
- [x] 69 The charger test and the rate switch test keep each step's `Result`, put the rate back, then assert the put-back first; the charger test takes the one-copy lock and refuses to start at 60 Hz (both passed on the laptop)
- [x] 70 Window tests in `cargo t`: the window, flyout and popup drawn off screen over fakes (Slint's testing backend), 13 scripted clicks that check the exact commands sent, the window and the log, and 6 pictures compared with `crates/blade-lite/tests/screens/`. `CLAUDE.md` and the `stage` skill make threads run them and ask the user only what software can't see
- [ ] 71 Window tests for the custom colour field (type `#00B7C3`, Enter, check the colour sent) and the Custom sliders (drag CPU boost one step, check the command). Both reviewers
- [x] 72 Touchpad key (numpad ".") red only while the touchpad and Num Lock are both off; Num Lock read each 1 s poll, nothing painted during a resume pause (profile flag `plain_with_num_lock`; checked on the laptop)
- [ ] 73 Mic key (numpad 0 on the digit layer, mic on the second layer): red while the mic is muted and Num Lock is off, plain with Num Lock on, as ticket 72. Fn + mic today shows the Windows 11 popup "No supported apps in use for mic mute" and the key stays plain (user, 2026-11-09). First find a mute state Windows really keeps (such as the default capture device's mute, read and watched) and check it on the laptop. Both reviewers
- [x] 74 Razer software warning: at start and after resume the app looks for running Razer programs, services and drivers ("razer", "synapse", or an "rz" start), shows a warning in the window while one runs and opens the window once; sends nothing different. The 5 Razer driver updates are hidden with `tools/Block-RazerDriverUpdates.ps1 -Apply` (run by the user 2026-11-10, all 5 hidden). Whether those drivers break blade-lite was not tried
- [ ] 75 Temperature warning: a popup when the CPU or GPU stays above a limit (such as 95 °C) for a few minutes, from the readings the app already polls. Once per hot spell, not on each poll
- [ ] 76 Battery health in the header: wear (full charge capacity against design capacity) and cycle count, read from Windows (WMI `BatteryFullChargedCapacity`, `BatteryStaticData`, `BatteryCycleCount`). Read only, no controller command
- [ ] 77 Export and import settings, and "Reset to defaults". Import goes through the same checks as loading `config.toml` (an invalid file changes nothing), then applies as a start does. Both reviewers
- [x] 79 Showing the window or flyout puts the brightness slider back to the laptop's value and ends a drag the window lost, so a drag cut short or a Narrator set-value is not left shown and a stale mouse-up sends nothing (checked on the laptop)
- [x] 80 Quitting from the tray on battery puts back the rate from before battery, unless the panel left 60 Hz since (changed outside the app); a failure goes to the log (checked on the laptop)
- [x] 81 The 60 Hz on battery is set without saving it (`ChangeDisplaySettingsExW` flags 0), so a sign-out, restart or crash on battery starts at the saved rate; the put-back on AC still saves (checked on the laptop: charger test, and a crash then sign-out came back at 240 Hz)
- [ ] 82 The 60 Hz on battery is not saved (ticket 81), so Windows may put the saved rate back while still on battery: after a monitor or dock is plugged in, a GPU switch, the lid, or a resume. The app then leaves the high rate, with the rate buttons locked. First check on the laptop which of these does it (note it in `docs/NOTES.md`). If any does, on `Resumed` or a display change while on battery, set 60 Hz again when the panel is back at the rate saved before battery. Both reviewers
- [ ] 83 A crash on battery, then an app start without a Windows restart, leaves 60 Hz on AC: the start reads 60 Hz, so no rate is kept to put back. When the panel is at 60 Hz on battery and the saved rate differs, take the saved rate as the one to put back. Also: the put-back saves the rate it sets, so a rate that was never saved (set by another tool) then overwrites the user's saved rate; decide whether the put-back should keep the saved rate instead. Both reviewers
- [ ] 84 A Narrator set-value on the brightness slider stays shown, unsent, when the window is minimised and restored or brought back with Alt+Tab: ticket 79 puts the slider back only in `show_window` and `toggle_flyout` (ticket 79 review). Put it back on focus too, if it matters. Display only. Both reviewers
- [x] 85 Fn + F10 / F11 keyboard brightness: the keys send no code, only Fn; the app reads the level (`0383`) every 250 ms while Fn is held (up to 30 s) and once when Fn comes up (no hook, not in the wake pause), and shows, saves and sends the vapor light at it. Checked on the laptop 2026-11-10
- [x] 86 Fans still after the app was ended in Task Manager on battery (user, 2026-11-10): no app fault found. A kill sends nothing; a repeat on battery read Balanced with the fan byte auto and the fans running (0–1700 RPM at 51 °C). The app sets a fixed fan only in Custom, never below 1600 RPM, so no setting it sends can stop a fan; a start on battery can send a saved Custom fan (ticket 87). Details in notes
- [x] 87 A start reads the power source (`GetSystemPowerStatus`) before it sends the preset, so a start on battery sends Balanced for a saved Performance or Custom (no manual fan on battery); a resume reads it again, since the charger may have been pulled in the sleep. An unknown source or a failed read counts as battery and is logged, until the next power event. After a failed start, preset change or switch, the next power event sends the preset again, even for the same source, and the 60 Hz rate follows the event even when that send fails (checked on the laptop: a start on battery showed Balanced at once, Performance came back on AC)
- [ ] 88 After a failed start, preset change or power switch the preset is sent again only on the next power event (ticket 87), so Performance or Custom can keep running on battery while the app shows Balanced until then; a failed register of the power events also leaves the start's power source for the whole run. A power event during the 2 s pause after a wake is sent at once, not after the pause (ticket 87 review). Also: a resume that reads a new source (charger pulled in the sleep, or unknown) changes the preset but not the 60 Hz rate, which follows only events; and no test fails a send partway (after the Custom mode write, before the fan target). Decide whether to send again on the next poll, and to only note the source during the pause. Both reviewers

## Still to check on the laptop
Do these in one go, on AC, with "Start with Windows" on and Num Lock off. First note your preset, keyboard colour, lid logo and native mode; the last step puts them back.

1. **Read-only config (57).** Make `%APPDATA%\blade-lite\config.toml` read-only. Press the power-mode key. The popup and the header show the new mode, and the window shows `sent, but not saved`. Make the file writable again.
2. **Config save (61).** Change the preset in the app. `config.toml` has the new preset, and no `config.toml.tmp` is left in that folder.
3. **Sleep, then hibernate.** Set a keyboard colour that is not white. Sleep the laptop and wake it. About 2 s after wake the colour is back (the 2 s wait after wake). Press Fn + numpad Enter: the preset steps (the ticket 45 test sent only Fn, `04 0A`: Num Lock was on, fixed in ticket 59). Do the same with a hibernate.
4. **Restart (44, 18).** Turn the lid logo off. Pick the FHD 440 Hz native mode. Restart. The app starts with Windows, the logo stays off, and the screen starts at FHD 440 Hz.
5. **Put it back.** Set your native mode, keyboard colour, lid logo and preset back as you noted them. Restart once more for the native mode.
6. **App after ticket 70.** Start the app. Open the window and the flyout once, and click one preset. They work as before (ticket 70 changed only types in the shell code).
7. **Busy (55).** Nothing to do now. If the log ever shows a Busy error, check whether the command took effect.
8. **Brightness drag (68).** In the window, drag the keyboard brightness slider slowly and hold it. The percent beside it follows. The keyboard light changes only when you let go. Do the same in the flyout.
9. **Fans on battery (86).** Unplug the charger. In Balanced, run a CPU load (such as a game or a big build) for 2 minutes. When the app shows the CPU above about 65 °C, the fans show RPM (not 0) and you hear them. If they stay at 0 RPM, stop the load at once and note the CPU temperature.

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
- [ ] 78 Installer: uninstall closes the app, removes the "Blade Lite" Run value, and asks whether to keep `%APPDATA%\blade-lite` (config and log). Part of 39 or a ticket beside it

## Maybe later (not tickets yet)
- "Open log folder" in the tray menu
- Update check: read GitHub releases about once a week and show a "new version" note; downloads nothing
